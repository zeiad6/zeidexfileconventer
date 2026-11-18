//! Deep functional smoke tests, version/disk-size inspection, and diagnostic report formatting
//! shared by `zeidex-convert doctor` and the desktop Engines & Diagnostics Center.

use crate::diagnostics;
use crate::tools::{self, Tool};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EngineHealthState {
    Healthy,
    Missing,
    Broken,
}

impl EngineHealthState {
    pub fn as_str(self) -> &'static str {
        match self {
            EngineHealthState::Healthy => "healthy",
            EngineHealthState::Missing => "missing",
            EngineHealthState::Broken => "broken",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SmokeResult {
    pub id: &'static str,
    pub name: &'static str,
    pub state: EngineHealthState,
    pub path: Option<String>,
    pub version: Option<String>,
    pub disk_bytes: Option<u64>,
    pub exit_code: Option<i32>,
    pub stderr: String,
    pub note: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelCheck {
    pub id: &'static str,
    pub label: &'static str,
    pub label_ar: &'static str,
    pub present: bool,
    pub path: Option<String>,
    pub disk_bytes: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HealthSummary {
    pub profile: &'static str,
    pub os: &'static str,
    pub arch: &'static str,
    pub engines: Vec<SmokeResult>,
    pub models: Vec<ModelCheck>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FailedJobLog {
    pub name: String,
    pub error: String,
}

const SMOKE_TIMEOUT: Duration = Duration::from_secs(8);
const VERSION_TIMEOUT: Duration = Duration::from_secs(4);
const MAX_DIR_ENTRIES: usize = 8192;

fn base_command(bin: &Path) -> Command {
    let bin = tools::normalize_engine_path(bin);
    #[allow(unused_mut)]
    let mut cmd = Command::new(&bin);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    if let Some((key, dir)) = tools::bundled_env(&bin) {
        cmd.env(key, tools::normalize_engine_path(&dir));
    }
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    cmd
}

/// Executes a command synchronously with a bounded deadline so an unresponsive GUI/office binary
/// cannot hang the CLI or UI health check.
fn run_bounded(mut cmd: Command, timeout: Duration) -> std::result::Result<Output, String> {
    let mut child = cmd.spawn().map_err(|e| format!("failed to spawn: {e}"))?;
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return child.wait_with_output().map_err(|e| format!("wait_with_output failed: {e}")),
            Ok(None) => {
                if start.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!("timed out after {}s", timeout.as_secs()));
                }
                thread::sleep(Duration::from_millis(25));
            }
            Err(e) => return Err(format!("process poll failed: {e}")),
        }
    }
}

/// Sums the size of a single file, or an entire directory tree when `root` is inside a bundled
/// or locally downloaded engine folder.
pub fn dir_or_file_size(bin: &Path) -> Option<u64> {
    let bin = tools::normalize_engine_path(bin);
    let target =
        if tools::is_bundled_path(&bin) { engine_package_root(&bin).unwrap_or(bin.clone()) } else { bin.clone() };
    if target.is_file() {
        return fs::metadata(&target).map(|m| m.len()).ok();
    }
    if !target.is_dir() {
        return None;
    }
    let mut total = 0u64;
    let mut seen = 0usize;
    let mut stack = vec![target];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = fs::read_dir(&dir) else { continue };
        for entry in rd.flatten() {
            seen += 1;
            if seen > MAX_DIR_ENTRIES {
                return Some(total);
            }
            let p = entry.path();
            if p.is_dir() {
                stack.push(p);
            } else if let Ok(meta) = entry.metadata() {
                total = total.saturating_add(meta.len());
            }
        }
    }
    Some(total)
}

fn engine_package_root(bin: &Path) -> Option<PathBuf> {
    let parent = bin.parent()?;
    match parent.file_name().and_then(|n| n.to_str()) {
        Some("bin" | "Scripts" | "program" | "MacOS") => {
            let up = parent.parent()?;
            if up.file_name().is_some_and(|n| n == "Library" || n == "Contents") {
                return up.parent().map(Path::to_path_buf);
            }
            if up.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("app")) {
                return up.parent().map(Path::to_path_buf);
            }
            Some(up.to_path_buf())
        }
        _ => Some(parent.to_path_buf()),
    }
}

fn extract_first_line(text: &str) -> Option<String> {
    text.lines().map(str::trim).find(|l| !l.is_empty()).map(|l| l.chars().take(72).collect())
}

