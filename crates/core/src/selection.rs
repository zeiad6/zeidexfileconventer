//! Files the user has selected outside the app, for the keyboard shortcut that opens the
//! wheel without dragging (Tangerine's "Enter on a file"). Windows asks Explorer for the
//! selection of the foreground window; elsewhere the user copies files (Ctrl+C) first and
//! we read the clipboard, which file managers fill with `file://` URIs.

use std::path::PathBuf;

/// Paths from clipboard text: `text/uri-list`, GNOME's `copy\nfile://...`, or plain paths.
pub fn parse_uri_list(text: &str) -> Vec<PathBuf> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#') && *l != "copy" && *l != "cut")
        .filter_map(|l| match l.strip_prefix("file://") {
            // file:///home/u/a%20b.jpg (local host only)
            Some(rest) => {
                rest.strip_prefix("localhost").or(Some(rest)).filter(|r| r.starts_with('/')).map(percent_decode)
            }
            None if l.starts_with('/') || is_windows_abs(l) => Some(l.to_string()),
            None => None,
        })
        .map(|p| PathBuf::from(windows_from_uri(&p)))
        .filter(|p| p.exists())
        .collect()
}

fn is_windows_abs(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() > 2 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/')
}

/// `/C:/Users/x` (from a file URI) -> `C:/Users/x`.
fn windows_from_uri(p: &str) -> String {
    let b = p.as_bytes();
    if b.len() > 3 && b[0] == b'/' && b[1].is_ascii_alphabetic() && b[2] == b':' {
        p[1..].to_string()
    } else {
        p.to_string()
    }
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            let hex = std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok());
            if let Some(v) = hex {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// PowerShell that prints the paths selected in the foreground Explorer window.
pub const EXPLORER_SELECTION_PS: &str = r#"
Add-Type -Namespace Zeidex -Name Win -MemberDefinition '[DllImport("user32.dll")] public static extern System.IntPtr GetForegroundWindow();'
$h = [int64][Zeidex.Win]::GetForegroundWindow()
(New-Object -ComObject Shell.Application).Windows() |
  Where-Object { $_.HWND -eq $h } |
  ForEach-Object { $_.Document.SelectedItems() | ForEach-Object { $_.Path } }
"#;

/// Selection of the foreground Explorer window (Windows only; empty elsewhere or on failure).
pub fn explorer_selection() -> Vec<PathBuf> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let out = std::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", EXPLORER_SELECTION_PS])
            .creation_flags(CREATE_NO_WINDOW)
            .output();
        if let Ok(out) = out {
            return String::from_utf8_lossy(&out.stdout)
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .map(PathBuf::from)
                .filter(|p| p.exists())
                .collect();
        }
    }
    Vec::new()
}

/// What may follow a Shapefile's name or companion files for primary formats.
const SHAPEFILE_PARTS: &[&str] = &[
    "shx", "dbf", "prj", "cpg", "sbn", "sbx", "fbn", "fbx", "ain", "aih", "atx", "ixs", "mxs", "qix", "qmd", "qpj", "shp.xml",
];

/// Filters out companion files (e.g. .dbf, .shx dropped alongside .shp) so they are not processed as separate items.
pub fn filter_companions(paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut shapefile_stems = std::collections::HashSet::new();
    for p in paths {
        if let Some(ext) = p.extension().and_then(|e| e.to_str()) {
            if ext.eq_ignore_ascii_case("shp") {
                if let Some(stem) = p.file_stem().and_then(|s| s.to_str()) {
                    let dir = p.parent().unwrap_or_else(|| std::path::Path::new(""));
                    shapefile_stems.insert(dir.join(stem));
                }
            }
        }
    }
    if shapefile_stems.is_empty() {
        return paths.to_vec();
    }
    paths
        .iter()
        .filter(|p| {
            let dir = p.parent().unwrap_or_else(|| std::path::Path::new(""));
            let Some(file_name) = p.file_name().and_then(|n| n.to_str()) else { return true };
            for stem_path in &shapefile_stems {
                if let Some(stem) = stem_path.file_name().and_then(|s| s.to_str()) {
                    if dir == stem_path.parent().unwrap_or_else(|| std::path::Path::new("")) && file_name.starts_with(stem) {
                        let remainder = &file_name[stem.len()..];
                        if remainder.starts_with('.') {
                            let sub_ext = remainder[1..].to_ascii_lowercase();
                            if SHAPEFILE_PARTS.contains(&sub_ext.as_str()) {
                                return false; // companion file, skip!
                            }
                        }
                    }
                }
            }
            true
        })
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_shapefile_companions() {
        let shp = PathBuf::from("/tmp/roads.shp");
        let dbf = PathBuf::from("/tmp/roads.dbf");
        let shx = PathBuf::from("/tmp/roads.shx");
        let other = PathBuf::from("/tmp/other.dbf");
        let filtered = filter_companions(&[shp.clone(), dbf, shx, other.clone()]);
        assert_eq!(filtered, vec![shp, other]);
    }

    #[test]
    fn uri_drive_letters() {
        assert_eq!(windows_from_uri("/C:/Users/x"), "C:/Users/x");
        assert_eq!(windows_from_uri("/home/x"), "/home/x");
        assert_eq!(percent_decode("a%20b%25c%e2%9c%93"), "a b%c✓");
        assert_eq!(percent_decode("bad%zz%"), "bad%zz%");
    }

    /// Unix paths: on Windows the same parsing is covered by `windows_from_uri`.
    #[cfg(unix)]
    #[test]
    fn clipboard_formats() {
        use std::fs;
        let d = std::env::temp_dir().join(format!("zeidex sel {}", std::process::id()));
        fs::create_dir_all(&d).unwrap();
        let f = d.join("a b%.txt");
        fs::write(&f, "").unwrap();
        let uri = format!("file://{}", f.to_string_lossy().replace(' ', "%20").replace("b%.", "b%25."));
        assert_eq!(parse_uri_list(&format!("copy\n{uri}\n")), vec![f.clone()], "GNOME clipboard");
        assert_eq!(parse_uri_list(&format!("# comment\r\n{uri}\r\n")), vec![f.clone()], "text/uri-list");
        assert_eq!(parse_uri_list(&f.to_string_lossy()), vec![f.clone()], "plain path");
        assert!(parse_uri_list("hello world\nhttps://x.y/z").is_empty());
        assert!(parse_uri_list("file:///no/such/file").is_empty());
        assert_eq!(windows_from_uri("/C:/Users/x"), "C:/Users/x");
        assert!(EXPLORER_SELECTION_PS.contains("SelectedItems"));
        fs::remove_dir_all(&d).unwrap();
    }
}
