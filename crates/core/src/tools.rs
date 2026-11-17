//! External engines and where to find them.
//!
//! Lookup order: `ZEIDEX_<TOOL>` env var, bundled `engines/` dirs, `PATH`, known install dirs.

use std::env;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tool {
    Ffmpeg,
    Ffprobe,
    Vips,
    VipsHeader,
    Ghostscript,
    Qpdf,
    SevenZip,
    LibreOffice,
    Pandoc,
    Calibre,
    Tesseract,
    Whisper,
    ExifTool,
    Zbar,
    Qrencode,
    /// Real-ESRGAN ncnn/Vulkan build: AI super-resolution on the GPU (CPU fallback).
    RealEsrgan,
    /// rembg CLI: AI background removal with a local U²-Net ONNX model.
    Rembg,
}

impl Tool {
    pub const ALL: [Tool; 17] = [
        Tool::Ffmpeg,
        Tool::Ffprobe,
        Tool::Vips,
        Tool::VipsHeader,
        Tool::Ghostscript,
        Tool::Qpdf,
        Tool::SevenZip,
        Tool::LibreOffice,
        Tool::Pandoc,
        Tool::Calibre,
        Tool::Tesseract,
        Tool::Whisper,
        Tool::ExifTool,
        Tool::Zbar,
        Tool::Qrencode,
        Tool::RealEsrgan,
        Tool::Rembg,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Tool::Ffmpeg => "FFmpeg",
            Tool::Ffprobe => "FFprobe",
            Tool::Vips => "libvips",
            Tool::VipsHeader => "vipsheader",
            Tool::Ghostscript => "Ghostscript",
            Tool::Qpdf => "qpdf",
            Tool::SevenZip => "7-Zip",
            Tool::LibreOffice => "LibreOffice",
            Tool::Pandoc => "Pandoc",
            Tool::Calibre => "Calibre",
            Tool::Tesseract => "Tesseract OCR",
            Tool::Whisper => "whisper.cpp",
            Tool::ExifTool => "ExifTool",
            Tool::Zbar => "ZBar (QR)",
            Tool::Qrencode => "qrencode",
            Tool::RealEsrgan => "Real-ESRGAN (AI upscale)",
            Tool::Rembg => "rembg (AI background removal)",
        }
    }

    fn env_key(self) -> &'static str {
        match self {
            Tool::Ffmpeg => "ZEIDEX_FFMPEG",
            Tool::Ffprobe => "ZEIDEX_FFPROBE",
            Tool::Vips => "ZEIDEX_VIPS",
            Tool::VipsHeader => "ZEIDEX_VIPSHEADER",
            Tool::Ghostscript => "ZEIDEX_GS",
            Tool::Qpdf => "ZEIDEX_QPDF",
            Tool::SevenZip => "ZEIDEX_7Z",
            Tool::LibreOffice => "ZEIDEX_SOFFICE",
            Tool::Pandoc => "ZEIDEX_PANDOC",
            Tool::Calibre => "ZEIDEX_EBOOK_CONVERT",
            Tool::Tesseract => "ZEIDEX_TESSERACT",
            Tool::Whisper => "ZEIDEX_WHISPER",
            Tool::ExifTool => "ZEIDEX_EXIFTOOL",
            Tool::Zbar => "ZEIDEX_ZBARIMG",
            Tool::Qrencode => "ZEIDEX_QRENCODE",
            Tool::RealEsrgan => "ZEIDEX_REALESRGAN",
            Tool::Rembg => "ZEIDEX_REMBG",
        }
    }

    fn names(self) -> &'static [&'static str] {
        if cfg!(windows) {
            match self {
                Tool::Ffmpeg => &["ffmpeg.exe"],
                Tool::Ffprobe => &["ffprobe.exe"],
                Tool::Vips => &["vips.exe"],
                Tool::VipsHeader => &["vipsheader.exe"],
                Tool::Ghostscript => &["gswin64c.exe", "gswin32c.exe", "gs.exe"],
                Tool::Qpdf => &["qpdf.exe"],
                Tool::SevenZip => &["7z.exe", "7za.exe"],
                // soffice.com waits for the conversion; soffice.exe may return early.
                Tool::LibreOffice => &["soffice.com", "soffice.exe"],
                Tool::Pandoc => &["pandoc.exe"],
                Tool::Calibre => &["ebook-convert.exe"],
                Tool::Tesseract => &["tesseract.exe"],
                Tool::Whisper => &["whisper-cli.exe", "whisper.exe"],
                Tool::ExifTool => &["exiftool.exe", "exiftool(-k).exe"],
                Tool::Zbar => &["zbarimg.exe"],
                Tool::Qrencode => &["qrencode.exe"],
                Tool::RealEsrgan => &["realesrgan-ncnn-vulkan.exe"],
                Tool::Rembg => &["rembg.exe"],
            }
        } else {
            match self {
                Tool::Ffmpeg => &["ffmpeg"],
                Tool::Ffprobe => &["ffprobe"],
                Tool::Vips => &["vips"],
                Tool::VipsHeader => &["vipsheader"],
                Tool::Ghostscript => &["gs"],
                Tool::Qpdf => &["qpdf"],
                Tool::SevenZip => &["7zz", "7z", "7za"],
                Tool::LibreOffice => &["soffice", "libreoffice"],
                Tool::Pandoc => &["pandoc"],
                Tool::Calibre => &["ebook-convert"],
                Tool::Tesseract => &["tesseract"],
                Tool::Whisper => &["whisper-cli", "whisper-cpp"],
                Tool::ExifTool => &["exiftool"],
                Tool::Zbar => &["zbarimg"],
                Tool::Qrencode => &["qrencode"],
                Tool::RealEsrgan => &["realesrgan-ncnn-vulkan"],
                Tool::Rembg => &["rembg"],
            }
        }
    }

    /// Stable id for the UI and the CLI: never translated, never containing spaces, so the
    /// Engines panel can name a tool in a command without matching a display label.
    pub fn slug(self) -> &'static str {
        match self {
            Tool::Ffmpeg => "ffmpeg",
            Tool::Ffprobe => "ffprobe",
            Tool::Vips => "vips",
            Tool::VipsHeader => "vipsheader",
            Tool::Ghostscript => "ghostscript",
            Tool::Qpdf => "qpdf",
            Tool::SevenZip => "7zip",
            Tool::LibreOffice => "libreoffice",
            Tool::Pandoc => "pandoc",
            Tool::Calibre => "calibre",
            Tool::Tesseract => "tesseract",
            Tool::Whisper => "whisper",
            Tool::ExifTool => "exiftool",
            Tool::Zbar => "zbar",
            Tool::Qrencode => "qrencode",
            Tool::RealEsrgan => "realesrgan",
            Tool::Rembg => "rembg",
        }
    }

    pub fn by_slug(slug: &str) -> Option<Tool> {
        Tool::ALL.into_iter().find(|t| t.slug() == slug)
    }

    pub fn install_hint(self) -> &'static str {
        if cfg!(windows) {
            match self {
                Tool::Ffmpeg | Tool::Ffprobe => "winget install Gyan.FFmpeg",
                Tool::Vips | Tool::VipsHeader => "scoop install libvips (or use the bundled build)",
                Tool::Ghostscript => "winget install ArtifexSoftware.GhostScript",
                Tool::Qpdf => "winget install QPDF.QPDF",
                Tool::SevenZip => "winget install 7zip.7zip",
                Tool::LibreOffice => "winget install TheDocumentFoundation.LibreOffice",
                Tool::Pandoc => "winget install JohnMacFarlane.Pandoc",
                Tool::Calibre => "winget install calibre.calibre",
                Tool::Tesseract => "winget install UB-Mannheim.TesseractOCR",
                Tool::ExifTool => "winget install OliverBetz.ExifTool",
                Tool::Zbar => "zbar for Windows (zbar.sourceforge.net), then put zbarimg.exe on PATH",
                Tool::Qrencode => "scoop install qrencode",
                Tool::Whisper => {
                    "whisper.cpp release (github.com/ggml-org/whisper.cpp) + set ZEIDEX_WHISPER_MODEL to a ggml-*.bin"
                }
                Tool::RealEsrgan => ESRGAN_HINT,
                Tool::Rembg => REMBG_HINT,
            }
        } else if cfg!(target_os = "macos") {
            match self {
                Tool::Ffmpeg | Tool::Ffprobe => "brew install ffmpeg",
                Tool::Vips | Tool::VipsHeader => "brew install vips",
                Tool::Ghostscript => "brew install ghostscript",
                Tool::Qpdf => "brew install qpdf",
                Tool::SevenZip => "brew install sevenzip (the bundled 7zz is used when present)",
                Tool::LibreOffice => "brew install --cask libreoffice",
                Tool::Pandoc => "brew install pandoc",
                Tool::Calibre => "brew install --cask calibre",
                Tool::Tesseract => "brew install tesseract tesseract-lang",
                Tool::ExifTool => "brew install exiftool",
                Tool::Zbar => "brew install zbar",
                Tool::Qrencode => "brew install qrencode",
                Tool::Whisper => "brew install whisper-cpp + set ZEIDEX_WHISPER_MODEL to a ggml-*.bin",
                Tool::RealEsrgan => ESRGAN_HINT,
                Tool::Rembg => REMBG_HINT,
            }
        } else {
            match self {
                Tool::Ffmpeg | Tool::Ffprobe => "sudo apt install ffmpeg",
                Tool::Vips | Tool::VipsHeader => "sudo apt install libvips-tools",
                Tool::Ghostscript => "sudo apt install ghostscript",
                Tool::Qpdf => "sudo apt install qpdf",
                Tool::SevenZip => "sudo apt install 7zip  (older distros: p7zip-full)",
                Tool::LibreOffice => "sudo apt install libreoffice",
                Tool::Pandoc => "sudo apt install pandoc",
                Tool::Calibre => "sudo apt install calibre",
                Tool::Tesseract => "sudo apt install tesseract-ocr tesseract-ocr-ara",
                Tool::ExifTool => "sudo apt install libimage-exiftool-perl",
                Tool::Zbar => "sudo apt install zbar-tools",
                Tool::Qrencode => "sudo apt install qrencode",
                Tool::Whisper => {
                    "build whisper.cpp (github.com/ggml-org/whisper.cpp) + set ZEIDEX_WHISPER_MODEL to a ggml-*.bin"
                }
                Tool::RealEsrgan => ESRGAN_HINT,
                Tool::Rembg => REMBG_HINT,
            }
        }
    }

    fn known_dirs(self) -> Vec<PathBuf> {
        let mut dirs = Vec::new();

        if cfg!(windows) {
            // Installers disagree on where a per-user install lands: the machine-wide
            // Program Files, the 32-bit view, LocalAppData and LocalAppData\Programs are all
            // used by at least one of the tools below. `ProgramW6432` matters in a 32-bit
            // process, where `ProgramFiles` points at Program Files (x86).
            let roots: Vec<PathBuf> = ["ProgramFiles", "ProgramW6432", "ProgramFiles(x86)", "LOCALAPPDATA", "APPDATA"]
                .iter()
                .filter_map(|k| env::var_os(k).map(PathBuf::from))
                .chain(env::var_os("LOCALAPPDATA").map(PathBuf::from).map(|p| p.join("Programs")))
                .collect();
            for root in &roots {
                match self {
                    Tool::LibreOffice => dirs.push(root.join("LibreOffice").join("program")),
                    Tool::Calibre => {
                        dirs.push(root.join("Calibre2"));
                        dirs.push(root.join("Calibre"));
                    }
                    Tool::Tesseract => dirs.push(root.join("Tesseract-OCR")),
                    Tool::SevenZip => dirs.push(root.join("7-Zip")),
                    Tool::Pandoc => dirs.push(root.join("Pandoc")),
                    Tool::Ghostscript => dirs.extend(versioned_bins(&root.join("gs"), "")),
                    Tool::Qpdf => dirs.extend(versioned_bins(root, "qpdf")),
                    Tool::ExifTool => dirs.push(root.join("exiftool")),
                    Tool::Zbar => {
                        dirs.push(root.join("ZBar").join("bin"));
                        dirs.push(root.join("zbar").join("bin"));
                    }
                    Tool::Whisper => dirs.push(root.join("whisper.cpp")),
                    Tool::RealEsrgan => dirs.push(root.join("realesrgan")),
                    Tool::Rembg => dirs.push(root.join("rembg")),
                    Tool::Qrencode => dirs.push(root.join("qrencode")),
                    _ => {}
                }
            }
        } else if cfg!(target_os = "macos") {
            // A .app bundle is not on PATH; its executable lives under Contents/MacOS.
            let mut apps = vec![PathBuf::from("/Applications")];
            if let Some(home) = env::var_os("HOME") {
                apps.push(PathBuf::from(home).join("Applications"));
            }
            for app in &apps {
                match self {
                    Tool::LibreOffice => dirs.push(app.join("LibreOffice.app").join("Contents").join("MacOS")),
                    Tool::Calibre => dirs.push(app.join("calibre.app").join("Contents").join("MacOS")),
                    Tool::ExifTool => dirs.push(app.join("ExifTool").join("bin")),
                    _ => {}
                }
            }
            // Homebrew and MacPorts put their links here; Intel and Apple Silicon differ.
            dirs.push("/opt/homebrew/bin".into());
            dirs.push("/usr/local/bin".into());
            dirs.push("/opt/local/bin".into());
        } else {
            match self {
                Tool::LibreOffice => {
                    dirs.push("/usr/lib/libreoffice/program".into());
                    dirs.push("/usr/lib64/libreoffice/program".into());
                    dirs.extend(versioned_programs(Path::new("/opt"), "libreoffice"));
                    dirs.push("/opt/libreoffice/program".into());
                }
                Tool::Calibre => dirs.push("/opt/calibre".into()),
                Tool::RealEsrgan => dirs.push("/usr/local/realesrgan".into()),
                Tool::Whisper => dirs.push("/usr/local/whisper.cpp".into()),
                _ => {}
            }
            dirs.push("/snap/bin".into());
            dirs.push("/var/lib/flatpak/exports/bin".into());
        }
        dirs
    }
}

