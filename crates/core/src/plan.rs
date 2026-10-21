//! Turns an operation on some files into concrete engine invocations.
//!
//! Planning is pure: it only builds argv lists inside a scratch dir. `runner` executes them,
//! which keeps every routing decision unit-testable without the engines installed.

use crate::format::{category_of_ext, ext_of, stem_of, Category};
use crate::ops::{Compression, CropSpec, Level, Op, Rect, Size};
use crate::tools::Tool;
use crate::{Error, Result};
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default)]
pub struct MediaInfo {
    pub duration: f64,
    pub has_video: bool,
    pub has_audio: bool,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
}

#[derive(Clone, Copy, Debug)]
pub struct ImageInfo {
    pub width: u32,
    pub height: u32,
    pub bands: u32,
}

/// Facts about the first input that some plans need (probed by the runner).
#[derive(Clone, Debug, Default)]
pub struct Ctx {
    pub media: Option<MediaInfo>,
    /// Every input's media info, for ops that combine files (join). Empty otherwise.
    pub inputs: Vec<MediaInfo>,
    pub image: Option<ImageInfo>,
    pub size: u64,
}

#[derive(Debug)]
pub enum Step {
    Run { tool: Tool, args: Vec<OsString>, cwd: Option<PathBuf>, progress_secs: Option<f64> },
    MkDir(PathBuf),
    Copy { src: PathBuf, dst: PathBuf },
    WriteFile { path: PathBuf, contents: String },
    Subtitle { src: PathBuf, dst: PathBuf },
    JpegsToPdf { jpegs: Vec<PathBuf>, dst: PathBuf },
    /// Try candidates in order; keep the first output that fits `target`, else the smallest.
    SmallestUnder { target: u64, candidates: Vec<(Vec<Step>, PathBuf)>, dst: PathBuf },
}

#[derive(Debug)]
pub struct Plan {
    pub steps: Vec<Step>,
    /// File or directory inside the scratch dir holding the result.
    pub output: PathBuf,
    /// Final name beside the original (made unique by the runner).
    pub name: String,
}

macro_rules! args {
    ($($x:expr),* $(,)?) => { vec![$(OsString::from($x)),*] };
}

fn run(tool: Tool, args: Vec<OsString>) -> Step {
    Step::Run { tool, args, cwd: None, progress_secs: None }
}

fn suffixed(a: &Path, suffix: &str) -> OsString {
    let mut s = a.as_os_str().to_owned();
    s.push(suffix);
    s
}

fn prefixed(prefix: &str, p: &Path) -> OsString {
    let mut s = OsString::from(prefix);
    s.push(p.as_os_str());
    s
}

fn named(stem: &str, suffix: &str, ext: &str) -> String {
    let ext = if ext == "tgz" { "tar.gz" } else { ext };
    if suffix.is_empty() {
        format!("{stem}.{ext}")
    } else {
        format!("{stem}-{suffix}.{ext}")
    }
}

fn unsupported(what: &str, from: &str) -> Error {
    Error::Unsupported(format!("{what} is not available for .{from} files"))
}

