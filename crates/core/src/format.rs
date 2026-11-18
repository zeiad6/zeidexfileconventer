//! Format registry: what a file is, and what it can be converted to.

use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Category {
    Image,
    Video,
    Audio,
    Pdf,
    Document,
    Slides,
    Spreadsheet,
    Ebook,
    Subtitle,
    Archive,
}

impl Category {
    pub const ALL: [Category; 10] = [
        Category::Image,
        Category::Video,
        Category::Audio,
        Category::Pdf,
        Category::Document,
        Category::Slides,
        Category::Spreadsheet,
        Category::Ebook,
        Category::Subtitle,
        Category::Archive,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Category::Image => "image",
            Category::Video => "video",
            Category::Audio => "audio",
            Category::Pdf => "pdf",
            Category::Document => "document",
            Category::Slides => "slides",
            Category::Spreadsheet => "spreadsheet",
            Category::Ebook => "ebook",
            Category::Subtitle => "subtitle",
            Category::Archive => "archive",
        }
    }

    fn inputs(self) -> &'static [&'static str] {
        match self {
            Category::Image => &[
                "jpg", "png", "webp", "heic", "avif", "tiff", "bmp", "gif", "svg", "jxl", "jp2", "ico", "apng", "ppm",
                "tga", "dds", "pcx", "psd", "dng", "cr2", "cr3", "nef", "arw", "rw2", "raf", "raw", "livp",
            ],
            Category::Video => &[
                "mp4", "mov", "m4v", "mkv", "webm", "avi", "wmv", "flv", "mpg", "3gp", "ts", "m2ts", "3g2", "ogv",
                "vob", "mxf",
            ],
            Category::Audio => &[
                "mp3", "m4a", "aac", "wav", "flac", "ogg", "opus", "aiff", "wma", "ac3", "amr", "caf", "weba", "mka",
                "ape", "wv", "w64",
            ],
            Category::Pdf => &["pdf"],
            Category::Document => {
                &["docx", "doc", "odt", "rtf", "pages", "md", "txt", "html", "rst", "tex", "org", "adoc", "typ"]
            }
            Category::Slides => &["pptx", "ppt", "odp", "key"],
            Category::Spreadsheet => &["xlsx", "xls", "ods", "csv", "tsv", "numbers"],
            Category::Ebook => &["epub", "mobi", "azw3", "fb2"],
            Category::Subtitle => &["srt", "vtt", "ass", "ssa", "smi", "lrc"],
            Category::Archive => {
                &["zip", "tar", "tgz", "gz", "7z", "rar", "bz2", "xz", "tbz2", "txz", "cab", "iso", "deb", "rpm"]
            }
        }
    }

    fn outputs(self) -> &'static [&'static str] {
        match self {
            Category::Image => {
                &["jpg", "png", "webp", "heic", "avif", "tiff", "bmp", "gif", "pdf", "ico", "jxl", "jp2"]
            }
            Category::Video => &["mp4", "mov", "mkv", "webm", "avi", "wmv", "gif", "mp3", "m4a", "wav"],
            Category::Audio => &["mp3", "m4a", "wav", "flac", "ogg", "opus", "aiff", "wma", "ac3"],
            Category::Pdf => &["docx", "pptx", "xlsx", "epub", "txt", "md", "jpg", "png"],
            Category::Document => &["pdf", "docx", "odt", "rtf", "md", "txt", "html", "epub"],
            Category::Slides => &["pdf", "pptx", "odp", "png", "jpg"],
            Category::Spreadsheet => &["xlsx", "ods", "csv", "tsv", "pdf", "html"],
            Category::Ebook => &["epub", "pdf", "mobi", "azw3", "docx", "txt", "md", "html"],
            Category::Subtitle => &["srt", "vtt", "txt", "ass"],
            Category::Archive => &["zip", "7z", "tar", "tgz", "tbz2", "txz"],
        }
    }
}

