//! On-demand engine downloader for Lite installs (strictly user-initiated, SHA-256 verified).
//!
//! Uses the pinned URLs and digests in `packaging/engines.json` and writes only into the user's
//! `app_local_data_dir/engines/` folder. No networking crate is linked: downloads invoke the OS
//! `curl` binary only when the user clicks Download in the Engines panel.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};
use zeidex_core::sha256;
use zeidex_core::tools::{self, Tool};

const ENGINES_JSON: &str = include_str!("../../../packaging/engines.json");

#[derive(Deserialize)]
struct Manifest {
    platforms: HashMap<String, PlatformEntry>,
    models: HashMap<String, ModelEntry>,
}

#[derive(Deserialize)]
struct PlatformEntry {
    components: HashMap<String, ComponentEntry>,
}

#[derive(Clone, Deserialize)]
pub struct ComponentEntry {
    pub url: String,
    pub sha256: String,
    pub size: u64,
    pub kind: String,
}

#[derive(Clone, Deserialize)]
struct ModelEntry {
    name: String,
    url: String,
    sha256: String,
}

#[derive(Clone, Serialize)]
pub struct DownloadProgress {
    pub id: String,
    pub stage: &'static str,
    pub percent: Option<u8>,
    pub downloaded_bytes: Option<u64>,
    pub total_bytes: Option<u64>,
    pub speed_bps: Option<u64>,
    pub detail: Option<String>,
}

fn platform_key() -> Option<&'static str> {
    if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        Some("windows-x64")
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        Some("linux-x64")
    } else if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
        Some("linux-arm64")
    } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        Some("osx-x64")
    } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        Some("osx-arm64")
    } else {
        None
    }
}

fn manifest() -> Option<Manifest> {
    serde_json::from_str(ENGINES_JSON).ok()
}

fn component_key(slug: &str) -> &str {
    match slug {
        "vips" | "vipsheader" => "libvips",
        "ffprobe" if cfg!(not(target_os = "macos")) => "ffmpeg",
        other => other,
    }
}

/// Returns the pinned download spec for `slug` on the current platform, if a portable archive
/// exists in `packaging/engines.json`.
pub fn download_spec(slug: &str) -> Option<ComponentEntry> {
    let key = platform_key()?;
    let doc = manifest()?;
    let plat = doc.platforms.get(key)?;
    let comp_name = component_key(slug);
    let entry = plat.components.get(comp_name)?.clone();
    // Only extractable archives / single binaries are supported on-demand; DMG/MSI require full
    // system installers and stay on the `install_engine` terminal path.
    if matches!(entry.kind.as_str(), "zip" | "tar.gz" | "tar.xz" | "raw" | "raw.gz") {
        Some(entry)
    } else {
        None
    }
}

pub fn local_engines_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let base = app.path().app_local_data_dir().map_err(|e| e.to_string())?;
    Ok(tools::normalize_engine_path(&base.join("engines")))
}

fn emit_stage(app: &AppHandle, id: &str, stage: &'static str, detail: Option<String>) {
    let _ = app.emit(
        "engine-download",
        DownloadProgress {
            id: id.to_string(),
            stage,
            percent: None,
            downloaded_bytes: None,
            total_bytes: None,
            speed_bps: None,
            detail,
        },
    );
}

fn fetch_and_verify(
    app: &AppHandle,
    id: &str,
    url: &str,
    expected_sha256: &str,
    expected_size: u64,
    dest: &Path,
) -> Result<(), String> {
    let curl = if cfg!(windows) { "curl.exe" } else { "curl" };
    let mut cmd = Command::new(curl);
    cmd.args(["-fsSL", "--retry", "2", "--connect-timeout", "20", url, "-o"])
        .arg(dest)
        .stdin(Stdio::null())
        .stdout(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let stop = Arc::new(AtomicBool::new(false));
    let monitor_stop = Arc::clone(&stop);
    let monitor_app = app.clone();
    let monitor_id = id.to_string();
    let monitor_dest = dest.to_path_buf();
    let handle = std::thread::spawn(move || {
        let started = Instant::now();
        while !monitor_stop.load(Ordering::Relaxed) {
            if let Ok(meta) = fs::metadata(&monitor_dest) {
                let bytes = meta.len();
                let secs = started.elapsed().as_secs_f64().max(0.1);
                let speed = (bytes as f64 / secs) as u64;
                let pct = bytes.saturating_mul(100).checked_div(expected_size).unwrap_or(0).min(99) as u8;
                let _ = monitor_app.emit(
                    "engine-download",
                    DownloadProgress {
                        id: monitor_id.clone(),
                        stage: "downloading",
                        percent: Some(pct),
                        downloaded_bytes: Some(bytes),
                        total_bytes: Some(expected_size),
                        speed_bps: Some(speed),
                        detail: None,
                    },
                );
            }
            std::thread::sleep(Duration::from_millis(160));
        }
    });

    let out = cmd.output().map_err(|e| format!("failed to run {curl}: {e}"));
    stop.store(true, Ordering::Relaxed);
    let _ = handle.join();

    let out = out?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(if stderr.is_empty() { format!("download failed ({})", out.status) } else { stderr });
    }
    let file = fs::File::open(dest).map_err(|e| e.to_string())?;
    let actual = sha256::of_reader(file).map_err(|e| e.to_string())?;
    if !actual.eq_ignore_ascii_case(expected_sha256) {
        let _ = fs::remove_file(dest);
        return Err(format!("SHA-256 mismatch: expected {expected_sha256}, got {actual}"));
    }
    Ok(())
}

