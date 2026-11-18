//! External engines and where to find them.
//!
//! Lookup order: `ZEIDEX_<TOOL>` env var, bundled `engines/` dirs, `PATH`, known install dirs.

use std::collections::HashMap;
use std::env;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EngineTier {
    /// Core runtime shipped in both Lite and Full builds (FFmpeg, FFprobe, libvips, 7-Zip).
    Core,
    /// Heavy/optional engines shipped in Full builds or installed on-demand in Lite builds.
    Optional,
}

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

    pub fn tier(self) -> EngineTier {
        match self {
            Tool::Ffmpeg | Tool::Ffprobe | Tool::Vips | Tool::VipsHeader | Tool::SevenZip => EngineTier::Core,
            Tool::Ghostscript
            | Tool::Qpdf
            | Tool::LibreOffice
            | Tool::Pandoc
            | Tool::Calibre
            | Tool::Tesseract
            | Tool::Whisper
            | Tool::ExifTool
            | Tool::Zbar
            | Tool::Qrencode
            | Tool::RealEsrgan
            | Tool::Rembg => EngineTier::Optional,
        }
    }

    pub fn is_lite_core(self) -> bool {
        self.tier() == EngineTier::Core
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

    /// The one shell line the Engines panel's Install button runs, or `None` when the install
    /// needs a manual step the app must not pretend to execute (putting a binary on PATH,
    /// downloading a model). Kept beside `install_hint` so a hint and its command cannot drift.
    pub fn install_command(self) -> Option<&'static str> {
        if self == Tool::Rembg {
            // A bare `pipx` is not on Windows and, even on Unix, may not be installed either.
            // The Windows line installs Python first, then uses that interpreter's pip.
            return Some(if cfg!(windows) {
                // No double quotes anywhere: the line is re-quoted for `cmd /k` by the launcher,
                // and `rembg[cli]` needs no quoting in cmd (brackets are only special to sh).
                "winget install --id Python.Python.3.12 -e --accept-package-agreements --accept-source-agreements \
                 && py -3.12 -m pip install --user --upgrade rembg[cli] \
                 && echo. && echo Put u2net.onnx in %USERPROFILE%\\.u2net then restart Zeidex"
            } else if cfg!(target_os = "macos") {
                "python3 -m pip install --user --upgrade \"rembg[cli]\" \
                 && echo \"Put u2net.onnx in ~/.u2net, then reopen Zeidex\""
            } else {
                "pipx install \"rembg[cli]\" || python3 -m pip install --user --upgrade \"rembg[cli]\" \
                 && echo \"Put u2net.onnx in ~/.u2net, then reopen Zeidex\""
            });
        }
        let hint = self.install_hint();
        // Only a real package-manager line is executed. A hint that explains a manual step
        // ("put it on PATH", "build from source") is printed instead of run as if it were a
        // command; guessing wrong there would run nonsense in a shell.
        const RUNNERS: [&str; 9] =
            ["winget ", "scoop ", "choco ", "brew ", "sudo ", "apt ", "dnf ", "pacman ", "pipx "];
        if !RUNNERS.iter().any(|prefix| hint.starts_with(prefix)) {
            return None;
        }
        // A trailing parenthetical is advice for the reader ("or use the bundled build"), not
        // part of the command; feeding it to the shell would make the package manager choke.
        Some(hint.split(" (").next().unwrap_or(hint).trim_end())
    }

    fn known_dirs(self) -> Vec<PathBuf> {
        let mut dirs = Vec::new();

        // pip --user and pipx put console scripts here on every Unix; it is not always on PATH.
        if !cfg!(windows) {
            if let Some(home) = env::var_os("HOME") {
                dirs.push(PathBuf::from(&home).join(".local").join("bin"));
            }
        }

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
                    Tool::Rembg => {
                        dirs.push(root.join("rembg"));
                        // `py -m pip install --user` puts console scripts under
                        // %APPDATA%\Python\Python3xx\Scripts, which is not on PATH by default.
                        if root.ends_with("APPDATA") {
                            if let Ok(rd) = root.join("Python").read_dir() {
                                for entry in rd.flatten() {
                                    let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
                                    if name.starts_with("python3") {
                                        dirs.push(entry.path().join("Scripts"));
                                    }
                                }
                            }
                        }
                    }
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
            // pip --user on macOS: ~/Library/Python/3.x/bin.
            if let Some(home) = env::var_os("HOME") {
                if let Ok(rd) = PathBuf::from(home).join("Library").join("Python").read_dir() {
                    for entry in rd.flatten() {
                        dirs.push(entry.path().join("bin"));
                    }
                }
            }
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
const REMBG_HINT_UNIX: &str =
    "python3 -m pip install --user \"rembg[cli]\" (or: pipx install \"rembg[cli]\"), then put u2net.onnx in ~/.u2net (or set ZEIDEX_REMBG_MODEL); nothing is downloaded at run time";
// pipx is not part of Windows and is not shipped with Python, so the old hint ended in
// "'pipx' is not recognized". The app installs Python first, then uses its own pip.
const REMBG_HINT_WIN: &str =
    "Python 3.12 + \"pip install --user rembg[cli]\" (no pipx needed), then put u2net.onnx in %USERPROFILE%\\.u2net (or set ZEIDEX_REMBG_MODEL); nothing is downloaded at run time";
const REMBG_HINT: &str = if cfg!(windows) { REMBG_HINT_WIN } else { REMBG_HINT_UNIX };

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

static BUNDLE_DIRS: OnceLock<Mutex<Vec<PathBuf>>> = OnceLock::new();

fn bundle_dir_store() -> &'static Mutex<Vec<PathBuf>> {
    BUNDLE_DIRS.get_or_init(|| Mutex::new(Vec::new()))
}

