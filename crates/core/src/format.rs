//! Format registry: what a file is, and what it can be converted to.

use std::path::Path;

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
                "jpg", "png", "webp", "heic", "avif", "tiff", "bmp", "gif", "svg",
                "jxl", "jp2", "ico", "apng", "ppm", "tga", "dds", "pcx", "psd",
                "dng", "cr2", "cr3", "nef", "arw", "rw2", "raf", "raw", "livp",
            ],
            Category::Video => &[
                "mp4", "mov", "m4v", "mkv", "webm", "avi", "wmv", "flv", "mpg", "3gp", "ts",
                "m2ts", "3g2", "ogv", "vob", "mxf",
            ],
            Category::Audio => &[
                "mp3", "m4a", "aac", "wav", "flac", "ogg", "opus", "aiff", "wma",
                "ac3", "amr", "caf", "weba", "mka", "ape", "wv", "w64",
            ],
            Category::Pdf => &["pdf"],
            Category::Document => &[
                "docx", "doc", "odt", "rtf", "pages", "md", "txt", "html",
                "rst", "tex", "org", "adoc", "typ",
            ],
            Category::Slides => &["pptx", "ppt", "odp", "key"],
            Category::Spreadsheet => &["xlsx", "xls", "ods", "csv", "tsv", "numbers"],
            Category::Ebook => &["epub", "mobi", "azw3", "fb2"],
            Category::Subtitle => &["srt", "vtt", "ass", "ssa", "smi", "lrc"],
            Category::Archive => &[
                "zip", "tar", "tgz", "gz", "7z", "rar", "bz2", "xz",
                "tbz2", "txz", "cab", "iso", "deb", "rpm",
            ],
        }
    }

    fn outputs(self) -> &'static [&'static str] {
        match self {
            Category::Image => &["jpg", "png", "webp", "heic", "avif", "tiff", "bmp", "gif", "pdf", "ico", "jxl", "jp2"],
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
}