fn extract_archive(archive: &Path, kind: &str, target: &Path, comp_name: &str) -> Result<(), String> {
    fs::create_dir_all(target).map_err(|e| e.to_string())?;
    match kind {
        "raw" => {
            let dst = target.join(comp_name);
            fs::copy(archive, &dst).map_err(|e| e.to_string())?;
            make_executable(&dst);
            Ok(())
        }
        "raw.gz" => {
            let dst = target.join(comp_name);
            let out = Command::new("gzip")
                .args(["-dc"])
                .arg(archive)
                .stdin(Stdio::null())
                .output()
                .map_err(|e| format!("gzip failed: {e}"))?;
            if !out.status.success() {
                return Err(format!("gzip extraction failed: {}", String::from_utf8_lossy(&out.stderr)));
            }
            fs::write(&dst, out.stdout).map_err(|e| e.to_string())?;
            make_executable(&dst);
            Ok(())
        }
        "tar.gz" | "tar.xz" => {
            let mut cmd = Command::new("tar");
            cmd.arg("-xf").arg(archive).arg("-C").arg(target).stdin(Stdio::null());
            let out = cmd.output().map_err(|e| format!("tar failed: {e}"))?;
            if !out.status.success() {
                return Err(format!("tar extraction failed: {}", String::from_utf8_lossy(&out.stderr)));
            }
            flatten_single_subdir(target)?;
            mark_tree_executables(target);
            Ok(())
        }
        "zip" => {
            if let Some(seven) = tools::locate(Tool::SevenZip) {
                let mut cmd = Command::new(seven);
                cmd.args(["x", "-y"]).arg(format!("-o{}", target.display())).arg(archive).stdin(Stdio::null());
                #[cfg(windows)]
                {
                    use std::os::windows::process::CommandExt;
                    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
                    cmd.creation_flags(CREATE_NO_WINDOW);
                }
                let out = cmd.output().map_err(|e| format!("7z failed: {e}"))?;
                if !out.status.success() {
                    return Err(format!("7z extraction failed: {}", String::from_utf8_lossy(&out.stderr)));
                }
            } else if cfg!(windows) {
                let out = Command::new("tar")
                    .args(["-xf"])
                    .arg(archive)
                    .arg("-C")
                    .arg(target)
                    .stdin(Stdio::null())
                    .output()
                    .map_err(|e| format!("tar failed: {e}"))?;
                if !out.status.success() {
                    return Err(format!("zip extraction failed: {}", String::from_utf8_lossy(&out.stderr)));
                }
            } else {
                let out = Command::new("unzip")
                    .args(["-q", "-o"])
                    .arg(archive)
                    .arg("-d")
                    .arg(target)
                    .stdin(Stdio::null())
                    .output()
                    .map_err(|e| format!("unzip failed: {e}"))?;
                if !out.status.success() {
                    return Err(format!("unzip failed: {}", String::from_utf8_lossy(&out.stderr)));
                }
            }
            flatten_single_subdir(target)?;
            mark_tree_executables(target);
            Ok(())
        }
        other => Err(format!("unsupported archive kind: {other}")),
    }
}

/// Unwraps a single top-level wrapper folder (common in GitHub release zips/tarballs) so
/// `tools::with_engine_bins` sees `<engine>/bin` or `<engine>/<exe>` directly.
fn flatten_single_subdir(dir: &Path) -> Result<(), String> {
    let entries: Vec<PathBuf> = fs::read_dir(dir).map_err(|e| e.to_string())?.flatten().map(|e| e.path()).collect();
    if entries.len() == 1 && entries[0].is_dir() {
        let inner = &entries[0];
        let has_known_layout =
            inner.file_name().is_some_and(|n| matches!(n.to_str(), Some("bin" | "lib" | "Library" | "program")));
        if !has_known_layout {
            for child in fs::read_dir(inner).map_err(|e| e.to_string())?.flatten() {
                let dst = dir.join(child.file_name());
                fs::rename(child.path(), dst).map_err(|e| e.to_string())?;
            }
            let _ = fs::remove_dir(inner);
        }
    }
    Ok(())
}