/// Strips the Windows extended-length verbatim prefix (`\\?\` or `\\?\UNC\`) and normalizes
/// directory separators (`/` -> `\`) so C/C++ engines (Tesseract, FFmpeg, Ghostscript, 7-Zip,
/// LibreOffice, etc.) never receive `\\?\` or mixed slashes.
pub fn normalize_windows_path_str(raw: &str) -> String {
    let stripped = if let Some(unc) = raw.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{unc}")
    } else if let Some(rest) = raw.strip_prefix(r"\\?\") {
        rest.to_string()
    } else {
        raw.to_string()
    };
    stripped.replace('/', "\\")
}

/// Normalizes a filesystem path before passing it to an external engine or environment variable.
/// On Windows, strips any `\\?\` verbatim prefix introduced by `canonicalize()` or Tauri's
/// `resource_dir()` and normalizes `/` to `\`. On other platforms this is a no-op.
pub fn normalize_engine_path(path: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        PathBuf::from(normalize_windows_path_str(&path.to_string_lossy()))
    }
    #[cfg(not(windows))]
    {
        path.to_path_buf()
    }
}

/// Register directories holding bundled or on-demand local engines.
pub fn set_bundle_dirs(dirs: Vec<PathBuf>) {
    let normalized = dirs.into_iter().map(|d| normalize_engine_path(&d)).collect();
    *bundle_dir_store().lock().unwrap_or_else(|e| e.into_inner()) = normalized;
}

/// Adds an engine root directory if not already registered (e.g., after an on-demand download).
pub fn register_bundle_dir(dir: PathBuf) {
    let dir = normalize_engine_path(&dir);
    let mut list = bundle_dir_store().lock().unwrap_or_else(|e| e.into_inner());
    if !list.contains(&dir) {
        list.push(dir);
    }
}

/// Clears cached binary probe results so newly downloaded engines are re-evaluated.
pub fn clear_probe_cache() {
    probe_cache().lock().unwrap_or_else(|e| e.into_inner()).clear();
    broken_cache().lock().unwrap_or_else(|e| e.into_inner()).clear();
}

