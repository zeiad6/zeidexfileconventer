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
}

impl Tool {
    pub const ALL: [Tool; 11] = [
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
            }
        }
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
            }
        }
    }

    fn known_dirs(self) -> Vec<PathBuf> {
        let mut dirs = Vec::new();
        if cfg!(windows) {
            let roots: Vec<PathBuf> = ["ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA"]
                .iter()
                .filter_map(|k| env::var_os(k).map(PathBuf::from))
                .collect();
            for root in &roots {
                match self {
                    Tool::LibreOffice => dirs.push(root.join("LibreOffice").join("program")),
                    Tool::Calibre => dirs.push(root.join("Calibre2")),
                    Tool::Tesseract => dirs.push(root.join("Tesseract-OCR")),
                    Tool::SevenZip => dirs.push(root.join("7-Zip")),
                    Tool::Pandoc => dirs.push(root.join("Pandoc")),
                    Tool::Ghostscript => dirs.extend(versioned_bins(&root.join("gs"), "")),
                    Tool::Qpdf => dirs.extend(versioned_bins(root, "qpdf")),
                    _ => {}
                }
            }
        } else {
            match self {
                Tool::LibreOffice => dirs.push("/usr/lib/libreoffice/program".into()),
                Tool::Calibre => dirs.push("/opt/calibre".into()),
                _ => {}
            }
            dirs.push("/snap/bin".into());
        }
        dirs
    }
}

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
    // A bundled engine may live in its own `<engine>/bin` (libvips needs its DLLs beside it).
    let mut all = Vec::new();
    for d in dirs {
        if let Ok(rd) = d.read_dir() {
            all.extend(rd.flatten().map(|e| e.path().join("bin")));
        }
        all.push(d);
    }
    all
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
    let in_dirs = |dirs: &[PathBuf]| {
        dirs.iter().flat_map(|d| names.iter().map(move |n| d.join(n))).find(|p| p.is_file())
    };
    in_dirs(&bundle_dirs())
        .or_else(|| names.iter().find_map(|n| which(n)))
        .or_else(|| in_dirs(&tool.known_dirs()))
}

pub fn require(tool: Tool) -> crate::Result<PathBuf> {
    locate(tool).ok_or(crate::Error::MissingTool(tool))
}