const COMPOUND_EXTS: &[(&str, &str)] = &[
    (".tar.gz", "tgz"),
    (".tar.bz2", "tbz2"),
    (".tar.bz", "tbz2"),
    (".tbz2", "tbz2"),
    (".tar.xz", "txz"),
    (".txz", "txz"),
    (".tar.7z", "tar7z"),
    (".tar.z", "tz"),
    (".kepub.epub", "epub"),
];

/// Lower-cased extension with aliases folded (`jpeg`→`jpg`, `x.tar.gz`→`tgz`).
pub fn ext_of(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_str()?.to_ascii_lowercase();
    for (compound, alias) in COMPOUND_EXTS {
        if name.ends_with(compound) {
            return Some((*alias).into());
        }
    }
    let ext = name.rsplit_once('.')?.1;
    Some(normalize(ext).to_string())
}

pub fn normalize(ext: &str) -> &str {
    match ext {
        "jpeg" | "jpe" | "jfif" => "jpg",
        "tif" => "tiff",
        "heif" | "hif" => "heic",
        "htm" | "xhtml" => "html",
        "markdown" => "md",
        "aif" | "aifc" => "aiff",
        "oga" => "ogg",
        "mpeg" | "mpe" | "m1v" | "m2v" => "mpg",
        "dib" => "bmp",
        "svgz" => "svg",
        "j2k" | "jpf" | "jpx" => "jp2",
        "mts" | "m2t" => "ts",
        "wave" => "wav",
        "m4b" => "m4a",
        other => other,
    }
}

/// File name without its extension; `a.tar.gz` gives `a`.
pub fn stem_of(path: &Path) -> String {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
    let lower = name.to_ascii_lowercase();
    for (compound, _) in COMPOUND_EXTS {
        if lower.ends_with(compound) {
            return name[..name.len() - compound.len()].to_string();
        }
    }
    match name.rsplit_once('.') {
        Some((stem, _)) if !stem.is_empty() => stem.to_string(),
        _ => name.to_string(),
    }
}

pub fn category_of_ext(ext: &str) -> Option<Category> {
    Category::ALL.into_iter().find(|c| c.inputs().contains(&ext))
}

pub fn category_of(path: &Path) -> Option<Category> {
    category_of_ext(&ext_of(path)?)
}

/// Conversion targets for one file, without its own format.
pub fn targets(path: &Path) -> Vec<&'static str> {
    let Some(ext) = ext_of(path) else { return vec![] };
    let Some(cat) = category_of_ext(&ext) else { return vec![] };
    cat.outputs().iter().copied().filter(|t| *t != ext).collect()
}

/// Targets every file in the selection supports, in registry order.
pub fn common_targets<P: AsRef<Path>>(paths: &[P]) -> Vec<&'static str> {
    let mut iter = paths.iter();
    let Some(first) = iter.next() else { return vec![] };
    let mut acc = targets(first.as_ref());
    for p in iter {
        let t = targets(p.as_ref());
        acc.retain(|x| t.contains(x));
    }
    acc
}

/// The category shared by every file, if there is one.
pub fn common_category<P: AsRef<Path>>(paths: &[P]) -> Option<Category> {
    let first = category_of(paths.first()?.as_ref())?;
    paths.iter().all(|p| category_of(p.as_ref()) == Some(first)).then_some(first)
}

/// Expands any dropped directories recursively into regular files (bounded depth and count,
/// skipping hidden entries) while preserving all dropped regular files in order.
pub fn expand_dropped_paths<P: AsRef<Path>>(paths: &[P]) -> Vec<PathBuf> {
    const MAX_DEPTH: usize = 8;
    const MAX_FILES: usize = 2000;
    let mut out = Vec::new();
    let mut stack: Vec<(PathBuf, usize)> = paths.iter().rev().map(|p| (p.as_ref().to_path_buf(), 0)).collect();
    while let Some((p, depth)) = stack.pop() {
        if out.len() >= MAX_FILES {
            break;
        }
        if p.is_dir() {
            if depth >= MAX_DEPTH {
                continue;
            }
            let Ok(rd) = std::fs::read_dir(&p) else { continue };
            let mut entries: Vec<PathBuf> =
                rd.flatten().filter(|e| !e.file_name().to_string_lossy().starts_with('.')).map(|e| e.path()).collect();
            entries.sort();
            for child in entries.into_iter().rev() {
                stack.push((child, depth + 1));
            }
        } else if !out.contains(&p) {
            out.push(p);
        }
    }
    out
}

