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
            Category::Image => &["jpg", "png", "webp", "heic", "avif", "tiff", "bmp", "gif", "svg"],
            Category::Video => &["mp4", "mov", "m4v", "mkv", "webm", "avi", "wmv", "flv", "mpg", "3gp", "ts"],
            Category::Audio => &["mp3", "m4a", "aac", "wav", "flac", "ogg", "opus", "aiff", "wma"],
            Category::Pdf => &["pdf"],
            Category::Document => &["docx", "doc", "odt", "rtf", "pages", "md", "txt", "html"],
            Category::Slides => &["pptx", "ppt", "odp", "key"],
            Category::Spreadsheet => &["xlsx", "xls", "ods", "csv", "tsv", "numbers"],
            Category::Ebook => &["epub", "mobi", "azw3", "fb2"],
            Category::Subtitle => &["srt", "vtt"],
            Category::Archive => &["zip", "tar", "tgz", "gz", "7z", "rar", "bz2", "xz"],
        }
    }

    fn outputs(self) -> &'static [&'static str] {
        match self {
            Category::Image => &["jpg", "png", "webp", "heic", "avif", "tiff", "bmp", "gif", "pdf"],
            Category::Video => &["mp4", "mov", "mkv", "webm", "avi", "wmv", "gif", "mp3", "m4a", "wav"],
            Category::Audio => &["mp3", "m4a", "wav", "flac", "ogg", "opus", "aiff", "wma"],
            Category::Pdf => &["docx", "pptx", "epub", "txt", "jpg", "png"],
            Category::Document => &["pdf", "docx", "odt", "rtf", "md", "txt", "html", "epub"],
            Category::Slides => &["pdf", "pptx", "odp", "png", "jpg"],
            Category::Spreadsheet => &["xlsx", "ods", "csv", "tsv", "pdf", "html"],
            Category::Ebook => &["epub", "pdf", "mobi", "azw3", "docx", "txt", "md", "html"],
            Category::Subtitle => &["srt", "vtt", "txt"],
            Category::Archive => &["zip", "7z", "tar", "tgz"],
        }
    }
}

/// Lower-cased extension with aliases folded (`jpeg`→`jpg`, `x.tar.gz`→`tgz`).
pub fn ext_of(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_str()?.to_ascii_lowercase();
    if name.ends_with(".tar.gz") {
        return Some("tgz".into());
    }
    let ext = name.rsplit_once('.')?.1;
    Some(normalize(ext).to_string())
}

pub fn normalize(ext: &str) -> &str {
    match ext {
        "jpeg" | "jpe" => "jpg",
        "tif" => "tiff",
        "heif" => "heic",
        "htm" => "html",
        "markdown" => "md",
        "aif" => "aiff",
        "oga" => "ogg",
        "mpeg" => "mpg",
        other => other,
    }
}

/// File name without its extension; `a.tar.gz` gives `a`.
pub fn stem_of(path: &Path) -> String {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".tar.gz") {
        return name[..name.len() - 7].to_string();
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
        assert_eq!(ext_of(Path::new("x.tar.gz")).as_deref(), Some("tgz"));
        assert_eq!(stem_of(Path::new("dir/My.Backup.tar.gz")), "My.Backup");
        assert_eq!(stem_of(Path::new(".bashrc")), ".bashrc");
        assert_eq!(ext_of(Path::new("noext")), None);
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
