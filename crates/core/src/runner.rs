//! Executes plans: probes inputs, runs engines, reports progress, places results beside originals.

use crate::format::{category_of, stem_of, Category};
use crate::ops::{Op, Rect};
use crate::plan::{self, Ctx, ImageInfo, MediaInfo, Step};
use crate::tools::{self, Tool};
use crate::{pdf, subtitle, Error, Result};
use std::ffi::OsString;
use std::fs;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;

#[derive(Debug)]
pub enum Event {
    Start { index: usize, total: usize, file: String },
    Progress(f32),
    Done(PathBuf),
    Failed { file: String, error: String },
}

/// Runs `op` on `inputs`: one output per file, or one for the whole selection for multi ops.
pub fn run(op: &Op, inputs: &[PathBuf], mut on: impl FnMut(Event)) -> Vec<Result<PathBuf>> {
    let inputs: Vec<PathBuf> = inputs.iter().map(|p| std::path::absolute(p).unwrap_or_else(|_| p.clone())).collect();
    let units: Vec<Vec<PathBuf>> =
        if op.is_multi() { vec![inputs] } else { inputs.into_iter().map(|p| vec![p]).collect() };
    let total = units.len();
    let mut results = Vec::with_capacity(total);
    for (index, unit) in units.iter().enumerate() {
        let file = unit[0].file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        on(Event::Start { index, total, file: file.clone() });
        let r = run_unit(op, unit, &mut on);
        match &r {
            Ok(p) => on(Event::Done(p.clone())),
            Err(e) => on(Event::Failed { file, error: e.to_string() }),
        }
        results.push(r);
    }
    results
}

static JOB: AtomicUsize = AtomicUsize::new(0);

fn run_unit(op: &Op, inputs: &[PathBuf], on: &mut dyn FnMut(Event)) -> Result<PathBuf> {
    let src = &inputs[0];
    if !src.exists() {
        return Err(Error::BadOption(format!("{} does not exist", src.display())));
    }
    let dest = src.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."));
    // Scratch dir beside the output so the final rename never crosses filesystems.
    let tmp = dest.join(format!(".zeidex-{}-{}", std::process::id(), JOB.fetch_add(1, Ordering::Relaxed)));
    fs::create_dir_all(&tmp)?;
    let result = (|| {
        let ctx = probe(op, inputs);
        let plan = plan::plan(op, inputs, &ctx, &tmp)?;
        let n = plan.steps.len().max(1) as f32;
        for (i, step) in plan.steps.iter().enumerate() {
            exec(step, &mut |f| on(Event::Progress((i as f32 + f) / n)))?;
        }
        if !plan.output.exists() {
            return Err(Error::Failed { tool: "engine".into(), detail: "no output file was produced".into() });
        }
        let fin = unique_path(&dest.join(&plan.name));
        fs::rename(&plan.output, &fin)?;
        Ok(fin)
    })();
    let _ = fs::remove_dir_all(&tmp);
    result
}

fn exec(step: &Step, report: &mut dyn FnMut(f32)) -> Result<()> {
    match step {
        Step::Run { tool, args, cwd, progress_secs } => run_tool(*tool, args, cwd.as_deref(), *progress_secs, report),
        Step::MkDir(p) => Ok(fs::create_dir_all(p)?),
        Step::Copy { src, dst } => Ok(fs::copy(src, dst).map(|_| ())?),
        Step::WriteFile { path, contents } => Ok(fs::write(path, contents)?),
        Step::Subtitle { src, dst } => {
            let text = String::from_utf8_lossy(&fs::read(src)?).into_owned();
            let to = dst.extension().and_then(|e| e.to_str()).unwrap_or("srt");
            Ok(fs::write(dst, subtitle::write(&subtitle::parse(&text), to))?)
        }
        Step::JpegsToPdf { jpegs, dst } => {
            let data = jpegs.iter().map(fs::read).collect::<std::io::Result<Vec<_>>>()?;
            let mut out = fs::File::create(dst)?;
            Ok(pdf::jpegs_to_pdf(&data, &mut out)?)
        }
        Step::SmallestUnder { target, candidates, dst } => {
            let mut best: Option<(u64, &PathBuf)> = None;
            for (steps, out) in candidates {
                for st in steps {
                    exec(st, &mut |_| {})?;
                }
                let size = fs::metadata(out)?.len();
                if best.map_or(true, |(b, _)| size < b) {
                    best = Some((size, out));
                }
                if size <= *target {
                    break;
                }
            }
            let (_, out) = best.ok_or_else(|| Error::Unsupported("no candidates".into()))?;
            Ok(fs::rename(out, dst)?)
        }
    }
}