pub fn plan(op: &Op, inputs: &[PathBuf], ctx: &Ctx, tmp: &Path) -> Result<Plan> {
    let src = inputs.first().ok_or_else(|| Error::BadOption("no input files".into()))?;
    let from = ext_of(src).unwrap_or_default();
    let cat = category_of_ext(&from);
    let stem = stem_of(src);
    let mut s = Vec::new();
    let is = |c: Category| cat == Some(c);
    let av = is(Category::Video) || is(Category::Audio);

    let (output, name) = match op {
        Op::Convert { to } => {
            let out = convert(src, &from, to, tmp, ctx, &mut s)?;
            let name = if out.extension().is_none() { format!("{stem} pages") } else { named(&stem, "", to) };
            (out, name)
        }
        Op::Compress { how, max } => compress(src, &from, cat, how, *max, ctx, tmp, &mut s, &stem)?,
        Op::StripMetadata => {
            if is(Category::Image) && from != "bmp" && from != "svg" {
                let out = tmp.join(format!("clean.{from}"));
                let opts = if from == "jpg" { "[Q=95,strip]" } else { "[strip]" };
                let input = image_src(src, &from, tmp, &mut s);
                s.push(run(Tool::Vips, args!["copy", &input, suffixed(&out, opts)]));
                (out, named(&stem, "clean", &from))
            } else if av {
                let to = av_out_ext(&from, cat);
                let out = tmp.join(format!("clean.{to}"));
                let mut a = ffmpeg_in(src);
                a.extend(args!["-map", "0", "-map_metadata", "-1", "-map_chapters", "-1", "-c", "copy", &out]);
                s.push(run(Tool::Ffmpeg, a));
                (out, named(&stem, "clean", to))
            } else {
                return Err(unsupported("Removing metadata", &from));
            }
        }
        Op::Info => return Err(Error::Unsupported("info is read with runner::info".into())),
        Op::Archive { kind } => {
            let out = pack(inputs, kind, tmp, &mut s);
            let base = if inputs.len() == 1 { display_name(src) } else { "archive".into() };
            (out, named(&base, "", kind))
        }
        Op::Extract if is(Category::Archive) => (extract(src, tmp, &mut s), stem.clone()),
        Op::Resize(size) if is(Category::Image) => {
            let (input, out, to) = image_io(src, &from, tmp, "resized", &mut s);
            s.push(match size {
                Size::Percent(p) => run(Tool::Vips, args!["resize", &input, &out, format!("{}", *p as f64 / 100.0)]),
                Size::Max(m) => run(
                    Tool::Vips,
                    args!["thumbnail", &input, &out, m.to_string(), "--height", m.to_string(), "--size", "down"],
                ),
            });
            (out, named(&stem, "resized", to))
        }
        Op::Crop(spec) if is(Category::Image) => {
            let Rect { x: left, y: top, w, h } = match spec {
                CropSpec::Box(r) => *r,
                CropSpec::Ratio(rw, rh) => {
                    let info = ctx.image.ok_or_else(|| Error::Unsupported("could not read the image size".into()))?;
                    let (w, h) = crop_box(info.width, info.height, *rw, *rh);
                    Rect { x: (info.width - w) / 2, y: (info.height - h) / 2, w, h }
                }
            };
            let (input, out, to) = image_io(src, &from, tmp, "cropped", &mut s);
            s.push(run(
                Tool::Vips,
                args!["extract_area", &input, &out, left.to_string(), top.to_string(), w.to_string(), h.to_string()],
            ));
            (out, named(&stem, "cropped", to))
        }
        Op::Crop(spec) if is(Category::Video) => {
            let to = av_out_ext(&from, cat);
            let out = tmp.join(format!("cropped.{to}"));
            let filter = match spec {
                // yuv420p needs even dimensions.
                CropSpec::Box(r) => format!("crop={}:{}:{}:{}", r.w & !1, r.h & !1, r.x, r.y),
                CropSpec::Ratio(rw, rh) => {
                    format!("crop=w='trunc(min(iw,ih*{rw}/{rh})/2)*2':h='trunc(min(ih,iw*{rh}/{rw})/2)*2'")
                }
            };
            let mut a = ffmpeg_in(src);
            a.extend(args!["-vf", filter]);
            a.extend(strs(video_codec(to)));
            a.extend(args!["-c:a", "copy", &out]);
            s.push(ffmpeg(a, ctx));
            (out, named(&stem, "cropped", to))
        }
        Op::Rotate { degrees } => match cat {
            Some(Category::Image) => {
                let (input, out, to) = image_io(src, &from, tmp, "rotated", &mut s);
                s.push(run(Tool::Vips, args!["rot", &input, &out, format!("d{degrees}")]));
                (out, named(&stem, "rotated", to))
            }
            Some(Category::Video) => {
                let to = av_out_ext(&from, cat);
                let out = tmp.join(format!("rotated.{to}"));
                let filter = match degrees {
                    90 => "transpose=1",
                    270 => "transpose=2",
                    _ => "hflip,vflip",
                };
                let mut a = ffmpeg_in(src);
                a.extend(args!["-vf", filter]);
                a.extend(strs(video_codec(to)));
                a.extend(args!["-c:a", "copy", &out]);
                s.push(ffmpeg(a, ctx));
                (out, named(&stem, "rotated", to))
            }
            Some(Category::Pdf) => {
                let out = tmp.join("rotated.pdf");
                s.push(run(Tool::Qpdf, args![src, &out, format!("--rotate=+{degrees}")]));
                (out, named(&stem, "rotated", "pdf"))
            }
            _ => return Err(unsupported("Rotate", &from)),
        },
        Op::Background { rgb, padding } if is(Category::Image) => {
            let info = ctx.image.ok_or_else(|| Error::Unsupported("could not read the image size".into()))?;
            let [r, g, b] = *rgb;
            let bg = match info.bands {
                1 => format!("{}", (r as u32 + g as u32 + b as u32) / 3),
                2 => format!("{} 255", (r as u32 + g as u32 + b as u32) / 3),
                4 => format!("{r} {g} {b} 255"),
                _ => format!("{r} {g} {b}"),
            };
            let (input, out, to) = image_io(src, &from, tmp, "framed", &mut s);
            let p = *padding;
            s.push(run(
                Tool::Vips,
                args![
                    "embed", &input, &out, p.to_string(), p.to_string(),
                    (info.width + 2 * p).to_string(), (info.height + 2 * p).to_string(),
                    "--extend", "background", "--background", bg
                ],
            ));
            (out, named(&stem, "framed", to))
        }
        Op::Grayscale if is(Category::Image) => {
            let (input, out, to) = image_io(src, &from, tmp, "bw", &mut s);
            s.push(run(Tool::Vips, args!["colourspace", &input, &out, "b-w"]));
            (out, named(&stem, "bw", to))
        }
        Op::ImagesToPdf => {
            let jpegs = images_to_jpegs(inputs, tmp, &mut s);
            let out = tmp.join("images.pdf");
            s.push(Step::JpegsToPdf { jpegs, dst: out.clone() });
            (out, "images.pdf".into())
        }
        Op::Collage { across, gap } => {
            let mut cells = Vec::new();
            for (i, img) in inputs.iter().enumerate() {
                let f = ext_of(img).unwrap_or_default();
                let input = image_src(img, &f, tmp, &mut s);
                let cell = format!("c{i}.jpg");
                s.push(run(
                    Tool::Vips,
                    args!["thumbnail", &input, suffixed(&tmp.join(&cell), "[background=255]"), "800", "--height", "800"],
                ));
                cells.push(cell);
            }
            // Relative names + cwd: vips splits the image list on spaces, and tmp may contain some.
            s.push(Step::Run {
                tool: Tool::Vips,
                args: args![
                    "arrayjoin", cells.join(" "), "collage.jpg[Q=90]", "--across", across.to_string(),
                    "--shim", gap.to_string(), "--background", "255 255 255", "--halign", "centre", "--valign", "centre"
                ],
                cwd: Some(tmp.to_path_buf()),
                progress_secs: None,
            });
            (tmp.join("collage.jpg"), "collage.jpg".into())
        }
        Op::Ocr { to, lang } => {
            let input = match cat {
                Some(Category::Pdf) => {
                    let tif = tmp.join("pages.tif");
                    s.push(run(
                        Tool::Ghostscript,
                        args!["-dSAFER", "-dBATCH", "-dNOPAUSE", "-dQUIET", "-r300", "-sDEVICE=tiffgray",
                            "-sCompression=lzw", prefixed("-sOutputFile=", &tif), src],
                    ));
                    tif
                }
                Some(Category::Image) if ["png", "jpg", "tiff", "bmp"].contains(&from.as_str()) => src.clone(),
                Some(Category::Image) => {
                    let png = tmp.join("ocr-in.png");
                    let input = image_src(src, &from, tmp, &mut s);
                    s.push(run(Tool::Vips, args!["copy", &input, &png]));
                    png
                }
                _ => return Err(unsupported("OCR", &from)),
            };
            let base = tmp.join("ocr");
            let mut a = args![&input, &base, "-l", lang];
            if to == "pdf" {
                a.push("pdf".into());
            }
            s.push(run(Tool::Tesseract, a));
            (tmp.join(format!("ocr.{to}")), named(&stem, "ocr", to))
        }
        Op::Trim { start, end, silence } if av => {
            let to = av_out_ext(&from, cat);
            let out = tmp.join(format!("trimmed.{to}"));
            let mut a = ffmpeg_in(src);
            a.extend(args!["-ss", format!("{start}")]);
            if let Some(e) = end {
                a.extend(args!["-to", format!("{e}")]);
            }
            if *silence && is(Category::Audio) {
                let sr = "silenceremove=start_periods=1:start_threshold=-50dB";
                a.extend(args!["-af", format!("{sr},areverse,{sr},areverse")]);
            }
            a.extend(encode_args(to));
            a.push(out.clone().into());
            let total = end.or(ctx.media.as_ref().map(|m| m.duration)).map(|e| e - start);
            s.push(Step::Run { tool: Tool::Ffmpeg, args: a, cwd: None, progress_secs: total });
            (out, named(&stem, "trimmed", to))
        }
        Op::Speed { factor } if av => {
            let to = av_out_ext(&from, cat);
            let out = tmp.join(format!("speed.{to}"));
            let media = ctx.media.clone().unwrap_or(MediaInfo { duration: 0.0, has_video: is(Category::Video), has_audio: true, ..MediaInfo::default() });
            let mut a = ffmpeg_in(src);
            if media.has_video {
                a.extend(args!["-filter:v", format!("setpts=PTS/{factor}")]);
            }
            if media.has_audio {
                a.extend(args!["-filter:a", atempo_chain(*factor)]);
            }
            a.extend(encode_args(to));
            a.push(out.clone().into());
            let total = (media.duration > 0.0).then(|| media.duration / factor);
            s.push(Step::Run { tool: Tool::Ffmpeg, args: a, cwd: None, progress_secs: total });
            (out, named(&stem, &format!("{factor}x"), to))
        }
        Op::Snapshot { at } if is(Category::Video) => {
            let out = tmp.join("frame.png");
            let mut a = args!["-hide_banner", "-nostdin", "-y", "-loglevel", "error", "-ss", format!("{at}"), "-i", src];
            a.extend(args!["-frames:v", "1", &out]);
            s.push(run(Tool::Ffmpeg, a));
            (out, named(&stem, &format!("frame-{}s", *at as u64), "png"))
        }
        Op::ExtractAudio if is(Category::Video) => {
            let out = tmp.join("audio.mp3");
            let mut a = ffmpeg_in(src);
            a.extend(encode_args("mp3"));
            a.push(out.clone().into());
            s.push(ffmpeg(a, ctx));
            (out, named(&stem, "", "mp3"))
        }
        Op::Mute if is(Category::Video) => {
            let to = av_out_ext(&from, cat);
            let out = tmp.join(format!("muted.{to}"));
            let mut a = ffmpeg_in(src);
            a.extend(args!["-an", "-c:v", "copy", &out]);
            s.push(run(Tool::Ffmpeg, a));
            (out, named(&stem, "muted", to))
        }
        Op::Split { parts, at } if av => {
            let to = av_out_ext(&from, cat);
            let dir = tmp.join("parts");
            s.push(Step::MkDir(dir.clone()));
            let pattern = dir.join(format!("{}-%02d.{to}", stem.replace('%', "%%")));
            let mut a = ffmpeg_in(src);
            a.extend(encode_args(to));
            // Re-encode with forced keyframes: stream copy can only cut at existing keyframes.
            if at.is_empty() {
                let dur = ctx.media.as_ref().map(|m| m.duration).filter(|d| *d > 0.0)
                    .ok_or_else(|| Error::Unsupported("could not read the duration".into()))?;
                let seg = format!("{:.3}", dur / *parts as f64 + 0.001);
                a.extend(args!["-force_key_frames", format!("expr:gte(t,n_forced*{seg})"), "-f", "segment", "-segment_time", &seg]);
            } else {
                let times = at.iter().map(|t| format!("{t:.3}")).collect::<Vec<_>>().join(",");
                // Forced keyframes land exactly on the cut; without a delta the muxer misses them.
                a.extend(args!["-force_key_frames", &times, "-f", "segment", "-segment_times", &times, "-segment_time_delta", "0.05"]);
            }
            a.extend(args!["-reset_timestamps", "1", &pattern]);
            s.push(ffmpeg(a, ctx));
            (dir, format!("{stem} parts"))
        }
        Op::Join if av && ctx.inputs.len() == inputs.len() && ctx.inputs.iter().all(|m| m.duration > 0.0) => {
            let to = if is(Category::Video) { "mp4" } else { av_out_ext(&from, cat) };
            let out = tmp.join(format!("joined.{to}"));
            let mut a = args!["-hide_banner", "-nostdin", "-y", "-loglevel", "error", "-progress", "pipe:1", "-nostats"];
            for p in inputs {
                a.extend(args!["-i", p]);
            }
            a.extend(args!["-filter_complex", join_graph(&ctx.inputs, is(Category::Video)), "-map", "[a]"]);
            if is(Category::Video) {
                a.extend(args!["-map", "[v]"]);
                a.extend(strs(video_codec(to)));
            }
            a.extend(strs(audio_codec(to)));
            a.extend(strs(container_flags(to)));
            a.push(out.clone().into());
            let total: f64 = ctx.inputs.iter().map(|m| m.duration).sum();
            s.push(Step::Run { tool: Tool::Ffmpeg, args: a, cwd: None, progress_secs: Some(total) });
            (out, named("joined", "", to))
        }
        // Fallback without probe data: the concat demuxer (needs matching streams).
        Op::Join if av => {
            let to = if is(Category::Video) { "mp4" } else { av_out_ext(&from, cat) };
            let list = tmp.join("list.txt");
            let contents: String = inputs
                .iter()
                .map(|p| format!("file '{}'\n", p.to_string_lossy().replace('\'', "'\\''")))
                .collect();
            s.push(Step::WriteFile { path: list.clone(), contents });
            let out = tmp.join(format!("joined.{to}"));
            let mut a = args!["-hide_banner", "-nostdin", "-y", "-loglevel", "error", "-f", "concat", "-safe", "0", "-i", &list];
            a.extend(encode_args(to));
            a.push(out.clone().into());
            s.push(run(Tool::Ffmpeg, a));
            (out, named("joined", "", to))
        }
        Op::Normalize if av => audio_filter(src, &from, cat, "loudnorm=I=-16:TP=-1.5:LRA=11,aresample=48000", "normalized", ctx, tmp, &mut s, &stem),
        Op::Channels { count, left, right } if av => {
            let to = av_out_ext(&from, cat);
            let out = tmp.join(format!("channels.{to}"));
            let mut a = ffmpeg_in(src);
            if is(Category::Video) {
                a.extend(args!["-c:v", "copy"]);
            }
            // aformat first so mono sources have a c1 to pan from.
            let pan = if *count == 1 {
                format!("aformat=channel_layouts=stereo,pan=mono|c0={:.3}*c0+{:.3}*c1", left / 2.0, right / 2.0)
            } else {
                format!("aformat=channel_layouts=stereo,pan=stereo|c0={left}*c0|c1={right}*c1")
            };
            a.extend(args!["-af", pan]);
            a.extend(strs(audio_codec(to)));
            a.push(out.clone().into());
            s.push(ffmpeg(a, ctx));
            (out, named(&stem, if *count == 1 { "mono" } else { "stereo" }, to))
        }
        Op::Bleep { start, end } if av => {
            let to = av_out_ext(&from, cat);
            let out = tmp.join(format!("bleeped.{to}"));
            let graph = format!(
                "[0:a]volume=0:enable='between(t,{start},{end})'[m];\
                 sine=frequency=1000:duration={end},volume=0.25,volume=0:enable='lt(t,{start})'[b];\
                 [m][b]amix=inputs=2:duration=first:normalize=0[a]"
            );
            let mut a = ffmpeg_in(src);
            a.extend(args!["-filter_complex", graph, "-map", "0:v?", "-map", "[a]", "-c:v", "copy"]);
            a.extend(strs(audio_codec(to)));
            a.push(out.clone().into());
            s.push(ffmpeg(a, ctx));
            (out, named(&stem, "bleeped", to))
        }
        Op::Redact { boxes, fill, range } if is(Category::Image) || is(Category::Video) => {
            let video = is(Category::Video);
            let (input, to) = if video {
                (src.to_path_buf(), av_out_ext(&from, cat))
            } else if ["jpg", "png", "bmp", "tiff"].contains(&from.as_str()) {
                (src.to_path_buf(), if from == "jpg" { "jpg" } else { "png" })
            } else {
                // FFmpeg's HEIC/AVIF/SVG support is patchy: decode with libvips first.
                let png = tmp.join("redact-in.png");
                let decoded = image_src(src, &from, tmp, &mut s);
                s.push(run(Tool::Vips, args!["copy", &decoded, &png]));
                (png, "png")
            };
            let out = tmp.join(format!("redacted.{to}"));
            let mut a = ffmpeg_in(&input);
            a.extend(args!["-filter_complex", redact_graph(boxes, *fill, *range), "-map", "[v]"]);
            if video {
                a.extend(args!["-map", "0:a?", "-c:a", "copy"]);
                a.extend(strs(video_codec(to)));
            } else {
                a.extend(args!["-frames:v", "1", "-q:v", "2"]);
            }
            a.push(out.clone().into());
            s.push(ffmpeg(a, ctx));
            (out, named(&stem, "redacted", to))
        }
        Op::Visualize { shape } if is(Category::Audio) => {
            let size = match shape.as_str() {
                "portrait" => "720x1280",
                "square" => "1080x1080",
                _ => "1280x720",
            };
            let out = tmp.join("visualizer.mp4");
            let mut a = ffmpeg_in(src);
            a.extend(args![
                "-filter_complex", format!("[0:a]showwaves=s={size}:mode=cline:colors=0xF28C28,format=yuv420p[v]"),
                "-map", "[v]", "-map", "0:a", "-c:v", "libx264", "-preset", "medium", "-crf", "23",
                "-c:a", "aac", "-b:a", "192k", "-shortest", "-movflags", "+faststart", &out
            ]);
            s.push(ffmpeg(a, ctx));
            (out, named(&stem, "visualizer", "mp4"))
        }
        Op::MergePdf => {
            let out = tmp.join("merged.pdf");
            let mut a = args!["--empty", "--pages"];
            a.extend(inputs.iter().map(OsString::from));
            a.extend(args!["--", &out]);
            s.push(run(Tool::Qpdf, a));
            (out, "merged.pdf".into())
        }
        Op::SplitPdf if is(Category::Pdf) => {
            let dir = tmp.join("pages");
            s.push(Step::MkDir(dir.clone()));
            s.push(run(Tool::Qpdf, args!["--split-pages", src, dir.join(format!("{stem}-%d.pdf"))]));
            (dir, format!("{stem} pages"))
        }
        Op::Pages { spec } if is(Category::Pdf) => {
            let out = tmp.join("pages.pdf");
            s.push(run(Tool::Qpdf, args![src, "--pages", ".", spec, "--", &out]));
            (out, named(&stem, "pages", "pdf"))
        }
        _ => return Err(unsupported("This tool", &from)),
    };
    Ok(Plan { steps: s, output, name })
}

