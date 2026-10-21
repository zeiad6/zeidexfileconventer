//! Frontal face detection (Viola–Jones) on a greyscale buffer, std only. Uses OpenCV's
//! `haarcascade_frontalface_default.xml` (Intel License Agreement, see `data/`), embedded in
//! the binary so redaction works offline with no extra engine.

use crate::ops::Rect;

static FRONTAL_FACE: &str = include_str!("../data/haarcascade_frontalface_default.xml");

struct Weak {
    feature: usize,
    threshold: f32,
    left: f32,
    right: f32,
}

struct Stage {
    threshold: f32,
    weak: Vec<Weak>,
}

/// One weighted rectangle of a Haar feature: (x, y, w, h, weight).
type WeightedRect = (u32, u32, u32, u32, f32);
/// A rectangle scaled to the current window, weight already divided by the window area.
type ScaledRect = (usize, usize, usize, usize, f64);

pub struct Cascade {
    width: u32,
    height: u32,
    stages: Vec<Stage>,
    /// Rectangles per feature, in base-window coordinates.
    features: Vec<Vec<WeightedRect>>,
}

/// Text of every `<tag>...</tag>` in `xml`, in order.
fn tag_texts<'a>(xml: &'a str, tag: &str) -> Vec<&'a str> {
    let (open, close) = (format!("<{tag}>"), format!("</{tag}>"));
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(i) = rest.find(&open) {
        let body = &rest[i + open.len()..];
        let Some(j) = body.find(&close) else { break };
        out.push(body[..j].trim());
        rest = &body[j + close.len()..];
    }
    out
}

fn nums(s: &str) -> Vec<f32> {
    s.split_whitespace().filter_map(|t| t.parse::<f32>().ok()).collect()
}

impl Cascade {
    /// Parses an OpenCV "new format" stump cascade.
    pub fn parse(xml: &str) -> Option<Cascade> {
        let width = tag_texts(xml, "width").first()?.parse().ok()?;
        let height = tag_texts(xml, "height").first()?.parse().ok()?;
        let stages_xml = &xml[xml.find("<stages>")?..xml.find("<features>")?];
        let features_xml = &xml[xml.find("<features>")?..];
        let mut stages = Vec::new();
        // Each stage starts at its <maxWeakCount>; split there and read the parts in order.
        for chunk in stages_xml.split("<maxWeakCount>").skip(1) {
            let threshold = tag_texts(chunk, "stageThreshold").first()?.parse().ok()?;
            let nodes = tag_texts(chunk, "internalNodes");
            let leaves = tag_texts(chunk, "leafValues");
            let weak = nodes
                .iter()
                .zip(&leaves)
                .map(|(n, l)| {
                    let (n, l) = (nums(n), nums(l));
                    // stumps only: "left right feature threshold" + 2 leaves
                    (n.len() == 4 && l.len() == 2).then(|| Weak {
                        feature: n[2] as usize,
                        threshold: n[3],
                        left: l[0],
                        right: l[1],
                    })
                })
                .collect::<Option<Vec<_>>>()?;
            stages.push(Stage { threshold, weak });
        }
        let features = tag_texts(features_xml, "rects")
            .iter()
            .map(|block| {
                tag_texts(block, "_")
                    .iter()
                    .map(|r| {
                        let v = nums(r.trim_end_matches('.'));
                        (v.len() == 5).then(|| (v[0] as u32, v[1] as u32, v[2] as u32, v[3] as u32, v[4]))
                    })
                    .collect::<Option<Vec<_>>>()
            })
            .collect::<Option<Vec<_>>>()?;
        let max_feature = stages.iter().flat_map(|s| s.weak.iter().map(|w| w.feature)).max()?;
        (max_feature < features.len() && !stages.is_empty()).then_some(Cascade { width, height, stages, features })
    }

    pub fn frontal_face() -> Cascade {
        Cascade::parse(FRONTAL_FACE).expect("embedded cascade is valid")
    }