/// Returns `"full"` when optional engines (e.g. Pandoc, Ghostscript, Whisper, Real-ESRGAN,
/// LibreOffice, Calibre) are staged inside a registered bundle directory, otherwise `"lite"`.
pub fn build_profile() -> &'static str {
    let bundles = bundle_dirs();
    let has_optional =
        [Tool::Pandoc, Tool::Ghostscript, Tool::Whisper, Tool::RealEsrgan, Tool::LibreOffice, Tool::Calibre]
            .into_iter()
            .any(|tool| tool.names().iter().any(|name| bundles.iter().any(|dir| dir.join(name).is_file())));
    if has_optional {
        "full"
    } else {
        "lite"
    }
}

/// True when `path` resides inside one of the registered bundle/local engine directories.
pub fn is_bundled_path(path: &Path) -> bool {
    let path = normalize_engine_path(path);
    bundle_dirs().iter().any(|d| path.starts_with(d))
}

fn bundle_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> =
        bundle_dir_store().lock().unwrap_or_else(|e| e.into_inner()).iter().map(|d| normalize_engine_path(d)).collect();
    if let Some(exe_dir) = env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf)) {
        dirs.push(normalize_engine_path(&exe_dir.join("engines")));
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
    let bin = normalize_engine_path(bin);
    relocated_env(&bin)
        .filter(|(key, _)| env::var_os(key).is_none() && bundle_dirs().iter().any(|d| bin.starts_with(d)))
        .map(|(k, p)| (k, normalize_engine_path(&p)))
}

fn has_traineddata(dir: &Path) -> bool {
    let Ok(rd) = std::fs::read_dir(dir) else { return false };
    rd.flatten().any(|e| e.path().extension().is_some_and(|ext| ext == "traineddata"))
}

/// Finds Tesseract's `tessdata` directory across both conda `Library/share/tessdata` and flat
/// `tessdata` / `share/tessdata` packaging layouts.
pub fn find_tessdata_dir(bin: &Path) -> Option<PathBuf> {
    let bin = normalize_engine_path(bin);
    let bin_dir = bin.parent()?;
    let mut candidates = Vec::new();
    if let Some(root) = bin_dir.parent() {
        candidates.push(root.join("share").join("tessdata"));
        candidates.push(root.join("Library").join("share").join("tessdata"));
        candidates.push(root.join("tessdata"));
        if root.file_name().is_some_and(|n| n == "Library") {
            if let Some(pkg_root) = root.parent() {
                candidates.push(pkg_root.join("tessdata"));
                candidates.push(pkg_root.join("share").join("tessdata"));
            }
        }
    }
    candidates.push(bin_dir.join("tessdata"));
    candidates.push(bin_dir.join("share").join("tessdata"));
    candidates.push(bin_dir.join("Library").join("share").join("tessdata"));

    candidates
        .iter()
        .find(|p| p.is_dir() && has_traineddata(p))
        .or_else(|| candidates.iter().find(|p| p.is_dir()))
        .map(|p| normalize_engine_path(p))
}

fn relocated_env(bin: &Path) -> Option<(&'static str, PathBuf)> {
    let bin = normalize_engine_path(bin);
    let root = bin.parent()?.parent()?;
    // Unix conda trees put data under `share/` and `lib/`; Windows trees add a `Library/`
    // level. Accept either, so one extraction rule works on all three platforms.
    let first_dir = |candidates: &[PathBuf]| candidates.iter().find(|p| p.is_dir()).cloned();
    let (key, dir) = match bin.file_stem()?.to_str()? {
        "tesseract" => ("TESSDATA_PREFIX", find_tessdata_dir(&bin)?),
        // `lib`, not the tree root: a relocated conda libvips looks for `$VIPSHOME/vips-modules-*`.
        "vips" | "vipsheader" => ("VIPSHOME", first_dir(&[root.join("lib"), root.join("Library").join("lib")])?),
        _ => return None,
    };
    Some((key, normalize_engine_path(&dir)))
}

pub fn which(name: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    env::split_paths(&path).map(|d| normalize_engine_path(&d.join(name))).find(|p| p.is_file())
}

