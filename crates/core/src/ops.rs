//! Operations a user can run, the tool catalog shown in the wheel, and option parsing.

use crate::format::Category::{self, *};
use crate::{Error, Result};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    High,
    Medium,
    Low,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Compression {
    Percent(u8),
    TargetBytes(u64),
    Preset(Level),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Size {
    Percent(u32),
    Max(u32),
}

/// Pixel rectangle in source-image coordinates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Adjust {
    pub brightness: f64,
    pub contrast: f64,
    pub saturation: f64,
    pub gamma: f64,
    pub hue: f64,
    pub sharpen: f64,
    pub denoise: f64,
    pub grain: f64,
    /// Local contrast (large-radius unsharp mask), Tangerine's "clarity".
    pub clarity: f64,
}

impl Adjust {
    pub const NEUTRAL: Adjust = Adjust {
        brightness: 0.0,
        contrast: 1.0,
        saturation: 1.0,
        gamma: 1.0,
        hue: 0.0,
        sharpen: 0.0,
        denoise: 0.0,
        grain: 0.0,
        clarity: 0.0,
    };
}

/// `Op::Redact::auto` value asking for face detection instead of OCR.
pub const FACES: &str = "faces";

/// `exif` option key -> ExifTool tag.
pub const EXIF_FIELDS: [(&str, &str); 7] = [
    ("artist", "Artist"),
    ("copyright", "Copyright"),
    ("description", "ImageDescription"),
    ("date", "DateTimeOriginal"),
    ("make", "Make"),
    ("model", "Model"),
    ("keywords", "XMP-dc:Subject"),
];

/// Tags `zeidex tool tags` may set; others are ignored so a typo cannot write junk keys.
pub const TAG_KEYS: [&str; 7] = ["title", "artist", "album", "date", "genre", "comment", "copyright"];

#[derive(Clone, Debug, PartialEq)]
pub enum CropSpec {
    Ratio(u32, u32),
    Box(Rect),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Op {
    /// `quality` (1-100) applies to lossy image encoders and to video/audio re-encodes.
    Convert {
        to: String,
        quality: Option<u8>,
    },
    Compress {
        how: Compression,
        max: Option<u32>,
    },
    StripMetadata,
    Info,
    /// `password` encrypts (AES-256) zip/7z; `split` writes volumes of that many bytes.
    Archive {
        kind: String,
        password: Option<String>,
        split: Option<u64>,
    },
    Extract {
        password: Option<String>,
    },
    ExtractMotion,
    Resize(Size),
    Crop(CropSpec),
    Rotate {
        degrees: u32,
    },
    Background {
        rgb: [u8; 3],
        padding: u32,
    },
    Grayscale,
    ImagesToPdf,
    Collage {
        across: u32,
        gap: u32,
    },
    Ocr {
        to: String,
        lang: String,
    },
    Trim {
        start: f64,
        end: Option<f64>,
        silence: bool,
    },
    Speed {
        factor: f64,
    },
    Snapshot {
        at: f64,
    },
    ExtractAudio,
    Mute,
    Split {
        parts: u32,
        at: Vec<f64>,
    },
    Join,
    Normalize,
    Channels {
        count: u32,
        left: f64,
        right: f64,
    },
    Bleep {
        start: f64,
        end: f64,
    },
    /// Blur (`fill: None`) or paint over boxes, optionally only between two times (video).
    /// `auto` replaces `boxes` with what is found in the image: text lines for a Tesseract
    /// language code, or faces for the special value `"faces"`.
    Redact {
        boxes: Vec<Rect>,
        fill: Option<[u8; 3]>,
        range: Option<(f64, f64)>,
        auto: Option<String>,
    },
    Visualize {
        shape: String,
    },
    /// Photo/video look: eq + sharpen + denoise + grain, all through FFmpeg.
    Adjust(Adjust),
    /// Composite a transparent PNG (drawn at preview size) over the original at full size.
    Overlay {
        png: std::path::PathBuf,
        width: u32,
        height: u32,
    },
    /// Container tags (title, artist...) for audio/video, copied without re-encoding.
    SetTags(Vec<(String, String)>),
    /// Logo over an image or video. `pos`: tl, tr, bl, br, center; `scale`: % of the width.
    Watermark {
        image: std::path::PathBuf,
        pos: String,
        opacity: f64,
        scale: f64,
    },
    /// Move every cue of a subtitle file by `ms` (negative = earlier).
    ShiftSubs {
        ms: i64,
    },
    ExtractSubs,
    /// Video + subtitle file: as a selectable track (`burn: false`) or drawn into the picture.
    AddSubs {
        burn: bool,
    },
    Reverse,
    Loop {
        count: u32,
    },
    Stabilize,
    Frames {
        fps: f64,
    },
    /// Several images into an MP4 slideshow, `secs` per image with short fades.
    Slideshow {
        secs: f64,
    },
    Fade {
        secs: f64,
    },
    Protect {
        password: String,
    },
    Unlock {
        password: String,
    },
    /// Speech to SRT subtitles with whisper.cpp (`lang` "auto" or an ISO code).
    Transcribe {
        lang: String,
    },
    /// Multi-size Windows .ico from an image.
    Icon,
    Checksum,
    Rename {
        pattern: String,
        start: usize,
    },
    /// Several single-file ops applied one after another (`resize max=1600 | compress percent=60`).
    Recipe(Vec<Op>),
    /// EXIF edit through ExifTool: (tag, value) pairs, GPS position, or GPS removal.
    Exif {
        tags: Vec<(String, String)>,
        gps: Option<(f64, f64)>,
        remove_gps: bool,
    },
    /// Decode QR codes / barcodes in an image (ZBar).
    QrRead,
    /// Fit into a social format (9:16, 1:1, 4:5, 16:9) over a blurred copy of itself.
    Social {
        ratio: (u32, u32),
    },
    /// 4×4 grid of frames from across a video.
    ContactSheet,
    /// Downscale a video to at most `height` lines.
    VideoHeight {
        height: u32,
    },
    /// Video + audio file: replace the video's soundtrack.
    ReplaceAudio,
    DenoiseAudio,
    RemoveSilence,
    /// Responsive web images: WebP + AVIF at several widths, plus a `<picture>` snippet.
    WebSet,
    /// Remove haze (dark channel prior), `strength` 0..1.
    Dehaze {
        strength: f64,
    },
    /// Find identical files (size, then SHA-256) under the selection; optionally move copies away.
    Duplicates {
        move_dupes: bool,
    },
    MergePdf,
    /// Pages picked from several PDFs: (file index, page), both 1-based, plus output rotations.
    Arrange {
        items: Vec<(usize, u32)>,
        rotate: Vec<(u32, u32)>,
    },
    SplitPdf,
    /// `rotate` holds (output page, degrees) pairs.
    Pages {
        spec: String,
        rotate: Vec<(u32, u32)>,
    },
}

impl Op {
    /// Ops that turn the whole selection into one output instead of one output per file.
    pub fn is_multi(&self) -> bool {
        matches!(
            self,
            Op::Archive { .. }
                | Op::ImagesToPdf
                | Op::Collage { .. }
                | Op::Join
                | Op::MergePdf
                | Op::AddSubs { .. }
                | Op::Slideshow { .. }
                | Op::ReplaceAudio
                | Op::Arrange { .. }
        )
    }