const ESRGAN_HINT: &str = "realesrgan-ncnn-vulkan release (github.com/xinntao/Real-ESRGAN/releases): put it on PATH with its models folder beside it";
const REMBG_HINT: &str =
    "pipx install \"rembg[cli]\", then put u2net.onnx in ~/.u2net (or set ZEIDEX_REMBG_MODEL); nothing is downloaded at run time";

/// `<root>/<prefix>*/bin` for installers that put the version in the folder name.
fn versioned_bins(root: &Path, prefix: &str) -> Vec<PathBuf> {
    let Ok(rd) = root.read_dir() else { return vec![] };
    let mut out: Vec<PathBuf> = rd
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().to_ascii_lowercase().starts_with(prefix))
        .map(|e| e.path().join("bin"))
        .collect();
    out.sort();
    out.reverse(); // newest version first
    out
}

/// `<root>/<prefix>*/program` for LibreOffice tarballs unpacked under `/opt`.
fn versioned_programs(root: &Path, prefix: &str) -> Vec<PathBuf> {
    let Ok(rd) = root.read_dir() else { return vec![] };
    let mut out: Vec<PathBuf> = rd
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().to_ascii_lowercase().starts_with(prefix))
        .map(|e| e.path().join("program"))
        .collect();
    out.sort();
    out.reverse();
    out
}