/// Every place `tool` may live, in lookup order: explicit override, the package, PATH, then the
/// installer's known folders. Kept public so an install or a diagnostic can show all candidates.
pub fn candidates(tool: Tool) -> Vec<PathBuf> {
    candidates_in(tool, &bundle_dirs())
}

fn candidates_in(tool: Tool, bundles: &[PathBuf]) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    let names = tool.names();
    out.extend(
        env::var_os(tool.env_key()).map(PathBuf::from).map(|p| normalize_engine_path(&p)).filter(|p| p.is_file()),
    );
    for dir in bundles {
        out.extend(names.iter().map(|n| normalize_engine_path(&dir.join(n))).filter(|p| p.is_file()));
    }
    out.extend(names.iter().filter_map(|n| which(n)));
    for dir in tool.known_dirs() {
        out.extend(names.iter().map(|n| normalize_engine_path(&dir.join(n))).filter(|p| p.is_file()));
    }
    out.dedup();
    out
}

pub fn locate(tool: Tool) -> Option<PathBuf> {
    // One scan of the engine tree per lookup: `usable` needs to know whether a candidate came
    // from the package, and the tree can hold a dozen engines.
    let bundles = bundle_dirs();
    candidates_in(tool, &bundles).into_iter().find(|p| usable(tool, p, &bundles))
}

/// Engines whose packages carry a separate DLL closure (leptonica, libtiff, the VC runtime…).
/// Copying only the executable leaves a binary that dies in the loader, so a bundled copy of one
/// of these is started once before it is trusted; a working system install is preferred when the
/// bundled one cannot run.
const PROBED: [Tool; 5] = [Tool::Tesseract, Tool::Vips, Tool::VipsHeader, Tool::Ghostscript, Tool::Qrencode];

/// A probe gets this long before the binary counts as alive. Tesseract answers in milliseconds;
/// rembg and LibreOffice are Python/Java-sized starts and are not probed at all.
const PROBE_TIMEOUT: Duration = Duration::from_secs(3);

/// Probe outcome per binary path for this session: `locate` runs on every description and plan,
/// so each candidate may be started at most once.
static PROBES: OnceLock<Mutex<HashMap<PathBuf, bool>>> = OnceLock::new();
/// Bundled binaries that were found but could not start, with the tool they belong to.
static BROKEN: OnceLock<Mutex<HashMap<PathBuf, Tool>>> = OnceLock::new();

fn probe_cache() -> &'static Mutex<HashMap<PathBuf, bool>> {
    PROBES.get_or_init(|| Mutex::new(HashMap::new()))
}

fn broken_cache() -> &'static Mutex<HashMap<PathBuf, Tool>> {
    BROKEN.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Whether `bin` starts at all. A missing DLL kills the process inside the loader, before `main`,
