//! Turns an operation on some files into concrete engine invocations.
//!
//! Planning is pure: it only builds argv lists inside a scratch dir. `runner` executes them,
//! which keeps every routing decision unit-testable without the engines installed.

use crate::format::{category_of_ext, ext_of, stem_of, Category};
use crate::ops::{Adjust, Compression, CropSpec, Level, Op, Rect, Size};
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
#[derive(Clone, Debug)]
pub struct Ctx {
    pub media: Option<MediaInfo>,
    /// Every input's media info, for ops that combine files (join). Empty otherwise.
    pub inputs: Vec<MediaInfo>,
    /// Text boxes found by OCR for `redact --boxes auto`.
    pub detected: Vec<Rect>,
    /// Pixel size of the first input, for ops that size things relative to it.
    pub dims: Option<(u32, u32)>,
    pub whisper_model: Option<PathBuf>,
    /// Real-ESRGAN's `models` folder (AI upscale).
    pub esrgan_models: Option<PathBuf>,
    /// rembg's ONNX model file (AI background removal).
    pub rembg_model: Option<PathBuf>,
    /// ExifTool is installed (lossless metadata removal for images).
    pub exiftool: bool,
    /// libvips is installed. When it is not, image work falls back to the bundled FFmpeg so a
    /// plain install converts images without asking the user to install another package.
    pub vips: bool,
    pub image: Option<ImageInfo>,
    pub size: u64,
}

impl Default for Ctx {
    fn default() -> Self {
        // Plans assume libvips unless the runner says otherwise: that keeps the routing tests
        // (which describe the preferred tool chain) independent of what this machine has.
        Ctx {
            media: None,
            inputs: Vec::new(),
            detected: Vec::new(),
            dims: None,
            whisper_model: None,
            esrgan_models: None,
            rembg_model: None,
            exiftool: false,
            vips: true,
            image: None,
            size: 0,
        }
    }
}

