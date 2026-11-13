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
use std::sync::Mutex;
use std::thread;

/// Progress of one unit (`index`) of a job. Units can run in parallel, so events of
/// different units interleave.
#[derive(Debug)]
pub enum Event {
    Start { index: usize, total: usize, file: String },
    Progress { index: usize, fraction: f32 },
    Done { index: usize, output: PathBuf },
    Failed { index: usize, file: String, error: String },
}

/// Default number of files processed at once. Engines are multithreaded themselves, so a
/// few parallel units are enough to keep every core busy on batches of small files.
pub fn default_parallelism() -> usize {
    std::thread::available_parallelism().map_or(1, |n| n.get()).div_ceil(2).clamp(1, 4)
}

/// Runs `op` on `inputs`: one output per file, or one for the whole selection for multi ops.
pub fn run(op: &Op, inputs: &[PathBuf], on: impl FnMut(Event) + Send) -> Vec<Result<PathBuf>> {
    run_with(op, inputs, default_parallelism(), on)
}

/// `run` with an explicit number of files processed at the same time.
pub fn run_with(op: &Op, inputs: &[PathBuf], parallel: usize, on: impl FnMut(Event) + Send) -> Vec<Result<PathBuf>> {
    let inputs: Vec<PathBuf> = inputs.iter().map(|p| std::path::absolute(p).unwrap_or_else(|_| p.clone())).collect();
    let units: Vec<Vec<PathBuf>> =
        if op.is_multi() { vec![inputs] } else { inputs.into_iter().map(|p| vec![p]).collect() };
    let total = units.len();
    let on = Mutex::new(on);
    let emit = |e: Event| (on.lock().unwrap_or_else(|p| p.into_inner()))(e);
    let next = AtomicUsize::new(0);
    let results: Vec<Mutex<Option<Result<PathBuf>>>> = (0..total).map(|_| Mutex::new(None)).collect();
    std::thread::scope(|scope| {
        for _ in 0..parallel.clamp(1, total.max(1)) {
            scope.spawn(|| loop {
                let index = next.fetch_add(1, Ordering::Relaxed);
                let Some(unit) = units.get(index) else { break };
                let file = unit[0].file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                emit(Event::Start { index, total, file: file.clone() });
                let r = run_unit(op, unit, &mut |fraction| emit(Event::Progress { index, fraction }));
                match &r {
                    Ok(p) => emit(Event::Done { index, output: p.clone() }),
                    Err(e) => emit(Event::Failed { index, file, error: e.to_string() }),
                }
                *results[index].lock().unwrap_or_else(|p| p.into_inner()) = Some(r);
            });
        }
    });
    results
        .into_iter()
        .map(|m| {
            m.into_inner()
                .unwrap_or_else(|p| p.into_inner())
                .unwrap_or_else(|| Err(Error::Unsupported("not run".into())))
        })
        .collect()
}

static JOB: AtomicUsize = AtomicUsize::new(0);
/// Serialises "pick a free name + rename" so parallel units never claim the same name.
static PLACE: Mutex<()> = Mutex::new(());

fn run_unit(op: &Op, inputs: &[PathBuf], progress: &mut dyn FnMut(f32)) -> Result<PathBuf> {
    let src = &inputs[0];
    if !src.exists() {
        return Err(Error::BadOption(format!("{} does not exist", src.display())));
    }
    match op {
        Op::Checksum => return checksum(src),
        Op::Rename { .. } | Op::Duplicates { .. } | Op::QrRead => {
            return Err(Error::Unsupported("this tool reports text: use runner::text_tool".into()))
        }
        _ => {}
    }
    let dest = src.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."));
    // Scratch dir beside the output so the final rename never crosses filesystems.
    let tmp = dest.join(format!(".zeidex-{}-{}", std::process::id(), JOB.fetch_add(1, Ordering::Relaxed)));
    fs::create_dir_all(&tmp)?;
    let result = (|| {
        let steps: Vec<&Op> = match op {
            Op::Recipe(steps) => steps.iter().collect(),
            single => vec![single],
        };
        let mut input = inputs.to_vec();
        let mut last = None;
        for (k, step) in steps.iter().enumerate() {
            let stage = tmp.join(format!("stage{k}"));
            fs::create_dir_all(&stage)?;
            let ctx = probe(step, &input);
            let plan = plan::plan(step, &input, &ctx, &stage)?;
            let n = (plan.steps.len().max(1) * steps.len()) as f32;
            let base = (k * plan.steps.len().max(1)) as f32;
            for (i, s) in plan.steps.iter().enumerate() {
                exec(s, &mut |f| progress((base + i as f32 + f) / n))?;
            }
            if !plan.output.exists() {
                return Err(Error::Failed { tool: "engine".into(), detail: "no output file was produced".into() });
            }
            if k + 1 < steps.len() {
                if plan.output.is_dir() {
                    return Err(Error::Unsupported("recipe steps must produce one file".into()));
                }
                // Keep the original stem so the next step (and the final name) build on it.
                let ext = plan.output.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_default();
                let carried = stage.join(format!("{}.{ext}", stem_of(src)));
                fs::rename(&plan.output, &carried)?;
                input = vec![carried];
            }
            last = Some(plan);
        }
        let plan = last.ok_or_else(|| Error::Unsupported("empty recipe".into()))?;
        let _guard = PLACE.lock().unwrap_or_else(|p| p.into_inner());
        let target = dest.join(&plan.name);
        let fin = if plan.output.is_dir() { unique_dir(&target) } else { unique_path(&target) };
        fs::rename(&plan.output, &fin)?;
        Ok(fin)
    })();
    let _ = fs::remove_dir_all(&tmp);
    result
}