static BUNDLE_DIRS: OnceLock<Vec<PathBuf>> = OnceLock::new();

/// Register directories holding bundled engines (the app's resource dir). Call once at startup.
pub fn set_bundle_dirs(dirs: Vec<PathBuf>) {
    let _ = BUNDLE_DIRS.set(dirs);
}

fn bundle_dirs() -> Vec<PathBuf> {
    let mut dirs = BUNDLE_DIRS.get().cloned().unwrap_or_default();
    if let Some(exe_dir) = env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf)) {
        dirs.push(exe_dir.join("engines"));
    }
    with_engine_bins(dirs)
}

/// Each registered dir, then the layouts its engines unpack into:
///
///   `<engine>/bin`             most release archives (ffmpeg, pandoc, qpdf)
///   `<engine>/Library/bin`     conda trees on Windows (Ghostscript, Tesseract)
///   `<engine>/Scripts`         conda Python environments (rembg)
///   `<engine>/program`         LibreOffice, on every OS
///   `<engine>/Contents/MacOS`  a bare .app bundle
///   `<engine>/*.app/Contents/MacOS`  a .app beside other files
///   `<engine>`                 a single binary or a flat extract
///
/// The flat dir goes last so a tool unpacked straight into it still wins over nothing.
fn with_engine_bins(dirs: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut all = Vec::new();
    for d in dirs {
        if let Ok(rd) = d.read_dir() {
            for e in rd.flatten() {
                let p = e.path();
                all.push(p.join("bin"));
                all.push(p.join("Library").join("bin"));
                all.push(p.join("Scripts"));
                all.push(p.join("program"));
                all.push(p.join("Contents").join("MacOS"));
                if let Ok(inner) = p.read_dir() {
                    for app in inner.flatten() {
                        if app.path().extension().is_some_and(|x| x.eq_ignore_ascii_case("app")) {
                            all.push(app.path().join("Contents").join("MacOS"));
                        }
                    }
                }
                all.push(p);
            }
        }
        all.push(d);
    }
    all
}