fn command(bin: &Path) -> Command {
    #[allow(unused_mut)]
    let mut cmd = Command::new(bin);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

fn run_tool(
    tool: Tool, args: &[OsString], cwd: Option<&Path>, progress_secs: Option<f64>, report: &mut dyn FnMut(f32),
) -> Result<()> {
    let bin = tools::require(tool)?;
    let mut cmd = command(&bin);
    cmd.args(args).stdin(Stdio::null()).stderr(Stdio::piped());
    cmd.stdout(if progress_secs.is_some() { Stdio::piped() } else { Stdio::null() });
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    let mut child = cmd.spawn().map_err(|e| Error::Failed { tool: tool.label().into(), detail: e.to_string() })?;
    let mut stderr = child.stderr.take().expect("piped");
    let err_reader = thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stderr.read_to_end(&mut buf);
        String::from_utf8_lossy(&buf).into_owned()
    });
    if let (Some(total), Some(out)) = (progress_secs.filter(|t| *t > 0.0), child.stdout.take()) {
        for line in BufReader::new(out).lines().map_while(std::result::Result::ok) {
            let v = line.strip_prefix("out_time_us=").or_else(|| line.strip_prefix("out_time_ms="));
            if let Some(us) = v.and_then(|v| v.trim().parse::<f64>().ok()) {
                report((us / 1e6 / total).clamp(0.0, 1.0) as f32);
            }
        }
    }
    let status = child.wait()?;
    let err = err_reader.join().unwrap_or_default();
    if status.success() {
        Ok(())
    } else {
        let tail: String = err.trim().chars().rev().take(800).collect::<Vec<_>>().into_iter().rev().collect();
        Err(Error::Failed { tool: tool.label().into(), detail: format!("exit {:?}: {tail}", status.code()) })
    }
}