/// and the status is an NTSTATUS (`0xC0000135` = -1073741515, `0xC0000139` = -1073741511). Any
/// other outcome — success, or an ordinary error from an unsupported `--version` — proves the
/// loader resolved everything the binary needs.
fn starts(bin: &Path) -> bool {
    let bin = normalize_engine_path(bin);
    if let Some(known) = probe_cache().lock().unwrap_or_else(|e| e.into_inner()).get(&bin).copied() {
        return known;
    }
    let mut cmd = Command::new(&bin);
    cmd.arg("--version").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    // Probe the binary the way the app will run it: a relocated conda tree needs its data path.
    if let Some((key, dir)) = relocated_env(&bin) {
        cmd.env(key, normalize_engine_path(&dir));
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let ok = match cmd.spawn() {
        Ok(mut child) => {
            // A loader failure exits immediately; a hung binary must not hang the caller, so the
            // wait is bounded and a binary still running at the deadline counts as alive.
            let deadline = std::time::Instant::now() + PROBE_TIMEOUT;
            loop {
                match child.try_wait() {
                    Ok(Some(status)) => {
                        let code = status.code().unwrap_or(0) as u32;
                        break code != 0xC000_0135 && code != 0xC000_0139;
                    }
                    Ok(None) if std::time::Instant::now() < deadline => {
                        std::thread::sleep(Duration::from_millis(20));
                    }
                    Ok(None) => {
                        let _ = child.kill();
                        break true;
                    }
                    Err(_) => break true,
                }
            }
        }
        // Cannot even be spawned (wrong architecture, no execute bit): treat as unusable rather
        // than reporting a tool that will fail on the first real job.
        Err(_) => false,
    };
    probe_cache().lock().unwrap_or_else(|e| e.into_inner()).insert(bin, ok);
    ok
}

/// A binary in the app's own `engines/` tree is probed; a system install is trusted. Users do
/// install engines themselves and the app cannot test every one of them on every launch.
fn usable(tool: Tool, bin: &Path, bundles: &[PathBuf]) -> bool {
    if !bundles.iter().any(|d| bin.starts_with(d)) || !PROBED.contains(&tool) {
        return true;
    }
    let ok = starts(bin);
    if !ok {
        broken_cache().lock().unwrap_or_else(|e| e.into_inner()).insert(bin.to_path_buf(), tool);
    }
    ok
}

/// Every tool whose only copies were found but refused to start, as `(tool, path)`, for the
/// Engines panel: the row then says "missing DLL" instead of showing a green check that fails
/// on the first job.
pub fn broken_engines() -> Vec<(Tool, PathBuf)> {
    let broken = broken_cache().lock().unwrap_or_else(|e| e.into_inner());
    let mut rows: Vec<(Tool, PathBuf)> = broken.iter().map(|(p, t)| (*t, p.clone())).collect();
    rows.sort_by(|a, b| a.0.slug().cmp(b.0.slug()));
    rows.dedup_by(|a, b| a.0 == b.0);
    rows
}

/// whisper.cpp model: `ZEIDEX_WHISPER_MODEL`, else the first `*.bin` in a bundled `models/` dir.
pub fn whisper_model() -> Option<PathBuf> {
    if let Some(p) = env::var_os("ZEIDEX_WHISPER_MODEL").map(PathBuf::from).filter(|p| p.is_file()) {
        return Some(normalize_engine_path(&p));
    }
    bundled_model("bin")
}

/// First `*.<ext>` in a bundled `models/` dir (also checks `<exe_dir>/models` so standalone
/// CLI binaries never depend on the caller's working directory).
fn bundled_model(ext: &str) -> Option<PathBuf> {
    let mut dirs = bundle_dirs();
    if let Some(exe_dir) = env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf)) {
        dirs.push(normalize_engine_path(&exe_dir));
    }
    dirs.iter()
        .map(|d| d.join("models"))
        .filter_map(|d| d.read_dir().ok())
        .flat_map(|rd| rd.flatten())
        .map(|e| normalize_engine_path(&e.path()))
        .find(|p| p.extension().is_some_and(|e| e == ext))
}

/// Real-ESRGAN networks: `ZEIDEX_REALESRGAN_MODELS`, else the `models` folder beside the
/// engine, which is how its release archive unpacks.
pub fn esrgan_models() -> Option<PathBuf> {
    if let Some(p) = env::var_os("ZEIDEX_REALESRGAN_MODELS").map(PathBuf::from).filter(|p| p.is_dir()) {
        return Some(normalize_engine_path(&p));
    }
    locate(Tool::RealEsrgan)?.parent().map(|d| normalize_engine_path(&d.join("models"))).filter(|d| d.is_dir())
}

/// rembg's ONNX model: `ZEIDEX_REMBG_MODEL`, else a bundled `models/*.onnx`, else rembg's own
/// cache (`~/.u2net/u2net.onnx`). The path is always passed explicitly, so rembg never
/// downloads a model while the app runs.
pub fn rembg_model() -> Option<PathBuf> {
    if let Some(p) = env::var_os("ZEIDEX_REMBG_MODEL").map(PathBuf::from).filter(|p| p.is_file()) {
        return Some(normalize_engine_path(&p));
    }
    if let Some(p) = bundled_model("onnx") {
        return Some(p);
    }
    let home = env::var_os("HOME").or_else(|| env::var_os("USERPROFILE"))?;
    Some(normalize_engine_path(&PathBuf::from(home).join(".u2net").join("u2net.onnx"))).filter(|p| p.is_file())
}