/// Writes `<file>.sha256` (sha256sum format) beside the file and returns its path.
fn checksum(src: &Path) -> Result<PathBuf> {
    if src.is_dir() {
        return Err(Error::Unsupported("checksums are for files".into()));
    }
    let digest = crate::sha256::of_reader(fs::File::open(src)?)?;
    let name = src.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let _guard = PLACE.lock().unwrap_or_else(|p| p.into_inner());
    let out = unique_path(&src.with_file_name(format!("{name}.sha256")));
    fs::write(&out, format!("{digest}  {name}\n"))?;
    Ok(out)
}

/// Tools whose result is a report rather than a file (QR contents, duplicate groups, renames).
pub fn is_text_tool(op: &Op) -> bool {
    matches!(op, Op::Info | Op::QrRead | Op::Duplicates { .. } | Op::Rename { .. })
}

/// Runs a text tool over the whole selection and returns its report.
pub fn text_tool(op: &Op, inputs: &[PathBuf]) -> Result<String> {
    let name = |p: &Path| p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    match op {
        Op::Info => {
            let mut out = String::new();
            for p in inputs {
                out += &format!("== {}\n{}\n", name(p), info(p).unwrap_or_else(|e| e.to_string()));
            }
            Ok(out)
        }
        Op::QrRead => {
            let mut out = String::new();
            for p in inputs {
                out += &format!("== {}\n{}\n", name(p), qr_read(p).unwrap_or_else(|e| e.to_string()));
            }
            Ok(out)
        }
        Op::Rename { .. } => {
            let pairs = rename(op, inputs)?;
            Ok(pairs.iter().map(|(a, b)| format!("{} -> {}\n", name(a), name(b))).collect())
        }
        Op::Duplicates { move_dupes } => duplicates(inputs, *move_dupes),
        _ => Err(Error::Unsupported("not a text tool".into())),
    }
}

/// Decoded QR codes / barcodes in an image, one per line (ZBar).
pub fn qr_read(src: &Path) -> Result<String> {
    let out =
        command(&tools::require(Tool::Zbar)?).args(["--quiet", "--raw"]).arg(src).stdin(Stdio::null()).output()?;
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    // zbarimg exits 4 when the image simply has no code in it.
    match (out.status.code(), text.is_empty()) {
        (Some(0), false) => Ok(text),
        (Some(0 | 4), _) => Err(Error::Unsupported("no QR code or barcode found".into())),
        _ => Err(Error::Failed { tool: "ZBar".into(), detail: String::from_utf8_lossy(&out.stderr).trim().into() }),
    }
}