/// Targets for a heterogeneous selection: every output at least one file can become, with the
/// number of files that support it, most-covered first. A drag of ten photos and one video
/// therefore offers JPG (10/11) before MP4 (1/11), and the wheel is never empty just because
/// the files do not share a category.
pub fn mixed_targets<P: AsRef<Path>>(paths: &[P]) -> Vec<(&'static str, usize)> {
    let mut counts: Vec<(&'static str, usize, Vec<Category>)> = Vec::new();
    let mut cats: Vec<Category> = Vec::new();
    for p in paths {
        let Some(ext) = ext_of(p.as_ref()) else { continue };
        let Some(cat) = category_of_ext(&ext) else { continue };
        if !cats.contains(&cat) {
            cats.push(cat);
        }
        // A file votes once per target, never twice for the same output.
        let mut voted: Vec<&'static str> = Vec::new();
        for t in cat.outputs() {
            if *t == ext || voted.contains(t) {
                continue;
            }
            voted.push(t);
            match counts.iter_mut().find(|(x, _, _)| x == t) {
                Some((_, n, cs)) => {
                    *n += 1;
                    if !cs.contains(&cat) {
                        cs.push(cat);
                    }
                }
                None => counts.push((t, 1, vec![cat])),
            }
        }
    }
    // Stable sort: equal coverage keeps the registry order.
    counts.sort_by_key(|c| std::cmp::Reverse(c.1));
    // Reserve one segment per category first, so a drag of ten photos and one clip still
    // offers a video target; then fill the remaining slots by coverage.
    let mut chosen: Vec<&'static str> = Vec::new();
    for cat in &cats {
        if chosen.len() == 8 {
            break;
        }
        if let Some((t, _, _)) = counts.iter().find(|(t, _, cs)| cs.contains(cat) && !chosen.contains(t)) {
            chosen.push(t);
        }
    }
    for (t, _, _) in &counts {
        if chosen.len() == 8 {
            break;
        }
        if !chosen.contains(t) {
            chosen.push(t);
        }
    }
    counts.into_iter().filter(|(t, _, _)| chosen.contains(t)).map(|(t, n, _)| (t, n)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aliases_and_double_extensions() {
        assert_eq!(ext_of(Path::new("A.JPEG")).as_deref(), Some("jpg"));
        assert_eq!(ext_of(Path::new("photo.jfif")).as_deref(), Some("jpg"));
        assert_eq!(ext_of(Path::new("vector.svgz")).as_deref(), Some("svg"));
        assert_eq!(ext_of(Path::new("x.tar.gz")).as_deref(), Some("tgz"));
        assert_eq!(ext_of(Path::new("archive.tar.bz2")).as_deref(), Some("tbz2"));
        assert_eq!(ext_of(Path::new("pack.tar.xz")).as_deref(), Some("txz"));
        assert_eq!(stem_of(Path::new("dir/My.Backup.tar.gz")), "My.Backup");
        assert_eq!(stem_of(Path::new("dir/data.tar.bz2")), "data");
        assert_eq!(stem_of(Path::new(".bashrc")), ".bashrc");
        assert_eq!(ext_of(Path::new("noext")), None);
        assert_eq!(category_of_ext("jxl"), Some(Category::Image));
        assert_eq!(category_of_ext("m2ts"), Some(Category::Video));
        assert_eq!(category_of_ext("ac3"), Some(Category::Audio));
        assert_eq!(category_of_ext("ass"), Some(Category::Subtitle));
        assert_eq!(category_of_ext("rst"), Some(Category::Document));
    }

    #[test]
    fn targets_exclude_self_and_intersect() {
        let t = targets(Path::new("a.png"));
        assert!(t.contains(&"jpg") && t.contains(&"pdf") && !t.contains(&"png"));
        assert_eq!(common_targets(&["a.png", "b.mp3"]), Vec::<&str>::new());
        assert_eq!(common_targets(&["a.png", "b.mp4"]), vec!["gif"]);
        let both = common_targets(&["a.png", "b.jpg"]);
        assert!(both.contains(&"webp") && !both.contains(&"png") && !both.contains(&"jpg"));
        assert_eq!(common_category(&["a.png", "b.JPG"]), Some(Category::Image));
        assert_eq!(common_category(&["a.png", "b.mp3"]), None);
        assert!(targets(Path::new("x.unknown")).is_empty());
    }

    #[test]
    fn mixed_selection_never_has_an_empty_wheel() {
        // The old behaviour: no shared category, no shared target, empty wheel.
        assert!(common_targets(&["a.png", "b.mp3"]).is_empty());
        let mixed = mixed_targets(&["a.png", "b.mp3"]);
        assert!(!mixed.is_empty(), "mixed drag must still offer conversions");
        assert_eq!(mixed.len(), 8, "capped so the wheel stays readable");
        // Ten photos and one clip: the photo formats carry the higher coverage.
        let mut paths: Vec<String> = (0..10).map(|i| format!("p{i}.png")).collect();
        paths.push("clip.mp4".into());
        let mixed = mixed_targets(&paths);
        assert_eq!(mixed[0], ("gif", 11), "gif is the one output both photos and the clip can become");
        assert!(mixed.iter().any(|(t, n)| *t == "jpg" && *n == 10), "the photos still lead with their own formats");
        assert!(
            mixed.iter().any(|(t, n)| matches!(*t, "mov" | "mkv" | "webm" | "avi" | "wmv") && *n == 1),
            "the clip still gets a video target of its own (its own format is never offered)"
        );
        assert!(mixed.iter().all(|(t, _)| *t != "png"), "a format shared by no file is not offered");
        // A single file offers its own targets, capped for the wheel.
        let single = mixed_targets(&["a.png"]);
        assert_eq!(single.len(), 8.min(targets(Path::new("a.png")).len()));
        assert!(single.iter().any(|(t, n)| *t == "jpg" && *n == 1));
        assert!(mixed_targets::<&str>(&[]).is_empty());
    }

    #[test]
    fn expands_dropped_directories_and_multi_file_selections() {
        let dir = std::env::temp_dir().join(format!("zeidex-expand-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("nested")).unwrap();
        std::fs::write(dir.join("a.mkv"), b"1").unwrap();
        std::fs::write(dir.join("nested/b.mkv"), b"2").unwrap();
        std::fs::write(dir.join(".hidden.mkv"), b"3").unwrap();
        let extra = dir.join("c.mkv");
        std::fs::write(&extra, b"4").unwrap();

        let expanded = expand_dropped_paths(&[dir.join("nested"), dir.join("a.mkv"), extra.clone()]);
        assert_eq!(expanded.len(), 3);
        assert!(expanded.iter().any(|p| p.ends_with("b.mkv")));
        assert!(expanded.iter().any(|p| p.ends_with("a.mkv")));
        assert!(expanded.iter().any(|p| p.ends_with("c.mkv")));
        assert_eq!(common_category(&expanded), Some(Category::Video));
        let targets = common_targets(&expanded);
        assert!(targets.contains(&"mp4") && targets.contains(&"mp3") && !targets.contains(&"mkv"));

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn extension_to_wedge_mapping_for_video_and_pdf() {
        let mkv = targets(Path::new("/media/movie.mkv"));
        assert_eq!(category_of(Path::new("/media/movie.mkv")), Some(Category::Video));
        for expected in ["mp4", "mp3", "gif", "webm"] {
            assert!(mkv.contains(&expected), "expected {expected} wedge for .mkv, got {mkv:?}");
        }

        let pdf = targets(Path::new("/docs/report.pdf"));
        assert_eq!(category_of(Path::new("/docs/report.pdf")), Some(Category::Pdf));
        for expected in ["docx", "png", "txt", "md"] {
            assert!(pdf.contains(&expected), "expected {expected} wedge for .pdf, got {pdf:?}");
        }
    }
}