/// Queries the binary's version string quickly.
pub fn detect_version(tool: Tool, bin: &Path) -> Option<String> {
    let args: &[&str] = match tool {
        Tool::Ffmpeg | Tool::Ffprobe => &["-version"],
        Tool::Ghostscript => &["--version"],
        Tool::SevenZip => &["i"],
        Tool::ExifTool => &["-ver"],
        Tool::RealEsrgan => &["-h"],
        Tool::Whisper | Tool::Rembg => &["--help"],
        _ => &["--version"],
    };
    let mut cmd = base_command(bin);
    cmd.args(args);
    let out = run_bounded(cmd, VERSION_TIMEOUT).ok()?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    extract_first_line(&stdout).or_else(|| extract_first_line(&stderr))
}

fn system_tessdata_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(env_dir) = std::env::var_os("TESSDATA_PREFIX") {
        dirs.push(tools::normalize_engine_path(&PathBuf::from(env_dir)));
    }
    if let Some(tess) = tools::locate(Tool::Tesseract) {
        if let Some(d) = tools::find_tessdata_dir(&tess) {
            dirs.push(d);
        }
    }
    for candidate in [
        "/usr/share/tesseract-ocr/5/tessdata",
        "/usr/share/tesseract-ocr/4.00/tessdata",
        "/usr/share/tessdata",
        "/opt/homebrew/share/tessdata",
        "/usr/local/share/tessdata",
    ] {
        dirs.push(PathBuf::from(candidate));
    }
    dirs
}

fn find_traineddata(lang_file: &str) -> Option<PathBuf> {
    system_tessdata_dirs().into_iter().map(|d| d.join(lang_file)).find(|p| p.is_file())
}

/// Scans required OCR language files and AI model weights relative to the bundled engine roots
/// (never relative to CWD).
pub fn scan_models() -> Vec<ModelCheck> {
    let eng = find_traineddata("eng.traineddata").map(|p| tools::normalize_engine_path(&p));
    let ara = find_traineddata("ara.traineddata").map(|p| tools::normalize_engine_path(&p));
    let whisper = tools::whisper_model();
    let rembg = tools::rembg_model();
    let esrgan = tools::esrgan_models();

    vec![
        ModelCheck {
            id: "tesseract-eng",
            label: "Tesseract English (eng.traineddata)",
            label_ar: "بيانات Tesseract الإنجليزية (eng.traineddata)",
            present: eng.is_some(),
            disk_bytes: eng.as_deref().and_then(dir_or_file_size),
            path: eng.map(|p| p.to_string_lossy().into_owned()),
        },
        ModelCheck {
            id: "tesseract-ara",
            label: "Tesseract Arabic (ara.traineddata)",
            label_ar: "بيانات Tesseract العربية (ara.traineddata)",
            present: ara.is_some(),
            disk_bytes: ara.as_deref().and_then(dir_or_file_size),
            path: ara.map(|p| p.to_string_lossy().into_owned()),
        },
        ModelCheck {
            id: "whisper-tiny",
            label: "Whisper model (ggml-*.bin)",
            label_ar: "نموذج Whisper (ggml-*.bin)",
            present: whisper.is_some(),
            disk_bytes: whisper.as_deref().and_then(dir_or_file_size),
            path: whisper.map(|p| p.to_string_lossy().into_owned()),
        },
        ModelCheck {
            id: "rembg-u2net",
            label: "rembg model (u2net.onnx)",
            label_ar: "نموذج إزالة الخلفية (u2net.onnx)",
            present: rembg.is_some(),
            disk_bytes: rembg.as_deref().and_then(dir_or_file_size),
            path: rembg.map(|p| p.to_string_lossy().into_owned()),
        },
        ModelCheck {
            id: "esrgan-models",
            label: "Real-ESRGAN models",
            label_ar: "نماذج Real-ESRGAN للتكبير",
            present: esrgan.is_some(),
            disk_bytes: esrgan.as_deref().and_then(dir_or_file_size),
            path: esrgan.map(|p| p.to_string_lossy().into_owned()),
        },
    ]
}

fn smoke_args(tool: Tool, bin: &Path) -> Vec<OsString> {
    match tool {
        Tool::Ffmpeg => [
            "-hide_banner",
            "-nostdin",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "color=c=black:s=16x16:d=0.1",
            "-frames:v",
            "1",
            "-f",
            "null",
            "-",
        ]
        .into_iter()
        .map(OsString::from)
        .collect(),
        Tool::Ffprobe => vec![OsString::from("-version")],
        Tool::Vips | Tool::VipsHeader | Tool::Qpdf | Tool::Pandoc | Tool::Calibre | Tool::Zbar | Tool::Qrencode => {
            vec![OsString::from("--version")]
        }
        Tool::Ghostscript => ["-dNODISPLAY", "-dBATCH", "-c", "quit"].into_iter().map(OsString::from).collect(),
        Tool::SevenZip => vec![OsString::from("i")],
        Tool::LibreOffice => ["--headless", "--nologo", "--nodefault", "--norestore", "--version"]
            .into_iter()
            .map(OsString::from)
            .collect(),
        Tool::Tesseract => {
            let mut a = Vec::new();
            if let Some((_, dir)) = tools::bundled_env(bin) {
                a.push(OsString::from("--tessdata-dir"));
                a.push(dir.into_os_string());
            }
            a.push(OsString::from("--list-langs"));
            a
        }
        Tool::Whisper | Tool::RealEsrgan => vec![OsString::from("-h")],
        Tool::ExifTool => vec![OsString::from("-ver")],
        Tool::Rembg => vec![OsString::from("--help")],
    }
}

