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

/// Writes `data` as `name` beside `src`, never overwriting (`name 2.png`...).
pub fn save_beside(src: &Path, name: &str, data: &[u8]) -> Result<PathBuf> {
    let dir = src.parent().unwrap_or(Path::new("."));
    crate::runner::write_new(&dir.join(name), data)
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
                "--session",
                "--dest=org.freedesktop.FileManager1",
                "--type=method_call",
                "/org/freedesktop/FileManager1",
                "org.freedesktop.FileManager1.ShowItems",
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
            let target = dir.join(name);
            if p.is_dir() {
                let dst = crate::runner::unique_dir(&target);
                return fs::rename(p, &dst)
                    .map(|_| dst)
                    .map_err(|_| Error::Unsupported("moving folders across drives".into()));
            }
            if let Ok(dst) = crate::runner::commit_new(p, &target) {
                return Ok(dst);
            }
            // Another drive: copy to a staging name there, commit it, then drop the original.
            let staged = crate::runner::staging_path(dir, "part");
            let copied =
                fs::copy(p, &staged).map_err(Error::from).and_then(|_| crate::runner::commit_new(&staged, &target));
            match copied {
                Ok(dst) => {
                    fs::remove_file(p)?;
                    Ok(dst)
                }
                Err(e) => {
                    let _ = fs::remove_file(&staged);
                    Err(e)
                }
            }
        })
        .collect()
}

/// Overwrite patterns, DoD 5220.22-M style: zeros, ones, then pseudo-random bytes.
const SHRED_PASSES: [Option<u8>; 3] = [Some(0x00), Some(0xFF), None];
/// Write size per call while overwriting.
const SHRED_CHUNK: usize = 1 << 20;

/// Destroys files: overwrites each one in place three times ([`SHRED_PASSES`]), flushing every
/// pass to the device, then gives it a meaningless name, truncates and deletes it. Every path
/// is checked before anything is touched, so one bad entry cannot leave a half-shredded batch.
///
/// Honest limit, shown in the UI as well: SSDs, phones (flash wear levelling) and journaling or
/// copy-on-write filesystems (APFS, Btrfs, ZFS) can keep older copies of the blocks elsewhere.
/// There, only full-disk encryption protects deleted data.
pub fn shred(paths: &[PathBuf]) -> Result<usize> {
    for p in paths {
        let meta = fs::symlink_metadata(p)?;
        if !meta.file_type().is_file() {
            return Err(Error::BadOption(format!("{} is not a regular file; nothing was shredded", p.display())));
        }
    }
    for p in paths {
        shred_one(p)?;
    }
    Ok(paths.len())
}

fn shred_one(path: &Path) -> Result<()> {
    use std::io::{Seek, SeekFrom, Write};
    let len = fs::metadata(path)?.len();
    let mut f = fs::OpenOptions::new().write(true).open(path)?;
    let mut rng = seed(path);
    let mut buf = vec![0u8; SHRED_CHUNK];
    for pass in SHRED_PASSES {
        f.seek(SeekFrom::Start(0))?;
        let mut left = len;
        while left > 0 {
            let n = left.min(SHRED_CHUNK as u64) as usize;
            match pass {
                Some(byte) => buf[..n].fill(byte),
                None => buf[..n].iter_mut().for_each(|b| *b = next_byte(&mut rng)),
            }
            f.write_all(&buf[..n])?;
            left -= n as u64;
        }
        f.sync_all()?;
    }
    drop(f);
    // Hide the original name in the directory entry too before the inode goes.
    let dir = path.parent().unwrap_or(Path::new("."));
    let hidden = crate::runner::staging_path(dir, "shred");
    fs::rename(path, &hidden)?;
    let f = fs::OpenOptions::new().write(true).open(&hidden)?;
    f.set_len(0)?;
    f.sync_all()?;
    drop(f);
    fs::remove_file(&hidden)?;
    Ok(())
}

/// xorshift64* state seeded from the std hasher's per-process random keys. The overwrite only
/// needs bytes that are not the old data, not cryptographic randomness.
fn seed(path: &Path) -> u64 {
    use std::hash::{BuildHasher, Hash, Hasher};
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    path.hash(&mut h);
    std::time::SystemTime::now().hash(&mut h);
    h.finish() | 1
}