    pub fn parse(id: &str, opts: &HashMap<String, String>) -> Result<Op> {
        let get = |k: &str| opts.get(k).map(|s| s.trim()).filter(|s| !s.is_empty());
        let num = |k: &str, d: f64| get(k).map_or(Ok(d), |v| v.parse::<f64>().map_err(|_| bad(k, v)));
        let int = |k: &str, d: u32| get(k).map_or(Ok(d), |v| v.parse::<u32>().map_err(|_| bad(k, v)));
        let time = |k: &str| get(k).map(|v| parse_time(v).ok_or_else(|| bad(k, v))).transpose();
        Ok(match id {
            "compress" => {
                let how = if let Some(t) = get("target") {
                    Compression::TargetBytes(parse_size(t).ok_or_else(|| bad("target", t))?)
                } else if let Some(p) = get("percent") {
                    let p: u8 = p.trim_end_matches('%').parse().map_err(|_| bad("percent", p))?;
                    if !(1..=95).contains(&p) {
                        return Err(bad("percent", "must be 1-95"));
                    }
                    Compression::Percent(p)
                } else {
                    Compression::Preset(match get("preset").unwrap_or("medium") {
                        "high" => Level::High,
                        "low" => Level::Low,
                        "medium" => Level::Medium,
                        v => return Err(bad("preset", v)),
                    })
                };
                let max = get("max").map(|v| v.parse().map_err(|_| bad("max", v))).transpose()?;
                Op::Compress { how, max }
            }
            "strip" | "strip-exif" | "strip-metadata" => Op::StripMetadata,
            "info" => Op::Info,
            "archive" => {
                let kind = get("format").unwrap_or("zip").to_string();
                if !["zip", "7z", "tar", "tgz"].contains(&kind.as_str()) {
                    return Err(bad("format", &kind));
                }
                let password = get("password").map(str::to_string);
                if password.is_some() && !["zip", "7z"].contains(&kind.as_str()) {
                    return Err(bad("password", "only zip and 7z archives can be encrypted"));
                }
                let split = get("split").map(|v| parse_size(v).ok_or_else(|| bad("split", v))).transpose()?;
                if split.is_some_and(|b| b < 100_000) {
                    return Err(bad("split", "parts must be at least 100KB"));
                }
                Op::Archive { kind, password, split }
            }
            "extract" => Op::Extract { password: get("password").map(str::to_string) },
            "motion" => Op::ExtractMotion,
            "resize" => match (get("scale"), get("max")) {
                (Some(s), _) => {
                    Op::Resize(Size::Percent(s.trim_end_matches('%').parse().map_err(|_| bad("scale", s))?))
                }
                (None, Some(m)) => Op::Resize(Size::Max(m.parse().map_err(|_| bad("max", m))?)),
                (None, None) => Op::Resize(Size::Max(1920)),
            },
            "crop" => match get("box") {
                Some(b) => Op::Crop(CropSpec::Box(parse_rect(b)?)),
                None => {
                    let (w, h) = parse_ratio(get("ratio").unwrap_or("1:1"))?;
                    Op::Crop(CropSpec::Ratio(w, h))
                }
            },
            "rotate" => {
                let degrees = int("degrees", 90)?;
                if ![90, 180, 270].contains(&degrees) {
                    return Err(bad("degrees", "use 90, 180 or 270"));
                }
                Op::Rotate { degrees }
            }
            "background" => {
                Op::Background { rgb: parse_color(get("color").unwrap_or("#ffffff"))?, padding: int("padding", 64)? }
            }
            "grayscale" => Op::Grayscale,
            "images-to-pdf" => Op::ImagesToPdf,
            "collage" => Op::Collage { across: int("across", 3)?.max(1), gap: int("gap", 16)? },
            "ocr" => {
                let to = get("to").unwrap_or("pdf").to_string();
                if to != "pdf" && to != "txt" {
                    return Err(bad("to", &to));
                }
                let lang = get("lang").unwrap_or("eng").to_string();
                if !lang.chars().all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '_') {
                    return Err(bad("lang", &lang));
                }
                Op::Ocr { to, lang }
            }
            "trim" => {
                let start = time("start")?.unwrap_or(0.0);
                let end = time("end")?;
                if end.is_some_and(|e| e <= start) {
                    return Err(bad("end", "must be after start"));
                }
                Op::Trim { start, end, silence: matches!(get("silence"), Some("1" | "true" | "yes")) }
            }
            "speed" => {
                let factor = num("factor", 2.0)?;
                if !(0.25..=4.0).contains(&factor) {
                    return Err(bad("factor", "use 0.25-4"));
                }
                Op::Speed { factor }
            }
            "snapshot" => Op::Snapshot { at: time("at")?.unwrap_or(0.0) },
            "extract-audio" => Op::ExtractAudio,
            "mute" => Op::Mute,
            "split" => {
                let mut at = Vec::new();
                for t in get("at").unwrap_or("").split(',').map(str::trim).filter(|t| !t.is_empty()) {
                    at.push(parse_time(t).filter(|v| *v > 0.0).ok_or_else(|| bad("at", t))?);
                }
                at.sort_by(f64::total_cmp);
                at.dedup();
                Op::Split { parts: int("parts", 2)?.clamp(2, 100), at }
            }
            "join" => Op::Join,
            "normalize" => Op::Normalize,
            "channels" => {
                let count = int("count", 1)?;
                if count != 1 && count != 2 {
                    return Err(bad("count", "use 1 (mono) or 2 (stereo)"));
                }
                let (left, right) = (num("left", 1.0)?, num("right", 1.0)?);
                if !(0.0..=2.0).contains(&left) || !(0.0..=2.0).contains(&right) {
                    return Err(bad("left/right", "use 0-2"));
                }
                Op::Channels { count, left, right }
            }
            "bleep" => {
                let start = time("start")?.ok_or_else(|| bad("start", "required"))?;
                let end = time("end")?.ok_or_else(|| bad("end", "required"))?;
                if end <= start {
                    return Err(bad("end", "must be after start"));
                }
                Op::Bleep { start, end }
            }
            "redact" => {
                let spec = get("boxes").ok_or_else(|| bad("boxes", "required (x,y,w,h;... or auto)"))?;
                let auto = if spec == "faces" {
                    Some(FACES.to_string())
                } else if spec == "auto" || spec == "text" {
                    let lang = get("lang").unwrap_or("eng");
                    if !lang.chars().all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '_') {
                        return Err(bad("lang", lang));
                    }
                    Some(lang.to_string())
                } else {
                    None
                };
                let boxes = (if auto.is_some() { "" } else { spec })
                    .split(';')
                    .map(str::trim)
                    .filter(|b| !b.is_empty())
                    .map(parse_rect)
                    .collect::<Result<Vec<_>>>()?;
                if boxes.is_empty() && auto.is_none() {
                    return Err(bad("boxes", "required"));
                }
                let fill = match get("style").unwrap_or("blur") {
                    "blur" => None,
                    c => Some(parse_color(c)?),
                };
                let range = match (time("start")?, time("end")?) {
                    (None, None) => None,
                    (s, e) => {
                        let (s, e) = (s.unwrap_or(0.0), e.unwrap_or(f64::MAX));
                        if e <= s {
                            return Err(bad("end", "must be after start"));
                        }
                        Some((s, e))
                    }
                };
                Op::Redact { boxes, fill, range, auto }
            }
            "annotate" => return Err(Error::Unsupported("annotate runs in the app's editor".into())),
            "adjust" => {
                let r = |k: &str, d: f64, lo: f64, hi: f64| -> Result<f64> {
                    let v = num(k, d)?;
                    if (lo..=hi).contains(&v) {
                        Ok(v)
                    } else {
                        Err(bad(k, &format!("use {lo}..{hi}")))
                    }
                };
                let a = Adjust {
                    brightness: r("brightness", 0.0, -1.0, 1.0)?,
                    contrast: r("contrast", 1.0, 0.0, 3.0)?,
                    saturation: r("saturation", 1.0, 0.0, 3.0)?,
                    gamma: r("gamma", 1.0, 0.1, 5.0)?,
                    hue: r("hue", 0.0, -180.0, 180.0)?,
                    sharpen: r("sharpen", 0.0, 0.0, 3.0)?,
                    denoise: r("denoise", 0.0, 0.0, 20.0)?,
                    grain: r("grain", 0.0, 0.0, 60.0)?,
                    clarity: r("clarity", 0.0, 0.0, 2.0)?,
                };
                if a == Adjust::NEUTRAL {
                    return Err(bad("adjust", "nothing to change"));
                }
                Op::Adjust(a)
            }
            "tags" => {
                let pairs: Vec<(String, String)> =
                    TAG_KEYS.iter().filter_map(|k| get(k).map(|v| (k.to_string(), v.to_string()))).collect();
                if pairs.is_empty() {
                    return Err(bad(
                        "tags",
                        "set at least one of title, artist, album, date, genre, comment, copyright",
                    ));
                }
                Op::SetTags(pairs)
            }
            "watermark" => {
                let image = get("image").ok_or_else(|| bad("image", "path to a logo is required"))?;
                let pos = get("pos").unwrap_or("br").to_string();
                if !["tl", "tr", "bl", "br", "center"].contains(&pos.as_str()) {
                    return Err(bad("pos", "use tl, tr, bl, br or center"));
                }
                let opacity = num("opacity", 0.8)?;
                let scale = num("scale", 20.0)?;
                if !(0.05..=1.0).contains(&opacity) || !(1.0..=100.0).contains(&scale) {
                    return Err(bad("opacity/scale", "use opacity 0.05-1 and scale 1-100"));
                }
                Op::Watermark { image: image.into(), pos, opacity, scale }
            }
            "shift-subs" => {
                let v = get("seconds").ok_or_else(|| bad("seconds", "required, e.g. -1.5"))?;
                let secs: f64 = v.parse().map_err(|_| bad("seconds", v))?;
                Op::ShiftSubs { ms: (secs * 1000.0).round() as i64 }
            }
            "extract-subs" => Op::ExtractSubs,
            "add-subs" => Op::AddSubs { burn: false },
            "burn-subs" => Op::AddSubs { burn: true },
            "reverse" => Op::Reverse,
            "loop" => Op::Loop { count: int("count", 3)?.clamp(2, 100) },
            "stabilize" => Op::Stabilize,
            "frames" => {
                let fps = num("fps", 1.0)?;
                if !(0.01..=60.0).contains(&fps) {
                    return Err(bad("fps", "use 0.01-60"));
                }
                Op::Frames { fps }
            }
            "slideshow" => {
                let secs = num("seconds", 3.0)?;
                if !(1.0..=60.0).contains(&secs) {
                    return Err(bad("seconds", "use 1-60"));
                }
                Op::Slideshow { secs }
            }
            "fade" => {
                let secs = num("seconds", 1.0)?;
                if !(0.1..=10.0).contains(&secs) {
                    return Err(bad("seconds", "use 0.1-10"));
                }
                Op::Fade { secs }
            }
            "protect" | "unlock" => {
                let password = get("password").ok_or_else(|| bad("password", "required"))?.to_string();
                if id == "protect" {
                    Op::Protect { password }
                } else {
                    Op::Unlock { password }
                }
            }
            "transcribe" => {
                let lang = get("lang").unwrap_or("auto").to_string();
                if lang.len() > 8 || !lang.chars().all(|c| c.is_ascii_alphabetic()) {
                    return Err(bad("lang", &lang));
                }
                Op::Transcribe { lang }
            }
            "icon" => Op::Icon,
            "checksum" => Op::Checksum,
            "rename" => {
                let pattern = get("pattern").ok_or_else(|| bad("pattern", "required, e.g. trip-{n:3}"))?.to_string();
                Op::Rename { pattern, start: int("start", 1)? as usize }
            }
            "recipe" => Op::Recipe(parse_recipe(get("steps").ok_or_else(|| bad("steps", "required"))?)?),
            "exif" => {
                let tags: Vec<(String, String)> = EXIF_FIELDS
                    .iter()
                    .filter_map(|(key, tag)| get(key).map(|v| (tag.to_string(), v.to_string())))
                    .collect();
                let gps = get("gps")
                    .map(|v| {
                        let (la, lo) = v.split_once(',').ok_or_else(|| bad("gps", "use lat,lon"))?;
                        match (la.trim().parse::<f64>(), lo.trim().parse::<f64>()) {
                            (Ok(a), Ok(o)) if a.abs() <= 90.0 && o.abs() <= 180.0 => Ok((a, o)),
                            _ => Err(bad("gps", v)),
                        }
                    })
                    .transpose()?;
                let remove_gps = matches!(get("remove-gps"), Some("1" | "true" | "yes"));
                if tags.is_empty() && gps.is_none() && !remove_gps {
                    return Err(bad("exif", "set a field, gps=lat,lon or remove-gps=1"));
                }
                Op::Exif { tags, gps, remove_gps }
            }
            "qr-read" => Op::QrRead,
            "social" => {
                let ratio = parse_ratio(get("ratio").unwrap_or("9:16"))?;
                if ![(9, 16), (1, 1), (4, 5), (16, 9)].contains(&ratio) {
                    return Err(bad("ratio", "use 9:16, 1:1, 4:5 or 16:9"));
                }
                Op::Social { ratio }
            }
            "contact-sheet" => Op::ContactSheet,
            "video-size" => {
                let height = int("height", 720)?;
                if ![2160, 1440, 1080, 720, 480, 360].contains(&height) {
                    return Err(bad("height", "use 2160, 1440, 1080, 720, 480 or 360"));
                }
                Op::VideoHeight { height }
            }
            "replace-audio" => Op::ReplaceAudio,
            "denoise-audio" => Op::DenoiseAudio,
            "remove-silence" => Op::RemoveSilence,
            "web-set" => Op::WebSet,
            "dehaze" => {
                let strength = num("strength", 0.8)?;
                if !(0.05..=1.0).contains(&strength) {
                    return Err(bad("strength", "use 0.05-1"));
                }
                Op::Dehaze { strength }
            }
            "duplicates" => Op::Duplicates { move_dupes: matches!(get("move"), Some("1" | "true" | "yes")) },
            "visualize" => {
                let shape = get("shape").unwrap_or("landscape").to_string();
                if !["landscape", "portrait", "square"].contains(&shape.as_str()) {
                    return Err(bad("shape", &shape));
                }
                Op::Visualize { shape }
            }
            "merge-pdf" => Op::MergePdf,
            "arrange" => {
                let mut items = Vec::new();
                for it in get("items").ok_or_else(|| bad("items", "required, e.g. 1:1,2:1"))?.split(',') {
                    let parsed = it.trim().split_once(':').and_then(|(f, p)| Some((f.parse().ok()?, p.parse().ok()?)));
                    match parsed {
                        Some((f, p)) if f > 0 && p > 0 => items.push((f, p)),
                        _ => return Err(bad("items", it)),
                    }
                }
                let mut rotate = Vec::new();
                for r in get("rotate").unwrap_or("").split(',').map(str::trim).filter(|x| !x.is_empty()) {
                    match r.split_once(':').and_then(|(p, d)| Some((p.parse::<u32>().ok()?, d.parse::<u32>().ok()?))) {
                        Some((p, d)) if p > 0 && [90, 180, 270].contains(&d) => rotate.push((p, d)),
                        _ => return Err(bad("rotate", r)),
                    }
                }
                Op::Arrange { items, rotate }
            }
            "split-pdf" => Op::SplitPdf,
            "pages" => {
                let spec: String = get("spec").unwrap_or("1-z").chars().filter(|c| !c.is_whitespace()).collect();
                // Passed to qpdf as one argv entry; still refuse anything that is not a page range.
                if spec.is_empty() || !spec.chars().all(|c| c.is_ascii_digit() || ",-zr:".contains(c)) {
                    return Err(bad("spec", &spec));
                }
                let mut rotate = Vec::new();
                for item in get("rotate").unwrap_or("").split(',').map(str::trim).filter(|x| !x.is_empty()) {
                    let parsed = item
                        .split_once(':')
                        .and_then(|(pg, d)| Some((pg.parse::<u32>().ok()?, d.parse::<u32>().ok()?)));
                    match parsed {
                        Some((pg, d)) if pg > 0 && [90, 180, 270].contains(&d) => rotate.push((pg, d)),
                        _ => return Err(bad("rotate", item)),
                    }
                }
                Op::Pages { spec, rotate }
            }
            other => return Err(Error::Unsupported(format!("unknown tool `{other}`"))),
        })
    }
}