/// Files under `inputs` (folders are walked) grouped by identical content. With `move_dupes`,
/// every copy but the first of each group is moved into a `Duplicates` folder beside it.
pub fn duplicates(inputs: &[PathBuf], move_dupes: bool) -> Result<String> {
    use std::collections::HashMap;
    let mut files = Vec::new();
    let mut stack: Vec<PathBuf> = inputs.to_vec();
    while let Some(p) = stack.pop() {
        if p.is_dir() {
            for e in fs::read_dir(&p)?.flatten() {
                let name = e.file_name();
                // Skip hidden entries and our own Duplicates folders so reruns are stable.
                if !name.to_string_lossy().starts_with('.') && name != "Duplicates" {
                    stack.push(e.path());
                }
            }
        } else if p.is_file() {
            files.push(p);
        }
    }
    files.sort();
    files.dedup();
    let mut by_size: HashMap<u64, Vec<PathBuf>> = HashMap::new();
    for f in files {
        by_size.entry(fs::metadata(&f)?.len()).or_default().push(f);
    }
    let mut groups: Vec<Vec<PathBuf>> = Vec::new();
    for (size, same) in by_size {
        if same.len() < 2 || size == 0 {
            continue;
        }
        let mut by_hash: HashMap<String, Vec<PathBuf>> = HashMap::new();
        for f in same {
            by_hash.entry(crate::sha256::of_reader(fs::File::open(&f)?)?).or_default().push(f);
        }
        groups.extend(by_hash.into_values().filter(|g| g.len() > 1));
    }
    for g in &mut groups {
        g.sort();
    }
    groups.sort();
    if groups.is_empty() {
        return Ok("no duplicates found\n".into());
    }
    let mut report = String::new();
    let mut wasted = 0u64;
    for g in &groups {
        let size = fs::metadata(&g[0]).map_or(0, |m| m.len());
        wasted += size * (g.len() as u64 - 1);
        report += &format!("{} copies, {} bytes each:\n", g.len(), size);
        for (i, f) in g.iter().enumerate() {
            let mut line = format!("  {}", f.display());
            if move_dupes && i > 0 {
                let dir = f.parent().unwrap_or(Path::new(".")).join("Duplicates");
                fs::create_dir_all(&dir)?;
                let dst = unique_path(&dir.join(f.file_name().unwrap_or_default()));
                fs::rename(f, &dst)?;
                line += &format!("  -> {}", dst.display());
            }
            report += &line;
            report.push('\n');
        }
    }
    report += &format!("{} groups, {wasted} bytes in extra copies\n", groups.len());
    Ok(report)
}

/// Batch rename in place; returns (old, new) pairs.
pub fn rename(op: &Op, inputs: &[PathBuf]) -> Result<Vec<(PathBuf, PathBuf)>> {
    let Op::Rename { pattern, start } = op else { return Err(Error::Unsupported("not a rename".into())) };
    let plan = crate::rename::plan(inputs, pattern, *start)?;
    crate::rename::apply(&plan)?;
    Ok(plan)
}

