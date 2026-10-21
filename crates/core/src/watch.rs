//! Watch folders: notice new files in a directory and hand them over once they stop growing.
//! Polling (std only) instead of OS notifications: it behaves the same on Windows, Linux,
//! network shares and removable drives.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

pub struct Watcher {
    dir: PathBuf,
    exts: Vec<String>,
    /// Size seen at the previous poll for files not handed out yet.
    pending: HashMap<PathBuf, u64>,
    /// Files already handed out, present at start, or written by us.
    done: HashSet<PathBuf>,
}

impl Watcher {
    /// `exts` are lower-case extensions to react to; empty means every file. Files already in
    /// the folder are ignored: only what arrives afterwards is processed.
    pub fn new(dir: &Path, exts: &[String]) -> Watcher {
        let mut w =
            Watcher { dir: dir.to_path_buf(), exts: exts.to_vec(), pending: HashMap::new(), done: HashSet::new() };
        w.done = w.candidates().into_iter().map(|(p, _)| p).collect();
        w
    }

    fn candidates(&self) -> Vec<(PathBuf, u64)> {
        let Ok(rd) = fs::read_dir(&self.dir) else { return vec![] };
        rd.flatten()
            .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
            .filter_map(|e| {
                let meta = e.metadata().ok()?;
                meta.is_file().then(|| (e.path(), meta.len()))
            })
            .filter(|(p, _)| self.exts.is_empty() || crate::format::ext_of(p).is_some_and(|x| self.exts.contains(&x)))
            .collect()
    }

    /// New files whose size did not change since the previous poll (copy finished).
    pub fn poll(&mut self) -> Vec<PathBuf> {
        let mut ready = Vec::new();
        let mut next = HashMap::new();
        for (path, size) in self.candidates() {
            if self.done.contains(&path) {
                continue;
            }
            if self.pending.get(&path) == Some(&size) && size > 0 {
                self.done.insert(path.clone());
                ready.push(path);
            } else {
                next.insert(path, size);
            }
        }
        self.pending = next;
        ready.sort();
        ready
    }

    /// Our own outputs must not trigger the rule again (e.g. compress *.png beside *.png).
    pub fn mark_done(&mut self, path: &Path) {
        self.done.insert(path.to_path_buf());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waits_for_stable_size_and_skips_old_and_own_files() {
        let d = std::env::temp_dir().join(format!("zeidex-watch-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join("old.heic"), "x").unwrap();
        let mut w = Watcher::new(&d, &["heic".to_string()]);
        assert!(w.poll().is_empty(), "existing files are ignored");
        let new = d.join("new.heic");
        fs::write(&new, "12").unwrap();
        fs::write(d.join("note.txt"), "ignored ext").unwrap();
        fs::write(d.join(".partial.heic"), "hidden").unwrap();
        assert!(w.poll().is_empty(), "first sighting: maybe still copying");
        fs::write(&new, "1234").unwrap();
        assert!(w.poll().is_empty(), "still growing");
        assert_eq!(w.poll(), vec![new.clone()]);
        assert!(w.poll().is_empty(), "handed out once");
        let out = d.join("new-compressed.heic");
        w.mark_done(&out);
        fs::write(&out, "out").unwrap();
        w.poll();
        assert!(w.poll().is_empty(), "own outputs never trigger");
        fs::remove_dir_all(&d).unwrap();
    }
}