    /// Faces in a `w`x`h` greyscale image, after grouping overlapping hits.
    pub fn detect(&self, gray: &[u8], w: usize, h: usize) -> Vec<Rect> {
        assert_eq!(gray.len(), w * h);
        let stride = w + 1;
        let mut ii = vec![0u64; stride * (h + 1)];
        let mut sq = vec![0u64; stride * (h + 1)];
        for y in 0..h {
            let (mut row, mut row_sq) = (0u64, 0u64);
            for x in 0..w {
                let v = gray[y * w + x] as u64;
                row += v;
                row_sq += v * v;
                ii[(y + 1) * stride + x + 1] = ii[y * stride + x + 1] + row;
                sq[(y + 1) * stride + x + 1] = sq[y * stride + x + 1] + row_sq;
            }
        }
        let sum = |t: &[u64], x: usize, y: usize, rw: usize, rh: usize| {
            (t[(y + rh) * stride + x + rw] + t[y * stride + x]) as f64
                - (t[y * stride + x + rw] + t[(y + rh) * stride + x]) as f64
        };
        let mut hits = Vec::new();
        let mut scale = 1.0f64;
        while (self.width as f64 * scale) as usize <= w.min(h) && (self.height as f64 * scale) as usize <= w.min(h) {
            let (ww, wh) = ((self.width as f64 * scale) as usize, (self.height as f64 * scale) as usize);
            // Normalisation window: the base window minus a 1 px border, scaled.
            let e = scale.round() as usize;
            let (ew, eh) = (
                ((self.width - 2) as f64 * scale).round() as usize,
                ((self.height - 2) as f64 * scale).round() as usize,
            );
            let inv_area = 1.0 / (ew * eh) as f64;
            // Scale every rectangle; rect 0's weight is re-derived so rounding keeps each
            // feature zero-sum (as OpenCV's legacy detector does).
            let scaled: Vec<Vec<ScaledRect>> = self
                .features
                .iter()
                .map(|f| {
                    let mut rs: Vec<ScaledRect> = f
                        .iter()
                        .map(|&(x, y, rw, rh, wt)| {
                            let s = |v: u32| (v as f64 * scale).round() as usize;
                            // Rounding may push a rectangle one pixel past the window: clip it.
                            let (sx, sy) = (s(x).min(ww - 1), s(y).min(wh - 1));
                            (sx, sy, s(rw).clamp(1, ww - sx), s(rh).clamp(1, wh - sy), wt as f64)
                        })
                        .collect();
                    let rest: f64 = rs.iter().skip(1).map(|r| r.4 * (r.2 * r.3) as f64).sum();
                    rs[0].4 = -rest / (rs[0].2 * rs[0].3) as f64;
                    rs.iter_mut().for_each(|r| r.4 *= inv_area);
                    rs
                })
                .collect();
            let step = ((scale * 1.5).round() as usize).max(1);
            let mut y = 0;
            while y + wh <= h {
                let mut x = 0;
                while x + ww <= w {
                    let mean = sum(&ii, x + e, y + e, ew, eh) * inv_area;
                    let var = sum(&sq, x + e, y + e, ew, eh) * inv_area - mean * mean;
                    let std = if var > 0.0 { var.sqrt() } else { 1.0 };
                    let passed = self.stages.iter().all(|stage| {
                        let total: f32 = stage
                            .weak
                            .iter()
                            .map(|wk| {
                                let val: f64 = scaled[wk.feature]
                                    .iter()
                                    .map(|&(rx, ry, rw, rh, wt)| wt * sum(&ii, x + rx, y + ry, rw, rh))
                                    .sum();
                                if val < wk.threshold as f64 * std {
                                    wk.left
                                } else {
                                    wk.right
                                }
                            })
                            .sum();
                        total >= stage.threshold
                    });
                    if passed {
                        hits.push(Rect { x: x as u32, y: y as u32, w: ww as u32, h: wh as u32 });
                    }
                    x += step;
                }
                y += step;
            }
            scale *= SCALE_STEP;
        }
        group(&hits, MIN_NEIGHBOURS)
    }
}