fn make_executable(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = fs::metadata(path) {
            let mut perms = meta.permissions();
            perms.set_mode(perms.mode() | 0o755);
            let _ = fs::set_permissions(path, perms);
        }
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
}

fn mark_tree_executables(root: &Path) {
    #[cfg(unix)]
    {
        let mut stack = vec![root.to_path_buf()];
        while let Some(dir) = stack.pop() {
            let Ok(rd) = fs::read_dir(&dir) else { continue };
            for entry in rd.flatten() {
                let p = entry.path();
                if p.is_dir() {
                    stack.push(p);
                } else if p.extension().is_none() || p.parent().and_then(|d| d.file_name()).is_some_and(|n| n == "bin")
                {
                    make_executable(&p);
                }
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = root;
    }
}

/// Downloads, verifies SHA-256, extracts, and registers an optional engine inside the user's
/// `app_local_data_dir/engines/` folder.
pub fn download_and_install(app: &AppHandle, id: &str) -> Result<PathBuf, String> {
    let tool = Tool::by_slug(id).ok_or_else(|| format!("unknown engine `{id}`"))?;
    let spec = download_spec(id).ok_or_else(|| format!("no portable archive configured for `{id}`"))?;
    let root = local_engines_dir(app)?;
    let staging = root.join(format!(".staging-{id}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&staging);
    fs::create_dir_all(&staging).map_err(|e| e.to_string())?;

    let result = (|| -> Result<PathBuf, String> {
        let archive = staging.join(format!("{id}.archive"));
        emit_stage(app, id, "downloading", None);
        fetch_and_verify(app, id, &spec.url, &spec.sha256, spec.size, &archive)?;
        emit_stage(app, id, "extracting", None);

        let target = root.join(component_key(id));
        let _ = fs::remove_dir_all(&target);
        extract_archive(&archive, &spec.kind, &target, component_key(id))?;

        // Whisper also requires the tiny GGML model file in `engines/models/`.
        if tool == Tool::Whisper && tools::whisper_model().is_none() {
            if let Some(model) = manifest().and_then(|m| m.models.get("whisper-tiny").cloned()) {
                let model_dir = root.join("models");
                fs::create_dir_all(&model_dir).map_err(|e| e.to_string())?;
                let model_dst = model_dir.join(&model.name);
                let tmp_model = staging.join(&model.name);
                emit_stage(app, id, "downloading", Some(model.name.clone()));
                fetch_and_verify(app, id, &model.url, &model.sha256, 0, &tmp_model)?;
                fs::rename(&tmp_model, &model_dst).map_err(|e| e.to_string())?;
            }
        }

        tools::register_bundle_dir(root.clone());
        tools::clear_probe_cache();

        let located = tools::locate(tool).unwrap_or(target);
        emit_stage(app, id, "done", Some(located.to_string_lossy().into_owned()));
        Ok(located)
    })();

    let _ = fs::remove_dir_all(&staging);
    if let Err(ref e) = result {
        emit_stage(app, id, "failed", Some(e.clone()));
    }
    result
}

/// Downloads every missing portable engine available for the active platform sequentially.
pub fn download_all_missing(app: &AppHandle) -> Result<Vec<String>, String> {
    let mut installed = Vec::new();
    let mut seen_components = HashSet::new();
    for tool in Tool::ALL {
        if tools::locate(tool).is_some() {
            continue;
        }
        let slug = tool.slug();
        if download_spec(slug).is_none() {
            continue;
        }
        let key = component_key(slug).to_string();
        if !seen_components.insert(key) {
            continue;
        }
        download_and_install(app, slug)?;
        installed.push(slug.to_string());
    }
    Ok(installed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engines_json_parses_with_sha256_digests() {
        let doc = manifest().expect("packaging/engines.json must parse");
        assert!(!doc.platforms.is_empty());
        for (plat, entry) in &doc.platforms {
            for (name, comp) in &entry.components {
                assert_eq!(comp.sha256.len(), 64, "{plat}/{name} has invalid sha256");
                assert!(comp.url.starts_with("https://"), "{plat}/{name} must use https");
            }
        }
    }
}