#[derive(Debug)]
pub enum Step {
    Run {
        tool: Tool,
        args: Vec<OsString>,
        cwd: Option<PathBuf>,
        progress_secs: Option<f64>,
    },
    MkDir(PathBuf),
    Copy {
        src: PathBuf,
        dst: PathBuf,
    },
    WriteFile {
        path: PathBuf,
        contents: String,
    },
    Subtitle {
        src: PathBuf,
        dst: PathBuf,
        shift_ms: i64,
    },
    Ico {
        pngs: Vec<(u32, PathBuf)>,
        dst: PathBuf,
    },
    /// PPM in, dehazed PPM out (pure Rust, see `dehaze`).
    Dehaze {
        src: PathBuf,
        dst: PathBuf,
        strength: f32,
    },
    JpegsToPdf {
        jpegs: Vec<PathBuf>,
        dst: PathBuf,
    },
    /// Extract MP4 video from a Motion Photo or Live Photo
    ExtractMotion {
        src: PathBuf,
        dst: PathBuf,
    },
    /// Try candidates in order; keep the first output that fits `target`, else the smallest.
    SmallestUnder {
        target: u64,
        candidates: Vec<(Vec<Step>, PathBuf)>,
        dst: PathBuf,
    },
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
        Op::Convert { to, quality } => {
            let out = convert(src, &from, to, *quality, tmp, ctx, &mut s)?;
            let name = if out.extension().is_none() { format!("{stem} pages") } else { named(&stem, "", to) };
            (out, name)
        }
        Op::ExtractMotion => {
            let out = tmp.join("motion.mp4");
            s.push(Step::ExtractMotion { src: src.to_path_buf(), dst: out.clone() });
            (out, named(&stem, "motion", "mp4"))
        }
        Op::Compress { how, max } => compress(src, &from, cat, how, *max, ctx, tmp, &mut s, &stem)?,
        Op::StripMetadata => {
            if is(Category::Image) && ctx.exiftool && from != "svg" {
                // Lossless: ExifTool rewrites only the metadata, never the pixels.
                let out = tmp.join(format!("clean.{from}"));
                s.push(run(Tool::ExifTool, args!["-q", "-all=", "-o", &out, "--", src]));
                (out, named(&stem, "clean", &from))
            } else if is(Category::Image) && from != "bmp" && from != "svg" {
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
            } else if is(Category::Pdf) {
                // A document rebuilt from an empty one keeps the pages but not /Info or XMP
                // (works on every qpdf; --remove-info needs 11.10+).
                let out = tmp.join("clean.pdf");
                s.push(run(Tool::Qpdf, args!["--empty", "--pages", src, "1-z", "--", &out]));
                (out, named(&stem, "clean", "pdf"))
            } else {
                return Err(unsupported("Removing metadata", &from));
            }
        }
        Op::Info => return Err(Error::Unsupported("info is read with runner::info".into())),
        Op::Archive { kind, password, split } => {
            let base = if inputs.len() == 1 { display_name(src) } else { "archive".into() };
            match split {
                // Volumes (name.zip.001, .002...) go into one folder.
                Some(bytes) => {
                    let dir = tmp.join("parts");
                    s.push(Step::MkDir(dir.clone()));
                    pack(inputs, kind, &dir, &mut s, password.as_deref(), Some(*bytes));
                    (dir, format!("{base} parts"))
                }
                None => (pack(inputs, kind, tmp, &mut s, password.as_deref(), None), named(&base, "", kind)),
            }
        }
        Op::Extract { password } if is(Category::Archive) => {
            (extract(src, tmp, &mut s, password.as_deref()), stem.clone())
        }
        Op::Exif { tags, gps, remove_gps } if is(Category::Image) => {
            let out = tmp.join(format!("exif.{from}"));
            let mut a = args!["-q", "-o", &out];
            if *remove_gps {
                a.push("-gps:all=".into());
            }
            for (tag, value) in tags {
                a.push(format!("-{tag}={value}").into());
            }
            if let Some((lat, lon)) = gps {
                a.extend(args![
                    format!("-GPSLatitude={}", lat.abs()),
                    format!("-GPSLatitudeRef={}", if *lat < 0.0 { "S" } else { "N" }),
                    format!("-GPSLongitude={}", lon.abs()),
                    format!("-GPSLongitudeRef={}", if *lon < 0.0 { "W" } else { "E" })
                ]);
            }
            a.extend(args!["--", src]);
            s.push(run(Tool::ExifTool, a));
            (out, named(&stem, "exif", &from))
        }
        Op::Social { ratio } if is(Category::Image) || is(Category::Video) => {
            let (w, h) = social_canvas(*ratio);
            let graph = format!(
                "[0:v]split[a][b];[a]scale={w}:{h}:force_original_aspect_ratio=increase,crop={w}:{h},boxblur=24:2[bg];\
                 [b]scale={w}:{h}:force_original_aspect_ratio=decrease[fg];[bg][fg]overlay=(W-w)/2:(H-h)/2,setsar=1[v]"
            );
            let tag = format!("{}x{}", ratio.0, ratio.1);
            visual_filter(src, &from, cat, &graph, &tag, ctx, tmp, &mut s, &stem)
        }
        Op::ContactSheet if is(Category::Video) => {
            let dur = ctx.media.as_ref().map(|m| m.duration).filter(|d| *d > 0.0)
                .ok_or_else(|| Error::Unsupported("could not read the duration".into()))?;
            let out = tmp.join("sheet.jpg");
            let mut a = ffmpeg_in(src);
            a.extend(args![
                "-vf", format!("fps={:.5},scale=480:-2,tile=4x4:padding=6:margin=6", SHEET_TILES as f64 / dur),
                "-frames:v", "1", "-q:v", "3", &out
            ]);
            s.push(ffmpeg(a, ctx));
            (out, named(&stem, "sheet", "jpg"))
        }
        Op::VideoHeight { height } if is(Category::Video) => {
            let to = av_out_ext(&from, cat);
            let out = tmp.join(format!("resized.{to}"));
            let mut a = ffmpeg_in(src);
            a.extend(args!["-vf", format!("scale=-2:'min({height},ih)'"), "-c:a", "copy"]);
            a.extend(strs(video_codec(to)));
            a.push(out.clone().into());
            s.push(ffmpeg(a, ctx));
            (out, named(&stem, &format!("{height}p"), to))
        }
        Op::ReplaceAudio => {
            let find = |c: Category| inputs.iter().find(|p| crate::format::category_of(p) == Some(c));
            let (Some(video), Some(audio)) = (find(Category::Video), find(Category::Audio)) else {
                return Err(Error::BadOption("select one video and one audio file".into()));
            };
            let to = av_out_ext(&ext_of(video).unwrap_or_default(), Some(Category::Video));
            let out = tmp.join(format!("newaudio.{to}"));
            let mut a = ffmpeg_in(video);
            a.extend(args!["-i", audio, "-map", "0:v:0", "-map", "1:a:0", "-c:v", "copy"]);
            a.extend(strs(audio_codec(to)));
            a.extend(args!["-shortest", &out]);
            s.push(run(Tool::Ffmpeg, a));
            (out, named(&stem_of(video), "newaudio", to))
        }
        Op::DenoiseAudio if av => audio_filter(src, &from, cat, "afftdn=nf=-25", "denoised", ctx, tmp, &mut s, &stem),
        Op::RemoveSilence if is(Category::Audio) => audio_filter(
            src,
            &from,
            cat,
            "silenceremove=start_periods=1:start_threshold=-45dB:stop_periods=-1:stop_duration=0.6:stop_threshold=-45dB",
            "nosilence",
            ctx,
            tmp,
            &mut s,
            &stem,
        ),
        Op::WebSet if is(Category::Image) => {
            let dir = tmp.join("web");
            s.push(Step::MkDir(dir.clone()));
            let input = image_src(src, &from, tmp, &mut s);
            let source_w = ctx.dims.map_or(u32::MAX, |(w, _)| w);
            let mut widths: Vec<u32> = WEB_WIDTHS.iter().copied().filter(|w| *w <= source_w).collect();
            if widths.is_empty() {
                widths.push(source_w);
            }
            let srcset = |ext: &str, opts: &str, s: &mut Vec<Step>| {
                let mut set = Vec::new();
                for w in &widths {
                    let file = format!("{stem}-{w}.{ext}");
                    let dst = suffixed(&dir.join(&file), opts);
                    s.push(run(Tool::Vips, args!["thumbnail", &input, dst, w.to_string(), "--size", "down"]));
                    set.push(format!("{file} {w}w"));
                }
                set.join(", ")
            };
            let avif = srcset("avif", "[Q=55,strip]", &mut s);
            let webp = srcset("webp", "[Q=80,strip]", &mut s);
            let fallback = format!("{stem}-{}.webp", widths[widths.len() / 2]);
            let html = format!(
                "<picture>\n  <source type=\"image/avif\" srcset=\"{avif}\" sizes=\"100vw\">\n  \
                 <source type=\"image/webp\" srcset=\"{webp}\" sizes=\"100vw\">\n  \
                 <img src=\"{fallback}\" alt=\"\" loading=\"lazy\" decoding=\"async\">\n</picture>\n"
            );
            s.push(Step::WriteFile { path: dir.join(format!("{stem}.html")), contents: html });
            (dir, format!("{stem} web"))
        }
        Op::Dehaze { strength } if is(Category::Image) => {
            let (input, to) = ffmpeg_image_input(src, &from, tmp, &mut s);
            let (raw_in, raw_out) = (tmp.join("in.ppm"), tmp.join("out.ppm"));
            let out = tmp.join(format!("dehazed.{to}"));
            s.push(run(
                Tool::Ffmpeg,
                args!["-hide_banner", "-nostdin", "-y", "-loglevel", "error", "-i", &input, "-frames:v", "1", "-pix_fmt", "rgb24", &raw_in],
            ));
            s.push(Step::Dehaze { src: raw_in, dst: raw_out.clone(), strength: *strength as f32 });
            s.push(run(
                Tool::Ffmpeg,
                args!["-hide_banner", "-nostdin", "-y", "-loglevel", "error", "-i", &raw_out, "-frames:v", "1", "-q:v", "2", &out],
            ));
            (out, named(&stem, "dehazed", to))
        }
        Op::QrRead | Op::Duplicates { .. } => return Err(Error::Unsupported("handled by the runner".into())),
        Op::Resize(size) if is(Category::Image) => {
            let (out, to) = if ctx.vips {
                let (input, out, to) = image_io(src, &from, tmp, "resized", &mut s);
                s.push(match size {
                    Size::Percent(p) => run(Tool::Vips, args!["resize", &input, &out, format!("{}", *p as f64 / 100.0)]),
                    Size::Max(m) => run(
                        Tool::Vips,
                        args!["thumbnail", &input, &out, m.to_string(), "--height", m.to_string(), "--size", "down"],
                    ),
                });
                (out, to)
            } else {
                let (input, out, to) = image_io_ffmpeg(src, &from, tmp, "resized");
                let vf = match size {
                    Size::Percent(p) => format!("scale=iw*{}:ih*{}", *p as f64 / 100.0, *p as f64 / 100.0),
                    Size::Max(m) => {
                        format!("scale='min(iw,{m})':'min(ih,{m})':force_original_aspect_ratio=decrease")
                    }
                };
                ffmpeg_still(&input, &out, to, None, Some(&vf), &mut s);
                (out, to)
            };
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
            let (out, to) = if ctx.vips {
                let (input, out, to) = image_io(src, &from, tmp, "cropped", &mut s);
                s.push(run(
                    Tool::Vips,
                    args!["extract_area", &input, &out, left.to_string(), top.to_string(), w.to_string(), h.to_string()],
                ));
                (out, to)
            } else {
                let (input, out, to) = image_io_ffmpeg(src, &from, tmp, "cropped");
                let vf = format!("crop={w}:{h}:{left}:{top}");
                ffmpeg_still(&input, &out, to, None, Some(&vf), &mut s);
                (out, to)
            };
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
                let (out, to) = if ctx.vips {
                    let (input, out, to) = image_io(src, &from, tmp, "rotated", &mut s);
                    s.push(run(Tool::Vips, args!["rot", &input, &out, format!("d{degrees}")]));
                    (out, to)
                } else {
                    let (input, out, to) = image_io_ffmpeg(src, &from, tmp, "rotated");
                    let vf = match degrees {
                        90 => "transpose=1".to_string(),
                        270 => "transpose=2".to_string(),
                        _ => "hflip,vflip".to_string(),
                    };
                    ffmpeg_still(&input, &out, to, None, Some(&vf), &mut s);
                    (out, to)
                };
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
                    "embed",
                    &input,
                    &out,
                    p.to_string(),
                    p.to_string(),
                    (info.width + 2 * p).to_string(),
                    (info.height + 2 * p).to_string(),
                    "--extend",
                    "background",
                    "--background",
                    bg
                ],
            ));
            (out, named(&stem, "framed", to))
        }
        Op::Grayscale if is(Category::Image) => {
            let (out, to) = if ctx.vips {
                let (input, out, to) = image_io(src, &from, tmp, "bw", &mut s);
                s.push(run(Tool::Vips, args!["colourspace", &input, &out, "b-w"]));
                (out, to)
            } else {
                let (input, out, to) = image_io_ffmpeg(src, &from, tmp, "bw");
                ffmpeg_still(&input, &out, to, None, Some("hue=s=0"), &mut s);
                (out, to)
            };
            (out, named(&stem, "bw", to))
        }
        Op::ImagesToPdf => {
            let jpegs = images_to_jpegs(inputs, tmp, &mut s, ctx.vips);
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
                    args![
                        "thumbnail",
                        &input,
                        suffixed(&tmp.join(&cell), "[background=255]"),
                        "800",
                        "--height",
                        "800"
                    ],
                ));
                cells.push(cell);
            }
            // Relative names + cwd: vips splits the image list on spaces, and tmp may contain some.
            s.push(Step::Run {
                tool: Tool::Vips,
                args: args![
                    "arrayjoin",
                    cells.join(" "),
                    "collage.jpg[Q=90]",
                    "--across",
                    across.to_string(),
                    "--shim",
                    gap.to_string(),
                    "--background",
                    "255 255 255",
                    "--halign",
                    "centre",
                    "--valign",
                    "centre"
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
                        args![
                            "-dSAFER",
                            "-dBATCH",
                            "-dNOPAUSE",
                            "-dQUIET",
                            "-r300",
                            "-sDEVICE=tiffgray",
                            "-sCompression=lzw",
                            prefixed("-sOutputFile=", &tif),
                            src
                        ],
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
            let media = ctx.media.clone().unwrap_or(MediaInfo {
                duration: 0.0,
                has_video: is(Category::Video),
                has_audio: true,
                ..MediaInfo::default()
            });
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
            let mut a =
                args!["-hide_banner", "-nostdin", "-y", "-loglevel", "error", "-ss", format!("{at}"), "-i", src];
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
                let dur = ctx
                    .media
                    .as_ref()
                    .map(|m| m.duration)
                    .filter(|d| *d > 0.0)
                    .ok_or_else(|| Error::Unsupported("could not read the duration".into()))?;
                let seg = format!("{:.3}", dur / *parts as f64 + 0.001);
                a.extend(args![
                    "-force_key_frames",
                    format!("expr:gte(t,n_forced*{seg})"),
                    "-f",
                    "segment",
                    "-segment_time",
                    &seg
                ]);
            } else {
                let times = at.iter().map(|t| format!("{t:.3}")).collect::<Vec<_>>().join(",");
                // Forced keyframes land exactly on the cut; without a delta the muxer misses them.
                a.extend(args![
                    "-force_key_frames",
                    &times,
                    "-f",
                    "segment",
                    "-segment_times",
                    &times,
                    "-segment_time_delta",
                    "0.05"
                ]);
            }
            a.extend(args!["-reset_timestamps", "1", &pattern]);
            s.push(ffmpeg(a, ctx));
            (dir, format!("{stem} parts"))
        }
        Op::Join if av && ctx.inputs.len() == inputs.len() && ctx.inputs.iter().all(|m| m.duration > 0.0) => {
            let to = if is(Category::Video) { "mp4" } else { av_out_ext(&from, cat) };
            let out = tmp.join(format!("joined.{to}"));
            let mut a =
                args!["-hide_banner", "-nostdin", "-y", "-loglevel", "error", "-progress", "pipe:1", "-nostats"];
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
            if inputs.iter().any(|p| p.to_string_lossy().contains(['\n', '\r'])) {
                return Err(Error::BadOption("file names with line breaks cannot be joined".into()));
            }
            let list = tmp.join("list.txt");
            let contents: String =
                inputs.iter().map(|p| format!("file '{}'\n", p.to_string_lossy().replace('\'', "'\\''"))).collect();
            s.push(Step::WriteFile { path: list.clone(), contents });
            let out = tmp.join(format!("joined.{to}"));
            let mut a = args![
                "-hide_banner",
                "-nostdin",
                "-y",
                "-loglevel",
                "error",
                "-f",
                "concat",
                "-safe",
                "0",
                "-i",
                &list
            ];
            a.extend(encode_args(to));
            a.push(out.clone().into());
            s.push(run(Tool::Ffmpeg, a));
            (out, named("joined", "", to))
        }
        Op::Normalize if av => audio_filter(
            src,
            &from,
            cat,
            "loudnorm=I=-16:TP=-1.5:LRA=11,aresample=48000",
            "normalized",
            ctx,
            tmp,
            &mut s,
            &stem,
        ),
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
        Op::Redact { auto: Some(what), .. } if !is(Category::Image) => {
            let kind = if what == crate::ops::FACES { "Automatic face detection" } else { "Automatic text detection" };
            return Err(unsupported(kind, &from));
        }
        Op::Redact { boxes, fill, range, auto } if is(Category::Image) || is(Category::Video) => {
            let boxes = if auto.is_some() { &ctx.detected } else { boxes };
            if boxes.is_empty() {
                let what = if auto.as_deref() == Some(crate::ops::FACES) { "no faces were" } else { "no text was" };
                return Err(Error::Unsupported(format!("{what} found to redact")));
            }
            let graph = redact_graph(boxes, *fill, *range);
            visual_filter(src, &from, cat, &graph, "redacted", ctx, tmp, &mut s, &stem)
        }
        Op::Adjust(adj) if is(Category::Image) || is(Category::Video) => {
            let graph = format!("[0:v]{}[v]", adjust_chain(adj));
            visual_filter(src, &from, cat, &graph, "adjusted", ctx, tmp, &mut s, &stem)
        }
        Op::Overlay { png, width, height } if is(Category::Image) => {
            let (input, to) = ffmpeg_image_input(src, &from, tmp, &mut s);
            let out = tmp.join(format!("annotated.{to}"));
            let mut a = ffmpeg_in(&input);
            a.extend(args![
                "-i",
                png,
                "-filter_complex",
                format!("[1:v]scale={width}:{height}[o];[0:v][o]overlay=0:0[v]"),
                "-map",
                "[v]",
                "-frames:v",
                "1",
                "-q:v",
                "2",
                &out
            ]);
            s.push(run(Tool::Ffmpeg, a));
            (out, named(&stem, "annotated", to))
        }
        Op::SetTags(pairs) if av => {
            let to = av_out_ext(&from, cat);
            let out = tmp.join(format!("tagged.{to}"));
            let mut a = ffmpeg_in(src);
            a.extend(args!["-map", "0", "-c", "copy"]);
            for (k, v) in pairs {
                a.extend(args!["-metadata", format!("{k}={v}")]);
            }
            a.push(out.clone().into());
            s.push(run(Tool::Ffmpeg, a));
            (out, named(&stem, "tagged", to))
        }
        Op::Watermark { image, pos, opacity, scale } if is(Category::Image) || is(Category::Video) => {
            let (w, _) = ctx.dims.ok_or_else(|| Error::Unsupported("could not read the picture size".into()))?;
            let logo_w = ((w as f64 * scale / 100.0) as u32).max(8);
            let m = (w / 50).max(4);
            let (x, y) = match pos.as_str() {
                "tl" => (format!("{m}"), format!("{m}")),
                "tr" => (format!("W-w-{m}"), format!("{m}")),
                "bl" => (format!("{m}"), format!("H-h-{m}")),
                "center" => ("(W-w)/2".into(), "(H-h)/2".into()),
                _ => (format!("W-w-{m}"), format!("H-h-{m}")),
            };
            let graph = format!(
                "[1:v]scale={logo_w}:-1,format=rgba,colorchannelmixer=aa={opacity}[wm];[0:v][wm]overlay={x}:{y}[v]"
            );
            let video = is(Category::Video);
            let (input, to) = if video {
                (src.to_path_buf(), av_out_ext(&from, cat))
            } else {
                ffmpeg_image_input(src, &from, tmp, &mut s)
            };
            let out = tmp.join(format!("watermarked.{to}"));
            let mut a = ffmpeg_in(&input);
            a.extend(args!["-i", image, "-filter_complex", graph, "-map", "[v]"]);
            if video {
                a.extend(args!["-map", "0:a?", "-c:a", "copy"]);
                a.extend(strs(video_codec(to)));
            } else {
                a.extend(args!["-frames:v", "1", "-q:v", "2"]);
            }
            a.push(out.clone().into());
            s.push(ffmpeg(a, ctx));
            (out, named(&stem, "watermarked", to))
        }
        Op::ShiftSubs { ms } if is(Category::Subtitle) => {
            let out = tmp.join(format!("shifted.{from}"));
            s.push(Step::Subtitle { src: src.to_path_buf(), dst: out.clone(), shift_ms: *ms });
            (out, named(&stem, "shifted", &from))
        }
        Op::ExtractSubs if is(Category::Video) => {
            let out = tmp.join("subs.srt");
            let mut a = ffmpeg_in(src);
            a.extend(args!["-map", "0:s:0", "-c:s", "srt", &out]);
            s.push(run(Tool::Ffmpeg, a));
            (out, named(&stem, "", "srt"))
        }
        Op::AddSubs { burn } => {
            let find = |c: Category| inputs.iter().find(|p| crate::format::category_of(p) == Some(c));
            let (Some(video), Some(subs)) = (find(Category::Video), find(Category::Subtitle)) else {
                return Err(Error::BadOption("select one video and one .srt/.vtt file".into()));
            };
            let vext = ext_of(video).unwrap_or_default();
            let vstem = stem_of(video);
            let sext = ext_of(subs).unwrap_or_else(|| "srt".into());
            if *burn {
                let to = av_out_ext(&vext, Some(Category::Video));
                let out = tmp.join(format!("burned.{to}"));
                // Relative subtitle name + cwd: the subtitles filter's own path escaping is
                // fragile (drive colons, quotes), so keep the path out of the filter string.
                let local = format!("subs.{sext}");
                s.push(Step::Copy { src: subs.clone(), dst: tmp.join(&local) });
                let mut a = ffmpeg_in(video);
                a.extend(args!["-vf", format!("subtitles={local}"), "-c:a", "copy"]);
                a.extend(strs(video_codec(to)));
                a.push(out.clone().into());
                s.push(Step::Run {
                    tool: Tool::Ffmpeg,
                    args: a,
                    cwd: Some(tmp.to_path_buf()),
                    progress_secs: ctx.media.as_ref().map(|m| m.duration),
                });
                (out, named(&vstem, "subtitled", to))
            } else {
                let (to, codec) = match vext.as_str() {
                    "mp4" | "mov" => (vext.as_str(), "mov_text"),
                    "webm" => ("webm", "webvtt"),
                    _ => ("mkv", "srt"),
                };
                let to: &'static str = match to {
                    "mp4" => "mp4",
                    "mov" => "mov",
                    "webm" => "webm",
                    _ => "mkv",
                };
                let out = tmp.join(format!("subtitled.{to}"));
                let mut a = ffmpeg_in(video);
                a.extend(args!["-i", subs, "-map", "0", "-map", "1:0", "-c", "copy", "-c:s", codec, &out]);
                s.push(run(Tool::Ffmpeg, a));
                (out, named(&vstem, "subtitled", to))
            }
        }
        Op::Reverse if av => {
            let media = ctx.media.clone().unwrap_or_default();
            if media.duration > REVERSE_MAX_SECS {
                return Err(Error::Unsupported(format!(
                    "reverse keeps the whole clip in memory; trim it under {REVERSE_MAX_SECS:.0}s first"
                )));
            }
            let to = av_out_ext(&from, cat);
            let out = tmp.join(format!("reversed.{to}"));
            let mut a = ffmpeg_in(src);
            if media.has_video || is(Category::Video) {
                a.extend(args!["-vf", "reverse"]);
            }
            if media.has_audio || is(Category::Audio) {
                a.extend(args!["-af", "areverse"]);
            }
            a.extend(encode_args(to));
            a.push(out.clone().into());
            s.push(ffmpeg(a, ctx));
            (out, named(&stem, "reversed", to))
        }
        Op::Loop { count } if av => {
            let to = av_out_ext(&from, cat);
            let out = tmp.join(format!("looped.{to}"));
            let a = args![
                "-hide_banner",
                "-nostdin",
                "-y",
                "-loglevel",
                "error",
                "-stream_loop",
                (count - 1).to_string(),
                "-i",
                src,
                "-c",
                "copy",
                &out
            ];
            s.push(run(Tool::Ffmpeg, a));
            (out, named(&stem, &format!("x{count}"), to))
        }
        Op::Stabilize if is(Category::Video) => {
            let to = av_out_ext(&from, cat);
            let out = tmp.join(format!("stable.{to}"));
            let mut a = ffmpeg_in(src);
            a.extend(args!["-vf", "deshake", "-c:a", "copy"]);
            a.extend(strs(video_codec(to)));
            a.push(out.clone().into());
            s.push(ffmpeg(a, ctx));
            (out, named(&stem, "stabilized", to))
        }
        Op::Frames { fps } if is(Category::Video) => {
            let dir = tmp.join("frames");
            s.push(Step::MkDir(dir.clone()));
            let mut a = ffmpeg_in(src);
            a.extend(args!["-vf", format!("fps={fps}"), dir.join(format!("{}-%05d.png", stem.replace('%', "%%")))]);
            s.push(ffmpeg(a, ctx));
            (dir, format!("{stem} frames"))
        }
        Op::Slideshow { secs } => {
            let (w, h) = slideshow_canvas(ctx.dims);
            let mut a = args!["-hide_banner", "-nostdin", "-y", "-loglevel", "error"];
            let mut graph = Vec::new();
            let mut pads = String::new();
            let fade = (secs / 6.0).min(0.5);
            for (i, img) in inputs.iter().enumerate() {
                let f = ext_of(img).unwrap_or_default();
                let (input, _) = ffmpeg_image_input(img, &f, tmp, &mut s);
                a.extend(args!["-loop", "1", "-t", format!("{secs}"), "-i", &input]);
                graph.push(format!(
                    "[{i}:v]scale={w}:{h}:force_original_aspect_ratio=decrease,pad={w}:{h}:(ow-iw)/2:(oh-ih)/2:color=black,setsar=1,fps=30,format=yuv420p,\
                     fade=t=in:st=0:d={fade},fade=t=out:st={}:d={fade}[v{i}]",
                    secs - fade
                ));
                pads += &format!("[v{i}]");
            }
            graph.push(format!("{pads}concat=n={}:v=1:a=0[v]", inputs.len()));
            let out = tmp.join("slideshow.mp4");
            a.extend(args!["-filter_complex", graph.join(";"), "-map", "[v]"]);
            a.extend(strs(video_codec("mp4")));
            a.extend(args!["-movflags", "+faststart", &out]);
            s.push(Step::Run { tool: Tool::Ffmpeg, args: a, cwd: None, progress_secs: None });
            (out, "slideshow.mp4".into())
        }
        Op::Fade { secs } if av => {
            let media = ctx
                .media
                .clone()
                .filter(|m| m.duration > 0.0)
                .ok_or_else(|| Error::Unsupported("could not read the duration".into()))?;
            let secs = secs.min(media.duration / 2.0);
            let end = media.duration - secs;
            let to = av_out_ext(&from, cat);
            let out = tmp.join(format!("faded.{to}"));
            let mut a = ffmpeg_in(src);
            if media.has_video {
                a.extend(args!["-vf", format!("fade=t=in:st=0:d={secs},fade=t=out:st={end}:d={secs}")]);
            }
            if media.has_audio {
                a.extend(args!["-af", format!("afade=t=in:st=0:d={secs},afade=t=out:st={end}:d={secs}")]);
            }
            a.extend(encode_args(to));
            a.push(out.clone().into());
            s.push(ffmpeg(a, ctx));
            (out, named(&stem, "faded", to))
        }
        Op::Protect { password } if is(Category::Pdf) => {
            let out = tmp.join("protected.pdf");
            s.push(run(Tool::Qpdf, args!["--encrypt", password, password, "256", "--", src, &out]));
            (out, named(&stem, "protected", "pdf"))
        }
        Op::Unlock { password } if is(Category::Pdf) => {
            let out = tmp.join("unlocked.pdf");
            s.push(run(Tool::Qpdf, args![format!("--password={password}"), "--decrypt", src, &out]));
            (out, named(&stem, "unlocked", "pdf"))
        }
        Op::Transcribe { lang } if av => {
            let model = ctx.whisper_model.clone().ok_or_else(|| {
                Error::Unsupported("no whisper.cpp model: set ZEIDEX_WHISPER_MODEL to a ggml-*.bin file".into())
            })?;
            let wav = tmp.join("speech.wav");
            let mut a = ffmpeg_in(src);
            a.extend(args!["-vn", "-ac", "1", "-ar", "16000", "-c:a", "pcm_s16le", &wav]);
            s.push(ffmpeg(a, ctx));
            let base = tmp.join("transcript");
            s.push(run(Tool::Whisper, args!["-m", &model, "-f", &wav, "-l", lang, "-osrt", "-of", &base]));
            (tmp.join("transcript.srt"), named(&stem, "", "srt"))
        }
        Op::Upscale { scale, model } if is(Category::Image) || is(Category::Video) => {
            upscale(src, &from, is(Category::Video), *scale, model, ctx, tmp, &mut s, &stem)?
        }
        Op::RemoveBackground { to } if is(Category::Image) => {
            let model = ctx.rembg_model.clone().ok_or_else(|| {
                Error::Unsupported("background removal needs rembg and a local u2net.onnx model (see Engines)".into())
            })?;
            let (input, _) = ffmpeg_image_input(src, &from, tmp, &mut s);
            let cut = tmp.join("cutout.png");
            // u2net_custom + an explicit model path: rembg loads the file and never downloads.
            let extras = format!("{{\"model_path\": {}}}", json_string(&model.to_string_lossy()));
            s.push(run(Tool::Rembg, args!["i", "-m", "u2net_custom", "-x", extras, &input, &cut]));
            if to == "webp" {
                let out = tmp.join("cutout.webp");
                s.push(run(Tool::Vips, args!["copy", &cut, suffixed(&out, "[Q=90]")]));
                (out, named(&stem, "no-bg", "webp"))
            } else {
                (cut, named(&stem, "no-bg", "png"))
            }
        }
        Op::Icon if is(Category::Image) => {
            let (input, _) = ffmpeg_image_input(src, &from, tmp, &mut s);
            let mut pngs = Vec::new();
            for size in ICON_SIZES {
                let png = tmp.join(format!("i{size}.png"));
                s.push(run(
                    Tool::Ffmpeg,
                    args![
                        "-hide_banner", "-nostdin", "-y", "-loglevel", "error", "-i", &input, "-vf",
                        format!("scale={size}:{size}:force_original_aspect_ratio=decrease:flags=lanczos,format=rgba,pad={size}:{size}:(ow-iw)/2:(oh-ih)/2:color=black@0"),
                        "-frames:v", "1", &png
                    ],
                ));
                pngs.push((size, png));
            }
            let out = tmp.join("icon.ico");
            s.push(Step::Ico { pngs, dst: out.clone() });
            (out, named(&stem, "", "ico"))
        }
        Op::Checksum | Op::Rename { .. } | Op::Recipe(_) => {
            return Err(Error::Unsupported("handled by the runner".into()))
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
                "-filter_complex",
                format!("[0:a]showwaves=s={size}:mode=cline:colors=0xF28C28,format=yuv420p[v]"),
                "-map",
                "[v]",
                "-map",
                "0:a",
                "-c:v",
                "libx264",
                "-preset",
                "medium",
                "-crf",
                "23",
                "-c:a",
                "aac",
                "-b:a",
                "192k",
                "-shortest",
                "-movflags",
                "+faststart",
                &out
            ]);
            s.push(ffmpeg(a, ctx));
            (out, named(&stem, "visualizer", "mp4"))
        }
        Op::Arrange { items, rotate } => {
            if let Some((f, _)) = items.iter().find(|(f, _)| *f > inputs.len()) {
                return Err(Error::BadOption(format!("file {f} is not in the selection")));
            }
            let out = tmp.join("arranged.pdf");
            let mut a = args!["--empty", "--pages"];
            // Consecutive pages of the same file share one "file ranges" group.
            let mut k = 0;
            while k < items.len() {
                let f = items[k].0;
                let mut pages = Vec::new();
                while k < items.len() && items[k].0 == f {
                    pages.push(items[k].1.to_string());
                    k += 1;
                }
                a.extend(args![&inputs[f - 1], pages.join(",")]);
            }
            a.push("--".into());
            for deg in [90, 180, 270] {
                let pages: Vec<String> = rotate.iter().filter(|(_, d)| *d == deg).map(|(p, _)| p.to_string()).collect();
                if !pages.is_empty() {
                    a.push(format!("--rotate=+{deg}:{}", pages.join(",")).into());
                }
            }
            a.push(out.clone().into());
            s.push(run(Tool::Qpdf, a));
            (out, "arranged.pdf".into())
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
        Op::Pages { spec, rotate } if is(Category::Pdf) => {
            let out = tmp.join("pages.pdf");
            let mut a = args![src, "--pages", ".", spec, "--"];
            // --rotate applies to output page numbers, after the selection above.
            for deg in [90, 180, 270] {
                let pages: Vec<String> = rotate.iter().filter(|(_, d)| *d == deg).map(|(p, _)| p.to_string()).collect();
                if !pages.is_empty() {
                    a.push(format!("--rotate=+{deg}:{}", pages.join(",")).into());
                }
            }
            a.push(out.clone().into());
            s.push(run(Tool::Qpdf, a));
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

const SHEET_TILES: u32 = 16;
/// Widths for responsive web images (never upscaled).
const WEB_WIDTHS: [u32; 3] = [480, 960, 1600];

/// Output canvas for social formats: 1080 px on the short side.
fn social_canvas(ratio: (u32, u32)) -> (u32, u32) {
    let short = 1080;
    let (a, b) = ratio;
    if a >= b {
        (short * a / b, short)
    } else {
        (short, short * b / a)
    }
}

/// Clips longer than this are refused by `reverse` (FFmpeg buffers the whole clip).
const REVERSE_MAX_SECS: f64 = 120.0;
const ICON_SIZES: [u32; 7] = [16, 24, 32, 48, 64, 128, 256];
/// Slideshow frames never exceed this on their long side.
const SLIDESHOW_MAX: u32 = 1920;

/// Canvas of the first image, scaled down to fit `SLIDESHOW_MAX`, even-sized for H.264.
fn slideshow_canvas(dims: Option<(u32, u32)>) -> (u32, u32) {
    let (w, h) = dims.filter(|(w, h)| *w > 0 && *h > 0).unwrap_or((1920, 1080));
    let k = (SLIDESHOW_MAX as f64 / w.max(h) as f64).min(1.0);
    (((w as f64 * k) as u32).max(2) & !1, ((h as f64 * k) as u32).max(2) & !1)
}

/// Longest video AI upscale accepts: every frame is held as PNG on disk while it runs.
const MAX_UPSCALE_SECS: f64 = 10.0 * 60.0;

/// Real-ESRGAN network and the scale it runs at. The photo network is 4x only; smaller factors
/// are a Lanczos resize of its output, which keeps its detail.
fn esrgan_net(model: &str, scale: u32) -> (&'static str, u32) {
    if model == "anime" {
        ("realesr-animevideov3", scale)
    } else {
        ("realesrgan-x4plus", 4)
    }
}

#[allow(clippy::too_many_arguments)]
fn upscale(
    src: &Path,
    from: &str,
    video: bool,
    scale: u32,
    model: &str,
    ctx: &Ctx,
    tmp: &Path,
    s: &mut Vec<Step>,
    stem: &str,
) -> Result<(PathBuf, String)> {
    let models = ctx.esrgan_models.clone().ok_or_else(|| {
        Error::Unsupported("AI upscale needs realesrgan-ncnn-vulkan with its models folder (see Engines)".into())
    })?;
    let (net, native) = esrgan_net(model, scale);
    let esrgan = |input: &Path, output: &Path| {
        run(
            Tool::RealEsrgan,
            args!["-i", input, "-o", output, "-n", net, "-s", native.to_string(), "-m", &models, "-f", "png"],
        )
    };
    // Integer maths keeps even sizes for H.264; nothing to do when the network ran at `scale`.
    let shrink = (scale != native)
        .then(|| format!("scale=trunc(iw*{scale}/{native}/2)*2:trunc(ih*{scale}/{native}/2)*2:flags=lanczos"));
    if !video {
        let (input, _) = ffmpeg_image_input(src, from, tmp, s);
        let png = tmp.join("upscale-in.png");
        let mut a = ffmpeg_in(&input);
        a.extend(args!["-frames:v", "1", &png]);
        s.push(ffmpeg(a, ctx));
        let up = tmp.join("upscaled.png");
        s.push(esrgan(&png, &up));
        let to = if from == "jpg" { "jpg" } else { "png" };
        let out = tmp.join(format!("upscaled-final.{to}"));
        let mut a = ffmpeg_in(&up);
        if let Some(f) = &shrink {
            a.extend(args!["-vf", f]);
        }
        a.extend(args!["-frames:v", "1", "-q:v", "2", &out]);
        s.push(ffmpeg(a, ctx));
        return Ok((out, named(stem, &format!("x{scale}"), to)));
    }
    let m = ctx.media.as_ref().ok_or_else(|| Error::Unsupported("could not read the video".into()))?;
    if m.duration > MAX_UPSCALE_SECS {
        return Err(Error::Unsupported(format!(
            "AI video upscale takes clips up to {} minutes: trim the video first",
            MAX_UPSCALE_SECS / 60.0
        )));
    }
    let fps = if m.fps > 0.0 { m.fps } else { 30.0 };
    let (frames, up) = (tmp.join("frames"), tmp.join("upscaled"));
    s.push(Step::MkDir(frames.clone()));
    s.push(Step::MkDir(up.clone()));
    let mut a = ffmpeg_in(src);
    a.extend(args!["-map", "0:v:0", "-r", format!("{fps}"), frames.join("%08d.png")]);
    s.push(ffmpeg(a, ctx));
    s.push(esrgan(&frames, &up));
    let out = tmp.join("upscaled.mp4");
    let mut a = args![
        "-hide_banner",
        "-nostdin",
        "-y",
        "-loglevel",
        "error",
        "-progress",
        "pipe:1",
        "-nostats",
        "-framerate",
        format!("{fps}"),
        "-i",
        up.join("%08d.png"),
        "-i",
        src,
        "-map",
        "0:v",
        "-map",
        "1:a?"
    ];
    let vf = match &shrink {
        Some(f) => format!("{f},format=yuv420p"),
        None => "format=yuv420p".into(),
    };
    a.extend(args!["-vf", vf, "-c:v", "libx264", "-crf", "18", "-preset", "medium", "-c:a", "aac", "-b:a", "192k"]);
    a.extend(args!["-shortest", &out]);
    s.push(ffmpeg(a, ctx));
    Ok((out, named(stem, &format!("x{scale}"), "mp4")))
}

/// `s` as a JSON string literal (rembg's `-x` takes JSON; Windows paths carry backslashes).
fn json_string(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Image path FFmpeg decodes reliably, and the extension to write. HEIC/AVIF/SVG/WebP go
/// through libvips first because FFmpeg's support for them as stills is patchy.
fn ffmpeg_image_input(src: &Path, from: &str, tmp: &Path, s: &mut Vec<Step>) -> (PathBuf, &'static str) {
    if ["jpg", "png", "bmp", "tiff"].contains(&from) {
        return (src.to_path_buf(), if from == "jpg" { "jpg" } else { "png" });
    }
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let png = tmp.join(format!("decoded{}.png", N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
    let decoded = image_src(src, from, tmp, s);
    s.push(run(Tool::Vips, args!["copy", &decoded, &png]));
    (png, "png")
}

/// Runs a `[0:v]...[v]` filter graph over an image or a video (audio copied).
#[allow(clippy::too_many_arguments)]
fn visual_filter(
    src: &Path,
    from: &str,
    cat: Option<Category>,
    graph: &str,
    tag: &str,
    ctx: &Ctx,
    tmp: &Path,
    s: &mut Vec<Step>,
    stem: &str,
) -> (PathBuf, String) {
    let video = cat == Some(Category::Video);
    let (input, to) =
        if video { (src.to_path_buf(), av_out_ext(from, cat)) } else { ffmpeg_image_input(src, from, tmp, s) };
    let out = tmp.join(format!("{tag}.{to}"));
    let mut a = ffmpeg_in(&input);
    a.extend(args!["-filter_complex", graph, "-map", "[v]"]);
    if video {
        a.extend(args!["-map", "0:a?", "-c:a", "copy"]);
        a.extend(strs(video_codec(to)));
    } else {
        a.extend(args!["-frames:v", "1", "-q:v", "2"]);
    }
    a.push(out.clone().into());
    s.push(ffmpeg(a, ctx));
    (out, named(stem, tag, to))
}

/// FFmpeg filter chain for a look adjustment; neutral parts are left out.
pub fn adjust_chain(a: &Adjust) -> String {
    let mut f = vec![format!(
        "eq=brightness={}:contrast={}:saturation={}:gamma={}",
        a.brightness, a.contrast, a.saturation, a.gamma
    )];
    if a.hue != 0.0 {
        f.push(format!("hue=h={}", a.hue));
    }
    if a.denoise > 0.0 {
        f.push(format!("hqdn3d={}", a.denoise));
    }
    if a.sharpen > 0.0 {
        f.push(format!("unsharp=5:5:{}", a.sharpen));
    }
    if a.clarity > 0.0 {
        f.push(format!("unsharp=13:13:{}:13:13:0", a.clarity));
    }
    if a.grain > 0.0 {
        f.push(format!("noise=alls={}:allf=t", a.grain));
    }
    f.join(",")
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
            .map(|x| {
                format!(
                    "drawbox=x={}:y={}:w={}:h={}:color=0x{r:02X}{g:02X}{b:02X}@1:t=fill{enable}",
                    x.x, x.y, x.w, x.h
                )
            })
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

/// Still formats FFmpeg can write. HEIC, JXL and JP2 stay libvips-only; asking for one of them
/// without libvips produces a clear message instead of a failed encoder.
const FFMPEG_STILL: &[&str] = &["jpg", "png", "webp", "avif", "bmp", "gif", "tiff"];

/// libvips quality is 1-100; FFmpeg's JPEG scale is 1-31 where lower means better.
fn jpeg_quality(q: u8) -> u32 {
    (2 + (100 - q.clamp(1, 100) as u32) * 29 / 100).clamp(2, 31)
}

/// Encode one still image with FFmpeg. The fallback path for every image operation when libvips
/// is not installed, so the formats people actually convert keep working out of the box.
fn ffmpeg_still(src: &Path, dst: &Path, to: &str, quality: Option<u8>, vf: Option<&str>, s: &mut Vec<Step>) {
    let mut a = args!["-hide_banner", "-nostdin", "-y", "-loglevel", "error", "-i", src, "-frames:v", "1"];
    let mut filter = vf.map(str::to_string);
    match to {
        "jpg" => {
            let q = jpeg_quality(quality.unwrap_or(90));
            a.extend(args!["-q:v", q.to_string(), "-pix_fmt", "yuvj420p"]);
        }
        "webp" => {
            let q = quality.unwrap_or(80).clamp(1, 100);
            a.extend(args!["-c:v", "libwebp", "-quality", q.to_string(), "-compression_level", "4"]);
        }
        "png" => a.extend(args!["-compression_level", "9"]),
        "avif" => {
            let crf = 63u32.saturating_sub(quality.unwrap_or(60) as u32 / 2).clamp(8, 63);
            a.extend(args!["-c:v", "libaom-av1", "-crf", crf.to_string(), "-still-picture", "1", "-cpu-used", "6"]);
        }
        "gif" => {
            let base = filter.take().map(|f| format!("{f},")).unwrap_or_default();
            filter = Some(format!("{base}split[a][b];[a]palettegen=max_colors=256[p];[b][p]paletteuse=dither=bayer"));
        }
        _ => {}
    }
    if let Some(f) = filter {
        a.extend(args!["-vf", f]);
    }
    a.extend(args!["-map_metadata", "-1"]);
    a.push(dst.to_path_buf().into());
    s.push(run(Tool::Ffmpeg, a));
}

/// Input, output and extension for a same-format image edit without libvips.
fn image_io_ffmpeg(src: &Path, from: &str, tmp: &Path, tag: &str) -> (PathBuf, PathBuf, &'static str) {
    let to = match from {
        "jpg" | "jpeg" => "jpg",
        "webp" => "webp",
        "avif" => "avif",
        "gif" => "gif",
        "tiff" | "tif" => "tiff",
        _ => "png",
    };
    (src.to_path_buf(), tmp.join(format!("{tag}.{to}")), to)
}

fn images_to_jpegs(inputs: &[PathBuf], tmp: &Path, s: &mut Vec<Step>, vips: bool) -> Vec<PathBuf> {
    inputs
        .iter()
        .enumerate()
        .map(|(i, img)| {
            let f = ext_of(img).unwrap_or_default();
            let jpg = tmp.join(format!("p{i}.jpg"));
            if vips {
                let input = image_src(img, &f, tmp, s);
                s.push(run(Tool::Vips, args!["colourspace", &input, suffixed(&jpg, "[Q=92,background=255]"), "srgb"]));
            } else {
                ffmpeg_still(img, &jpg, "jpg", Some(92), None, s);
            }
            jpg
        })
        .collect()
}

// ---------- audio / video ----------

const GIF_FILTER: &str = "fps=12,scale='min(640,iw)':-1:flags=lanczos,split[a][b];[a]palettegen[p];[b][p]paletteuse";

fn ffmpeg_in(src: &Path) -> Vec<OsString> {
    args!["-hide_banner", "-nostdin", "-y", "-loglevel", "error", "-progress", "pipe:1", "-nostats", "-i", src]
}

fn ffmpeg(args: Vec<OsString>, ctx: &Ctx) -> Step {
    Step::Run { tool: Tool::Ffmpeg, args, cwd: None, progress_secs: ctx.media.as_ref().map(|m| m.duration) }
}

fn is_audio_ext(ext: &str) -> bool {
    matches!(
        ext,
        "mp3"
            | "m4a"
            | "wav"
            | "flac"
            | "ogg"
            | "opus"
            | "aiff"
            | "wma"
            | "ac3"
            | "amr"
            | "caf"
            | "weba"
            | "mka"
            | "ape"
            | "wv"
            | "w64"
    )
}

fn av_out_ext(from: &str, cat: Option<Category>) -> &'static str {
    const VIDEO: [&str; 6] = ["mp4", "mov", "mkv", "webm", "avi", "wmv"];
    const AUDIO: [&str; 9] = ["mp3", "m4a", "wav", "flac", "ogg", "opus", "aiff", "wma", "ac3"];
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
        "ac3" => &["-c:a", "ac3", "-b:a", "384k"],
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

/// Encoder arguments with an explicit quality (1-100, higher is better). The default values
/// from `video_codec`/`audio_codec` stay as the baseline and are overridden where the encoder
/// has a quality knob: CRF for x264/x265/VP9, `-q:a` for the VBR audio encoders. Lossless
/// codecs (FLAC, PCM, ALAC) ignore it by design, and GIF has no quality setting at all.
fn encode_args_at(to: &str, quality: Option<u8>) -> Vec<OsString> {
    let Some(q) = quality else { return encode_args(to) };
    if to == "gif" {
        return encode_args(to);
    }
    let mut a = Vec::new();
    if is_audio_ext(to) {
        a.push("-vn".into());
        a.extend(strs(audio_codec(to)));
        if matches!(to, "mp3" | "ogg" | "opus") {
            let level = ((q as f32 / 100.0) * 9.0).round() as u8;
            a.extend(args!["-q:a", &level.to_string()]);
        }
    } else {
        a.extend(strs(video_codec(to)));
        if matches!(to, "mp4" | "mov" | "mkv" | "webm") {
            // CRF 51 is the worst quality, 0 is lossless; quality 85 maps to about CRF 8.
            let crf = (51.0 - (q as f32 * 51.0 / 100.0)).round().clamp(0.0, 51.0) as u8;
            a.extend(args!["-crf", &crf.to_string()]);
        }
        a.extend(strs(audio_codec(to)));
    }
    a.extend(strs(container_flags(to)));
    a
}

#[allow(clippy::too_many_arguments)]
fn audio_filter(
    src: &Path,
    from: &str,
    cat: Option<Category>,
    filter: &str,
    tag: &str,
    ctx: &Ctx,
    tmp: &Path,
    s: &mut Vec<Step>,
    stem: &str,
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
    src: &Path,
    from: &str,
    cat: Option<Category>,
    how: &Compression,
    max: Option<u32>,
    ctx: &Ctx,
    tmp: &Path,
    s: &mut Vec<Step>,
    stem: &str,
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
            let input = if ctx.vips { image_src(src, from, tmp, s) } else { src.to_path_buf() };
            let vips = ctx.vips;
            let candidate = |q: u32| -> (Vec<Step>, PathBuf) {
                let out = tmp.join(format!("q{q}.{to}"));
                if !vips {
                    let mut steps = Vec::new();
                    let vf =
                        max.map(|m| format!("scale='min(iw,{m})':'min(ih,{m})':force_original_aspect_ratio=decrease"));
                    ffmpeg_still(src, &out, to, Some(q as u8), vf.as_deref(), &mut steps);
                    return (steps, out);
                }
                let opts = match to {
                    "png" => format!("[Q={q},palette,compression=9,strip]"),
                    "tiff" => format!("[Q={q},compression=jpeg,strip]"),
                    "jpg" => format!("[Q={q},strip,background=255]"),
                    _ => format!("[Q={q},strip]"),
                };
                let dst = suffixed(&out, &opts);
                let step = match max {
                    Some(m) => run(
                        Tool::Vips,
                        args!["thumbnail", &input, dst, m.to_string(), "--height", m.to_string(), "--size", "down"],
                    ),
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
                        args![
                            "-dSAFER",
                            "-dBATCH",
                            "-dNOPAUSE",
                            "-dQUIET",
                            "-sDEVICE=pdfwrite",
                            "-dCompatibilityLevel=1.5",
                            format!("-dPDFSETTINGS={preset}"),
                            prefixed("-sOutputFile=", &out),
                            src
                        ],
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
            let scale = max.map(|m| format!("scale=w='if(gt(iw,ih),min({m},iw),-2)':h='if(gt(iw,ih),-2,min({m},ih))'"));
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
                    p1.extend(args![
                        "-c:v",
                        "libx264",
                        "-preset",
                        "medium",
                        "-b:v",
                        &vb,
                        "-pass",
                        "1",
                        "-passlogfile",
                        &log,
                        "-an",
                        "-f",
                        "null",
                        null
                    ]);
                    let mut p2 = head;
                    p2.extend(args![
                        "-c:v",
                        "libx264",
                        "-preset",
                        "medium",
                        "-b:v",
                        &vb,
                        "-pass",
                        "2",
                        "-passlogfile",
                        &log,
                        "-pix_fmt",
                        "yuv420p"
                    ]);
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
                    head.extend(args![
                        "-c:v", "libx264", "-preset", "medium", "-crf", crf, "-pix_fmt", "yuv420p", "-c:a", "aac",
                        "-b:a", "128k"
                    ]);
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
                    let d = ctx
                        .media
                        .as_ref()
                        .map(|m| m.duration)
                        .filter(|d| *d > 0.0)
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
        "--headless",
        "--norestore",
        "--nolockcheck",
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

const PANDOC_IN: &[&str] = &["md", "txt", "html", "docx", "odt", "epub", "rtf", "rst", "tex", "org", "adoc", "typ"];

fn pandoc(src: &Path, from: &str, to: &str, tmp: &Path, s: &mut Vec<Step>) -> PathBuf {
    let reader = match from {
        "md" | "txt" => "markdown",
        "tex" => "latex",
        "adoc" => "asciidoc",
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
        args![
            "-dSAFER",
            "-dBATCH",
            "-dNOPAUSE",
            "-dQUIET",
            "-r300",
            device,
            "-dJPEGQ=90",
            prefixed("-sOutputFile=", &dir.join(format!("page-%03d.{to}"))),
            src
        ],
    ));
    dir
}

fn convert(
    src: &Path,
    from: &str,
    to: &str,
    quality: Option<u8>,
    tmp: &Path,
    ctx: &Ctx,
    s: &mut Vec<Step>,
) -> Result<PathBuf> {
    let cat = category_of_ext(from).ok_or_else(|| unsupported("Converting", from))?;
    if !crate::format::targets(src).contains(&to) {
        return Err(Error::Unsupported(format!("cannot convert .{from} to .{to}")));
    }
    // Two-hop conversions: produce `mid` first, then convert that.
    let via = |mid: &str, s: &mut Vec<Step>| -> Result<PathBuf> {
        let first = convert(src, from, mid, quality, tmp, ctx, s)?;
        let next_tmp = tmp.join(format!("via-{mid}"));
        s.push(Step::MkDir(next_tmp.clone()));
        convert_from(&first, mid, to, &next_tmp, ctx, s)
    };
    Ok(match cat {
        Category::Subtitle => {
            let out = tmp.join(format!("sub.{to}"));
            if matches!(from, "srt" | "vtt") && matches!(to, "srt" | "vtt" | "txt") {
                s.push(Step::Subtitle { src: src.to_path_buf(), dst: out.clone(), shift_ms: 0 });
            } else {
                let mut a = ffmpeg_in(src);
                a.push(out.clone().into());
                s.push(ffmpeg(a, ctx));
            }
            out
        }
        Category::Image if to == "pdf" => {
            let jpegs = images_to_jpegs(&[src.to_path_buf()], tmp, s, ctx.vips);
            let out = tmp.join("image.pdf");
            s.push(Step::JpegsToPdf { jpegs, dst: out.clone() });
            out
        }
        Category::Image if to == "bmp" => {
            let out = tmp.join("image.bmp");
            if ctx.vips {
                let input = image_src(src, from, tmp, s);
                let png = tmp.join("to-bmp.png");
                s.push(run(Tool::Vips, args!["copy", &input, &png]));
                s.push(run(
                    Tool::Ffmpeg,
                    args!["-hide_banner", "-nostdin", "-y", "-loglevel", "error", "-i", &png, &out],
                ));
            } else {
                ffmpeg_still(src, &out, "bmp", None, None, s);
            }
            out
        }
        Category::Image if to == "ico" => {
            let png = tmp.join("to-ico.png");
            if ctx.vips {
                let input = image_src(src, from, tmp, s);
                s.push(run(Tool::Vips, args!["copy", &input, &png]));
            } else {
                ffmpeg_still(src, &png, "png", None, None, s);
            }
            let out = tmp.join("image.ico");
            s.push(Step::Ico {
                pngs: vec![(16, png.clone()), (32, png.clone()), (48, png.clone()), (256, png)],
                dst: out.clone(),
            });
            out
        }
        Category::Image if from == "livp" || to == "mp4" => {
            let out = tmp.join("motion.mp4");
            s.push(Step::ExtractMotion { src: src.to_path_buf(), dst: out.clone() });
            out
        }
        Category::Image => {
            let out = tmp.join(format!("image.{to}"));
            if ctx.vips {
                let input = image_src(src, from, tmp, s);
                // libvips uses Q=1-100 for every lossy writer it ships (JPEG, WebP, HEIF, JXL, AVIF).
                let opts = match (to, quality) {
                    ("jpg" | "webp" | "heic" | "avif" | "jxl" | "jp2", Some(q)) => format!("[Q={q}]"),
                    ("jpg", None) => "[Q=90,background=255]".to_string(),
                    _ => String::new(),
                };
                s.push(run(Tool::Vips, args!["copy", &input, suffixed(&out, &opts)]));
            } else {
                if !FFMPEG_STILL.contains(&to) {
                    return Err(Error::Unsupported(format!(
                        "cannot write .{to} without libvips; install libvips-tools or pick {}",
                        FFMPEG_STILL.join(", ")
                    )));
                }
                ffmpeg_still(src, &out, to, quality, None, s);
            }
            out
        }
        Category::Video | Category::Audio => {
            let out = tmp.join(format!("media.{to}"));
            let mut a = ffmpeg_in(src);
            a.extend(encode_args_at(to, quality));
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
                    args![
                        "-dSAFER",
                        "-dBATCH",
                        "-dNOPAUSE",
                        "-dQUIET",
                        "-sDEVICE=txtwrite",
                        prefixed("-sOutputFile=", &out),
                        src
                    ],
                ));
                out
            }
            "docx" => soffice(src, "docx:MS Word 2007 XML", Some("writer_pdf_import"), tmp, s),
            "pptx" => soffice(src, "pptx", Some("impress_pdf_import"), tmp, s),
            "xlsx" => soffice(src, "xlsx", Some("calc_pdf_import"), tmp, s),
            _ => calibre(src, to, tmp, s),
        },
        Category::Document => match to {
            "pdf" if from == "md" || matches!(from, "rst" | "tex" | "org" | "adoc" | "typ") => via("docx", s)?,
            "md" | "epub" if PANDOC_IN.contains(&from) => pandoc(src, from, to, tmp, s),
            "md" | "epub" => via("docx", s)?,
            _ if from == "md" || matches!(from, "rst" | "tex" | "org" | "adoc" | "typ") => {
                pandoc(src, from, to, tmp, s)
            }
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
            let dir = extract(src, tmp, s, None);
            pack(&[dir.join("*")], to, tmp, s, None, None)
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
        _ => convert(src, from, to, None, tmp, ctx, s)?,
    })
}

// ---------- archives ----------

fn extract(src: &Path, tmp: &Path, s: &mut Vec<Step>, password: Option<&str>) -> PathBuf {
    // Always pass -p so an encrypted archive fails instead of waiting for a password prompt.
    let pw = format!("-p{}", password.unwrap_or(""));
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
            s.push(run(Tool::SevenZip, args!["x", "-y", &pw, prefixed("-o", &layer), "--", src]));
            s.push(run(Tool::SevenZip, args!["x", "-y", &pw, prefixed("-o", &dir), "--", layer.join(tar)]));
        }
        None => s.push(run(Tool::SevenZip, args!["x", "-y", &pw, prefixed("-o", &dir), "--", src])),
    }
    dir
}

fn stem_tar(src: &Path, suffix_len: usize) -> String {
    let name = src.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    name[..name.len() - suffix_len].to_string()
}

fn pack(
    inputs: &[PathBuf],
    kind: &str,
    tmp: &Path,
    s: &mut Vec<Step>,
    password: Option<&str>,
    split: Option<u64>,
) -> PathBuf {
    let seven = |t: &str, out: &Path, files: &[PathBuf]| {
        let mut a = args!["a", "-y", format!("-t{t}"), out];
        if let Some(pw) = password {
            a.push(format!("-p{pw}").into());
            // zip: AES instead of the weak ZipCrypto; 7z: also hide the file names.
            a.push(if t == "zip" { "-mem=AES256" } else { "-mhe=on" }.into());
        }
        if let Some(bytes) = split {
            a.push(format!("-v{bytes}b").into());
        }
        a.push("--".into());
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
            Op::Convert { to: to.into(), quality: None }
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
                        // Forward slashes on every OS so the expectations hold on Windows too.
                        args.iter().map(|a| a.to_string_lossy().replace('\\', "/")).collect::<Vec<_>>().join(" "),
                    )),
                    Step::SmallestUnder { candidates, .. } => candidates.iter().for_each(|(c, _)| walk(c, out)),
                    _ => {}
                }
            }
        }
        walk(&p.steps, &mut out);
        out
    }

    #[test]
    fn ai_upscale_and_background_removal() {
        assert!(plan_of("upscale", &[], &["/in/a.png"], Ctx::default()).is_err(), "needs Real-ESRGAN models");
        let esr = Ctx { esrgan_models: Some("/e/models".into()), ..Ctx::default() };

        // Photo network runs at 4x; 2x is a Lanczos resize of its output, JPEG stays JPEG.
        let p = plan_of("upscale", &[("scale", "2")], &["/in/a.jpg"], esr.clone()).unwrap();
        let a = argv(&p);
        assert_eq!(p.name, "a-x2.jpg");
        assert_eq!(
            a[1],
            (
                Tool::RealEsrgan,
                "-i /t/upscale-in.png -o /t/upscaled.png -n realesrgan-x4plus -s 4 -m /e/models -f png".into()
            )
        );
        assert!(a[2].1.contains("-vf scale=trunc(iw*2/4/2)*2:trunc(ih*2/4/2)*2:flags=lanczos"), "{}", a[2].1);

        // The anime network runs at the asked scale itself: no resize afterwards.
        let p = plan_of("upscale", &[("scale", "3"), ("model", "anime")], &["/in/b.png"], esr.clone()).unwrap();
        let a = argv(&p);
        assert!(a[1].1.contains("-n realesr-animevideov3 -s 3"));
        assert!(!a[2].1.contains("-vf"));
        assert_eq!(p.name, "b-x3.png");
        assert!(Op::parse("upscale", &[("scale".to_string(), "8".to_string())].into()).is_err());

        // Video: frames out, upscaled as a folder, encoded back with the original audio.
        let vid = Ctx { esrgan_models: Some("/e/models".into()), ..media(5.0) };
        let p = plan_of("upscale", &[("scale", "4")], &["/in/clip.mp4"], vid).unwrap();
        let a = argv(&p);
        assert_eq!(p.name, "clip-x4.mp4");
        assert!(a[0].1.contains("-map 0:v:0 -r 25 /t/frames/%08d.png"), "{}", a[0].1);
        assert_eq!(a[1].1, "-i /t/frames -o /t/upscaled -n realesrgan-x4plus -s 4 -m /e/models -f png");
        assert!(a[2].1.contains("-framerate 25 -i /t/upscaled/%08d.png -i /in/clip.mp4 -map 0:v -map 1:a?"));
        assert!(a[2].1.contains("-vf format=yuv420p -c:v libx264"));
        let long = Ctx { esrgan_models: Some("/e/models".into()), ..media(3600.0) };
        assert!(plan_of("upscale", &[], &["/in/long.mp4"], long).is_err(), "frame budget");

        assert!(plan_of("remove-bg", &[], &["/in/p.jpg"], Ctx::default()).is_err(), "needs a local model");
        let bg = Ctx { rembg_model: Some(r"C:\m\u2net.onnx".into()), ..Ctx::default() };
        let p = plan_of("remove-bg", &[], &["/in/p.jpg"], bg.clone()).unwrap();
        assert_eq!(p.name, "p-no-bg.png");
        let a = argv(&p);
        assert_eq!(a.len(), 1);
        // argv() turns backslashes into slashes for display; the JSON itself escapes them.
        assert_eq!(a[0].0, Tool::Rembg);
        assert!(a[0].1.starts_with("i -m u2net_custom -x {\"model_path\": "));
        assert_eq!(json_string(r"C:\m\u2net.onnx"), r#""C:\\m\\u2net.onnx""#);
        let p = plan_of("remove-bg", &[("to", "webp")], &["/in/p.heic"], bg).unwrap();
        assert_eq!(p.name, "p-no-bg.webp");
        assert_eq!(argv(&p).last().unwrap().0, Tool::Vips);
    }

    fn media(d: f64) -> Ctx {
        Ctx {
            media: Some(MediaInfo {
                duration: d,
                has_video: true,
                has_audio: true,
                width: 640,
                height: 360,
                fps: 25.0,
            }),
            ..Ctx::default()
        }
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

        let p = plan_of("to:xlsx", &[], &["/in/sheet.pdf"], Ctx::default()).unwrap();
        let a = argv(&p);
        assert_eq!(a[0].0, Tool::LibreOffice);
        assert!(a[0].1.contains("--infilter=calc_pdf_import") && a[0].1.contains("--convert-to xlsx"));
        assert_eq!(p.output, PathBuf::from("/t/lo/sheet.xlsx"));

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
    fn images_work_without_libvips() {
        let no_vips = Ctx { vips: false, ..Ctx::default() };
        let p = plan_of("to:jpg", &[], &["/in/a.png"], no_vips.clone()).unwrap();
        assert_eq!(argv(&p)[0].0, Tool::Ffmpeg, "conversion falls back to the bundled FFmpeg");
        assert!(argv(&p)[0].1.contains("-frames:v 1"));

        let p = plan_of("resize", &[("max", "800")], &["/in/a.heic"], no_vips.clone()).unwrap();
        assert_eq!(argv(&p)[0].0, Tool::Ffmpeg);
        assert!(argv(&p)[0].1.contains("scale='min(iw,800)'"));

        let sized = Ctx { vips: false, image: Some(ImageInfo { width: 400, height: 300, bands: 3 }), ..Ctx::default() };
        let p = plan_of("crop", &[("box", "0,0,100,100")], &["/in/a.png"], sized).unwrap();
        assert_eq!(argv(&p)[0].0, Tool::Ffmpeg);
        assert!(argv(&p)[0].1.contains("crop=100:100:0:0"));

        // Formats only libvips can write stay a clear error instead of a broken encode.
        let err = plan_of("to:heic", &[], &["/in/a.png"], no_vips).unwrap_err();
        assert!(format!("{err}").contains("libvips"));

        // And with libvips present the preferred chain is unchanged.
        let p = plan_of("to:jpg", &[], &["/in/a.png"], Ctx::default()).unwrap();
        assert_eq!(argv(&p)[0].0, Tool::Vips);
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
    fn adjust_overlay_tags_pages_pdfstrip() {
        let a = Adjust { saturation: 1.4, sharpen: 1.0, ..Adjust::NEUTRAL };
        assert_eq!(adjust_chain(&a), "eq=brightness=0:contrast=1:saturation=1.4:gamma=1,unsharp=5:5:1");
        let c = Adjust { clarity: 0.8, ..Adjust::NEUTRAL };
        assert!(adjust_chain(&c).ends_with("unsharp=13:13:0.8:13:13:0"));
        let p = plan_of("adjust", &[("saturation", "1.4")], &["/in/a.webp"], Ctx::default()).unwrap();
        let tools: Vec<Tool> = argv(&p).into_iter().map(|x| x.0).collect();
        assert_eq!(tools, vec![Tool::Vips, Tool::Ffmpeg]);
        assert_eq!(p.name, "a-adjusted.png");

        let op = Op::Overlay { png: "/t/ov.png".into(), width: 4000, height: 3000 };
        let p = plan(&op, &[PathBuf::from("/in/big.jpg")], &Ctx::default(), Path::new("/t")).unwrap();
        assert!(argv(&p)[0].1.contains("[1:v]scale=4000:3000[o];[0:v][o]overlay=0:0[v]"));
        assert_eq!(p.name, "big-annotated.jpg");

        let p = plan_of("tags", &[("title", "Hi there"), ("artist", "Z")], &["/in/s.mp3"], media(3.0)).unwrap();
        assert!(argv(&p)[0].1.contains("-map 0 -c copy -metadata title=Hi there -metadata artist=Z"));

        let p = plan_of("pages", &[("spec", "3,1,2"), ("rotate", "1:90,3:90,2:180")], &["/in/d.pdf"], Ctx::default())
            .unwrap();
        assert_eq!(argv(&p)[0].1, "/in/d.pdf --pages . 3,1,2 -- --rotate=+90:1,3 --rotate=+180:2 /t/pages.pdf");

        let p = plan_of("strip", &[], &["/in/d.pdf"], Ctx::default()).unwrap();
        assert_eq!(argv(&p)[0], (Tool::Qpdf, "--empty --pages /in/d.pdf 1-z -- /t/clean.pdf".into()));
    }

    #[test]
    fn v04_tools() {
        let img = Ctx { dims: Some((1000, 800)), ..Ctx::default() };
        let p = plan_of("watermark", &[("image", "/l/logo.png"), ("pos", "tr"), ("scale", "10")], &["/in/a.jpg"], img)
            .unwrap();
        assert!(argv(&p)[0]
            .1
            .contains("[1:v]scale=100:-1,format=rgba,colorchannelmixer=aa=0.8[wm];[0:v][wm]overlay=W-w-20:20[v]"));
        assert!(plan_of("watermark", &[("image", "/l.png")], &["/in/a.jpg"], Ctx::default()).is_err(), "needs size");

        let p = plan_of("burn-subs", &[], &["/in/s.srt", "/in/v.mp4"], media(10.0)).unwrap();
        assert!(matches!(&p.steps[1], Step::Run { cwd: Some(c), .. } if c == Path::new("/t")));
        assert!(argv(&p)[0].1.contains("-vf subtitles=subs.srt"));
        assert_eq!(p.name, "v-subtitled.mp4");
        let p = plan_of("add-subs", &[], &["/in/v.avi", "/in/s.vtt"], Ctx::default()).unwrap();
        assert!(argv(&p)[0].1.ends_with("-c:s srt /t/subtitled.mkv"));
        assert!(plan_of("add-subs", &[], &["/in/v.mp4", "/in/w.mp4"], Ctx::default()).is_err());

        let long = media(600.0);
        assert!(plan_of("reverse", &[], &["/in/v.mp4"], long).is_err());
        let p = plan_of("loop", &[("count", "3")], &["/in/v.mp4"], Ctx::default()).unwrap();
        assert!(argv(&p)[0].1.contains("-stream_loop 2 -i /in/v.mp4 -c copy"));
        let p = plan_of("fade", &[("seconds", "2")], &["/in/v.mp4"], media(10.0)).unwrap();
        assert!(argv(&p)[0].1.contains("fade=t=out:st=8:d=2") && argv(&p)[0].1.contains("afade=t=in"));

        let p = plan_of(
            "slideshow",
            &[("seconds", "3")],
            &["/in/a.jpg", "/in/b.heic"],
            Ctx { dims: Some((4000, 3000)), ..Ctx::default() },
        )
        .unwrap();
        let a = argv(&p);
        assert_eq!(a[0].0, Tool::Vips, "heic decoded first");
        assert!(a[1].1.contains("scale=1920:1440") && a[1].1.contains("concat=n=2:v=1:a=0[v]"));
        assert_eq!(slideshow_canvas(Some((801, 601))), (800, 600));

        let p = plan_of("protect", &[("password", "s3cret")], &["/in/d.pdf"], Ctx::default()).unwrap();
        assert_eq!(argv(&p)[0].1, "--encrypt s3cret s3cret 256 -- /in/d.pdf /t/protected.pdf");
        assert!(plan_of("transcribe", &[], &["/in/talk.mp3"], media(5.0)).is_err(), "needs a model");
        let ctx = Ctx { whisper_model: Some("/m/ggml-base.bin".into()), ..media(5.0) };
        let p = plan_of("transcribe", &[("lang", "ar")], &["/in/talk.mp3"], ctx).unwrap();
        assert_eq!(
            argv(&p)[1],
            (Tool::Whisper, "-m /m/ggml-base.bin -f /t/speech.wav -l ar -osrt -of /t/transcript".into())
        );
        assert_eq!(p.name, "talk.srt");
        let p = plan_of("icon", &[], &["/in/logo.png"], Ctx::default()).unwrap();
        assert_eq!(argv(&p).len(), 7);
        assert_eq!(p.name, "logo.ico");
        let p = plan_of("shift-subs", &[("seconds", "2")], &["/in/s.vtt"], Ctx::default()).unwrap();
        assert!(matches!(p.steps[0], Step::Subtitle { shift_ms: 2000, .. }));
    }

    #[test]
    fn v05_tools() {
        let p = plan_of(
            "exif",
            &[("artist", "Z"), ("gps", "-33.9,18.4"), ("remove-gps", "1")],
            &["/in/p.jpg"],
            Ctx::default(),
        )
        .unwrap();
        assert_eq!(
            argv(&p)[0].1,
            "-q -o /t/exif.jpg -gps:all= -Artist=Z -GPSLatitude=33.9 -GPSLatitudeRef=S -GPSLongitude=18.4 -GPSLongitudeRef=E -- /in/p.jpg"
        );
        let p = plan_of("strip", &[], &["/in/p.heic"], Ctx { exiftool: true, ..Ctx::default() }).unwrap();
        assert_eq!(argv(&p)[0], (Tool::ExifTool, "-q -all= -o /t/clean.heic -- /in/p.heic".into()));

        assert_eq!(social_canvas((9, 16)), (1080, 1920));
        assert_eq!(social_canvas((4, 5)), (1080, 1350));
        let p = plan_of("social", &[("ratio", "1:1")], &["/in/v.mov"], media(5.0)).unwrap();
        assert!(argv(&p)[0].1.contains("crop=1080:1080,boxblur") && p.name == "v-1x1.mov");
        let p = plan_of("contact-sheet", &[], &["/in/v.mp4"], media(32.0)).unwrap();
        assert!(argv(&p)[0].1.contains("fps=0.50000,scale=480:-2,tile=4x4"));
        let p = plan_of("video-size", &[("height", "480")], &["/in/v.mkv"], media(5.0)).unwrap();
        assert!(argv(&p)[0].1.contains("scale=-2:'min(480,ih)'") && p.name == "v-480p.mkv");
        let p = plan_of("replace-audio", &[], &["/in/song.mp3", "/in/v.webm"], Ctx::default()).unwrap();
        assert!(argv(&p)[0].1.contains("-map 0:v:0 -map 1:a:0 -c:v copy -c:a libopus"));

        let p = plan_of(
            "archive",
            &[("format", "zip"), ("password", "pw"), ("split", "1MB")],
            &["/in/a.txt"],
            Ctx::default(),
        )
        .unwrap();
        assert!(argv(&p)[0].1.starts_with("a -y -tzip /t/parts/archive.zip -ppw -mem=AES256 -v1000000b -- /in/a.txt"));
        assert_eq!(p.name, "a parts");
        let p = plan_of("extract", &[("password", "pw")], &["/in/a.7z"], Ctx::default()).unwrap();
        assert!(argv(&p)[0].1.starts_with("x -y -ppw -o/t/extracted"));
        let p = plan_of("extract", &[], &["/in/a.zip"], Ctx::default()).unwrap();
        assert!(argv(&p)[0].1.starts_with("x -y -p -o"), "never waits for a password prompt");

        let p = plan_of("web-set", &[], &["/in/hero.jpg"], Ctx { dims: Some((1200, 800)), ..Ctx::default() }).unwrap();
        let a = argv(&p);
        assert_eq!(a.len(), 4, "480 and 960 only (no upscaling), avif + webp");
        let html = p
            .steps
            .iter()
            .find_map(|s| match s {
                Step::WriteFile { contents, .. } => Some(contents.clone()),
                _ => None,
            })
            .unwrap();
        assert!(html.contains("hero-480.avif 480w, hero-960.avif 960w") && html.contains("src=\"hero-960.webp\""));
    }

    #[test]
    fn arrange_across_pdfs() {
        let p = plan_of(
            "arrange",
            &[("items", "2:1,1:3,1:1,2:2"), ("rotate", "2:90")],
            &["/in/a.pdf", "/in/b.pdf"],
            Ctx::default(),
        )
        .unwrap();
        assert_eq!(
            argv(&p)[0].1,
            "--empty --pages /in/b.pdf 1 /in/a.pdf 3,1 /in/b.pdf 2 -- --rotate=+90:2 /t/arranged.pdf"
        );
        assert!(plan_of("arrange", &[("items", "3:1")], &["/in/a.pdf", "/in/b.pdf"], Ctx::default()).is_err());
        assert!(Op::parse("arrange", &[("items".to_string(), "0:1".to_string())].into()).is_err());
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