fn strs(items: &[&str]) -> Vec<OsString> {
    items.iter().map(OsString::from).collect()
}

fn display_name(p: &Path) -> String {
    if p.is_dir() {
        p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "archive".into())
    } else {
        stem_of(p)
    }
}

/// Largest centred box with aspect `rw:rh` inside `w`x`h`.
pub fn crop_box(w: u32, h: u32, rw: u32, rh: u32) -> (u32, u32) {
    let (w64, h64, rw, rh) = (w as u64, h as u64, rw as u64, rh as u64);
    if w64 * rh > h64 * rw {
        ((h64 * rw / rh) as u32, h)
    } else {
        (w, (w64 * rh / rw) as u32)
    }
}

/// Blur or paint `boxes`. Blur crops each box from a copy, blurs it and overlays it back.
pub fn redact_graph(boxes: &[Rect], fill: Option<[u8; 3]>, range: Option<(f64, f64)>) -> String {
    let enable = match range {
        Some((a, b)) if b < f64::MAX => format!(":enable='between(t,{a},{b})'"),
        Some((a, _)) => format!(":enable='gte(t,{a})'"),
        None => String::new(),
    };
    if let Some([r, g, b]) = fill {
        let draws: Vec<String> = boxes
            .iter()
            .map(|x| format!("drawbox=x={}:y={}:w={}:h={}:color=0x{r:02X}{g:02X}{b:02X}@1:t=fill{enable}", x.x, x.y, x.w, x.h))
            .collect();
        return format!("[0:v]{}[v]", draws.join(","));
    }
    let n = boxes.len();
    let mut g = format!("[0:v]split={}[base]{}", n + 1, (0..n).map(|i| format!("[s{i}]")).collect::<String>());
    for (i, b) in boxes.iter().enumerate() {
        g += &format!(";[s{i}]crop={}:{}:{}:{},gblur=sigma=30[b{i}]", b.w, b.h, b.x, b.y);
    }
    let mut prev = "base".to_string();
    for (i, b) in boxes.iter().enumerate() {
        let next = if i + 1 == n { "v".to_string() } else { format!("o{i}") };
        g += &format!(";[{prev}][b{i}]overlay={}:{}{enable}[{next}]", b.x, b.y);
        prev = next;
    }
    g
}