/// The variable a bundled engine needs once its tree has left the prefix it was built under.
/// conda builds compile that prefix in, so inside a package Tesseract cannot find its language
/// data and libvips cannot find its HEIC/AVIF/JPEG XL modules. Engines outside the package keep
/// their own environment, as does anything the user already set.
pub fn bundled_env(bin: &Path) -> Option<(&'static str, PathBuf)> {
    relocated_env(bin).filter(|(key, _)| env::var_os(key).is_none() && bundle_dirs().iter().any(|d| bin.starts_with(d)))
}

fn relocated_env(bin: &Path) -> Option<(&'static str, PathBuf)> {
    let root = bin.parent()?.parent()?;
    // Unix conda trees put data under `share/` and `lib/`; Windows trees add a `Library/`
    // level. Accept either, so one extraction rule works on all three platforms.
    let first_dir = |candidates: &[PathBuf]| candidates.iter().find(|p| p.is_dir()).cloned();
    let (key, dir) = match bin.file_stem()?.to_str()? {
        "tesseract" => (
            "TESSDATA_PREFIX",
            first_dir(&[root.join("share").join("tessdata"), root.join("Library").join("share").join("tessdata")])?,
        ),
        // `lib`, not the tree root: a relocated conda libvips looks for `$VIPSHOME/vips-modules-*`.
        "vips" | "vipsheader" => ("VIPSHOME", first_dir(&[root.join("lib"), root.join("Library").join("lib")])?),
        _ => return None,
    };
    Some((key, dir))
}

