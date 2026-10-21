//! File actions (Tangerine's Control wheel): reveal, move, copy path, open a terminal.

use crate::tools::which;
use crate::{Error, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Paths as text, one per line.
pub fn paths_text(paths: &[PathBuf]) -> String {
    paths.iter().map(|p| p.to_string_lossy()).collect::<Vec<_>>().join("\n")
}

/// Writes `data` as `name` beside `src`, never overwriting (`name (2).png`...).
pub fn save_beside(src: &Path, name: &str, data: &[u8]) -> Result<PathBuf> {
    let dir = src.parent().unwrap_or(Path::new("."));
    let dst = crate::runner::unique_path(&dir.join(name));
    fs::write(&dst, data)?;
    Ok(dst)
}

/// Shows the file selected in the system file manager.
pub fn reveal(path: &Path) -> Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // explorer wants `/select,"C:\a b\c"` exactly; std's quoting of the whole arg breaks it.
        Command::new("explorer").raw_arg(format!("/select,\"{}\"", path.display())).spawn()?;
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let uri = crate::plan::file_url(path);
        let shown = Command::new("dbus-send")
            .args([
                "--session", "--dest=org.freedesktop.FileManager1", "--type=method_call",
                "/org/freedesktop/FileManager1", "org.freedesktop.FileManager1.ShowItems",
            ])
            .arg(format!("array:string:{uri}"))
            .arg("string:")
            .status()
            .is_ok_and(|s| s.success());
        if !shown {
            let dir = if path.is_dir() { path } else { path.parent().unwrap_or(path) };
            Command::new("xdg-open").arg(dir).spawn()?;
        }
        Ok(())
    }
}

/// Moves files into `dir`, falling back to copy + delete across filesystems.
pub fn move_to(paths: &[PathBuf], dir: &Path) -> Result<Vec<PathBuf>> {
    paths
        .iter()
        .map(|p| {
            let name = p.file_name().ok_or_else(|| Error::BadOption(format!("{} has no name", p.display())))?;
            let dst = crate::runner::unique_path(&dir.join(name));
            if fs::rename(p, &dst).is_err() {
                if p.is_dir() {
                    return Err(Error::Unsupported("moving folders across drives".into()));
                }
                fs::copy(p, &dst)?;
                fs::remove_file(p)?;
            }
            Ok(dst)
        })
        .collect()
}

/// Opens a terminal in `path` (or its folder).
pub fn open_terminal(path: &Path) -> Result<()> {
    let dir = if path.is_dir() { path } else { path.parent().unwrap_or(path) };
    #[cfg(windows)]
    {
        if which("wt.exe").is_some() {
            Command::new("wt.exe").arg("-d").arg(dir).spawn()?;
        } else {
            Command::new("cmd").args(["/c", "start", "", "cmd", "/k"]).current_dir(dir).spawn()?;
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let dir_s = dir.to_string_lossy();
        let candidates: [(&str, Vec<String>); 7] = [
            ("x-terminal-emulator", vec![]),
            ("gnome-terminal", vec![format!("--working-directory={dir_s}")]),
            ("konsole", vec!["--workdir".into(), dir_s.to_string()]),
            ("xfce4-terminal", vec![format!("--working-directory={dir_s}")]),
            ("kitty", vec!["--directory".into(), dir_s.to_string()]),
            ("alacritty", vec!["--working-directory".into(), dir_s.to_string()]),
            ("xterm", vec![]),
        ];
        for (bin, args) in candidates {
            if which(bin).is_some() {
                Command::new(bin).args(args).current_dir(dir).spawn()?;
                return Ok(());
            }
        }
        Err(Error::Unsupported("no terminal emulator found".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn move_and_paths() {
        let base = std::env::temp_dir().join(format!("zeidex-move-{}", std::process::id()));
        let (a, b) = (base.join("a"), base.join("b"));
        fs::create_dir_all(&a).unwrap();
        fs::create_dir_all(&b).unwrap();
        let f = a.join("x.txt");
        fs::write(&f, "1").unwrap();
        fs::write(b.join("x.txt"), "old").unwrap();
        let moved = move_to(std::slice::from_ref(&f), &b).unwrap();
        assert_eq!(moved, vec![b.join("x (2).txt")]);
        assert!(!f.exists());
        assert_eq!(paths_text(&[PathBuf::from("/a"), PathBuf::from("/b c")]), "/a\n/b c");
        let saved = save_beside(&b.join("x.txt"), "x.txt", b"new").unwrap();
        assert_eq!(saved, b.join("x (3).txt"));
        fs::remove_dir_all(&base).unwrap();
    }
}