/// Concat graph that fits every clip onto the first clip's canvas and frame rate, and gives
/// silent clips a silent track so `concat` sees matching streams.
pub fn join_graph(inputs: &[MediaInfo], video: bool) -> String {
    let first = &inputs[0];
    let (w, h) = (first.width.max(2) & !1, first.height.max(2) & !1);
    let fps = if first.fps > 0.0 { first.fps } else { 30.0 };
    let mut g = Vec::new();
    let mut pads = String::new();
    for (i, m) in inputs.iter().enumerate() {
        if video {
            g.push(format!(
                "[{i}:v]scale={w}:{h}:force_original_aspect_ratio=decrease,pad={w}:{h}:(ow-iw)/2:(oh-ih)/2,setsar=1,fps={fps},format=yuv420p[v{i}]"
            ));
            pads += &format!("[v{i}]");
        }
        g.push(if m.has_audio {
            format!("[{i}:a]aformat=sample_rates=48000:channel_layouts=stereo[a{i}]")
        } else {
            format!("anullsrc=r=48000:cl=stereo,atrim=duration={:.3}[a{i}]", m.duration)
        });
        pads += &format!("[a{i}]");
    }
    let outs = if video { "[v][a]" } else { "[a]" };
    g.push(format!("{pads}concat=n={}:v={}:a=1{outs}", inputs.len(), video as u8));
    g.join(";")
}

/// atempo only accepts 0.5..2 per instance on older FFmpeg builds, so chain it.
pub fn atempo_chain(mut f: f64) -> String {
    let mut parts = Vec::new();
    while f > 2.0 {
        parts.push("atempo=2".to_string());
        f /= 2.0;
    }
    while f < 0.5 {
        parts.push("atempo=0.5".to_string());
        f /= 0.5;
    }
    parts.push(format!("atempo={f}"));
    parts.join(",")
}

// ---------- images ----------

/// A path libvips can load: BMP goes through FFmpeg, names with `[` (vips option syntax) are copied.
fn image_src(src: &Path, from: &str, tmp: &Path, s: &mut Vec<Step>) -> PathBuf {
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    if from == "bmp" {
        let png = tmp.join(format!("in{n}.png"));
        s.push(run(Tool::Ffmpeg, args!["-hide_banner", "-nostdin", "-y", "-loglevel", "error", "-i", src, &png]));
        return png;
    }
    if src.to_string_lossy().contains('[') {
        let copy = tmp.join(format!("in{n}.{from}"));
        s.push(Step::Copy { src: src.to_path_buf(), dst: copy.clone() });
        return copy;
    }
    if from == "svg" {
        // Rasterise SVGs at 4x so small icons do not come out blurry.
        return PathBuf::from(suffixed(src, "[scale=4]"));
    }
    src.to_path_buf()
}

fn vips_out_ext(from: &str) -> &str {
    match from {
        "jpg" | "png" | "webp" | "heic" | "avif" | "tiff" | "gif" => from,
        _ => "png",
    }
}

/// Input path, output path (with save options) and output extension for a same-format image edit.
fn image_io(src: &Path, from: &str, tmp: &Path, tag: &str, s: &mut Vec<Step>) -> (PathBuf, PathBuf, &'static str) {
    let input = image_src(src, from, tmp, s);
    let to: &'static str = match vips_out_ext(from) {
        "jpg" => "jpg",
        "webp" => "webp",
        "heic" => "heic",
        "avif" => "avif",
        "tiff" => "tiff",
        "gif" => "gif",
        _ => "png",
    };
    (input, tmp.join(format!("{tag}.{to}")), to)
}

fn images_to_jpegs(inputs: &[PathBuf], tmp: &Path, s: &mut Vec<Step>) -> Vec<PathBuf> {
    inputs
        .iter()
        .enumerate()
        .map(|(i, img)| {
            let f = ext_of(img).unwrap_or_default();
            let input = image_src(img, &f, tmp, s);
            let jpg = tmp.join(format!("p{i}.jpg"));
            s.push(run(Tool::Vips, args!["colourspace", &input, suffixed(&jpg, "[Q=92,background=255]"), "srgb"]));
            jpg
        })
        .collect()
}

// ---------- audio / video ----------

const GIF_FILTER: &str =
    "fps=12,scale='min(640,iw)':-1:flags=lanczos,split[a][b];[a]palettegen[p];[b][p]paletteuse";

fn ffmpeg_in(src: &Path) -> Vec<OsString> {
    args!["-hide_banner", "-nostdin", "-y", "-loglevel", "error", "-progress", "pipe:1", "-nostats", "-i", src]
}

fn ffmpeg(args: Vec<OsString>, ctx: &Ctx) -> Step {
    Step::Run { tool: Tool::Ffmpeg, args, cwd: None, progress_secs: ctx.media.as_ref().map(|m| m.duration) }
}

fn is_audio_ext(ext: &str) -> bool {
    matches!(ext, "mp3" | "m4a" | "wav" | "flac" | "ogg" | "opus" | "aiff" | "wma")
}

fn av_out_ext(from: &str, cat: Option<Category>) -> &'static str {
    const VIDEO: [&str; 6] = ["mp4", "mov", "mkv", "webm", "avi", "wmv"];
    const AUDIO: [&str; 8] = ["mp3", "m4a", "wav", "flac", "ogg", "opus", "aiff", "wma"];
    let pool: &[&'static str] = if cat == Some(Category::Audio) { &AUDIO } else { &VIDEO };
    pool.iter().copied().find(|e| *e == from).unwrap_or(if cat == Some(Category::Audio) { "m4a" } else { "mp4" })
}