pub fn which(name: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    env::split_paths(&path).map(|d| d.join(name)).find(|p| p.is_file())
}

pub fn locate(tool: Tool) -> Option<PathBuf> {
    if let Some(p) = env::var_os(tool.env_key()).map(PathBuf::from) {
        if p.is_file() {
            return Some(p);
        }
    }
    let names = tool.names();
    let in_dirs =
        |dirs: &[PathBuf]| dirs.iter().flat_map(|d| names.iter().map(move |n| d.join(n))).find(|p| p.is_file());
    in_dirs(&bundle_dirs()).or_else(|| names.iter().find_map(|n| which(n))).or_else(|| in_dirs(&tool.known_dirs()))
}

/// whisper.cpp model: `ZEIDEX_WHISPER_MODEL`, else the first `*.bin` in a bundled `models/` dir.
pub fn whisper_model() -> Option<PathBuf> {
    if let Some(p) = env::var_os("ZEIDEX_WHISPER_MODEL").map(PathBuf::from).filter(|p| p.is_file()) {
        return Some(p);
    }
    bundled_model("bin")
}

/// First `*.<ext>` in a bundled `models/` dir.
fn bundled_model(ext: &str) -> Option<PathBuf> {
    bundle_dirs()
        .iter()
        .map(|d| d.join("models"))
        .filter_map(|d| d.read_dir().ok())
        .flat_map(|rd| rd.flatten())
        .map(|e| e.path())
        .find(|p| p.extension().is_some_and(|e| e == ext))
}

/// Real-ESRGAN networks: `ZEIDEX_REALESRGAN_MODELS`, else the `models` folder beside the
/// engine, which is how its release archive unpacks.
pub fn esrgan_models() -> Option<PathBuf> {
    if let Some(p) = env::var_os("ZEIDEX_REALESRGAN_MODELS").map(PathBuf::from).filter(|p| p.is_dir()) {
        return Some(p);
    }
    locate(Tool::RealEsrgan)?.parent().map(|d| d.join("models")).filter(|d| d.is_dir())
}

/// rembg's ONNX model: `ZEIDEX_REMBG_MODEL`, else a bundled `models/*.onnx`, else rembg's own
/// cache (`~/.u2net/u2net.onnx`). The path is always passed explicitly, so rembg never
/// downloads a model while the app runs.
pub fn rembg_model() -> Option<PathBuf> {
    if let Some(p) = env::var_os("ZEIDEX_REMBG_MODEL").map(PathBuf::from).filter(|p| p.is_file()) {
        return Some(p);
    }
    if let Some(p) = bundled_model("onnx") {
        return Some(p);
    }
    let home = env::var_os("HOME").or_else(|| env::var_os("USERPROFILE"))?;
    Some(PathBuf::from(home).join(".u2net").join("u2net.onnx")).filter(|p| p.is_file())
}

