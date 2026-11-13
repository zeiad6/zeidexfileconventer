//! Batch rename with patterns: `{name}`, `{n}`, `{n:3}`, `{ext}`, `{date}`, `{parent}`.
//! Renames happen in place, so every target is checked before anything moves.

use crate::format::stem_of;
use crate::{Error, Result};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Civil date (UTC) for a Unix timestamp, without a date library (Howard Hinnant's algorithm).
pub fn ymd(secs: i64) -> (i64, u32, u32) {
    let z = secs.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

fn modified(p: &Path) -> String {
    let secs = fs::metadata(p)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs() as i64);
    let (y, m, d) = ymd(secs);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Expands `pattern` for the `index`-th file (counting from `start`).
pub fn expand(pattern: &str, path: &Path, index: usize, start: usize) -> Result<String> {
    let ext = path.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_default();
    let mut out = String::new();
    let mut rest = pattern;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let close = rest[open..].find('}').ok_or_else(|| Error::BadOption(format!("unclosed {{ in {pattern}")))? + open;
        let token = &rest[open + 1..close];
        let n = start + index;
        out += &match token.split_once(':') {
            Some(("n", w)) => {
                let w: usize = w.parse().map_err(|_| Error::BadOption(format!("{{{token}}}")))?;
                format!("{n:0w$}")
            }
            None if token == "n" => n.to_string(),
            None if token == "name" => stem_of(path),
            None if token == "ext" => ext.clone(),
            None if token == "date" => modified(path),
            None if token == "parent" => {
                path.parent().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
            }
            _ => return Err(Error::BadOption(format!("unknown token {{{token}}}"))),
        };
        rest = &rest[close + 1..];
    }
    out.push_str(rest);
    if out.is_empty() || out.contains(['/', '\\']) || out == "." || out == ".." {
        return Err(Error::BadOption(format!("pattern gives an invalid name: {out:?}")));
    }
    // Keep the extension unless the pattern placed {ext} itself.
    if !pattern.contains("{ext}") && !ext.is_empty() {
        out = format!("{out}.{ext}");
    }
    Ok(out)
}

/// New paths for `paths`, validated: no duplicates, no overwriting files outside the set.
pub fn plan(paths: &[PathBuf], pattern: &str, start: usize) -> Result<Vec<(PathBuf, PathBuf)>> {
    let mut seen = HashSet::new();
    let sources: HashSet<&PathBuf> = paths.iter().collect();
    let mut out = Vec::new();
    for (i, p) in paths.iter().enumerate() {
        let new = p.with_file_name(expand(pattern, p, i, start)?);
        if !seen.insert(new.clone()) {
            return Err(Error::BadOption(format!("two files would be named {}", new.display())));
        }
        if new.exists() && !sources.contains(&new) {
            return Err(Error::BadOption(format!("{} already exists", new.display())));
        }
        out.push((p.clone(), new));
    }
    Ok(out)
}

/// Applies `plan` safely even when names swap (a→b, b→a): move everything to temporary
/// names first, then to the final names. A file that appeared under a new name after the
/// preview is never replaced: the whole batch is rolled back to the original names instead.
pub fn apply(plan: &[(PathBuf, PathBuf)]) -> Result<()> {
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    let temps: Vec<PathBuf> = plan
        .iter()
        .enumerate()
        .map(|(i, (old, _))| old.with_file_name(format!(".zeidex-rename-{stamp}-{i}")))
        .collect();
    for (i, ((old, _), tmp)) in plan.iter().zip(&temps).enumerate() {
        if let Err(e) = fs::rename(old, tmp) {
            for ((old, _), tmp) in plan.iter().zip(&temps).take(i) {
                let _ = fs::rename(tmp, old);
            }
            return Err(e.into());
        }
    }
    for (i, ((_, new), tmp)) in plan.iter().zip(&temps).enumerate() {
        if let Err(e) = crate::runner::move_no_replace(tmp, new) {
            // Undo: renamed files go back from their new name, the rest from their temp name.
            for (j, ((old, new), tmp)) in plan.iter().zip(&temps).enumerate() {
                let _ = fs::rename(if j < i { new } else { tmp }, old);
            }
            return Err(if e.kind() == std::io::ErrorKind::AlreadyExists {
                Error::BadOption(format!("{} already exists; nothing was renamed", new.display()))
            } else {
                e.into()
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates() {
        assert_eq!(ymd(0), (1970, 1, 1));
        assert_eq!(ymd(951_782_400), (2000, 2, 29));
        assert_eq!(ymd(1_792_540_800), (2026, 10, 21));
    }

    #[test]
    fn patterns_and_swaps() {
        let d = std::env::temp_dir().join(format!("zeidex-rename-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        let (a, b) = (d.join("a.jpg"), d.join("b.jpg"));
        fs::write(&a, "A").unwrap();
        fs::write(&b, "B").unwrap();
        assert_eq!(expand("trip-{n:3}", &a, 0, 1).unwrap(), "trip-001.jpg");
        assert_eq!(expand("{name}_{n}.{ext}", &a, 4, 1).unwrap(), "a_5.jpg");
        assert!(expand("{nope}", &a, 0, 1).is_err());
        assert!(expand("../x", &a, 0, 1).is_err());
        assert!(plan(&[a.clone(), b.clone()], "same", 1).is_err(), "duplicate names refused");
        // swap names through temporaries
        let p = vec![(a.clone(), b.clone()), (b.clone(), a.clone())];
        apply(&p).unwrap();
        assert_eq!(fs::read_to_string(&a).unwrap(), "B");
        fs::write(d.join("taken.jpg"), "").unwrap();
        assert!(plan(std::slice::from_ref(&a), "taken", 1).is_err(), "never overwrites other files");
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn a_file_created_after_the_preview_is_not_replaced() {
        let d = std::env::temp_dir().join(format!("zeidex-rename-late-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        let (a, b) = (d.join("a.txt"), d.join("b.txt"));
        fs::write(&a, "A").unwrap();
        fs::write(&b, "B").unwrap();
        let p = plan(&[a.clone(), b.clone()], "x-{n}", 1).unwrap();
        // Something else claims the second new name between preview and apply.
        fs::write(d.join("x-2.txt"), "intruder").unwrap();
        assert!(apply(&p).is_err());
        assert_eq!(fs::read_to_string(&a).unwrap(), "A", "rolled back");
        assert_eq!(fs::read_to_string(&b).unwrap(), "B", "rolled back");
        assert_eq!(fs::read_to_string(d.join("x-2.txt")).unwrap(), "intruder");
        assert!(!d.join("x-1.txt").exists());
        let names = fs::read_dir(&d).unwrap().count();
        assert_eq!(names, 3, "no temporary name is left behind");
        fs::remove_dir_all(&d).unwrap();
    }
}