fn video_codec(ext: &str) -> &'static [&'static str] {
    match ext {
        "webm" => &["-c:v", "libvpx-vp9", "-crf", "32", "-b:v", "0", "-row-mt", "1"],
        "avi" => &["-c:v", "mpeg4", "-q:v", "4"],
        "wmv" => &["-c:v", "wmv2", "-b:v", "2500k"],
        _ => &["-c:v", "libx264", "-preset", "medium", "-crf", "23", "-pix_fmt", "yuv420p"],
    }
}

fn audio_codec(ext: &str) -> &'static [&'static str] {
    match ext {
        "mp3" | "avi" => &["-c:a", "libmp3lame", "-q:a", "2"],
        "wav" => &["-c:a", "pcm_s16le"],
        "flac" => &["-c:a", "flac"],
        "ogg" => &["-c:a", "libvorbis", "-q:a", "5"],
        "opus" | "webm" => &["-c:a", "libopus", "-b:a", "128k"],
        "aiff" => &["-c:a", "pcm_s16be"],
        "wma" | "wmv" => &["-c:a", "wmav2", "-b:a", "192k"],
        _ => &["-c:a", "aac", "-b:a", "192k"],
    }
}

fn container_flags(ext: &str) -> &'static [&'static str] {
    match ext {
        "mp4" | "mov" | "m4a" => &["-movflags", "+faststart"],
        _ => &[],
    }
}

fn encode_args(to: &str) -> Vec<OsString> {
    if to == "gif" {
        return args!["-vf", GIF_FILTER, "-loop", "0", "-an"];
    }
    let mut a = Vec::new();
    if is_audio_ext(to) {
        a.push("-vn".into());
    } else {
        a.extend(strs(video_codec(to)));
    }
    a.extend(strs(audio_codec(to)));
    a.extend(strs(container_flags(to)));
    a
}

#[allow(clippy::too_many_arguments)]
fn audio_filter(
    src: &Path, from: &str, cat: Option<Category>, filter: &str, tag: &str, ctx: &Ctx, tmp: &Path,
    s: &mut Vec<Step>, stem: &str,
) -> (PathBuf, String) {
    let to = av_out_ext(from, cat);
    let out = tmp.join(format!("{tag}.{to}"));
    let mut a = ffmpeg_in(src);
    if cat == Some(Category::Video) {
        a.extend(args!["-c:v", "copy"]);
    }
    a.extend(args!["-af", filter]);
    a.extend(strs(audio_codec(to)));
    a.push(out.clone().into());
    s.push(ffmpeg(a, ctx));
    (out, named(stem, tag, to))
}

// ---------- compression ----------

#[allow(clippy::too_many_arguments)]
fn compress(
    src: &Path, from: &str, cat: Option<Category>, how: &Compression, max: Option<u32>, ctx: &Ctx, tmp: &Path,
    s: &mut Vec<Step>, stem: &str,
) -> Result<(PathBuf, String)> {
    let target = match how {
        Compression::TargetBytes(b) => Some(*b),
        Compression::Percent(p) => Some(ctx.size * (100 - *p as u64) / 100),
        Compression::Preset(_) => None,
    };
    let level = match how {
        Compression::Preset(l) => *l,
        _ => Level::Medium,
    };
    match cat {
        Some(Category::Image) => {
            let to = match from {
                "jpg" | "webp" | "heic" | "avif" | "png" | "tiff" => from,
                _ => "jpg",
            };
            let input = image_src(src, from, tmp, s);
            let candidate = |q: u32| -> (Vec<Step>, PathBuf) {
                let out = tmp.join(format!("q{q}.{to}"));
                let opts = match to {
                    "png" => format!("[Q={q},palette,compression=9,strip]"),
                    "tiff" => format!("[Q={q},compression=jpeg,strip]"),
                    "jpg" => format!("[Q={q},strip,background=255]"),
                    _ => format!("[Q={q},strip]"),
                };
                let dst = suffixed(&out, &opts);
                let step = match max {
                    Some(m) => run(Tool::Vips, args!["thumbnail", &input, dst, m.to_string(), "--height", m.to_string(), "--size", "down"]),
                    None => run(Tool::Vips, args!["copy", &input, dst]),
                };
                (vec![step], out)
            };
            let dst = tmp.join(format!("compressed.{to}"));
            let qualities: Vec<u32> = match (target, level) {
                (Some(_), _) => vec![90, 82, 75, 68, 60, 50, 40, 30, 20],
                (None, Level::High) => vec![82],
                (None, Level::Medium) => vec![70],
                (None, Level::Low) => vec![50],
            };
            let candidates = qualities.into_iter().map(candidate).collect();
            s.push(Step::SmallestUnder { target: target.unwrap_or(u64::MAX), candidates, dst: dst.clone() });
            Ok((dst, named(stem, "compressed", to)))
        }
        Some(Category::Pdf) => {
            let presets: &[&str] = match (target, level) {
                (Some(_), _) => &["/printer", "/ebook", "/screen"],
                (None, Level::High) => &["/printer"],
                (None, Level::Medium) => &["/ebook"],
                (None, Level::Low) => &["/screen"],
            };
            let candidates = presets
                .iter()
                .enumerate()
                .map(|(i, preset)| {
                    let out = tmp.join(format!("gs{i}.pdf"));
                    let step = run(
                        Tool::Ghostscript,
                        args!["-dSAFER", "-dBATCH", "-dNOPAUSE", "-dQUIET", "-sDEVICE=pdfwrite",
                            "-dCompatibilityLevel=1.5", format!("-dPDFSETTINGS={preset}"), prefixed("-sOutputFile=", &out), src],
                    );
                    (vec![step], out)
                })
                .collect();
            let dst = tmp.join("compressed.pdf");
            s.push(Step::SmallestUnder { target: target.unwrap_or(u64::MAX), candidates, dst: dst.clone() });
            Ok((dst, named(stem, "compressed", "pdf")))
        }
        Some(Category::Video) => {
            let to = if from == "mov" || from == "mkv" { from } else { "mp4" };
            let out = tmp.join(format!("compressed.{to}"));
            let media = ctx.media.clone().unwrap_or_default();
            let scale = max.map(|m| {
                format!("scale=w='if(gt(iw,ih),min({m},iw),-2)':h='if(gt(iw,ih),-2,min({m},ih))'")
            });
            let mut head = ffmpeg_in(src);
            if let Some(f) = &scale {
                head.extend(args!["-vf", f]);
            }
            let flags = strs(container_flags(to));
            match target {
                Some(bytes) => {
                    if media.duration <= 0.0 {
                        return Err(Error::Unsupported("could not read the video duration".into()));
                    }
                    // 5% headroom for container overhead.
                    let total_k = bytes as f64 * 8.0 / 1000.0 / media.duration * 0.95;
                    let audio_k = if media.has_audio { (total_k / 4.0).clamp(32.0, 128.0) } else { 0.0 };
                    let video_k = total_k - audio_k;
                    if video_k < 50.0 {
                        return Err(Error::BadOption(format!("target too small for a {:.0}s video", media.duration)));
                    }
                    let vb = format!("{video_k:.0}k");
                    let log = tmp.join("pass");
                    let null = if cfg!(windows) { "NUL" } else { "/dev/null" };
                    let mut p1 = head.clone();
                    p1.extend(args!["-c:v", "libx264", "-preset", "medium", "-b:v", &vb, "-pass", "1", "-passlogfile", &log, "-an", "-f", "null", null]);
                    let mut p2 = head;
                    p2.extend(args!["-c:v", "libx264", "-preset", "medium", "-b:v", &vb, "-pass", "2", "-passlogfile", &log, "-pix_fmt", "yuv420p"]);
                    if media.has_audio {
                        p2.extend(args!["-c:a", "aac", "-b:a", format!("{audio_k:.0}k")]);
                    }
                    p2.extend(flags);
                    p2.push(out.clone().into());
                    s.push(ffmpeg(p1, ctx));
                    s.push(ffmpeg(p2, ctx));
                }
                None => {
                    let crf = match level {
                        Level::High => "23",
                        Level::Medium => "28",
                        Level::Low => "32",
                    };
                    head.extend(args!["-c:v", "libx264", "-preset", "medium", "-crf", crf, "-pix_fmt", "yuv420p", "-c:a", "aac", "-b:a", "128k"]);
                    head.extend(flags);
                    head.push(out.clone().into());
                    s.push(ffmpeg(head, ctx));
                }
            }
            Ok((out, named(stem, "compressed", to)))
        }
        Some(Category::Audio) => {
            let to = match from {
                "mp3" | "m4a" | "ogg" | "opus" | "wma" => from,
                _ => "m4a",
            };
            let kbps = match target {
                Some(bytes) => {
                    let d = ctx.media.as_ref().map(|m| m.duration).filter(|d| *d > 0.0)
                        .ok_or_else(|| Error::Unsupported("could not read the audio duration".into()))?;
                    (bytes as f64 * 8.0 / 1000.0 / d * 0.97).clamp(24.0, 320.0) as u32
                }
                None => match level {
                    Level::High => 192,
                    Level::Medium => 128,
                    Level::Low => 96,
                },
            };
            let codec = match to {
                "mp3" => "libmp3lame",
                "ogg" => "libvorbis",
                "opus" => "libopus",
                "wma" => "wmav2",
                _ => "aac",
            };
            let out = tmp.join(format!("compressed.{to}"));
            let mut a = ffmpeg_in(src);
            a.extend(args!["-vn", "-c:a", codec, "-b:a", format!("{kbps}k")]);
            a.extend(strs(container_flags(to)));
            a.push(out.clone().into());
            s.push(ffmpeg(a, ctx));
            Ok((out, named(stem, "compressed", to)))
        }
        _ => Err(unsupported("Compress", from)),
    }
}