fn evaluate_realesrgan_smoke(status_ok: bool, stdout: &str, stderr: &str, has_models: bool) -> (bool, String) {
    let combined = format!("{stdout}\n{stderr}").to_ascii_lowercase();
    // Upstream realesrgan-ncnn-vulkan prints usage to stderr and returns -1 (0xffffffff) on `-h`.
    let banner_ok = status_ok
        || combined.contains("realesrgan-ncnn-vulkan")
        || (combined.contains("usage:") && combined.contains("-i infile"));
    if !banner_ok {
        return (false, "Real-ESRGAN failed to start | تعذّر تشغيل Real-ESRGAN".into());
    }
    if !has_models {
        return (false, "Real-ESRGAN models folder missing | مجلد نماذج Real-ESRGAN غير موجود".into());
    }
    (true, "Verified OK (Vulkan CLI + models ready)".into())
}

/// Runs a fast functional smoke test for `tool`.
pub fn smoke_test_tool(tool: Tool) -> SmokeResult {
    let broken = tools::broken_engines();
    let Some(raw_bin) = tools::locate(tool) else {
        if let Some((_, broken_path)) = broken.into_iter().find(|(t, _)| *t == tool) {
            return SmokeResult {
                id: tool.slug(),
                name: tool.label(),
                state: EngineHealthState::Broken,
                path: Some(tools::normalize_engine_path(&broken_path).to_string_lossy().into_owned()),
                version: None,
                disk_bytes: dir_or_file_size(&broken_path),
                exit_code: None,
                stderr: "Bundled binary refused to start (missing runtime/DLL or corrupted archive).".into(),
                note: "Broken binary".into(),
            };
        }
        return SmokeResult {
            id: tool.slug(),
            name: tool.label(),
            state: EngineHealthState::Missing,
            path: None,
            version: None,
            disk_bytes: None,
            exit_code: None,
            stderr: String::new(),
            note: tool.install_hint().into(),
        };
    };

    let bin = tools::normalize_engine_path(&raw_bin);
    let path_str = bin.to_string_lossy().into_owned();
    let disk_bytes = dir_or_file_size(&bin);
    let version = detect_version(tool, &bin);

    let mut cmd = base_command(&bin);
    cmd.args(smoke_args(tool, &bin));
    match run_bounded(cmd, SMOKE_TIMEOUT) {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout).to_string();
            let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
            let exit_code = out.status.code();
            let mut combined_err = stderr.clone();
            let mut ok = out.status.success();
            let note = if tool == Tool::RealEsrgan {
                let (esr_ok, esr_note) =
                    evaluate_realesrgan_smoke(ok, &stdout, &stderr, tools::esrgan_models().is_some());
                ok = esr_ok;
                if ok {
                    combined_err.clear();
                } else if combined_err.is_empty() {
                    combined_err = esr_note.clone();
                }
                esr_note
            } else if tool == Tool::Tesseract && ok {
                let langs: Vec<&str> = stdout.lines().skip(1).map(str::trim).collect();
                let has_eng = langs.contains(&"eng");
                let has_ara = langs.contains(&"ara");
                if !has_eng || !has_ara {
                    ok = false;
                    let missing = match (has_eng, has_ara) {
                        (false, false) => "eng.traineddata & ara.traineddata",
                        (false, true) => "eng.traineddata",
                        (true, false) => "ara.traineddata",
                        (true, true) => "",
                    };
                    let msg = format!("Missing OCR traineddata: {missing}");
                    if combined_err.is_empty() {
                        combined_err = format!("Loaded langs: [{}]; {msg}", langs.join(", "));
                    }
                    msg
                } else {
                    format!("Tesseract langs verified ({})", langs.join(", "))
                }
            } else if ok {
                "Verified OK".into()
            } else if let Some(diag) = diagnostics::diagnose(tool.label(), &stderr) {
                format!("{} | {}", diag.message_en, diag.message_ar)
            } else {
                format!("Exited with status {}", out.status)
            };

            SmokeResult {
                id: tool.slug(),
                name: tool.label(),
                state: if ok { EngineHealthState::Healthy } else { EngineHealthState::Broken },
                path: Some(path_str),
                version,
                disk_bytes,
                exit_code,
                stderr: if combined_err.is_empty() && !ok { stdout.trim().to_string() } else { combined_err },
                note,
            }
        }
        Err(err) => SmokeResult {
            id: tool.slug(),
            name: tool.label(),
            state: EngineHealthState::Broken,
            path: Some(path_str),
            version,
            disk_bytes,
            exit_code: None,
            stderr: err.clone(),
            note: err,
        },
    }
}