fn exec(step: &Step, report: &mut dyn FnMut(f32)) -> Result<()> {
    match step {
        Step::Run { tool, args, cwd, progress_secs } => run_tool(*tool, args, cwd.as_deref(), *progress_secs, report),
        Step::MkDir(p) => Ok(fs::create_dir_all(p)?),
        Step::Copy { src, dst } => Ok(fs::copy(src, dst).map(|_| ())?),
        Step::WriteFile { path, contents } => Ok(fs::write(path, contents)?),
        Step::Subtitle { src, dst, shift_ms } => {
            let text = String::from_utf8_lossy(&fs::read(src)?).into_owned();
            let to = dst.extension().and_then(|e| e.to_str()).unwrap_or("srt");
            let mut cues = subtitle::parse(&text);
            subtitle::shift(&mut cues, *shift_ms);
            Ok(fs::write(dst, subtitle::write(&cues, to))?)
        }
        Step::Dehaze { src, dst, strength } => {
            let (w, h, mut px) = crate::dehaze::read_ppm(&fs::read(src)?).ok_or_else(|| Error::Failed {
                tool: "dehaze".into(),
                detail: "could not read the decoded image".into(),
            })?;
            crate::dehaze::dehaze(&mut px, w, h, *strength);
            Ok(fs::write(dst, crate::dehaze::write_ppm(w, h, &px))?)
        }
        Step::Ico { pngs, dst } => {
            let data = pngs.iter().map(|(s, p)| Ok((*s, fs::read(p)?))).collect::<std::io::Result<Vec<_>>>()?;
            Ok(crate::ico::write_ico(&data, &mut fs::File::create(dst)?)?)
        }
        Step::JpegsToPdf { jpegs, dst } => {
            let data = jpegs.iter().map(fs::read).collect::<std::io::Result<Vec<_>>>()?;
            let mut out = fs::File::create(dst)?;
            Ok(pdf::jpegs_to_pdf(&data, &mut out)?)
        }
        Step::ExtractMotion { src, dst } => crate::motion::extract_motion_video(src, dst),
        Step::SmallestUnder { target, candidates, dst } => {
            let mut best: Option<(u64, &PathBuf)> = None;
            let mut last_err = None;
            for (steps, out) in candidates {
                // A candidate can fail on its own (an encoder option the engine lacks);
                // try the others before giving up.
                if let Err(e) = steps.iter().try_for_each(|st| exec(st, &mut |_| {})) {
                    last_err = Some(e);
                    continue;
                }
                let Ok(meta) = fs::metadata(out) else { continue };
                let size = meta.len();
                if best.map_or(true, |(b, _)| size < b) {
                    best = Some((size, out));
                }
                if size <= *target {
                    break;
                }
            }
            let (_, out) =
                best.ok_or_else(|| last_err.unwrap_or_else(|| Error::Unsupported("no candidates".into())))?;
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
    tool: Tool,
    args: &[OsString],
    cwd: Option<&Path>,
    progress_secs: Option<f64>,
    report: &mut dyn FnMut(f32),
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
        return Err(Error::Failed {
            tool: tool.label().into(),
            detail: String::from_utf8_lossy(&out.stderr).trim().into(),
        });
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
            if let Op::Redact { auto: Some(what), .. } = op {
                ctx.detected = if what == crate::ops::FACES {
                    detect_faces(src).unwrap_or_default()
                } else {
                    detect_text_file(src, what)
                        .or_else(|_| preview_png(src, 32767).and_then(|png| detect_text_png(&png, what)))
                        .unwrap_or_default()
                };
            }
        }
        _ => {}
    }
    if matches!(op, Op::StripMetadata) && category_of(src) == Some(Category::Image) {
        ctx.exiftool = tools::locate(Tool::ExifTool).is_some();
    }
    if matches!(op, Op::Watermark { .. } | Op::Slideshow { .. } | Op::WebSet) {
        ctx.dims = dimensions(src);
    }
    if matches!(op, Op::Transcribe { .. }) {
        ctx.whisper_model = tools::whisper_model();
    }
    ctx
}