/// Window growth per pyramid level.
const SCALE_STEP: f64 = 1.15;
/// A face needs more than this many overlapping raw hits (OpenCV's `minNeighbors`).
const MIN_NEIGHBOURS: usize = 3;

/// OpenCV-style `groupRectangles`: cluster similar boxes, average them, drop weak clusters
/// and clusters nested inside a stronger one.
fn group(rects: &[Rect], min_neighbours: usize) -> Vec<Rect> {
    const EPS: f64 = 0.2;
    let similar = |a: &Rect, b: &Rect| {
        let delta = EPS * (a.w.min(b.w) + a.h.min(b.h)) as f64 * 0.5;
        let d = |p: u32, q: u32| (p as f64 - q as f64).abs() <= delta;
        d(a.x, b.x) && d(a.y, b.y) && d(a.x + a.w, b.x + b.w) && d(a.y + a.h, b.y + b.h)
    };
    let mut label: Vec<usize> = (0..rects.len()).collect();
    fn root(l: &mut [usize], mut i: usize) -> usize {
        while l[i] != i {
            l[i] = l[l[i]];
            i = l[i];
        }
        i
    }
    for i in 0..rects.len() {
        for j in i + 1..rects.len() {
            if similar(&rects[i], &rects[j]) {
                let (a, b) = (root(&mut label, i), root(&mut label, j));
                label[a] = b;
            }
        }
    }
    let mut acc: std::collections::BTreeMap<usize, (u64, u64, u64, u64, usize)> = Default::default();
    for (i, r) in rects.iter().enumerate() {
        let e = acc.entry(root(&mut label, i)).or_default();
        *e = (e.0 + r.x as u64, e.1 + r.y as u64, e.2 + r.w as u64, e.3 + r.h as u64, e.4 + 1);
    }
    let clusters: Vec<(Rect, usize)> = acc
        .into_values()
        .filter(|c| c.4 > min_neighbours)
        .map(|(x, y, w, h, n)| {
            let k = n as u64;
            (Rect { x: (x / k) as u32, y: (y / k) as u32, w: (w / k) as u32, h: (h / k) as u32 }, n)
        })
        .collect();
    clusters
        .iter()
        .filter(|(r, n)| {
            !clusters.iter().any(|(o, m)| {
                o != r && m >= n && r.x >= o.x && r.y >= o.y && r.x + r.w <= o.x + o.w && r.y + r.h <= o.y + o.h
            })
        })
        .map(|(r, _)| *r)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_cascade_parses() {
        let c = Cascade::frontal_face();
        assert_eq!((c.width, c.height), (24, 24));
        assert_eq!(c.stages.len(), 25);
        assert_eq!(c.stages.iter().map(|s| s.weak.len()).sum::<usize>(), 2913);
        assert_eq!(c.features.len(), 2913);
        assert_eq!(c.features[0], vec![(6, 4, 12, 9, -1.0), (6, 7, 12, 3, 3.0)]);
    }

    #[test]
    fn flat_images_have_no_faces() {
        let c = Cascade::frontal_face();
        assert!(c.detect(&vec![128u8; 96 * 96], 96, 96).is_empty());
        let noise: Vec<u8> = (0..96 * 96u32).map(|i| (i.wrapping_mul(2_654_435_761) >> 24) as u8).collect();
        assert!(c.detect(&noise, 96, 96).is_empty());
    }

    #[test]
    fn grouping_needs_neighbours() {
        let r = |x, y| Rect { x, y, w: 40, h: 40 };
        let many = [r(10, 10), r(11, 10), r(10, 12), r(12, 11), r(200, 200)];
        assert_eq!(group(&many, 3), vec![Rect { x: 10, y: 10, w: 40, h: 40 }]);
        assert!(group(&many[..3], 3).is_empty());
    }
}