/// `resize max=1600 | compress percent=60 | strip` (a step may also be `convert to=webp`).
pub fn parse_recipe(spec: &str) -> Result<Vec<Op>> {
    let mut steps = Vec::new();
    for part in spec.split('|').map(str::trim).filter(|p| !p.is_empty()) {
        let op = parse_step(part)?;
        if op.is_multi()
            || matches!(
                op,
                Op::Recipe(_) | Op::Info | Op::Checksum | Op::Rename { .. } | Op::QrRead | Op::Duplicates { .. }
            )
        {
            return Err(bad("recipe", &format!("`{part}` cannot be a recipe step")));
        }
        steps.push(op);
    }
    if steps.len() < 2 {
        return Err(bad("recipe", "needs at least two steps separated by |"));
    }
    Ok(steps)
}

/// One `id key=value ...` step (`convert to=webp` for conversions); with `|` it is a recipe.
pub fn parse_step(part: &str) -> Result<Op> {
    if part.contains('|') {
        return Ok(Op::Recipe(parse_recipe(part)?));
    }
    let mut words = part.split_whitespace();
    let id = words.next().ok_or_else(|| bad("step", "empty"))?;
    let mut opts = HashMap::new();
    for w in words {
        let (k, v) = w.split_once('=').ok_or_else(|| bad("step", &format!("`{w}` should be key=value")))?;
        opts.insert(k.to_string(), v.to_string());
    }
    match id {
        "convert" => Ok(Op::Convert {
            to: opts.get("to").ok_or_else(|| bad("step", "convert needs to=EXT"))?.to_lowercase(),
            quality: parse_quality(opts.get("quality"))?,
        }),
        other => Op::parse(other, &opts),
    }
}