/// Runs a tool and returns its stdout.
pub fn capture(tool: Tool, args: &[&str], path: &Path) -> Result<String> {
    let out = command(&tools::require(tool)?).args(args).arg(path).stdin(Stdio::null()).output()?;
    if !out.status.success() {
        return Err(Error::Failed { tool: tool.label().into(), detail: String::from_utf8_lossy(&out.stderr).trim().into() });
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn probe(op: &Op, inputs: &[PathBuf]) -> Ctx {
    let src = &inputs[0];
    let size = fs::metadata(src).map(|m| m.len()).unwrap_or(0);
    let mut ctx = Ctx { size, ..Ctx::default() };
    match category_of(src) {
        Some(Category::Video | Category::Audio) => {
            ctx.media = probe_media(src);
            if matches!(op, Op::Join) {
                ctx.inputs = inputs.iter().map_while(|p| probe_media(p)).collect();
            }
        }
        Some(Category::Image) if matches!(op, Op::Crop(_) | Op::Background { .. }) => ctx.image = probe_image(src),
        // Full-size render (the preview never upscales), so OCR boxes are already in source pixels.
        Some(Category::Image) => {
            if let Op::Redact { auto: Some(lang), .. } = op {
                ctx.detected = preview_png(src, u32::MAX).and_then(|png| detect_text_png(&png, lang)).unwrap_or_default();
            }
        }
        _ => {}
    }
    ctx
}

pub fn probe_media(src: &Path) -> Option<MediaInfo> {
    let out = capture(
        Tool::Ffprobe,
        &["-v", "error", "-show_entries", "format=duration:stream=codec_type,width,height,avg_frame_rate", "-of", "default=noprint_wrappers=1"],
        src,
    )
    .ok()?;
    Some(parse_ffprobe(&out))
}

fn parse_ffprobe(out: &str) -> MediaInfo {
    let mut m = MediaInfo::default();
    // Stream fields follow their codec_type line; only the first video stream sets the canvas.
    let mut in_video = false;
    for line in out.lines() {
        match line.trim().split_once('=') {
            Some(("duration", v)) => m.duration = v.parse().unwrap_or(m.duration),
            Some(("codec_type", t)) => {
                in_video = t == "video" && !m.has_video;
                m.has_video |= t == "video";
                m.has_audio |= t == "audio";
            }
            Some(("width", v)) if in_video => m.width = v.parse().unwrap_or(0),
            Some(("height", v)) if in_video => m.height = v.parse().unwrap_or(0),
            Some(("avg_frame_rate", v)) if in_video => {
                if let Some((n, d)) = v.split_once('/') {
                    if let (Ok(n), Ok(d)) = (n.parse::<f64>(), d.parse::<f64>()) {
                        m.fps = if d > 0.0 { n / d } else { 0.0 };
                    }
                }
            }
            _ => {}
        }
    }
    m
}

pub fn probe_image(src: &Path) -> Option<ImageInfo> {
    let field = |f: &str| capture(Tool::VipsHeader, &["-f", f], src).ok()?.trim().parse::<u32>().ok();
    Some(ImageInfo { width: field("width")?, height: field("height")?, bands: field("bands")? })
}

/// Pixel size of an image or video (libvips first, FFprobe as fallback).
pub fn dimensions(src: &Path) -> Option<(u32, u32)> {
    if category_of(src) == Some(Category::Image) {
        if let Some(i) = probe_image(src) {
            return Some((i.width, i.height));
        }
    }
    probe_media(src).filter(|m| m.width > 0).map(|m| (m.width, m.height))
}

/// PNG preview no larger than `max` px: the image itself, or a frame from a video.
pub fn preview_png(src: &Path, max: u32) -> Result<Vec<u8>> {
    let out = std::env::temp_dir().join(format!("zeidex-preview-{}-{}.png", std::process::id(), JOB.fetch_add(1, Ordering::Relaxed)));
    let m = max.to_string();
    let via_ffmpeg = |seek: &str| -> Result<()> {
        let scale = format!("scale='min({max},iw)':'min({max},ih)':force_original_aspect_ratio=decrease");
        let args: Vec<OsString> = ["-hide_banner", "-nostdin", "-y", "-loglevel", "error", "-ss", seek, "-i"]
            .iter()
            .map(OsString::from)
            .chain([src.as_os_str().to_owned()])
            .chain(["-frames:v", "1", "-vf", &scale].iter().map(OsString::from))
            .chain([out.as_os_str().to_owned()])
            .collect();
        run_tool(Tool::Ffmpeg, &args, None, None, &mut |_| {})
    };
    let made = match category_of(src) {
        Some(Category::Video) => {
            let at = probe_media(src).map_or(0.0, |m| (m.duration / 3.0).min(5.0));
            via_ffmpeg(&format!("{at:.2}"))
        }
        Some(Category::Image) if tools::locate(Tool::Vips).is_some() && crate::format::ext_of(src).as_deref() != Some("bmp") => {
            let args: Vec<OsString> = vec!["thumbnail".into(), src.as_os_str().to_owned(), out.as_os_str().to_owned(), m.clone().into(), "--height".into(), m.into(), "--size".into(), "down".into()];
            run_tool(Tool::Vips, &args, None, None, &mut |_| {})
        }
        Some(Category::Image) => via_ffmpeg("0"),
        _ => Err(Error::Unsupported("no preview for this file type".into())),
    };
    let data = made.and_then(|_| Ok(fs::read(&out)?));
    let _ = fs::remove_file(&out);
    data
}

fn scratch_file(tag: &str, ext: &str) -> PathBuf {
    std::env::temp_dir().join(format!("zeidex-{tag}-{}-{}.{ext}", std::process::id(), JOB.fetch_add(1, Ordering::Relaxed)))
}

/// Text lines found by Tesseract in a PNG, as boxes in that PNG's pixels (for auto-redact).
pub fn detect_text_png(png: &[u8], lang: &str) -> Result<Vec<Rect>> {
    if !lang.chars().all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '_') {
        return Err(Error::BadOption(format!("lang={lang}")));
    }
    let input = scratch_file("ocr", "png");
    fs::write(&input, png)?;
    let out = command(&tools::require(Tool::Tesseract)?)
        .arg(&input)
        .args(["stdout", "-l", lang, "tsv"])
        .stdin(Stdio::null())
        .output();
    let _ = fs::remove_file(&input);
    let out = out?;
    if !out.status.success() {
        return Err(Error::Failed { tool: "Tesseract".into(), detail: String::from_utf8_lossy(&out.stderr).trim().into() });
    }
    Ok(parse_tsv(&String::from_utf8_lossy(&out.stdout)))
}

/// Merges confident words of each Tesseract line into one padded box.
fn parse_tsv(tsv: &str) -> Vec<Rect> {
    /// Minimum word confidence (0-100) worth hiding.
    const MIN_CONF: f64 = 40.0;
    const PAD: u32 = 4;
    // (block, paragraph, line) -> bounding box as (x0, y0, x1, y1)
    type LineKey = (u32, u32, u32);
    type Bounds = (u32, u32, u32, u32);
    let mut lines: Vec<(LineKey, Bounds)> = Vec::new();
    for row in tsv.lines().skip(1) {
        let c: Vec<&str> = row.split('\t').collect();
        if c.len() < 12 || c[0] != "5" || c[11].trim().is_empty() {
            continue;
        }
        let n = |i: usize| c[i].parse::<u32>().unwrap_or(0);
        if c[10].parse::<f64>().unwrap_or(-1.0) < MIN_CONF {
            continue;
        }
        let key = (n(2), n(3), n(4));
        let (x0, y0, x1, y1) = (n(6), n(7), n(6) + n(8), n(7) + n(9));
        match lines.iter_mut().find(|(k, _)| *k == key) {
            Some((_, b)) => *b = (b.0.min(x0), b.1.min(y0), b.2.max(x1), b.3.max(y1)),
            None => lines.push((key, (x0, y0, x1, y1))),
        }
    }
    lines
        .into_iter()
        .map(|(_, (x0, y0, x1, y1))| {
            let (x, y) = (x0.saturating_sub(PAD), y0.saturating_sub(PAD));
            Rect { x, y, w: x1 + PAD - x, h: y1 + PAD - y }
        })
        .collect()
}

/// Small PNG per PDF page for the page organizer (Ghostscript at `dpi`).
pub fn pdf_thumbnails(src: &Path, dpi: u32) -> Result<Vec<Vec<u8>>> {
    let dir = scratch_file("thumbs", "d");
    fs::create_dir_all(&dir)?;
    let mut out_arg = OsString::from("-sOutputFile=");
    out_arg.push(dir.join("p-%04d.png").as_os_str());
    let args: Vec<OsString> = vec![
        "-dSAFER".into(), "-dBATCH".into(), "-dNOPAUSE".into(), "-dQUIET".into(), "-sDEVICE=png16m".into(),
        format!("-r{dpi}").into(), out_arg, src.as_os_str().to_owned(),
    ];
    let result = run_tool(Tool::Ghostscript, &args, None, None, &mut |_| {}).and_then(|_| {
        let mut files: Vec<PathBuf> = fs::read_dir(&dir)?.flatten().map(|e| e.path()).collect();
        files.sort();
        Ok(files.iter().map(fs::read).collect::<std::io::Result<Vec<_>>>()?)
    });
    let _ = fs::remove_dir_all(&dir);
    result
}

/// Human-readable metadata dump for the Info tool.
pub fn info(src: &Path) -> Result<String> {
    match category_of(src) {
        Some(Category::Image) => capture(Tool::VipsHeader, &["-a"], src),
        Some(Category::Video | Category::Audio) => capture(Tool::Ffprobe, &["-hide_banner", "-show_format", "-show_streams"], src),
        Some(Category::Pdf) => {
            let pages = capture(Tool::Qpdf, &["--show-npages"], src)?;
            let check = capture(Tool::Qpdf, &["--check"], src).unwrap_or_default();
            Ok(format!("pages: {}\n{check}", pages.trim()))
        }
        _ => Err(Error::Unsupported("no metadata reader for this file type".into())),
    }
}

/// `name.ext`, then `name (2).ext`, `name (3).ext`... until free.
pub fn unique_path(p: &Path) -> PathBuf {
    if !p.exists() {
        return p.to_path_buf();
    }
    let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let stem = stem_of(p);
    let rest = &name[stem.len()..];
    let parent = p.parent().unwrap_or(Path::new("."));
    (2..)
        .map(|n| parent.join(format!("{stem} ({n}){rest}")))
        .find(|c| !c.exists())
        .expect("unbounded range")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("zeidex-test-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn tesseract_tsv_lines() {
        let tsv = "level\tpage_num\tblock_num\tpar_num\tline_num\tword_num\tleft\ttop\twidth\theight\tconf\ttext\n\
                   4\t1\t1\t1\t1\t0\t10\t20\t200\t30\t-1\t\n\
                   5\t1\t1\t1\t1\t1\t10\t20\t50\t30\t95.1\tHello\n\
                   5\t1\t1\t1\t1\t2\t70\t22\t60\t26\t90\tworld\n\
                   5\t1\t1\t1\t2\t1\t10\t80\t40\t20\t12\tnoise\n\
                   5\t1\t2\t1\t1\t1\t2\t2\t30\t10\t88\tTop\n";
        assert_eq!(
            parse_tsv(tsv),
            vec![Rect { x: 6, y: 16, w: 128, h: 38 }, Rect { x: 0, y: 0, w: 36, h: 16 }]
        );
    }

    /// Needs Ghostscript (and Tesseract for the OCR half); skipped when they are missing.
    #[test]
    fn thumbnails_and_text_detection_with_real_engines() {
        let Some(gs) = tools::locate(Tool::Ghostscript) else { return };
        let d = scratch("engines");
        let ps = d.join("t.ps");
        fs::write(&ps, "%!PS\n/Helvetica findfont 40 scalefont setfont\n60 700 moveto (Hello Zeidex 2026) show\nshowpage\nshowpage\n").unwrap();
        let pdf = d.join("t.pdf");
        let made = Command::new(&gs)
            .args(["-q", "-dSAFER", "-dBATCH", "-dNOPAUSE", "-sDEVICE=pdfwrite"])
            .arg(format!("-sOutputFile={}", pdf.display()))
            .arg(&ps)
            .status()
            .unwrap();
        assert!(made.success());
        let thumbs = pdf_thumbnails(&pdf, 20).unwrap();
        assert_eq!(thumbs.len(), 2);
        assert!(thumbs.iter().all(|t| t.starts_with(b"\x89PNG")));
        if tools::locate(Tool::Tesseract).is_some() {
            let page = pdf_thumbnails(&pdf, 72).unwrap().remove(0);
            let boxes = detect_text_png(&page, "eng").unwrap();
            assert_eq!(boxes.len(), 1, "{boxes:?}");
            assert!(boxes[0].w > 200 && boxes[0].y < 150, "{boxes:?}");
        }
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn unique_names() {
        let d = scratch("unique");
        let a = d.join("a.tar.gz");
        assert_eq!(unique_path(&a), a);
        fs::write(&a, "").unwrap();
        assert_eq!(unique_path(&a), d.join("a (2).tar.gz"));
        fs::write(d.join("a (2).tar.gz"), "").unwrap();
        assert_eq!(unique_path(&a), d.join("a (3).tar.gz"));
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn ffprobe_output() {
        let m = parse_ffprobe("codec_type=video\nwidth=640\nheight=360\navg_frame_rate=30000/1001\ncodec_type=audio\nwidth=N/A\nduration=12.5\n");
        assert!(m.has_video && m.has_audio && m.duration == 12.5);
        assert_eq!((m.width, m.height), (640, 360));
        assert!((m.fps - 29.97).abs() < 0.01);
        assert!(!parse_ffprobe("duration=N/A").has_audio);
    }

    /// End to end without engines: subtitle conversion is built in.
    #[test]
    fn subtitle_job_writes_beside_original_and_cleans_up() {
        let d = scratch("sub");
        let src = d.join("talk.srt");
        fs::write(&src, "1\n00:00:01,000 --> 00:00:02,000\nHi\n").unwrap();
        fs::write(d.join("talk.vtt"), "taken").unwrap();
        let mut events = Vec::new();
        let r = run(&Op::Convert { to: "vtt".into() }, std::slice::from_ref(&src), |e| events.push(format!("{e:?}")));
        let out = r[0].as_ref().unwrap();
        assert_eq!(out, &d.join("talk (2).vtt"));
        assert!(fs::read_to_string(out).unwrap().starts_with("WEBVTT"));
        assert!(fs::read_to_string(&src).unwrap().contains("00:00:01,000"), "original untouched");
        let leftovers: Vec<_> = fs::read_dir(&d).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().starts_with(".zeidex")).collect();
        assert!(leftovers.is_empty());
        assert!(events.first().unwrap().starts_with("Start") && events.last().unwrap().starts_with("Done"));
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn missing_input_fails_cleanly() {
        let r = run(&Op::Convert { to: "vtt".into() }, &[PathBuf::from("/nope/x.srt")], |_| {});
        assert!(r[0].is_err());
    }
}