fn next_byte(state: &mut u64) -> u8 {
    *state ^= *state >> 12;
    *state ^= *state << 25;
    *state ^= *state >> 27;
    (state.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 56) as u8
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

/// Runs a shell command in a visible terminal window, so package-manager prompts (a sudo
/// password, a winget agreement, brew's progress) stay reachable instead of running hidden.
///
/// The command comes from `Tool::install_hint()`, which is a static string per platform; nothing
/// here is user input, and the terminal is what the user would have opened themselves.
pub fn run_in_terminal(command: &str) -> Result<()> {
    #[cfg(windows)]
    {
        // `start` needs the empty title argument before the program name, or a quoted command
        // becomes the window title. `cmd /k` keeps the window open after it finishes.
        Command::new("cmd")
            .args(["/c", "start", "", "cmd", "/k", command])
            .spawn()
            .map_err(|e| Error::Failed { tool: "cmd".into(), detail: e.to_string() })?;
        Ok(())
    }
    #[cfg(target_os = "macos")]
    {
        let escaped = command.replace('\\', "\\\\").replace('"', "\\\"");
        let script = format!("tell application \"Terminal\" to do script \"{escaped}\"");
        Command::new("osascript")
            .args(["-e", &script, "-e", "tell application \"Terminal\" to activate"])
            .spawn()
            .map_err(|e| Error::Failed { tool: "osascript".into(), detail: e.to_string() })?;
        Ok(())
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        // Keep the window up after the command ends: the last lines of a failure are the part
        // worth reading, and a console that vanishes takes them with it.
        let script = format!("{command}; echo; printf 'Press Enter to close…'; read -r _");
        let candidates: [(&str, &[&str]); 7] = [
            ("x-terminal-emulator", &["-e", "sh", "-c"]),
            ("gnome-terminal", &["--", "sh", "-c"]),
            ("konsole", &["-e", "sh", "-c"]),
            ("xfce4-terminal", &["-x", "sh", "-c"]),
            ("kitty", &["-e", "sh", "-c"]),
            ("alacritty", &["-e", "sh", "-c"]),
            ("xterm", &["-e", "sh", "-c"]),
        ];
        for (bin, args) in candidates {
            if which(bin).is_some() {
                Command::new(bin)
                    .args(args)
                    .arg(&script)
                    .spawn()
                    .map_err(|e| Error::Failed { tool: bin.into(), detail: e.to_string() })?;
                return Ok(());
            }
        }
        Err(Error::Unsupported("no terminal emulator found to run the installer".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shred_overwrites_then_deletes() {
        let d = std::env::temp_dir().join(format!("zeidex-shred-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        let secret = d.join("secret.txt");
        fs::write(&secret, "x".repeat(SHRED_CHUNK + 17)).unwrap();
        let sub = d.join("folder");
        fs::create_dir(&sub).unwrap();

        // A folder in the batch refuses the whole batch before anything is touched.
        assert!(shred(&[secret.clone(), sub.clone()]).is_err());
        assert_eq!(fs::read(&secret).unwrap().len(), SHRED_CHUNK + 17);

        assert_eq!(shred(std::slice::from_ref(&secret)).unwrap(), 1);
        assert!(!secret.exists());
        let left: Vec<_> = fs::read_dir(&d).unwrap().flatten().map(|e| e.file_name()).collect();
        assert_eq!(left, vec![std::ffi::OsString::from("folder")], "no renamed leftover");

        let mut state = 7;
        let bytes: Vec<u8> = (0..256).map(|_| next_byte(&mut state)).collect();
        assert!(bytes.iter().any(|&b| b != bytes[0]), "the random pass is not a constant");
        fs::remove_dir_all(&d).unwrap();
    }

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
        assert_eq!(moved, vec![b.join("x 2.txt")]);
        assert!(!f.exists());
        assert_eq!(paths_text(&[PathBuf::from("/a"), PathBuf::from("/b c")]), "/a\n/b c");
        let saved = save_beside(&b.join("x.txt"), "x.txt", b"new").unwrap();
        assert_eq!(saved, b.join("x 3.txt"));
        fs::remove_dir_all(&base).unwrap();
    }
}