/// `--quality 85` / `quality=85`: an encoder quality in 1-100, the scale every GUI uses.
pub fn parse_quality(value: Option<&String>) -> Result<Option<u8>> {
    let Some(raw) = value.map(|v| v.trim()).filter(|v| !v.is_empty()) else { return Ok(None) };
    let q: u8 = raw.parse().map_err(|_| bad("quality", raw))?;
    if !(1..=100).contains(&q) {
        return Err(bad("quality", "must be 1-100"));
    }
    Ok(Some(q))
}

fn bad(key: &str, val: &str) -> Error {
    Error::BadOption(format!("{key}={val}"))
}

/// `90`, `1:30`, `01:02:03.5` → seconds.
pub fn parse_time(s: &str) -> Option<f64> {
    let mut total = 0.0;
    for part in s.split(':') {
        let v: f64 = part.parse().ok()?;
        if v < 0.0 {
            return None;
        }
        total = total * 60.0 + v;
    }
    (s.split(':').count() <= 3).then_some(total)
}

/// `5MB`, `500k`, `1.5GB`, `1200` → bytes (decimal units, like file managers show).
pub fn parse_size(s: &str) -> Option<u64> {
    let s = s.trim().to_ascii_lowercase();
    let s = s.strip_suffix('b').unwrap_or(&s);
    let (num, mul) = match s.chars().last()? {
        'k' => (&s[..s.len() - 1], 1e3),
        'm' => (&s[..s.len() - 1], 1e6),
        'g' => (&s[..s.len() - 1], 1e9),
        _ => (s, 1.0),
    };
    let v: f64 = num.trim().parse().ok()?;
    (v > 0.0).then_some((v * mul) as u64)
}

pub fn parse_ratio(s: &str) -> Result<(u32, u32)> {
    let (a, b) = s.split_once([':', 'x', '/']).ok_or_else(|| bad("ratio", s))?;
    match (a.trim().parse::<u32>(), b.trim().parse::<u32>()) {
        (Ok(a), Ok(b)) if a > 0 && b > 0 => Ok((a, b)),
        _ => Err(bad("ratio", s)),
    }
}

/// `x,y,w,h` in pixels.
pub fn parse_rect(s: &str) -> Result<Rect> {
    let v: Vec<u32> = s
        .split(',')
        .map(|p| p.trim().parse::<u32>())
        .collect::<std::result::Result<_, _>>()
        .map_err(|_| bad("box", s))?;
    match v[..] {
        [x, y, w, h] if w > 0 && h > 0 => Ok(Rect { x, y, w, h }),
        _ => Err(bad("box", s)),
    }
}

