//! Single-image haze removal with the dark channel prior (He, Sun & Tang, CVPR 2009) and a
//! guided filter (He, Sun & Tang, ECCV 2010) to refine the transmission map. Pure Rust on an
//! RGB24 buffer; FFmpeg does the decoding/encoding around it.

/// Smallest transmission kept, so dense haze does not blow up noise.
const T_MIN: f32 = 0.1;
/// Guided-filter regulariser.
const EPS: f32 = 1e-3;
/// Fraction of the haziest pixels used to estimate the atmospheric light.
const TOP_FRACTION: f32 = 0.001;

/// Dehazes `rgb` (w*h*3 bytes) in place. `strength` 0..1 scales how much haze is removed.
pub fn dehaze(rgb: &mut [u8], w: usize, h: usize, strength: f32) {
    assert_eq!(rgb.len(), w * h * 3, "buffer must be w*h*3");
    if w == 0 || h == 0 {
        return;
    }
    let omega = 0.95 * strength.clamp(0.0, 1.0);
    let r = (w.min(h) / 80).clamp(3, 25);
    let px = |i: usize| [rgb[i * 3] as f32, rgb[i * 3 + 1] as f32, rgb[i * 3 + 2] as f32];

    // 1. dark channel of the input -> atmospheric light from the haziest pixels
    let dark: Vec<f32> = (0..w * h).map(|i| px(i).into_iter().fold(f32::MAX, f32::min)).collect();
    let dark = min_filter(&dark, w, h, r);
    let mut order: Vec<usize> = (0..w * h).collect();
    let top = ((w * h) as f32 * TOP_FRACTION).ceil().max(1.0) as usize;
    order.select_nth_unstable_by(top - 1, |a, b| dark[*b].total_cmp(&dark[*a]));
    let brightest = order[..top].iter().copied().max_by(|a, b| {
        let s = |i: usize| px(i).iter().sum::<f32>();
        s(*a).total_cmp(&s(*b))
    });
    let atmo = brightest.map_or([255.0; 3], px).map(|c| c.max(1.0));

    // 2. transmission t = 1 - omega * dark(I / A)
    let norm: Vec<f32> = (0..w * h)
        .map(|i| {
            let p = px(i);
            (0..3).map(|c| p[c] / atmo[c]).fold(f32::MAX, f32::min)
        })
        .collect();
    let t: Vec<f32> = min_filter(&norm, w, h, r).into_iter().map(|d| 1.0 - omega * d).collect();

    // 3. refine t along edges with the grey image as guide
    let grey: Vec<f32> = (0..w * h)
        .map(|i| {
            let p = px(i);
            (0.299 * p[0] + 0.587 * p[1] + 0.114 * p[2]) / 255.0
        })
        .collect();
    let t = guided_filter(&grey, &t, w, h, r * 4, EPS);

    // 4. recover J = (I - A) / max(t, T_MIN) + A
    for (i, ti) in t.iter().enumerate() {
        let ti = ti.max(T_MIN);
        for c in 0..3 {
            let v = (rgb[i * 3 + c] as f32 - atmo[c]) / ti + atmo[c];
            rgb[i * 3 + c] = v.round().clamp(0.0, 255.0) as u8;
        }
    }
}

/// Reads a binary PPM (P6, maxval 255): (width, height, rgb24 pixels).
pub fn read_ppm(data: &[u8]) -> Option<(usize, usize, Vec<u8>)> {
    read_pnm(data, "P6", 3)
}

/// Reads a binary PGM (P5, maxval 255): (width, height, grey pixels).
pub fn read_pgm(data: &[u8]) -> Option<(usize, usize, Vec<u8>)> {
    read_pnm(data, "P5", 1)
}

fn read_pnm(data: &[u8], magic: &str, channels: usize) -> Option<(usize, usize, Vec<u8>)> {
    // Header: magic, width, height, maxval as whitespace-separated tokens (with # comments).
    let mut fields = Vec::new();
    let mut i = 0;
    while fields.len() < 4 {
        while data.get(i)?.is_ascii_whitespace() {
            i += 1;
        }
        if data[i] == b'#' {
            while *data.get(i)? != b'\n' {
                i += 1;
            }
            continue;
        }
        let start = i;
        while !data.get(i)?.is_ascii_whitespace() {
            i += 1;
        }
        fields.push(std::str::from_utf8(&data[start..i]).ok()?.to_string());
    }
    if fields[0] != magic || fields[3] != "255" {
        return None;
    }
    let (w, h): (usize, usize) = (fields[1].parse().ok()?, fields[2].parse().ok()?);
    let pixels = data.get(i + 1..i + 1 + w * h * channels)?.to_vec();
    Some((w, h, pixels))
}

pub fn write_ppm(w: usize, h: usize, rgb: &[u8]) -> Vec<u8> {
    let mut out = format!("P6\n{w} {h}\n255\n").into_bytes();
    out.extend_from_slice(rgb);
    out
}

/// Separable minimum over a (2r+1)² window.
fn min_filter(src: &[f32], w: usize, h: usize, r: usize) -> Vec<f32> {
    let mut tmp = vec![0.0; w * h];
    for y in 0..h {
        let row = &src[y * w..(y + 1) * w];
        for x in 0..w {
            let (a, b) = (x.saturating_sub(r), (x + r).min(w - 1));
            tmp[y * w + x] = row[a..=b].iter().copied().fold(f32::MAX, f32::min);
        }
    }
    let mut out = vec![0.0; w * h];
    for x in 0..w {
        for y in 0..h {
            let (a, b) = (y.saturating_sub(r), (y + r).min(h - 1));
            out[y * w + x] = (a..=b).map(|yy| tmp[yy * w + x]).fold(f32::MAX, f32::min);
        }
    }
    out
}