// ---------- documents ----------

/// `file:///` URL for LibreOffice's `-env:UserInstallation`.
pub fn file_url(p: &Path) -> String {
    let raw = p.to_string_lossy().replace('\\', "/");
    let mut url = String::from(if raw.starts_with('/') { "file://" } else { "file:///" });
    for b in raw.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~/:".contains(&b) {
            url.push(b as char);
        } else {
            url.push_str(&format!("%{b:02X}"));
        }
    }
    url
}

fn soffice(src: &Path, filter: &str, infilter: Option<&str>, tmp: &Path, s: &mut Vec<Step>) -> PathBuf {
    let outdir = tmp.join("lo");
    s.push(Step::MkDir(outdir.clone()));
    // Private profile per job: concurrent soffice runs otherwise fight over one profile lock.
    let mut a = args![
        "--headless", "--norestore", "--nolockcheck",
        format!("-env:UserInstallation={}", file_url(&tmp.join("lo-profile")))
    ];
    if let Some(f) = infilter {
        a.push(format!("--infilter={f}").into());
    }
    a.extend(args!["--convert-to", filter, "--outdir", &outdir, src]);
    s.push(run(Tool::LibreOffice, a));
    let stem = src.file_stem().unwrap_or(OsStr::new("out"));
    let ext = filter.split(':').next().unwrap_or(filter);
    let mut name = stem.to_owned();
    name.push(format!(".{ext}"));
    outdir.join(name)
}

const PANDOC_IN: [&str; 7] = ["md", "txt", "html", "docx", "odt", "epub", "rtf"];

fn pandoc(src: &Path, from: &str, to: &str, tmp: &Path, s: &mut Vec<Step>) -> PathBuf {
    let reader = match from {
        "md" | "txt" => "markdown",
        other => other,
    };
    let writer = match to {
        "md" => "gfm",
        "txt" => "plain",
        other => other,
    };
    let out = tmp.join(format!("pandoc.{to}"));
    s.push(run(Tool::Pandoc, args!["-s", "-f", reader, "-t", writer, "-o", &out, src]));
    out
}

fn calibre(src: &Path, to: &str, tmp: &Path, s: &mut Vec<Step>) -> PathBuf {
    let out = tmp.join(format!("ebook.{to}"));
    s.push(run(Tool::Calibre, args![src, &out]));
    out
}

fn pdf_to_images(src: &Path, to: &str, tmp: &Path, s: &mut Vec<Step>) -> PathBuf {
    let dir = tmp.join("pages");
    s.push(Step::MkDir(dir.clone()));
    let device = if to == "png" { "-sDEVICE=png16m" } else { "-sDEVICE=jpeg" };
    s.push(run(
        Tool::Ghostscript,
        args!["-dSAFER", "-dBATCH", "-dNOPAUSE", "-dQUIET", "-r300", device, "-dJPEGQ=90",
            prefixed("-sOutputFile=", &dir.join(format!("page-%03d.{to}"))), src],
    ));
    dir
}

fn convert(src: &Path, from: &str, to: &str, tmp: &Path, ctx: &Ctx, s: &mut Vec<Step>) -> Result<PathBuf> {
    let cat = category_of_ext(from).ok_or_else(|| unsupported("Converting", from))?;
    if !crate::format::targets(src).contains(&to) {
        return Err(Error::Unsupported(format!("cannot convert .{from} to .{to}")));
    }
    // Two-hop conversions: produce `mid` first, then convert that.
    let via = |mid: &str, s: &mut Vec<Step>| -> Result<PathBuf> {
        let first = convert(src, from, mid, tmp, ctx, s)?;
        let next_tmp = tmp.join(format!("via-{mid}"));
        s.push(Step::MkDir(next_tmp.clone()));
        convert_from(&first, mid, to, &next_tmp, ctx, s)
    };
    Ok(match cat {
        Category::Subtitle => {
            let out = tmp.join(format!("sub.{to}"));
            s.push(Step::Subtitle { src: src.to_path_buf(), dst: out.clone() });
            out
        }
        Category::Image if to == "pdf" => {
            let jpegs = images_to_jpegs(&[src.to_path_buf()], tmp, s);
            let out = tmp.join("image.pdf");
            s.push(Step::JpegsToPdf { jpegs, dst: out.clone() });
            out
        }
        Category::Image if to == "bmp" => {
            let input = image_src(src, from, tmp, s);
            let png = tmp.join("to-bmp.png");
            s.push(run(Tool::Vips, args!["copy", &input, &png]));
            let out = tmp.join("image.bmp");
            s.push(run(Tool::Ffmpeg, args!["-hide_banner", "-nostdin", "-y", "-loglevel", "error", "-i", &png, &out]));
            out
        }
        Category::Image => {
            let input = image_src(src, from, tmp, s);
            let out = tmp.join(format!("image.{to}"));
            let opts = if to == "jpg" { "[Q=90,background=255]" } else { "" };
            s.push(run(Tool::Vips, args!["copy", &input, suffixed(&out, opts)]));
            out
        }
        Category::Video | Category::Audio => {
            let out = tmp.join(format!("media.{to}"));
            let mut a = ffmpeg_in(src);
            a.extend(encode_args(to));
            a.push(out.clone().into());
            s.push(ffmpeg(a, ctx));
            out
        }
        Category::Pdf => match to {
            "jpg" | "png" => pdf_to_images(src, to, tmp, s),
            "txt" => {
                let out = tmp.join("text.txt");
                s.push(run(
                    Tool::Ghostscript,
                    args!["-dSAFER", "-dBATCH", "-dNOPAUSE", "-dQUIET", "-sDEVICE=txtwrite", prefixed("-sOutputFile=", &out), src],
                ));
                out
            }
            "docx" => soffice(src, "docx:MS Word 2007 XML", Some("writer_pdf_import"), tmp, s),
            "pptx" => soffice(src, "pptx", Some("impress_pdf_import"), tmp, s),
            _ => calibre(src, to, tmp, s),
        },
        Category::Document => match to {
            "pdf" if from == "md" => via("docx", s)?,
            "md" | "epub" if PANDOC_IN.contains(&from) => pandoc(src, from, to, tmp, s),
            "md" | "epub" => via("docx", s)?,
            _ if from == "md" => pandoc(src, from, to, tmp, s),
            "txt" => soffice(src, "txt:Text (encoded):UTF8", None, tmp, s),
            _ => soffice(src, to, None, tmp, s),
        },
        Category::Slides => match to {
            "png" | "jpg" => via("pdf", s)?,
            _ => soffice(src, to, None, tmp, s),
        },
        Category::Spreadsheet => {
            let infilter = match from {
                "csv" => Some("CSV:44,34,UTF8"),
                "tsv" => Some("CSV:9,34,UTF8"),
                _ => None,
            };
            let filter = match to {
                "csv" => "csv:Text - txt - csv (StarCalc):44,34,UTF8",
                "tsv" => "csv:Text - txt - csv (StarCalc):9,34,UTF8",
                other => other,
            };
            soffice(src, filter, infilter, tmp, s)
        }
        Category::Ebook => match to {
            "md" | "html" if from == "epub" => pandoc(src, from, to, tmp, s),
            "md" | "html" => via("epub", s)?,
            _ => calibre(src, to, tmp, s),
        },
        Category::Archive => {
            let dir = extract(src, tmp, s);
            pack(&[dir.join("*")], to, tmp, s)
        }
    })
}