pub fn parse_color(s: &str) -> Result<[u8; 3]> {
    let h = s.trim().trim_start_matches('#');
    let h = if h.len() == 3 { h.chars().flat_map(|c| [c, c]).collect::<String>() } else { h.to_string() };
    if h.len() != 6 {
        return Err(bad("color", s));
    }
    let p = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).map_err(|_| bad("color", s));
    Ok([p(0)?, p(2)?, p(4)?])
}

pub struct Param {
    pub key: &'static str,
    pub label: &'static str,
    pub label_ar: &'static str,
    pub default: &'static str,
}

pub struct ToolDef {
    pub id: &'static str,
    pub label: &'static str,
    pub label_ar: &'static str,
    pub cats: &'static [Category],
    /// Needs at least two files (merge, join, collage...).
    pub multi: bool,
    pub params: &'static [Param],
}

const fn p(key: &'static str, label: &'static str, label_ar: &'static str, default: &'static str) -> Param {
    Param { key, label, label_ar, default }
}

const ANY: &[Category] = &[Image, Video, Audio, Pdf, Document, Slides, Spreadsheet, Ebook, Subtitle, Archive];
const AV: &[Category] = &[Video, Audio];

pub const TOOLS: &[ToolDef] = &[
    ToolDef {
        id: "compress",
        label: "Compress",
        label_ar: "ضغط",
        cats: &[Image, Video, Audio, Pdf],
        multi: false,
        params: &[
            p("percent", "Reduce by %", "تقليل بنسبة %", "50"),
            p("target", "or target size (e.g. 5MB)", "أو حجم مستهدف (مثل 5MB)", ""),
            p("max", "Max side px (images)", "أقصى بُعد بالبكسل (صور)", ""),
        ],
    },
    ToolDef {
        id: "strip",
        label: "Remove metadata",
        label_ar: "إزالة البيانات الوصفية",
        cats: &[Image, Video, Audio, Pdf],
        multi: false,
        params: &[],
    },
    ToolDef {
        id: "tags",
        label: "Edit tags",
        label_ar: "تحرير الوسوم",
        cats: &[Video, Audio],
        multi: false,
        params: &[
            p("title", "Title", "العنوان", ""),
            p("artist", "Artist", "الفنان", ""),
            p("album", "Album", "الألبوم", ""),
            p("date", "Date", "التاريخ", ""),
            p("genre", "Genre", "النوع", ""),
            p("comment", "Comment", "تعليق", ""),
        ],
    },
    ToolDef {
        id: "watermark",
        label: "Watermark",
        label_ar: "علامة مائية",
        cats: &[Image, Video],
        multi: false,
        params: &[
            p("image", "Logo image", "صورة الشعار", ""),
            p("pos", "tl / tr / bl / br / center", "tl / tr / bl / br / center", "br"),
            p("opacity", "Opacity (0-1)", "الشفافية (0-1)", "0.8"),
            p("scale", "Width % of picture", "العرض % من الصورة", "20"),
        ],
    },
    ToolDef {
        id: "icon", label: "Make .ico", label_ar: "إنشاء أيقونة ico", cats: &[Image], multi: false, params: &[]
    },
    ToolDef {
        id: "motion",
        label: "Extract Motion Video",
        label_ar: "استخراج فيديو الصورة الحية",
        cats: &[Image],
        multi: false,
        params: &[],
    },
    ToolDef {
        id: "slideshow",
        label: "Slideshow video",
        label_ar: "فيديو عرض شرائح",
        cats: &[Image],
        multi: true,
        params: &[p("seconds", "Seconds per image", "ثوانٍ لكل صورة", "3")],
    },
    ToolDef {
        id: "fade",
        label: "Fade in/out",
        label_ar: "تلاشي دخول/خروج",
        cats: AV,
        multi: false,
        params: &[p("seconds", "Fade length (s)", "مدة التلاشي (ث)", "1")],
    },
    ToolDef { id: "reverse", label: "Reverse", label_ar: "عكس", cats: AV, multi: false, params: &[] },
    ToolDef {
        id: "loop",
        label: "Loop",
        label_ar: "تكرار",
        cats: AV,
        multi: false,
        params: &[p("count", "Times", "عدد المرات", "3")],
    },
    ToolDef {
        id: "stabilize",
        label: "Stabilize",
        label_ar: "تثبيت الاهتزاز",
        cats: &[Video],
        multi: false,
        params: &[],
    },
    ToolDef {
        id: "frames",
        label: "Export frames",
        label_ar: "تصدير الإطارات",
        cats: &[Video],
        multi: false,
        params: &[p("fps", "Frames per second", "إطارات في الثانية", "1")],
    },
    ToolDef {
        id: "transcribe",
        label: "Transcribe (subtitles)",
        label_ar: "تفريغ صوتي (ترجمة)",
        cats: AV,
        multi: false,
        params: &[p("lang", "Language (auto, ar, en…)", "اللغة (auto، ar، en…)", "auto")],
    },
    ToolDef {
        id: "extract-subs",
        label: "Extract subtitles",
        label_ar: "استخراج الترجمة",
        cats: &[Video],
        multi: false,
        params: &[],
    },
    ToolDef {
        id: "shift-subs",
        label: "Shift timing",
        label_ar: "إزاحة التوقيت",
        cats: &[Subtitle],
        multi: false,
        params: &[p("seconds", "Seconds (+ later, - earlier)", "ثوانٍ (+ تأخير، - تقديم)", "1")],
    },
    ToolDef {
        id: "add-subs",
        label: "Add subtitle track",
        label_ar: "إضافة مسار ترجمة",
        cats: &[],
        multi: true,
        params: &[],
    },
    ToolDef {
        id: "burn-subs",
        label: "Burn subtitles in",
        label_ar: "دمج الترجمة في الصورة",
        cats: &[],
        multi: true,
        params: &[],
    },
    ToolDef {
        id: "protect",
        label: "Password protect",
        label_ar: "حماية بكلمة مرور",
        cats: &[Pdf],
        multi: false,
        params: &[p("password", "Password", "كلمة المرور", "")],
    },
    ToolDef {
        id: "unlock",
        label: "Remove password",
        label_ar: "إزالة كلمة المرور",
        cats: &[Pdf],
        multi: false,
        params: &[p("password", "Current password", "كلمة المرور الحالية", "")],
    },
    ToolDef { id: "checksum", label: "SHA-256", label_ar: "بصمة SHA-256", cats: ANY, multi: false, params: &[] },
    ToolDef {
        id: "rename",
        label: "Batch rename",
        label_ar: "إعادة تسمية دفعية",
        cats: ANY,
        multi: false,
        params: &[
            p("pattern", "Pattern: {name} {n:3} {date} {ext}", "النمط: {name} {n:3} {date} {ext}", "{name}-{n:2}"),
            p("start", "Start number", "رقم البداية", "1"),
        ],
    },
    ToolDef {
        id: "recipe",
        label: "Recipe",
        label_ar: "وصفة",
        cats: ANY,
        multi: false,
        params: &[p(
            "steps",
            "Steps: resize max=1600 | compress percent=60",
            "الخطوات: resize max=1600 | compress percent=60",
            "",
        )],
    },
    ToolDef {
        id: "info",
        label: "Metadata",
        label_ar: "البيانات الوصفية",
        cats: &[Image, Video, Audio, Pdf],
        multi: false,
        params: &[],
    },
    ToolDef {
        id: "archive",
        label: "Archive",
        label_ar: "أرشفة",
        cats: ANY,
        multi: false,
        params: &[
            p("format", "zip / 7z / tar / tgz", "zip / 7z / tar / tgz", "zip"),
            p("password", "Password (zip/7z, AES-256)", "كلمة مرور (zip/7z، AES-256)", ""),
            p("split", "Split into parts of (e.g. 25MB)", "تقسيم إلى أجزاء بحجم (مثل 25MB)", ""),
        ],
    },
    ToolDef {
        id: "extract",
        label: "Extract",
        label_ar: "فك الأرشيف",
        cats: &[Archive],
        multi: false,
        params: &[p("password", "Password (if protected)", "كلمة المرور (إن كان محمياً)", "")],
    },
    ToolDef {
        id: "resize",
        label: "Resize",
        label_ar: "تغيير الحجم",
        cats: &[Image],
        multi: false,
        params: &[p("max", "Max side px", "أقصى بُعد بالبكسل", "1920"), p("scale", "or scale %", "أو نسبة %", "")],
    },
    ToolDef {
        id: "crop",
        label: "Crop",
        label_ar: "قص",
        cats: &[Image, Video],
        multi: false,
        params: &[
            p("ratio", "Aspect ratio", "نسبة الأبعاد", "1:1"),
            p("box", "or exact box x,y,w,h", "أو مربع دقيق x,y,w,h", ""),
        ],
    },
    ToolDef {
        id: "redact",
        label: "Redact",
        label_ar: "إخفاء مناطق",
        cats: &[Image, Video],
        multi: false,
        params: &[
            p("boxes", "Boxes x,y,w,h;…, text or faces", "المربعات x,y,w,h;… أو text أو faces", ""),
            p("style", "blur or color", "blur أو لون", "blur"),
            p("start", "From (video)", "من (فيديو)", ""),
            p("end", "To (video)", "إلى (فيديو)", ""),
        ],
    },
    ToolDef {
        id: "annotate", label: "Annotate", label_ar: "تعليق توضيحي", cats: &[Image], multi: false, params: &[]
    },
    ToolDef {
        id: "adjust",
        label: "Adjust colors",
        label_ar: "ضبط الألوان",
        cats: &[Image, Video],
        multi: false,
        params: &[
            p("brightness", "Brightness (-1..1)", "السطوع (-1..1)", "0"),
            p("contrast", "Contrast (0..3)", "التباين (0..3)", "1"),
            p("saturation", "Saturation (0..3)", "التشبع (0..3)", "1"),
            p("gamma", "Gamma", "غاما", "1"),
            p("hue", "Hue (°)", "تدرج اللون (°)", "0"),
            p("sharpen", "Sharpen (0..3)", "الحدة (0..3)", "0"),
            p("denoise", "Denoise (0..20)", "إزالة التشويش (0..20)", "0"),
            p("grain", "Grain (0..60)", "الحبيبات (0..60)", "0"),
            p("clarity", "Clarity (0..2)", "الوضوح (0..2)", "0"),
        ],
    },
    ToolDef {
        id: "rotate",
        label: "Rotate",
        label_ar: "تدوير",
        cats: &[Image, Video, Pdf],
        multi: false,
        params: &[p("degrees", "Degrees (90/180/270)", "الدرجات (90/180/270)", "90")],
    },
    ToolDef {
        id: "background",
        label: "Add background",
        label_ar: "إضافة خلفية",
        cats: &[Image],
        multi: false,
        params: &[p("color", "Color", "اللون", "#ffffff"), p("padding", "Padding px", "الهامش بالبكسل", "64")],
    },
    ToolDef {
        id: "grayscale",
        label: "Black & white",
        label_ar: "أبيض وأسود",
        cats: &[Image],
        multi: false,
        params: &[],
    },
    ToolDef {
        id: "images-to-pdf",
        label: "Create PDF",
        label_ar: "إنشاء PDF",
        cats: &[Image],
        multi: true,
        params: &[],
    },
    ToolDef {
        id: "collage",
        label: "Collage",
        label_ar: "كولاج",
        cats: &[Image],
        multi: true,
        params: &[p("across", "Columns", "الأعمدة", "3"), p("gap", "Gap px", "المسافة بالبكسل", "16")],
    },
    ToolDef {
        id: "ocr",
        label: "OCR (text)",
        label_ar: "استخراج النص OCR",
        cats: &[Image, Pdf],
        multi: false,
        params: &[p("to", "Output: pdf / txt", "الناتج: pdf / txt", "pdf"), p("lang", "Language", "اللغة", "ara+eng")],
    },
    ToolDef {
        id: "trim",
        label: "Trim",
        label_ar: "اقتطاع",
        cats: AV,
        multi: false,
        params: &[
            p("start", "Start (s or mm:ss)", "البداية (ث أو دد:ثث)", "0"),
            p("end", "End", "النهاية", ""),
            p("silence", "Cut edge silence (1/0)", "حذف الصمت في الأطراف (1/0)", "0"),
        ],
    },
    ToolDef {
        id: "speed",
        label: "Speed",
        label_ar: "السرعة",
        cats: AV,
        multi: false,
        params: &[p("factor", "Factor (0.25-4)", "المعامل (0.25-4)", "2")],
    },
    ToolDef {
        id: "snapshot",
        label: "Save frame",
        label_ar: "حفظ إطار",
        cats: &[Video],
        multi: false,
        params: &[p("at", "Time", "الوقت", "0")],
    },
    ToolDef {
        id: "extract-audio",
        label: "Extract audio",
        label_ar: "استخراج الصوت",
        cats: &[Video],
        multi: false,
        params: &[],
    },
    ToolDef { id: "mute", label: "Mute", label_ar: "كتم الصوت", cats: &[Video], multi: false, params: &[] },
    ToolDef {
        id: "split",
        label: "Split",
        label_ar: "تقسيم",
        cats: AV,
        multi: false,
        params: &[
            p("parts", "Equal parts", "أجزاء متساوية", "2"),
            p("at", "or cut at (e.g. 10,1:30)", "أو القص عند (مثل 10,1:30)", ""),
        ],
    },
    ToolDef { id: "join", label: "Join", label_ar: "دمج", cats: AV, multi: true, params: &[] },
    ToolDef {
        id: "normalize",
        label: "Normalize volume",
        label_ar: "توحيد مستوى الصوت",
        cats: AV,
        multi: false,
        params: &[],
    },
    ToolDef {
        id: "channels",
        label: "Channels",
        label_ar: "القنوات",
        cats: AV,
        multi: false,
        params: &[
            p("count", "1 = mono, 2 = stereo", "1 = أحادي، 2 = ستيريو", "1"),
            p("left", "Left volume (0-2)", "مستوى اليسار (0-2)", "1"),
            p("right", "Right volume (0-2)", "مستوى اليمين (0-2)", "1"),
        ],
    },
    ToolDef {
        id: "bleep",
        label: "Bleep",
        label_ar: "تصفير مقطع",
        cats: AV,
        multi: false,
        params: &[p("start", "Start", "البداية", ""), p("end", "End", "النهاية", "")],
    },
    ToolDef {
        id: "visualize",
        label: "Audio visualizer",
        label_ar: "فيديو موجة صوتية",
        cats: &[Audio],
        multi: false,
        params: &[p("shape", "landscape / portrait / square", "أفقي / عمودي / مربع", "landscape")],
    },
    ToolDef { id: "merge-pdf", label: "Merge PDFs", label_ar: "دمج PDF", cats: &[Pdf], multi: true, params: &[] },
    ToolDef {
        id: "split-pdf", label: "Split pages", label_ar: "فصل الصفحات", cats: &[Pdf], multi: false, params: &[]
    },
    ToolDef {
        id: "pages",
        label: "Organize pages",
        label_ar: "ترتيب الصفحات",
        cats: &[Pdf],
        multi: false,
        params: &[
            p("spec", "Pages, e.g. 3,1-2,z", "الصفحات، مثل 3,1-2,z", "1-z"),
            p("rotate", "Rotate, e.g. 1:90,3:180", "تدوير، مثل 1:90,3:180", ""),
        ],
    },
    ToolDef {
        id: "exif",
        label: "Edit EXIF / location",
        label_ar: "تحرير EXIF والموقع",
        cats: &[Image],
        multi: false,
        params: &[
            p("artist", "Artist", "المصوّر", ""),
            p("copyright", "Copyright", "حقوق النشر", ""),
            p("description", "Description", "الوصف", ""),
            p("date", "Date taken (YYYY:MM:DD HH:MM:SS)", "تاريخ الالتقاط (YYYY:MM:DD HH:MM:SS)", ""),
            p("make", "Camera make", "صانع الكاميرا", ""),
            p("model", "Camera model", "طراز الكاميرا", ""),
            p("gps", "Location lat,lon", "الموقع lat,lon", ""),
            p("remove-gps", "Remove location (1)", "حذف الموقع (1)", ""),
        ],
    },
    ToolDef {
        id: "qr-read",
        label: "Read QR / barcode",
        label_ar: "قراءة QR / باركود",
        cats: &[Image],
        multi: false,
        params: &[],
    },
    ToolDef {
        id: "social",
        label: "Social format",
        label_ar: "مقاس التواصل الاجتماعي",
        cats: &[Image, Video],
        multi: false,
        params: &[p("ratio", "9:16 / 1:1 / 4:5 / 16:9", "9:16 / 1:1 / 4:5 / 16:9", "9:16")],
    },
    ToolDef {
        id: "contact-sheet",
        label: "Contact sheet",
        label_ar: "لوحة مصغّرات",
        cats: &[Video],
        multi: false,
        params: &[],
    },
    ToolDef {
        id: "video-size",
        label: "Resolution",
        label_ar: "الدقة",
        cats: &[Video],
        multi: false,
        params: &[p("height", "2160 / 1440 / 1080 / 720 / 480 / 360", "2160 / 1440 / 1080 / 720 / 480 / 360", "720")],
    },
    ToolDef {
        id: "replace-audio",
        label: "Replace audio",
        label_ar: "استبدال الصوت",
        cats: &[],
        multi: true,
        params: &[],
    },
    ToolDef {
        id: "denoise-audio",
        label: "Reduce noise",
        label_ar: "تقليل الضوضاء",
        cats: AV,
        multi: false,
        params: &[],
    },
    ToolDef {
        id: "remove-silence",
        label: "Remove silences",
        label_ar: "حذف فترات الصمت",
        cats: &[Audio],
        multi: false,
        params: &[],
    },
    ToolDef {
        id: "web-set",
        label: "Responsive web images",
        label_ar: "صور ويب متجاوبة",
        cats: &[Image],
        multi: false,
        params: &[],
    },
    ToolDef {
        id: "duplicates",
        label: "Find duplicates",
        label_ar: "كشف المكررات",
        cats: ANY,
        multi: false,
        params: &[p("move", "Move copies to a Duplicates folder (1)", "نقل النسخ إلى مجلد Duplicates (1)", "")],
    },
    ToolDef {
        id: "dehaze",
        label: "Dehaze",
        label_ar: "إزالة الضباب",
        cats: &[Image],
        multi: false,
        params: &[p("strength", "Strength (0.05-1)", "القوة (0.05-1)", "0.8")],
    },
];