pub fn probe_media(src: &Path) -> Option<MediaInfo> {
    let out = capture(
        Tool::Ffprobe,
        &[
            "-v",
            "error",
            "-show_entries",
            "format=duration:stream=codec_type,width,height,avg_frame_rate",
            "-of",
            "default=noprint_wrappers=1",
        ],
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

/// One video frame at `at` seconds as PNG, at most `max` px. Seeking before `-i` is fast and
/// lands on the exact frame because FFmpeg decodes forward from the previous keyframe.
pub fn frame_png(src: &Path, at: f64, max: u32) -> Result<Vec<u8>> {
    let out = scratch_file("frame", "png");
    let scale = format!("scale='min({max},iw)':'min({max},ih)':force_original_aspect_ratio=decrease");
    let seek = format!("{:.3}", at.max(0.0));
    let args: Vec<OsString> = ["-hide_banner", "-nostdin", "-y", "-loglevel", "error", "-ss", &seek, "-i"]
        .iter()
        .map(OsString::from)
        .chain([src.as_os_str().to_owned()])
        .chain(["-frames:v", "1", "-vf", &scale].iter().map(OsString::from))
        .chain([out.as_os_str().to_owned()])
        .collect();
    let r = run_tool(Tool::Ffmpeg, &args, None, None, &mut |_| {}).and_then(|_| Ok(fs::read(&out)?));
    let _ = fs::remove_file(&out);
    r
}

/// Waveform picture of an audio (or video soundtrack) file for the trim timeline.
pub fn waveform_png(src: &Path, width: u32, height: u32) -> Result<Vec<u8>> {
    let out = scratch_file("wave", "png");
    let graph = format!("[0:a]aformat=channel_layouts=mono,showwavespic=s={width}x{height}:colors=0xF07B1D[v]");
    let args: Vec<OsString> = ["-hide_banner", "-nostdin", "-y", "-loglevel", "error", "-i"]
        .iter()
        .map(OsString::from)
        .chain([src.as_os_str().to_owned()])
        .chain(["-filter_complex", &graph, "-map", "[v]", "-frames:v", "1"].iter().map(OsString::from))
        .chain([out.as_os_str().to_owned()])
        .collect();
    let r = run_tool(Tool::Ffmpeg, &args, None, None, &mut |_| {}).and_then(|_| Ok(fs::read(&out)?));
    let _ = fs::remove_file(&out);
    r
}

/// PNG preview no larger than `max` px: the image itself, or a frame from a video.
pub fn preview_png(src: &Path, max: u32) -> Result<Vec<u8>> {
    let max = max.clamp(16, 32767);
    let out = std::env::temp_dir().join(format!(
        "zeidex-preview-{}-{}.png",
        std::process::id(),
        JOB.fetch_add(1, Ordering::Relaxed)
    ));
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
        Some(Category::Image)
            if tools::locate(Tool::Vips).is_some() && crate::format::ext_of(src).as_deref() != Some("bmp") =>
        {
            let args: Vec<OsString> = vec![
                "thumbnail".into(),
                src.as_os_str().to_owned(),
                out.as_os_str().to_owned(),
                m.clone().into(),
                "--height".into(),
                m.into(),
                "--size".into(),
                "down".into(),
            ];
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
    std::env::temp_dir().join(format!(
        "zeidex-{tag}-{}-{}.{ext}",
        std::process::id(),
        JOB.fetch_add(1, Ordering::Relaxed)
    ))
}

/// Text lines found by Tesseract in an image file, as boxes in that image's pixels.
pub fn detect_text_file(src: &Path, lang: &str) -> Result<Vec<Rect>> {
    if !lang.chars().all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '_') {
        return Err(Error::BadOption(format!("lang={lang}")));
    }
    let out = command(&tools::require(Tool::Tesseract)?)
        .arg(src)
        .args(["stdout", "-l", lang, "tsv"])
        .stdin(Stdio::null())
        .output();
    let out = out?;
    if !out.status.success() {
        return Err(Error::Failed {
            tool: "Tesseract".into(),
            detail: String::from_utf8_lossy(&out.stderr).trim().into(),
        });
    }
    Ok(parse_tsv(&String::from_utf8_lossy(&out.stdout)))
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
        return Err(Error::Failed {
            tool: "Tesseract".into(),
            detail: String::from_utf8_lossy(&out.stderr).trim().into(),
        });
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

/// Longest side the face detector works at; boxes are scaled back to the original.
const FACE_DETECT_MAX: u32 = 800;
/// Grow detected faces so hair line and chin are covered too.
const FACE_PAD: f64 = 0.15;

/// Faces in an image file, as boxes in its own pixels (padded for redaction).
pub fn detect_faces(src: &Path) -> Result<Vec<Rect>> {
    static CASCADE: std::sync::OnceLock<crate::faces::Cascade> = std::sync::OnceLock::new();
    let (full_w, full_h) = dimensions(src).ok_or_else(|| Error::Unsupported("could not read the image size".into()))?;
    let pgm = scratch_file("faces", "pgm");
    let scale = format!("scale='min({FACE_DETECT_MAX},iw)':-2,format=gray");
    let args: Vec<OsString> = ["-hide_banner", "-nostdin", "-y", "-loglevel", "error", "-i"]
        .iter()
        .map(OsString::from)
        .chain([src.as_os_str().to_owned()])
        .chain(["-frames:v", "1", "-vf", &scale].iter().map(OsString::from))
        .chain([pgm.as_os_str().to_owned()])
        .collect();
    let made = run_tool(Tool::Ffmpeg, &args, None, None, &mut |_| {});
    let data = made.and_then(|_| Ok(fs::read(&pgm)?));
    let _ = fs::remove_file(&pgm);
    let (w, h, gray) = crate::dehaze::read_pgm(&data?)
        .ok_or_else(|| Error::Failed { tool: "faces".into(), detail: "could not decode the image".into() })?;
    let k = full_w as f64 / w as f64;
    let faces = CASCADE.get_or_init(crate::faces::Cascade::frontal_face).detect(&gray, w, h);
    Ok(faces
        .into_iter()
        .map(|r| {
            let pad = r.w as f64 * FACE_PAD;
            let x = ((r.x as f64 - pad) * k).max(0.0) as u32;
            let y = ((r.y as f64 - pad) * k).max(0.0) as u32;
            let w = ((r.w as f64 + 2.0 * pad) * k) as u32;
            let h = ((r.h as f64 + 2.0 * pad) * k) as u32;
            Rect { x, y, w: w.min(full_w.saturating_sub(x)).max(1), h: h.min(full_h.saturating_sub(y)).max(1) }
        })
        .collect())
}

/// Faces in a PNG (the editor preview), in that PNG's pixels.
pub fn detect_faces_png(png: &[u8]) -> Result<Vec<Rect>> {
    let file = scratch_file("faces-in", "png");
    fs::write(&file, png)?;
    let r = detect_faces(&file);
    let _ = fs::remove_file(&file);
    r
}

/// Small PNG per PDF page for the page organizer (Ghostscript at `dpi`).
pub fn pdf_thumbnails(src: &Path, dpi: u32) -> Result<Vec<Vec<u8>>> {
    let dir = scratch_file("thumbs", "d");
    fs::create_dir_all(&dir)?;
    let mut out_arg = OsString::from("-sOutputFile=");
    out_arg.push(dir.join("p-%04d.png").as_os_str());
    let args: Vec<OsString> = vec![
        "-dSAFER".into(),
        "-dBATCH".into(),
        "-dNOPAUSE".into(),
        "-dQUIET".into(),
        "-sDEVICE=png16m".into(),
        format!("-r{dpi}").into(),
        out_arg,
        src.as_os_str().to_owned(),
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
        // ExifTool knows camera, lens and GPS tags; vipsheader is the fallback.
        Some(Category::Image) if tools::locate(Tool::ExifTool).is_some() => {
            capture(Tool::ExifTool, &["-G1", "-s"], src)
        }
        Some(Category::Image) => capture(Tool::VipsHeader, &["-a"], src),
        Some(Category::Video | Category::Audio) => {
            capture(Tool::Ffprobe, &["-hide_banner", "-show_format", "-show_streams"], src)
        }
        Some(Category::Pdf) => {
            let pages = capture(Tool::Qpdf, &["--show-npages"], src)?;
            let check = capture(Tool::Qpdf, &["--check"], src).unwrap_or_default();
            Ok(format!("pages: {}\n{check}", pages.trim()))
        }
        _ => Err(Error::Unsupported("no metadata reader for this file type".into())),
    }
}

/// `name`, then `name (2)`, `name (3)`... for folders, whose names may contain dots.
pub fn unique_dir(p: &Path) -> PathBuf {
    if !p.exists() {
        return p.to_path_buf();
    }
    let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let parent = p.parent().unwrap_or(Path::new("."));
    (2..).map(|n| parent.join(format!("{name} ({n})"))).find(|c| !c.exists()).expect("unbounded range")
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
    (2..).map(|n| parent.join(format!("{stem} ({n}){rest}"))).find(|c| !c.exists()).expect("unbounded range")
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
        assert_eq!(parse_tsv(tsv), vec![Rect { x: 6, y: 16, w: 128, h: 38 }, Rect { x: 0, y: 0, w: 36, h: 16 }]);
    }

    /// Needs FFmpeg; skipped when it is missing.
    #[test]
    fn frames_and_waveforms_with_real_engines() {
        let Some(ff) = tools::locate(Tool::Ffmpeg) else { return };
        let d = scratch("frames");
        let clip = d.join("c.mp4");
        let ok = Command::new(&ff)
            .args(["-loglevel", "error", "-f", "lavfi", "-i", "testsrc=size=320x240:rate=10", "-f", "lavfi", "-i"])
            .args(["sine=frequency=440", "-t", "2", "-pix_fmt", "yuv420p"])
            .arg(&clip)
            .status()
            .unwrap();
        assert!(ok.success());
        let a = frame_png(&clip, 0.0, 160).unwrap();
        let b = frame_png(&clip, 1.5, 160).unwrap();
        assert!(a.starts_with(b"\x89PNG") && b.starts_with(b"\x89PNG") && a != b, "different frames");
        assert!(waveform_png(&clip, 400, 60).unwrap().starts_with(b"\x89PNG"));
        fs::remove_dir_all(&d).unwrap();
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
        let r = run(&Op::Convert { to: "vtt".into(), quality: None }, std::slice::from_ref(&src), |e| {
            events.push(format!("{e:?}"))
        });
        assert!(events.iter().all(|e| e.contains("index: 0")));
        let out = r[0].as_ref().unwrap();
        assert_eq!(out, &d.join("talk (2).vtt"));
        assert!(fs::read_to_string(out).unwrap().starts_with("WEBVTT"));
        assert!(fs::read_to_string(&src).unwrap().contains("00:00:01,000"), "original untouched");
        let leftovers: Vec<_> = fs::read_dir(&d)
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with(".zeidex"))
            .collect();
        assert!(leftovers.is_empty());
        assert!(events.first().unwrap().starts_with("Start") && events.last().unwrap().starts_with("Done"));
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn parallel_batch_recipe_checksum_rename() {
        let d = scratch("batch");
        let files: Vec<PathBuf> = (0..6).map(|i| d.join(format!("s{i}.srt"))).collect();
        for f in &files {
            fs::write(f, "1\n00:00:01,000 --> 00:00:02,000\nHi\n").unwrap();
        }
        let seen = Mutex::new(Vec::new());
        let r = run_with(&Op::Convert { to: "vtt".into(), quality: None }, &files, 3, |e| {
            if let Event::Done { index, .. } = e {
                seen.lock().unwrap().push(index);
            }
        });
        assert!(r.iter().all(|x| x.is_ok()));
        assert_eq!(r[4].as_ref().unwrap(), &d.join("s4.vtt"), "results keep input order");
        let mut idx = seen.into_inner().unwrap();
        idx.sort();
        assert_eq!(idx, vec![0, 1, 2, 3, 4, 5]);

        // recipe: shift then convert, name built on the original stem
        let recipe = crate::ops::parse_recipe("shift-subs seconds=2 | convert to=vtt").unwrap();
        let out = run(&Op::Recipe(recipe), &files[..1], |_| {}).remove(0).unwrap();
        assert_eq!(out, d.join("s0 (2).vtt"));
        assert!(fs::read_to_string(&out).unwrap().contains("00:00:03.000 --> 00:00:04.000"));

        let sum = run(&Op::Checksum, &files[..1], |_| {}).remove(0).unwrap();
        let text = fs::read_to_string(&sum).unwrap();
        assert!(text.ends_with("  s0.srt\n") && text.len() == 64 + 2 + 6 + 1);

        let pairs = rename(&Op::Rename { pattern: "clip-{n:2}".into(), start: 1 }, &files[..2]).unwrap();
        assert_eq!(pairs[1].1, d.join("clip-02.srt"));
        assert!(d.join("clip-01.srt").exists() && !files[0].exists());
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn smallest_under_skips_failing_candidates() {
        let d = scratch("smallest");
        let good = d.join("good.txt");
        let dst = d.join("dst.txt");
        let step = Step::SmallestUnder {
            target: 1,
            candidates: vec![
                (vec![Step::Copy { src: d.join("missing"), dst: d.join("x") }], d.join("x")),
                (vec![Step::WriteFile { path: good.clone(), contents: "ok".into() }], good.clone()),
            ],
            dst: dst.clone(),
        };
        exec(&step, &mut |_| {}).unwrap();
        assert_eq!(fs::read_to_string(&dst).unwrap(), "ok");
        let all_bad = Step::SmallestUnder {
            target: 1,
            candidates: vec![(vec![Step::Copy { src: d.join("missing"), dst: d.join("y") }], d.join("y"))],
            dst: d.join("z"),
        };
        assert!(exec(&all_bad, &mut |_| {}).is_err(), "real error surfaces when nothing works");
        fs::create_dir_all(d.join("my.trip pages")).unwrap();
        assert_eq!(unique_dir(&d.join("my.trip pages")), d.join("my.trip pages (2)"));
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn duplicate_groups() {
        let d = scratch("dupes");
        fs::create_dir_all(d.join("sub")).unwrap();
        fs::write(d.join("a.txt"), "same").unwrap();
        fs::write(d.join("sub/b.txt"), "same").unwrap();
        fs::write(d.join("c.txt"), "diff").unwrap();
        fs::write(d.join("d.txt"), "same").unwrap();
        let report = duplicates(std::slice::from_ref(&d), true).unwrap();
        assert!(report.contains("3 copies, 4 bytes each") && report.contains("8 bytes in extra copies"), "{report}");
        assert!(
            d.join("a.txt").exists() && d.join("Duplicates/d.txt").exists() && d.join("sub/Duplicates/b.txt").exists()
        );
        assert_eq!(
            duplicates(std::slice::from_ref(&d), false).unwrap(),
            "no duplicates found\n",
            "Duplicates folders skipped"
        );
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn missing_input_fails_cleanly() {
        let r = run(&Op::Convert { to: "vtt".into(), quality: None }, &[PathBuf::from("/nope/x.srt")], |_| {});
        assert!(r[0].is_err());
    }
}