/// Second hop of a chain: the intermediate has a known extension but a temp name.
fn convert_from(src: &Path, from: &str, to: &str, tmp: &Path, ctx: &Ctx, s: &mut Vec<Step>) -> Result<PathBuf> {
    let cat = category_of_ext(from).ok_or_else(|| unsupported("Converting", from))?;
    Ok(match (cat, to) {
        (Category::Pdf, "jpg" | "png") => pdf_to_images(src, to, tmp, s),
        (Category::Document, "pdf") => soffice(src, "pdf", None, tmp, s),
        (Category::Document | Category::Ebook, _) => pandoc(src, from, to, tmp, s),
        _ => convert(src, from, to, tmp, ctx, s)?,
    })
}

// ---------- archives ----------

fn extract(src: &Path, tmp: &Path, s: &mut Vec<Step>) -> PathBuf {
    let dir = tmp.join("extracted");
    let name = src.file_name().map(|n| n.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    // 7-Zip unwraps one layer at a time: .tar.gz -> .tar -> files.
    let inner = [".tar.gz", ".tar.bz2", ".tar.xz"]
        .iter()
        .find(|e| name.ends_with(*e))
        .map(|e| format!("{}.tar", stem_tar(src, e.len())))
        .or_else(|| name.ends_with(".tgz").then(|| format!("{}.tar", stem_of(src))));
    match inner {
        Some(tar) => {
            let layer = tmp.join("layer");
            s.push(run(Tool::SevenZip, args!["x", "-y", prefixed("-o", &layer), "--", src]));
            s.push(run(Tool::SevenZip, args!["x", "-y", prefixed("-o", &dir), "--", layer.join(tar)]));
        }
        None => s.push(run(Tool::SevenZip, args!["x", "-y", prefixed("-o", &dir), "--", src])),
    }
    dir
}

fn stem_tar(src: &Path, suffix_len: usize) -> String {
    let name = src.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    name[..name.len() - suffix_len].to_string()
}

fn pack(inputs: &[PathBuf], kind: &str, tmp: &Path, s: &mut Vec<Step>) -> PathBuf {
    let seven = |t: &str, out: &Path, files: &[PathBuf]| {
        let mut a = args!["a", "-y", format!("-t{t}"), out, "--"];
        a.extend(files.iter().map(OsString::from));
        run(Tool::SevenZip, a)
    };
    match kind {
        "tgz" => {
            let tar = tmp.join("archive.tar");
            let out = tmp.join("archive.tgz");
            s.push(seven("tar", &tar, inputs));
            s.push(seven("gzip", &out, &[tar]));
            out
        }
        k => {
            let out = tmp.join(format!("archive.{k}"));
            s.push(seven(k, &out, inputs));
            out
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn plan_of(id: &str, kv: &[(&str, &str)], files: &[&str], ctx: Ctx) -> Result<Plan> {
        let op = if let Some(to) = id.strip_prefix("to:") {
            Op::Convert { to: to.into() }
        } else {
            let o: HashMap<String, String> = kv.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
            Op::parse(id, &o).unwrap()
        };
        let inputs: Vec<PathBuf> = files.iter().map(PathBuf::from).collect();
        plan(&op, &inputs, &ctx, Path::new("/t"))
    }

    fn argv(p: &Plan) -> Vec<(Tool, String)> {
        let mut out = Vec::new();
        fn walk(steps: &[Step], out: &mut Vec<(Tool, String)>) {
            for st in steps {
                match st {
                    Step::Run { tool, args, .. } => out.push((
                        *tool,
                        args.iter().map(|a| a.to_string_lossy().into_owned()).collect::<Vec<_>>().join(" "),
                    )),
                    Step::SmallestUnder { candidates, .. } => candidates.iter().for_each(|(c, _)| walk(c, out)),
                    _ => {}
                }
            }
        }
        walk(&p.steps, &mut out);
        out
    }

    fn media(d: f64) -> Ctx {
        Ctx { media: Some(MediaInfo { duration: d, has_video: true, has_audio: true, width: 640, height: 360, fps: 25.0 }), ..Ctx::default() }
    }

    #[test]
    fn image_and_media_conversions() {
        let p = plan_of("to:webp", &[], &["/in/a.png"], Ctx::default()).unwrap();
        assert_eq!(p.name, "a.webp");
        assert_eq!(argv(&p), vec![(Tool::Vips, "copy /in/a.png /t/image.webp".into())]);

        let p = plan_of("to:jpg", &[], &["/in/a.heic"], Ctx::default()).unwrap();
        assert!(argv(&p)[0].1.ends_with("image.jpg[Q=90,background=255]"));

        let p = plan_of("to:png", &[], &["/in/a.bmp"], Ctx::default()).unwrap();
        let a = argv(&p);
        assert_eq!(a[0].0, Tool::Ffmpeg);
        assert_eq!(a[1].0, Tool::Vips);

        let p = plan_of("to:mp3", &[], &["/in/v.mov"], media(10.0)).unwrap();
        let (tool, a) = &argv(&p)[0];
        assert_eq!(*tool, Tool::Ffmpeg);
        assert!(a.contains("-vn -c:a libmp3lame") && a.ends_with("/t/media.mp3"));
        assert!(matches!(p.steps[0], Step::Run { progress_secs: Some(d), .. } if d == 10.0));

        assert!(plan_of("to:mp4", &[], &["/in/a.png"], Ctx::default()).is_err());
    }

    #[test]
    fn document_routes() {
        let p = plan_of("to:pdf", &[], &["/in/r.docx"], Ctx::default()).unwrap();
        let a = argv(&p);
        assert_eq!(a[0].0, Tool::LibreOffice);
        assert!(a[0].1.contains("-env:UserInstallation=file:///t/lo-profile") && a[0].1.contains("--convert-to pdf"));
        assert_eq!(p.output, PathBuf::from("/t/lo/r.pdf"));

        let p = plan_of("to:pdf", &[], &["/in/notes.md"], Ctx::default()).unwrap();
        let tools: Vec<Tool> = argv(&p).into_iter().map(|x| x.0).collect();
        assert_eq!(tools, vec![Tool::Pandoc, Tool::LibreOffice]);

        let p = plan_of("to:epub", &[], &["/in/book.pdf"], Ctx::default()).unwrap();
        assert_eq!(argv(&p)[0].0, Tool::Calibre);

        let p = plan_of("to:png", &[], &["/in/deck.pptx"], Ctx::default()).unwrap();
        let tools: Vec<Tool> = argv(&p).into_iter().map(|x| x.0).collect();
        assert_eq!(tools, vec![Tool::LibreOffice, Tool::Ghostscript]);
        assert_eq!(p.name, "deck pages");

        let p = plan_of("to:tsv", &[], &["/in/s.xlsx"], Ctx::default()).unwrap();
        assert!(argv(&p)[0].1.contains("StarCalc):9,34,UTF8"));

        let p = plan_of("to:md", &[], &["/in/b.mobi"], Ctx::default()).unwrap();
        let tools: Vec<Tool> = argv(&p).into_iter().map(|x| x.0).collect();
        assert_eq!(tools, vec![Tool::Calibre, Tool::Pandoc]);
    }

    #[test]
    fn archives() {
        let p = plan_of("to:zip", &[], &["/in/x.tar.gz"], Ctx::default()).unwrap();
        let a = argv(&p);
        assert_eq!(a.len(), 3);
        assert!(a[1].1.contains("/t/layer/x.tar"));
        assert!(a[2].1.starts_with("a -y -tzip /t/archive.zip -- /t/extracted/*"));
        assert_eq!(p.name, "x.zip");

        let p = plan_of("archive", &[("format", "tgz")], &["/in/a.txt", "/in/b.txt"], Ctx::default()).unwrap();
        assert_eq!(p.name, "archive.tar.gz");
        assert_eq!(argv(&p).len(), 2);
    }

    #[test]
    fn compress_strategies() {
        let ctx = Ctx { size: 1_000_000, ..Ctx::default() };
        let p = plan_of("compress", &[("percent", "50")], &["/in/a.jpg"], ctx).unwrap();
        match &p.steps[0] {
            Step::SmallestUnder { target, candidates, .. } => {
                assert_eq!(*target, 500_000);
                assert_eq!(candidates.len(), 9);
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(p.name, "a-compressed.jpg");

        let p = plan_of("compress", &[("target", "10MB")], &["/in/v.mov"], media(100.0)).unwrap();
        let a = argv(&p);
        assert_eq!(a.len(), 2, "two-pass");
        // 10MB over 100s = 800 kbps * 0.95 = 760; audio gets 128 (clamped), video 632.
        assert!(a[0].1.contains("-b:v 632k -pass 1") && a[1].1.contains("-b:a 128k"));
        assert!(plan_of("compress", &[("target", "10KB")], &["/in/v.mp4"], media(100.0)).is_err());

        let p = plan_of("compress", &[("preset", "low")], &["/in/d.pdf"], Ctx::default()).unwrap();
        assert!(argv(&p)[0].1.contains("-dPDFSETTINGS=/screen"));
    }

    #[test]
    fn redact_join_split_channels() {
        let b = [Rect { x: 10, y: 20, w: 100, h: 50 }, Rect { x: 0, y: 0, w: 8, h: 8 }];
        assert_eq!(
            redact_graph(&b, None, None),
            "[0:v]split=3[base][s0][s1];[s0]crop=100:50:10:20,gblur=sigma=30[b0];[s1]crop=8:8:0:0,gblur=sigma=30[b1];\
             [base][b0]overlay=10:20[o0];[o0][b1]overlay=0:0[v]"
        );
        assert_eq!(
            redact_graph(&b[..1], Some([0, 0, 0]), Some((1.0, 2.5))),
            "[0:v]drawbox=x=10:y=20:w=100:h=50:color=0x000000@1:t=fill:enable='between(t,1,2.5)'[v]"
        );
        let p = plan_of("redact", &[("boxes", "1,1,4,4")], &["/in/a.heic"], Ctx::default()).unwrap();
        let tools: Vec<Tool> = argv(&p).into_iter().map(|x| x.0).collect();
        assert_eq!(tools, vec![Tool::Vips, Tool::Ffmpeg]);
        assert_eq!(p.name, "a-redacted.png");

        let clips = vec![
            MediaInfo { duration: 2.0, has_video: true, has_audio: true, width: 1280, height: 720, fps: 30.0 },
            MediaInfo { duration: 3.0, has_video: true, has_audio: false, width: 640, height: 480, fps: 25.0 },
        ];
        let g = join_graph(&clips, true);
        assert!(g.contains("[1:v]scale=1280:720:force_original_aspect_ratio=decrease,pad=1280:720"));
        assert!(g.contains("anullsrc=r=48000:cl=stereo,atrim=duration=3.000[a1]"));
        assert!(g.ends_with("[v0][a0][v1][a1]concat=n=2:v=1:a=1[v][a]"));
        let ctx = Ctx { inputs: clips, ..Ctx::default() };
        let p = plan_of("join", &[], &["/in/a.mp4", "/in/b.mov"], ctx).unwrap();
        assert!(matches!(p.steps[0], Step::Run { progress_secs: Some(t), .. } if t == 5.0));

        let p = plan_of("split", &[("at", "5,1:00")], &["/in/v.mp4"], Ctx::default()).unwrap();
        assert!(argv(&p)[0].1.contains("-segment_times 5.000,60.000"));
        let p = plan_of("channels", &[("count", "2"), ("left", "0")], &["/in/a.mp3"], media(3.0)).unwrap();
        assert!(argv(&p)[0].1.contains("pan=stereo|c0=0*c0|c1=1*c1"));
        let p = plan_of("crop", &[("box", "1,2,101,51")], &["/in/v.mp4"], media(3.0)).unwrap();
        assert!(argv(&p)[0].1.contains("crop=100:50:1:2"));
    }

    #[test]
    fn helpers() {
        assert_eq!(crop_box(1920, 1080, 1, 1), (1080, 1080));
        assert_eq!(crop_box(1000, 2000, 16, 9), (1000, 562));
        assert_eq!(atempo_chain(4.0), "atempo=2,atempo=2");
        assert_eq!(atempo_chain(0.25), "atempo=0.5,atempo=0.5");
        assert_eq!(file_url(Path::new("C:\\Users\\A B\\t")), "file:///C:/Users/A%20B/t");
        assert_eq!(file_url(Path::new("/tmp/x")), "file:///tmp/x");
    }

    #[test]
    fn tools_produce_named_outputs() {
        let img = Ctx { image: Some(ImageInfo { width: 400, height: 300, bands: 4 }), ..Ctx::default() };
        let p = plan_of("background", &[("color", "#000"), ("padding", "10")], &["/in/a.png"], img).unwrap();
        assert!(argv(&p)[0].1.contains("420 320 --extend background --background 0 0 0 255"));
        let p = plan_of("split", &[("parts", "4")], &["/in/v.mp4"], media(60.0)).unwrap();
        assert_eq!(p.name, "v parts");
        assert!(argv(&p)[0].1.contains("-segment_time 15.001"));
        let p = plan_of("merge-pdf", &[], &["/in/a.pdf", "/in/b.pdf"], Ctx::default()).unwrap();
        assert_eq!(argv(&p)[0].1, "--empty --pages /in/a.pdf /in/b.pdf -- /t/merged.pdf");
        let p = plan_of("ocr", &[("to", "txt"), ("lang", "ara")], &["/in/scan.pdf"], Ctx::default()).unwrap();
        assert_eq!(p.output, PathBuf::from("/t/ocr.txt"));
        assert!(plan_of("crop", &[], &["/in/a.png"], Ctx::default()).is_err(), "needs image size");
        assert!(plan_of("extract", &[], &["/in/a.png"], Ctx::default()).is_err());
    }
}