/// Tools the desktop app opens in its visual editor instead of an options form.
/// `trim` and `snapshot` use the frame-stepping timeline; `join` the clip-order list.
pub const EDITOR_TOOLS: [&str; 9] =
    ["crop", "redact", "annotate", "adjust", "pages", "merge-pdf", "trim", "snapshot", "join"];

/// Tools offered for a selection of `count` files sharing `cat` (`None` = mixed selection,
/// which gets the tools that work on any file).
pub fn tools_for(cat: Option<Category>, count: usize) -> Vec<&'static ToolDef> {
    TOOLS
        .iter()
        .filter(|t| match cat {
            Some(c) => t.cats.contains(&c) && (!t.multi || count >= 2),
            None => t.cats.len() == ANY.len(),
        })
        .collect()
}

/// Like `tools_for`, plus combinations of different kinds (a video and its subtitle file).
pub fn tools_for_paths<P: AsRef<std::path::Path>>(paths: &[P]) -> Vec<&'static ToolDef> {
    let cats: Vec<Option<Category>> = paths.iter().map(|p| crate::format::category_of(p.as_ref())).collect();
    let mut tools = tools_for(crate::format::common_category(paths), paths.len());
    let count = |c: Category| cats.iter().filter(|x| **x == Some(c)).count();
    if paths.len() == 2 && count(Video) == 1 && count(Subtitle) == 1 {
        tools.extend(TOOLS.iter().filter(|t| t.id == "add-subs" || t.id == "burn-subs"));
    }
    if paths.len() == 2 && count(Video) == 1 && count(Audio) == 1 {
        tools.extend(TOOLS.iter().filter(|t| t.id == "replace-audio"));
    }
    tools
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts(kv: &[(&str, &str)]) -> HashMap<String, String> {
        kv.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn parsers() {
        assert_eq!(parse_time("1:30"), Some(90.0));
        assert_eq!(parse_time("01:00:02.5"), Some(3602.5));
        assert_eq!(parse_time("x"), None);
        assert_eq!(parse_size("5MB"), Some(5_000_000));
        assert_eq!(parse_size("1.5g"), Some(1_500_000_000));
        assert_eq!(parse_size("0"), None);
        assert_eq!(parse_ratio("16:9").unwrap(), (16, 9));
        assert!(parse_ratio("0:9").is_err());
        assert_eq!(parse_color("#f80").unwrap(), [255, 136, 0]);
    }

    #[test]
    fn op_parsing_validates() {
        assert_eq!(
            Op::parse("compress", &opts(&[("target", "2MB")])).unwrap(),
            Op::Compress { how: Compression::TargetBytes(2_000_000), max: None }
        );
        assert!(Op::parse("compress", &opts(&[("percent", "99")])).is_err());
        assert!(Op::parse("pages", &opts(&[("spec", "1;rm -rf")])).is_err());
        assert_eq!(
            Op::parse("pages", &opts(&[("spec", "3, 1-2, z")])).unwrap(),
            Op::Pages { spec: "3,1-2,z".into(), rotate: vec![] }
        );
        assert_eq!(
            Op::parse("pages", &opts(&[("spec", "2,1"), ("rotate", "1:90, 2:270")])).unwrap(),
            Op::Pages { spec: "2,1".into(), rotate: vec![(1, 90), (2, 270)] }
        );
        assert!(Op::parse("pages", &opts(&[("rotate", "1:45")])).is_err());
        assert!(Op::parse("adjust", &opts(&[("contrast", "1")])).is_err(), "neutral adjust refused");
        assert!(
            matches!(Op::parse("adjust", &opts(&[("saturation", "1.5")])), Ok(Op::Adjust(a)) if a.saturation == 1.5)
        );
        assert!(Op::parse("adjust", &opts(&[("brightness", "2")])).is_err());
        assert_eq!(
            Op::parse("tags", &opts(&[("title", "مرحبا"), ("bogus", "x")])).unwrap(),
            Op::SetTags(vec![("title".into(), "مرحبا".into())])
        );
        assert!(Op::parse("trim", &opts(&[("start", "10"), ("end", "5")])).is_err());
        assert!(Op::parse("ocr", &opts(&[("lang", "eng;x")])).is_err());
        assert!(Op::parse("nope", &opts(&[])).is_err());
        assert_eq!(
            Op::parse("redact", &opts(&[("boxes", "1,2,30,40; 5,5,10,10"), ("style", "#000"), ("start", "2")]))
                .unwrap(),
            Op::Redact {
                boxes: vec![Rect { x: 1, y: 2, w: 30, h: 40 }, Rect { x: 5, y: 5, w: 10, h: 10 }],
                fill: Some([0, 0, 0]),
                range: Some((2.0, f64::MAX)),
                auto: None,
            }
        );
        assert!(Op::parse("redact", &opts(&[("boxes", "1,2,0,4")])).is_err());
        assert!(
            matches!(Op::parse("redact", &opts(&[("boxes", "auto"), ("lang", "ara+eng")])), Ok(Op::Redact { auto: Some(l), .. }) if l == "ara+eng")
        );
        assert_eq!(
            Op::parse("crop", &opts(&[("box", "10,20,300,200")])).unwrap(),
            Op::Crop(CropSpec::Box(Rect { x: 10, y: 20, w: 300, h: 200 }))
        );
        assert_eq!(
            Op::parse("split", &opts(&[("at", "1:30, 10,10")])).unwrap(),
            Op::Split { parts: 2, at: vec![10.0, 90.0] }
        );
        assert!(Op::parse("channels", &opts(&[("left", "3")])).is_err());
    }

    #[test]
    fn every_tool_id_parses_with_defaults() {
        for t in TOOLS {
            let o: HashMap<String, String> =
                t.params.iter().map(|p| (p.key.to_string(), p.default.to_string())).collect();
            let r = Op::parse(t.id, &o);
            // no sensible defaults: bleep needs a range, redact needs boxes, annotate is app-only
            // ...adjust/tags defaults change nothing (refused on purpose); some need user input
            let needs_input =
                ["bleep", "redact", "annotate", "adjust", "tags", "watermark", "protect", "unlock", "recipe", "exif"];
            assert!(r.is_ok() || needs_input.contains(&t.id), "{} -> {:?}", t.id, r.err());
        }
    }

    #[test]
    fn recipes_and_mixed_selections() {
        let r = parse_recipe("resize max=1600 | compress percent=60 | strip").unwrap();
        assert_eq!(r.len(), 3);
        assert_eq!(r[0], Op::Resize(Size::Max(1600)));
        assert_eq!(
            parse_recipe("convert to=WEBP | strip").unwrap()[0],
            Op::Convert { to: "webp".into(), quality: None }
        );
        assert_eq!(
            parse_step("convert to=webp quality=85").unwrap(),
            Op::Convert { to: "webp".into(), quality: Some(85) }
        );
        assert!(parse_step("convert to=webp quality=0").is_err());
        assert!(parse_step("convert to=webp quality=101").is_err());
        assert_eq!(Op::parse("strip-exif", &HashMap::new()).unwrap(), Op::StripMetadata);
        assert_eq!(Op::parse("strip-metadata", &HashMap::new()).unwrap(), Op::StripMetadata);
        assert!(parse_recipe("strip").is_err(), "one step is not a recipe");
        assert!(parse_recipe("merge-pdf | strip").is_err(), "multi-file tools are not steps");
        assert!(parse_recipe("resize max | strip").is_err());
        let ids = |p: &[&str]| tools_for_paths(p).iter().map(|t| t.id).collect::<Vec<_>>();
        assert!(ids(&["a.mp4", "a.srt"]).contains(&"burn-subs"));
        assert!(!ids(&["a.mp4", "b.mp4"]).contains(&"burn-subs"));
        let mixed = ids(&["a.png", "b.pdf"]);
        assert!(mixed.contains(&"archive") && mixed.contains(&"checksum") && mixed.contains(&"rename"));
        assert_eq!(Op::parse("shift-subs", &opts(&[("seconds", "-1.25")])).unwrap(), Op::ShiftSubs { ms: -1250 });
        assert!(Op::parse("watermark", &opts(&[("image", "l.png"), ("pos", "middle")])).is_err());
        assert!(Op::parse("transcribe", &opts(&[("lang", "ar;rm")])).is_err());
        assert_eq!(
            Op::parse("exif", &opts(&[("artist", "Zeiad"), ("gps", "24.7, 46.7")])).unwrap(),
            Op::Exif { tags: vec![("Artist".into(), "Zeiad".into())], gps: Some((24.7, 46.7)), remove_gps: false }
        );
        assert!(Op::parse("exif", &opts(&[("gps", "100,0")])).is_err());
        assert!(Op::parse("archive", &opts(&[("format", "tgz"), ("password", "x")])).is_err());
        assert!(matches!(
            Op::parse("archive", &opts(&[("split", "25MB")])),
            Ok(Op::Archive { split: Some(25_000_000), .. })
        ));
        assert!(Op::parse("social", &opts(&[("ratio", "3:2")])).is_err());
        assert!(ids(&["a.mp4", "a.m4a"]).contains(&"replace-audio"));
    }

    #[test]
    fn multi_tools_need_two_files() {
        let one: Vec<_> = tools_for(Some(Pdf), 1).iter().map(|t| t.id).collect();
        let two: Vec<_> = tools_for(Some(Pdf), 2).iter().map(|t| t.id).collect();
        assert!(!one.contains(&"merge-pdf") && two.contains(&"merge-pdf"));
        assert_eq!(
            tools_for(None, 3).iter().map(|t| t.id).collect::<Vec<_>>(),
            vec!["checksum", "rename", "recipe", "archive", "duplicates"]
        );
    }
}