pub fn require(tool: Tool) -> crate::Result<PathBuf> {
    locate(tool).ok_or(crate::Error::MissingTool(tool))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Engine archives unpack different ways; a bundled Ghostscript on Windows only exists under
    /// `<engine>/Library/bin`, so the search has to reach it, and the flat dir has to stay last.
    #[test]
    fn engine_bins_are_found_under_bin_and_library_bin() {
        let root = env::temp_dir().join(format!("zeidex-tools-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("gs").join("Library").join("bin")).unwrap();
        fs::create_dir_all(root.join("qpdf").join("bin")).unwrap();
        fs::write(root.join("gs").join("Library").join("bin").join("gswin64c.exe"), b"x").unwrap();

        let dirs = with_engine_bins(vec![root.clone()]);

        assert!(dirs.contains(&root.join("gs").join("Library").join("bin")));
        assert!(dirs.contains(&root.join("qpdf").join("bin")));
        assert_eq!(dirs.last(), Some(&root), "the flat dir is searched last");

        fs::remove_dir_all(&root).unwrap();
    }

    /// A bundled LibreOffice keeps `program/` (Linux and Windows), a macOS `.app` keeps
    /// `Contents/MacOS`, and a conda Python environment keeps console scripts in `Scripts`.
    /// Missing any of these is exactly how a bundled engine turns into "not found".
    #[test]
    fn engine_bins_cover_office_app_and_script_layouts() {
        let root = env::temp_dir().join(format!("zeidex-layouts-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("libreoffice").join("program")).unwrap();
        fs::create_dir_all(root.join("calibre").join("calibre.app").join("Contents").join("MacOS")).unwrap();
        fs::create_dir_all(root.join("rembg").join("Scripts")).unwrap();

        let dirs = with_engine_bins(vec![root.clone()]);

        assert!(dirs.contains(&root.join("libreoffice").join("program")));
        assert!(dirs.contains(&root.join("calibre").join("calibre.app").join("Contents").join("MacOS")));
        assert!(dirs.contains(&root.join("rembg").join("Scripts")));

        fs::remove_dir_all(&root).unwrap();
    }

    /// LibreOffice has a fixed home on every OS; a `.app` is not on PATH and a per-user
    /// Windows install is not under Program Files.
    #[test]
    fn libreoffice_known_dirs_are_complete() {
        let dirs = Tool::LibreOffice.known_dirs();
        if cfg!(windows) {
            let local: Vec<_> = dirs.iter().filter(|p| p.ends_with(Path::new("LibreOffice").join("program"))).collect();
            assert!(!local.is_empty(), "no LibreOffice/program candidate: {dirs:?}");
        } else if cfg!(target_os = "macos") {
            assert!(dirs.iter().any(|p| p.ends_with("LibreOffice.app/Contents/MacOS")));
        } else {
            assert!(dirs.contains(&PathBuf::from("/usr/lib/libreoffice/program")));
        }
    }

    /// A conda-built Tesseract or libvips only works inside a package when it is told where its
    /// data moved to; engines that carry no such tree get nothing.
    #[test]
    fn relocated_engines_are_pointed_at_their_data() {
        let root = env::temp_dir().join(format!("zeidex-reloc-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("bin")).unwrap();
        fs::create_dir_all(root.join("lib")).unwrap();
        fs::create_dir_all(root.join("share").join("tessdata")).unwrap();

        assert_eq!(
            relocated_env(&root.join("bin").join("tesseract")),
            Some(("TESSDATA_PREFIX", root.join("share").join("tessdata")))
        );
        assert_eq!(relocated_env(&root.join("bin").join("vips")), Some(("VIPSHOME", root.join("lib"))));
        assert_eq!(relocated_env(&root.join("bin").join("ffmpeg")), None);
        fs::remove_dir_all(root.join("share")).unwrap();
        assert_eq!(relocated_env(&root.join("bin").join("tesseract")), None, "no data tree, no variable");

        fs::remove_dir_all(&root).unwrap();
    }
}