/// Runs functional smoke tests across all 16 bundled/optional tools and required models.
pub fn test_all_engines() -> HealthSummary {
    let engines = Tool::ALL.iter().map(|t| smoke_test_tool(*t)).collect();
    let models = scan_models();
    HealthSummary {
        profile: tools::build_profile(),
        os: std::env::consts::OS,
        arch: std::env::consts::ARCH,
        engines,
        models,
    }
}

/// Builds a human- and issue-tracker-friendly plain-text diagnostic report with normalized paths.
pub fn format_error_report(app_version: &str, summary: &HealthSummary, failed_jobs: &[FailedJobLog]) -> String {
    let mut out = String::new();
    out.push_str("Zeidex Convert Diagnostic & Error Report\n");
    out.push_str("========================================\n");
    out.push_str(&format!("App Version: {app_version}\n"));
    out.push_str(&format!("OS / Arch:   {} ({})\n", summary.os, summary.arch));
    out.push_str(&format!("Profile:     {}\n\n", summary.profile));

    out.push_str("Models & Data Files:\n");
    for m in &summary.models {
        let status = if m.present { "OK" } else { "MISSING" };
        let path = m.path.as_deref().unwrap_or("-");
        out.push_str(&format!("  [{status:<7}] {:<36} {path}\n", m.label));
    }

    out.push_str("\nEngines:\n");
    for e in &summary.engines {
        let ver = e.version.as_deref().unwrap_or("-");
        let code = e.exit_code.map_or_else(|| "-".into(), |c| c.to_string());
        let path = e.path.as_deref().unwrap_or("-");
        out.push_str(&format!(
            "  [{:<7}] {:<24} path={path} | version={ver} | exit={code}\n",
            e.state.as_str().to_uppercase(),
            e.name
        ));
        if !e.stderr.is_empty() {
            out.push_str(&format!("            stderr: {}\n", e.stderr.replace('\n', " \\n ")));
        }
    }

    if !failed_jobs.is_empty() {
        out.push_str("\nRecent Failed Jobs:\n");
        for job in failed_jobs {
            out.push_str(&format!("  - {}: {}\n", job.name, job.error));
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_report_includes_os_engines_and_failed_jobs() {
        let summary = HealthSummary {
            profile: "lite",
            os: "windows",
            arch: "x86_64",
            engines: vec![
                SmokeResult {
                    id: "tesseract",
                    name: "Tesseract OCR",
                    state: EngineHealthState::Broken,
                    path: Some(r"C:\Users\ace\AppData\Local\Zeidex Convert\engines\ocr\tesseract.exe".into()),
                    version: Some("tesseract 5.4.0".into()),
                    disk_bytes: Some(12_000_000),
                    exit_code: Some(1),
                    stderr: "Failed loading language 'ara'".into(),
                    note: "Missing OCR traineddata: ara.traineddata".into(),
                },
                SmokeResult {
                    id: "calibre",
                    name: "Calibre",
                    state: EngineHealthState::Missing,
                    path: None,
                    version: None,
                    disk_bytes: None,
                    exit_code: None,
                    stderr: String::new(),
                    note: "Install Calibre".into(),
                },
            ],
            models: vec![],
        };
        let jobs = vec![FailedJobLog { name: "scan.pdf".into(), error: "exit 1".into() }];
        let report = format_error_report("0.3.5", &summary, &jobs);
        assert!(report.contains("App Version: 0.3.5"));
        assert!(report.contains("[BROKEN ] Tesseract OCR"));
        assert!(!report.contains(r"\\?\"));
        assert!(report.contains("Failed loading language 'ara'"));
        assert!(report.contains("scan.pdf: exit 1"));
    }

    #[test]
    fn scan_models_reports_all_five_model_slots() {
        let models = scan_models();
        let ids: Vec<&str> = models.iter().map(|m| m.id).collect();
        assert_eq!(ids, vec!["tesseract-eng", "tesseract-ara", "whisper-tiny", "rembg-u2net", "esrgan-models"]);
    }

    #[test]
    fn realesrgan_accepts_usage_exit_minus_one_when_models_exist() {
        let usage = "Usage: realesrgan-ncnn-vulkan.exe -i infile -o outfile [options]...";
        assert!(evaluate_realesrgan_smoke(false, "", usage, true).0);
        assert!(!evaluate_realesrgan_smoke(false, "", usage, false).0);
        assert!(!evaluate_realesrgan_smoke(false, "", "", true).0);
    }
}