pub fn require(tool: Tool) -> crate::Result<PathBuf> {
    locate(tool).ok_or(crate::Error::MissingTool(tool))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// rembg is the one engine that needs a Python runtime, so its install line is written by
    /// hand instead of reusing a package-manager hint. Windows has no `pipx` at all: the old hint
    /// sent users there and `cmd` answered `'pipx' is not recognized`.
    #[test]
    fn rembg_installs_without_pipx_on_windows() {
        let command = Tool::Rembg.install_command().expect("rembg must be installable from the panel");
        assert!(command.contains("rembg"), "the line has to install rembg: {command}");
        if cfg!(windows) {
            assert!(command.starts_with("winget "), "Windows installs Python first: {command}");
            assert!(!command.contains("pipx"), "pipx does not exist on Windows: {command}");
            assert!(!command.contains('"'), "double quotes fight the cmd /k launcher: {command}");
        }
        // A hint that is advice rather than a command must never be executed.
        assert!(Tool::RealEsrgan.install_command().is_none());
        if cfg!(target_os = "linux") {
            assert_eq!(Tool::Ffmpeg.install_command(), Some("sudo apt install ffmpeg"));
        }
    }

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

    #[test]
    fn windows_verbatim_paths_are_normalized() {
        assert_eq!(
            normalize_windows_path_str(
                r"\\?\C:\Users\ace\AppData\Local\Zeidex Convert\engines\ocr\Library\share\tessdata/ara.traineddata"
            ),
            r"C:\Users\ace\AppData\Local\Zeidex Convert\engines\ocr\Library\share\tessdata\ara.traineddata"
        );
        assert_eq!(
            normalize_windows_path_str(
                r"\\?\C:\Users\ace\AppData\Local\Zeidex Convert\engines\ocr\Library\share\tessdata"
            ),
            r"C:\Users\ace\AppData\Local\Zeidex Convert\engines\ocr\Library\share\tessdata"
        );
        assert_eq!(
            normalize_windows_path_str(r"\\?\UNC\fileserver\share\Zeidex Convert/doc.pdf"),
            r"\\fileserver\share\Zeidex Convert\doc.pdf"
        );
        assert_eq!(
            normalize_windows_path_str(r"C:\Users\ace\Documents/file with spaces.png"),
            r"C:\Users\ace\Documents\file with spaces.png"
        );
    }

    #[test]
    fn tessdata_resolves_both_library_and_flat_layouts() {
        let root = env::temp_dir().join(format!("zeidex-tessdata-layouts-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);

        let lib_bin = root.join("ocr-lib").join("Library").join("bin");
        let lib_tess = root.join("ocr-lib").join("Library").join("share").join("tessdata");
        fs::create_dir_all(&lib_bin).unwrap();
        fs::create_dir_all(&lib_tess).unwrap();
        fs::write(lib_tess.join("ara.traineddata"), b"stub").unwrap();
        assert_eq!(find_tessdata_dir(&lib_bin.join("tesseract")), Some(lib_tess.clone()));

        let flat_bin = root.join("ocr-flat").join("bin");
        let flat_tess = root.join("ocr-flat").join("tessdata");
        fs::create_dir_all(&flat_bin).unwrap();
        fs::create_dir_all(&flat_tess).unwrap();
        fs::write(flat_tess.join("eng.traineddata"), b"stub").unwrap();
        assert_eq!(find_tessdata_dir(&flat_bin.join("tesseract")), Some(flat_tess.clone()));

        let direct_dir = root.join("ocr-direct");
        let direct_tess = direct_dir.join("tessdata");
        fs::create_dir_all(&direct_tess).unwrap();
        fs::write(direct_tess.join("eng.traineddata"), b"stub").unwrap();
        assert_eq!(find_tessdata_dir(&direct_dir.join("tesseract")), Some(direct_tess));

        fs::remove_dir_all(&root).unwrap();
    }
}