/// Mean over a (2r+1)² window (clipped at the borders) via a summed-area table.
fn box_mean(src: &[f32], w: usize, h: usize, r: usize) -> Vec<f32> {
    let mut sat = vec![0f64; (w + 1) * (h + 1)];
    for y in 0..h {
        let mut row = 0f64;
        for x in 0..w {
            row += src[y * w + x] as f64;
            sat[(y + 1) * (w + 1) + x + 1] = sat[y * (w + 1) + x + 1] + row;
        }
    }
    let mut out = vec![0.0; w * h];
    for y in 0..h {
        let (y0, y1) = (y.saturating_sub(r), (y + r + 1).min(h));
        for x in 0..w {
            let (x0, x1) = (x.saturating_sub(r), (x + r + 1).min(w));
            let s = sat[y1 * (w + 1) + x1] - sat[y0 * (w + 1) + x1] - sat[y1 * (w + 1) + x0] + sat[y0 * (w + 1) + x0];
            out[y * w + x] = (s / ((y1 - y0) * (x1 - x0)) as f64) as f32;
        }
    }
    out
}

/// Edge-preserving smoothing of `p` guided by `guide` (He et al., ECCV 2010).
fn guided_filter(guide: &[f32], p: &[f32], w: usize, h: usize, r: usize, eps: f32) -> Vec<f32> {
    let mul = |a: &[f32], b: &[f32]| a.iter().zip(b).map(|(x, y)| x * y).collect::<Vec<f32>>();
    let mean_i = box_mean(guide, w, h, r);
    let mean_p = box_mean(p, w, h, r);
    let corr_ii = box_mean(&mul(guide, guide), w, h, r);
    let corr_ip = box_mean(&mul(guide, p), w, h, r);
    let n = w * h;
    let a: Vec<f32> =
        (0..n).map(|i| (corr_ip[i] - mean_i[i] * mean_p[i]) / (corr_ii[i] - mean_i[i] * mean_i[i] + eps)).collect();
    let b: Vec<f32> = (0..n).map(|i| mean_p[i] - a[i] * mean_i[i]).collect();
    let (mean_a, mean_b) = (box_mean(&a, w, h, r), box_mean(&b, w, h, r));
    (0..n).map(|i| mean_a[i] * guide[i] + mean_b[i]).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Deterministic "clear scene": blocks of strong colours.
    fn scene(w: usize, h: usize) -> Vec<u8> {
        let mut v = Vec::with_capacity(w * h * 3);
        for y in 0..h {
            for x in 0..w {
                let k = (x / 12 + 3 * (y / 12)) % 5;
                v.extend_from_slice(&[[200, 30, 20], [20, 160, 40], [30, 40, 190], [20, 20, 20], [180, 170, 30]][k]);
            }
        }
        v
    }

    fn mae(a: &[u8], b: &[u8]) -> f32 {
        a.iter().zip(b).map(|(x, y)| (*x as f32 - *y as f32).abs()).sum::<f32>() / a.len() as f32
    }

    #[test]
    fn removes_synthetic_haze() {
        let (w, h) = (96, 72);
        let clear = scene(w, h);
        // haze model I = J t + A (1 - t), t = 0.45, A = 230; the top quarter is sky (fully
        // hazy), which is where the atmospheric light is read in real photos.
        let sky = w * h / 4 * 3;
        let hazy: Vec<u8> = clear
            .iter()
            .enumerate()
            .map(|(i, &j)| if i < sky { 230 } else { (j as f32 * 0.45 + 230.0 * 0.55).round() as u8 })
            .collect();
        let mut out = hazy.clone();
        dehaze(&mut out, w, h, 1.0);
        // judge the scene below the sky
        let (before, after) = (mae(&hazy[sky..], &clear[sky..]), mae(&out[sky..], &clear[sky..]));
        assert!(after < before * 0.5, "haze error {before:.1} -> {after:.1}");
        let mut untouched = hazy.clone();
        dehaze(&mut untouched, w, h, 0.0);
        assert!(mae(&untouched, &hazy) < 1.0, "strength 0 changes nothing");
    }

    #[test]
    fn ppm_roundtrip() {
        let px = vec![1, 2, 3, 4, 5, 6];
        let ppm = write_ppm(2, 1, &px);
        assert_eq!(read_ppm(&ppm), Some((2, 1, px.clone())));
        let commented = b"P6\n# made by ffmpeg\n2 1\n255\n\x01\x02\x03\x04\x05\x06".to_vec();
        assert_eq!(read_ppm(&commented), Some((2, 1, px)));
        assert!(read_ppm(b"P5\n1 1\n255\n\x00").is_none());
        assert_eq!(read_pgm(b"P5\n2 1\n255\n\x07\x09"), Some((2, 1, vec![7, 9])));
    }

    #[test]
    fn filters_behave() {
        let v: Vec<f32> = (0..25).map(|i| i as f32).collect();
        assert_eq!(min_filter(&v, 5, 5, 1)[12], 6.0);
        let m = box_mean(&[2.0; 20], 5, 4, 2);
        assert!(m.iter().all(|x| (*x - 2.0).abs() < 1e-6));
        let flat = guided_filter(&[0.5; 16], &[0.3; 16], 4, 4, 1, EPS);
        assert!(flat.iter().all(|x| (*x - 0.3).abs() < 1e-4));
    }
}
