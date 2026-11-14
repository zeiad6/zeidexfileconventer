//! Tauri shell: windows, tray, IPC commands. All file work lives in zeidex-core.

mod hook;
#[cfg(windows)]
mod win_drag_hook;
#[cfg(windows)]
mod win_studio;

use hook::Trigger;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, WindowEvent};
use tauri_plugin_clipboard_manager::ClipboardExt;
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
use zeidex_core::ops::{self, Op};
use zeidex_core::runner::{self, Event};
use zeidex_core::{actions, b64, format, integrate, tools};

#[derive(Serialize)]
struct ParamOut {
    key: &'static str,
    label: &'static str,
    label_ar: &'static str,
    default: &'static str,
}

#[derive(Serialize)]
struct Choice {
    kind: &'static str,
    /// Opens the visual editor instead of an options form.
    editor: bool,
    id: String,
    label: String,
    label_ar: String,
    params: Vec<ParamOut>,
    /// Fixed options sent with the choice (saved recipes).
    preset: HashMap<String, String>,
    /// Outer-ring refinements: quality presets, or a tool's default options plus "Custom…".
    children: Vec<Sub>,
}

#[derive(Serialize)]
struct Sub {
    key: String,
    value: String,
    label: String,
    label_ar: String,
    /// Opens the options form instead of running with this value.
    custom: bool,
}

/// Outer-ring refinements for one choice. Lossy encoders get quality presets; tools with
/// options get one segment per documented default plus "Custom…".
fn children_for(kind: &str, id: &str, params: &[ParamOut]) -> Vec<Sub> {
    if kind == "convert" {
        const LOSSY: [&str; 12] =
            ["jpg", "webp", "avif", "heic", "jxl", "mp4", "mov", "mkv", "webm", "mp3", "ogg", "opus"];
        if !LOSSY.contains(&id) {
            return vec![];
        }
        let mut out = Vec::new();
        for (q, en, ar) in [(90, "Best", "أفضل"), (80, "High", "عالي"), (60, "Small", "صغير")] {
            out.push(Sub {
                key: "quality".into(),
                value: q.to_string(),
                label: format!("{en} {q}"),
                label_ar: format!("{ar} {q}"),
                custom: false,
            });
        }
        return out;
    }
    if kind == "tool" && !params.is_empty() {
        let mut out: Vec<Sub> = params
            .iter()
            .map(|p| Sub {
                key: p.key.into(),
                value: p.default.into(),
                label: format!("{} {}", p.label, p.default),
                label_ar: format!("{} {}", p.label_ar, p.default),
                custom: false,
            })
            .collect();
        out.push(Sub {
            key: "custom".into(),
            value: String::new(),
            label: "Custom…".into(),
            label_ar: "مخصّص…".into(),
            custom: true,
        });
        return out;
    }
    vec![]
}

#[derive(Serialize)]
struct Described {
    category: Option<&'static str>,
    /// The files do not share a category: conversions apply to the files that support them.
    mixed: bool,
    convert: Vec<Choice>,
    tools: Vec<Choice>,
    actions: Vec<Choice>,
}

/// Saved recipes the Tools ring shows; the studio lists every one.
const WHEEL_RECIPES: usize = 4;

const ACTIONS: [(&str, &str, &str); 11] = [
    ("float", "Float to shelf", "علّق في الرف العائم"),
    ("archive", "Archive to ZIP", "أرشفة إلى ZIP"),
    ("extract", "Extract here", "فك الأرشيف هنا"),
    ("rename", "Batch rename…", "إعادة تسمية دفعية…"),
    ("checksum", "SHA-256", "بصمة SHA-256"),
    ("reveal", "Show in folder", "إظهار في المجلد"),
    ("copy-path", "Copy path", "نسخ المسار"),
    ("move", "Move to…", "نقل إلى…"),
    ("terminal", "Terminal", "الطرفية"),
    ("send-phone", "Send to phone", "إرسال إلى الهاتف"),
    ("shred", "Shred…", "إتلاف نهائي…"),
];

#[derive(Clone, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum JobEvent {
    Start { job: u64, index: usize, total: usize, file: String, title: String },
    Progress { job: u64, index: usize, fraction: f32 },
    Done { job: u64, index: usize, output: String },
    Failed { job: u64, index: usize, error: String },
    Text { job: u64, title: String, text: String },
    // LocalDrop is waiting: `svg` is the pairing code the phone scans.
    Pair { job: u64, title: String, file: String, svg: String, address: String },
    // LocalDrop finished: the phone took the files, or its user declined them.
    Sent { job: u64, device: String, files: usize, declined: bool },
}

#[derive(Serialize)]
struct ToolStatus {
    name: &'static str,
    path: Option<String>,
    hint: &'static str,
}

static NEXT_JOB: AtomicU64 = AtomicU64::new(1);
static REVEAL: AtomicBool = AtomicBool::new(true);
/// Files handed over by `--open` before the home window was ready to receive them.
static PENDING: Mutex<Vec<String>> = Mutex::new(Vec::new());

#[tauri::command]
fn describe(app: AppHandle, paths: Vec<String>) -> Described {
    let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
    let cat = format::common_category(&paths);
    let common = format::common_targets(&paths);
    let mixed = common.is_empty() && paths.len() > 1;
    let total = paths.len();
    // A heterogeneous drag has no shared target; offer every output at least one file can
    // become, labelled with how many files carry it, instead of an empty ring.
    let convert: Vec<Choice> = if common.is_empty() && mixed {
        format::mixed_targets(&paths)
            .into_iter()
            .map(|(t, n)| {
                let children = children_for("convert", t, &[]);
                Choice {
                    kind: "convert",
                    editor: false,
                    id: t.into(),
                    label: format!("{} {n}/{total}", t.to_uppercase()),
                    label_ar: format!("{} {n}/{total}", t.to_uppercase()),
                    params: vec![],
                    preset: HashMap::new(),
                    children,
                }
            })
            .collect()
    } else {
        common
            .into_iter()
            .map(|t| Choice {
                kind: "convert",
                editor: false,
                id: t.into(),
                label: t.to_uppercase(),
                label_ar: t.to_uppercase(),
                params: vec![],
                preset: HashMap::new(),
                children: children_for("convert", t, &[]),
            })
            .collect()
    };
    let recipes: Vec<Recipe> = get_settings(app).recipes.into_iter().take(WHEEL_RECIPES).collect();
    let all: Vec<&ops::ToolDef> = ops::tools_for_paths(&paths).into_iter().filter(|t| t.id != "recipe").collect();
    let (shown, more) = ops::wheel_tools(&all, recipes.len());
    let saved = recipes.into_iter().map(|r| Choice {
        kind: "tool",
        editor: false,
        id: "recipe".into(),
        label: format!("★ {}", r.name),
        label_ar: format!("★ {}", r.name),
        params: vec![],
        preset: HashMap::from([("steps".to_string(), r.steps)]),
        children: vec![],
    });
    let more = more.then(|| Choice {
        kind: "action",
        editor: false,
        id: "more-tools".into(),
        label: "More tools…".into(),
        label_ar: "المزيد من الأدوات…".into(),
        params: vec![],
        preset: HashMap::new(),
        children: vec![],
    });
    let tools = shown
        .into_iter()
        .map(|t| {
            let params: Vec<ParamOut> = t
                .params
                .iter()
                .map(|p| ParamOut { key: p.key, label: p.label, label_ar: p.label_ar, default: p.default })
                .collect();
            let children = children_for("tool", t.id, &params);
            Choice {
                kind: "tool",
                editor: ops::EDITOR_TOOLS.contains(&t.id),
                id: t.id.into(),
                label: t.label.into(),
                label_ar: t.label_ar.into(),
                params,
                preset: HashMap::new(),
                children,
            }
        })
        .chain(saved)
        .chain(more)
        .collect();
    let actions = ACTIONS
        .iter()
        .map(|(id, label, ar)| Choice {
            kind: "action",
            editor: false,
            id: (*id).into(),
            label: (*label).into(),
            label_ar: (*ar).into(),
            params: vec![],
            preset: HashMap::new(),
            children: vec![],
        })
        .collect();
    Described { category: cat.map(|c| c.name()), mixed, convert, tools, actions }
}

fn file_name(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

#[tauri::command]
fn run_job(
    app: AppHandle,
    paths: Vec<String>,
    kind: String,
    id: String,
    opts: HashMap<String, String>,
) -> Result<(), String> {
    let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
    if paths.is_empty() {
        return Err("no files".into());
    }
    if kind == "action" {
        hook::hide(&app);
        return action(&app, &id, paths);
    }
    // Validate first: on bad options the wheel stays open so the form can show the error.
    let op = if kind == "convert" {
        let quality = ops::parse_quality(opts.get("quality")).map_err(|e| e.to_string())?;
        Op::Convert { to: id.clone(), quality }
    } else {
        Op::parse(&id, &opts).map_err(|e| e.to_string())?
    };
    hook::hide(&app);
    if runner::is_text_tool(&op) {
        // Reports (metadata, QR contents, duplicates, renames) go to the jobs window as text.
        let job = NEXT_JOB.fetch_add(1, Ordering::Relaxed);
        show(&app, "jobs");
        std::thread::spawn(move || {
            let text = runner::text_tool(&op, &paths).unwrap_or_else(|e| e.to_string());
            let _ = app.emit("job", JobEvent::Text { job, title: id, text });
        });
        return Ok(());
    }
    let title = if kind == "convert" { format!("→ {}", id.to_uppercase()) } else { id };
    spawn_job(app, op, paths, title, None);
    Ok(())
}

/// Runs `op` on a worker thread, streaming progress to the jobs window. `cleanup` is deleted
/// afterwards (temporary inputs such as an annotation overlay).
fn spawn_job(app: AppHandle, op: Op, paths: Vec<PathBuf>, title: String, cleanup: Option<PathBuf>) {
    let job = NEXT_JOB.fetch_add(1, Ordering::Relaxed);
    show(&app, "jobs");
    std::thread::spawn(move || {
        let results = runner::run(&op, &paths, |e| {
            let _ = app.emit("job", job_event(job, &title, e));
        });
        if let Some(tmp) = cleanup {
            let _ = std::fs::remove_file(tmp);
        }
        if REVEAL.load(Ordering::Relaxed) {
            if let Some(Ok(last)) = results.iter().rev().find(|r| r.is_ok()) {
                let _ = actions::reveal(last);
            }
        }
    });
}

fn job_event(job: u64, title: &str, e: Event) -> JobEvent {
    match e {
        Event::Start { index, total, file } => JobEvent::Start { job, index, total, file, title: title.to_string() },
        Event::Progress { index, fraction } => JobEvent::Progress { job, index, fraction },
        Event::Done { index, output } => JobEvent::Done { job, index, output: output.to_string_lossy().into_owned() },
        Event::Failed { index, error, .. } => JobEvent::Failed { job, index, error },
    }
}

fn action(app: &AppHandle, id: &str, paths: Vec<PathBuf>) -> Result<(), String> {
    match id {
        "reveal" => actions::reveal(&paths[0]).map_err(|e| e.to_string()),
        "terminal" => actions::open_terminal(&paths[0]).map_err(|e| e.to_string()),
        "copy-path" => app.clipboard().write_text(actions::paths_text(&paths)).map_err(|e| e.to_string()),
        "float" => {
            float_add(app.clone(), paths.iter().map(|p| p.to_string_lossy().into_owned()).collect());
            Ok(())
        }
        "move" => {
            let app = app.clone();
            // The folder picker blocks; keep it off the IPC thread.
            std::thread::spawn(move || {
                let Some(dir) = app.dialog().file().blocking_pick_folder().and_then(|d| d.into_path().ok()) else {
                    return;
                };
                if let Err(e) = actions::move_to(&paths, &dir) {
                    let _ = app.emit("job", JobEvent::Text { job: 0, title: "Move".into(), text: e.to_string() });
                }
            });
            Ok(())
        }
        // The action ring is reachable for mixed selections too, so the tools that work on any
        // file live here as well — the same ops the Tools ring offers, with their defaults.
        "archive" => {
            let op = ops::parse_step("archive format=zip").map_err(|e| e.to_string())?;
            spawn_job(app.clone(), op, paths, "ZIP".into(), None);
            Ok(())
        }
        "extract" => {
            spawn_job(app.clone(), Op::Extract { password: None }, paths, "extract".into(), None);
            Ok(())
        }
        "checksum" => {
            spawn_job(app.clone(), Op::Checksum, paths, "SHA-256".into(), None);
            Ok(())
        }
        // The ring shows the most useful tools; the studio window lists all of them.
        "more-tools" => {
            open_in_home(app.clone(), paths.iter().map(|p| p.to_string_lossy().into_owned()).collect());
            Ok(())
        }
        "send-phone" => {
            send_to_phone(app, paths);
            Ok(())
        }
        "shred" => {
            let app = app.clone();
            // The confirmation dialog blocks; keep it off the IPC thread.
            std::thread::spawn(move || shred_confirmed(&app, &paths));
            Ok(())
        }
        // The pattern field with live preview lives in the home window: send the files there
        // rather than renaming on a default pattern the user never saw.
        "rename" => {
            open_in_home(app.clone(), paths.iter().map(|p| p.to_string_lossy().into_owned()).collect());
            let _ = app.emit("open-tool", "rename");
            Ok(())
        }
        other => Err(format!("unknown action {other}")),
    }
}

/// Shredding cannot be undone, so it always asks first and says what it cannot promise.
fn shred_confirmed(app: &AppHandle, paths: &[PathBuf]) {
    let names: Vec<String> = paths.iter().take(5).map(|p| file_name(p)).collect();
    let more = if paths.len() > names.len() { format!("\n… +{}", paths.len() - names.len()) } else { String::new() };
    let message = format!(
        "{}{more}\n\n\
         Overwrite {n} file(s) three times and delete them? This cannot be undone.\n\
         SSDs, phones and copy-on-write disks may keep older copies elsewhere; only full-disk encryption covers those.\n\n\
         الكتابة فوق {n} ملف ثلاث مرات ثم حذفها؟ لا يمكن التراجع.\n\
         أقراص SSD والهواتف قد تحتفظ بنسخ قديمة في مكان آخر؛ التشفير الكامل للقرص وحده يحميها.",
        names.join("\n"),
        n = paths.len()
    );
    let confirmed = app
        .dialog()
        .message(message)
        .title("Shred — إتلاف نهائي")
        .kind(MessageDialogKind::Warning)
        .buttons(MessageDialogButtons::OkCancelCustom("Shred / إتلاف".into(), "Cancel / إلغاء".into()))
        .blocking_show();
    if !confirmed {
        return;
    }
    let job = NEXT_JOB.fetch_add(1, Ordering::Relaxed);
    let text = match actions::shred(paths) {
        Ok(n) => format!("{n} file(s) overwritten and deleted"),
        Err(e) => e.to_string(),
    };
    show(app, "jobs");
    let _ = app.emit("job", JobEvent::Text { job, title: "Shred".into(), text });
}

// ---------- LocalDrop: hand files to the phone app over the local network ----------

/// How long a pairing code stays valid.
const PAIR_WAIT: Duration = Duration::from_secs(10 * 60);
/// The running LocalDrop session; a new one, or `localdrop_cancel`, stops it.
static DROP_CANCEL: Mutex<Option<Arc<AtomicBool>>> = Mutex::new(None);

fn send_to_phone(app: &AppHandle, paths: Vec<PathBuf>) {
    let cancel = Arc::new(AtomicBool::new(false));
    if let Some(old) = DROP_CANCEL.lock().unwrap_or_else(|p| p.into_inner()).replace(cancel.clone()) {
        old.store(true, Ordering::Relaxed);
    }
    let job = NEXT_JOB.fetch_add(1, Ordering::Relaxed);
    show(app, "jobs");
    let app = app.clone();
    // Hashing large files takes a while: never on the IPC thread.
    std::thread::spawn(move || {
        let fail = |error: String| {
            let _ = app.emit("job", JobEvent::Failed { job, index: 0, error });
        };
        let offer = match zeidex_localdrop::offer(&paths) {
            Ok(o) => o,
            Err(e) => return fail(e.to_string()),
        };
        let svg = match offer.qr_svg() {
            Ok(svg) => svg,
            Err(e) => return fail(e.to_string()),
        };
        let bytes: u64 = offer.files().iter().map(|f| f.size).sum();
        let _ = app.emit(
            "job",
            JobEvent::Pair {
                job,
                title: "LocalDrop".into(),
                file: format!("{} · {:.1} MB", offer.files().len(), bytes as f64 / 1e6),
                svg,
                address: offer.link().addr.to_string(),
            },
        );
        let result = offer.serve(PAIR_WAIT, &cancel, |status| {
            let fraction = match status {
                zeidex_localdrop::Status::Waiting => return,
                zeidex_localdrop::Status::Connected { .. } => 0.0,
                zeidex_localdrop::Status::Sending { sent, total } => sent as f32 / total.max(1) as f32,
            };
            let _ = app.emit("job", JobEvent::Progress { job, index: 0, fraction });
        });
        match result {
            Ok(zeidex_localdrop::Delivery::Delivered { device, files, .. }) => {
                let _ = app.emit("job", JobEvent::Sent { job, device, files, declined: false });
            }
            Ok(zeidex_localdrop::Delivery::Declined { device }) => {
                let _ = app.emit("job", JobEvent::Sent { job, device, files: 0, declined: true });
            }
            Err(e) => fail(e.to_string()),
        }
        let mut slot = DROP_CANCEL.lock().unwrap_or_else(|p| p.into_inner());
        if slot.as_ref().is_some_and(|c| Arc::ptr_eq(c, &cancel)) {
            *slot = None;
        }
    });
}

#[tauri::command]
fn localdrop_cancel() {
    if let Some(c) = DROP_CANCEL.lock().unwrap_or_else(|p| p.into_inner()).take() {
        c.store(true, Ordering::Relaxed);
    }
}

// ---------- Float shelf: hold files on the orb, then drag them into any app ----------

static FLOAT: Mutex<Vec<String>> = Mutex::new(Vec::new());

#[tauri::command]
fn float_add(app: AppHandle, paths: Vec<String>) {
    hook::hide(&app);
    let mut held = FLOAT.lock().unwrap_or_else(|e| e.into_inner());
    for p in paths {
        if !held.contains(&p) {
            held.push(p);
        }
    }
    if let Some(w) = app.get_webview_window("shelf") {
        let _ = w.show();
    }
    let _ = app.emit("float-changed", held.len());
}

#[tauri::command]
fn float_list() -> Vec<String> {
    FLOAT.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

#[tauri::command]
fn float_clear(app: AppHandle) {
    FLOAT.lock().unwrap_or_else(|e| e.into_inner()).clear();
    let _ = app.emit("float-changed", 0);
}

/// Drag image shown under the cursor while files leave the orb.
#[tauri::command]
fn drag_icon() -> String {
    format!("data:image/png;base64,{}", b64::encode(include_bytes!("../icons/128x128.png")))
}

// ---------- visual editor ----------

#[derive(Serialize)]
struct EditorState {
    paths: Vec<String>,
    tool: String,
    video: bool,
    width: u32,
    height: u32,
    duration: f64,
    /// PNG data URL of the image or a video frame (empty for PDFs).
    preview: String,
    /// One small PNG data URL per page (page organizer only), across all selected PDFs.
    pages: Vec<String>,
    /// For each entry of `pages`: (file index in `paths`, page number), both 1-based.
    page_refs: Vec<(usize, u32)>,
    /// Frame rate, for frame-by-frame stepping on the trim timeline.
    fps: f64,
    /// Waveform picture (data URL) of the soundtrack, when there is one.
    waveform: String,
    /// Clips in selection order for the join list.
    clips: Vec<Clip>,
}

#[derive(Serialize)]
struct Clip {
    path: String,
    name: String,
    duration: f64,
    /// First-frame thumbnail (data URL); empty for audio.
    thumb: String,
}

/// Size of the frames shown while scrubbing (small = responsive).
const SCRUB_MAX: u32 = 960;
const THUMB_MAX: u32 = 240;
const WAVE_W: u32 = 1200;
const WAVE_H: u32 = 120;

const PREVIEW_MAX: u32 = 1600;
/// Thumbnail resolution for the page organizer (A4 ≈ 190×270 px).
const THUMB_DPI: u32 = 24;

/// Files and tool the editor window was opened for, and the preview PNG it shows (for OCR).
static EDITOR: Mutex<Option<(Vec<String>, String)>> = Mutex::new(None);
static EDITOR_PREVIEW: Mutex<Vec<u8>> = Mutex::new(Vec::new());

#[tauri::command]
fn open_editor(app: AppHandle, paths: Vec<String>, tool: String) {
    hook::hide(&app);
    *EDITOR.lock().unwrap_or_else(|e| e.into_inner()) = Some((paths, tool));
    show(&app, "editor");
    let _ = app.emit("editor-open", ());
}

/// Async: decoding previews and rendering page thumbnails can take a moment.
#[tauri::command]
async fn editor_state() -> Result<EditorState, String> {
    let (paths, tool) = EDITOR.lock().unwrap_or_else(|e| e.into_inner()).clone().ok_or("no file open")?;
    let src = PathBuf::from(&paths[0]);
    let data_url = |png: &[u8]| format!("data:image/png;base64,{}", b64::encode(png));
    if tool == "pages" || tool == "merge-pdf" {
        let (mut pages, mut page_refs) = (Vec::new(), Vec::new());
        for (f, path) in paths.iter().enumerate() {
            let thumbs = runner::pdf_thumbnails(Path::new(path), THUMB_DPI).map_err(|e| e.to_string())?;
            for (n, png) in thumbs.iter().enumerate() {
                pages.push(data_url(png));
                page_refs.push((f + 1, n as u32 + 1));
            }
        }
        return Ok(EditorState {
            paths,
            tool,
            video: false,
            width: 0,
            height: 0,
            duration: 0.0,
            preview: String::new(),
            pages,
            page_refs,
            fps: 0.0,
            waveform: String::new(),
            clips: vec![],
        });
    }
    if tool == "join" {
        let clips = paths
            .iter()
            .map(|p| {
                let path = Path::new(p);
                let media = runner::probe_media(path).unwrap_or_default();
                let thumb = if media.has_video {
                    runner::frame_png(path, 0.0, THUMB_MAX).map(|png| data_url(&png)).unwrap_or_default()
                } else {
                    String::new()
                };
                Clip { path: p.clone(), name: file_name(path), duration: media.duration, thumb }
            })
            .collect();
        return Ok(EditorState {
            paths,
            tool,
            video: true,
            width: 0,
            height: 0,
            duration: 0.0,
            preview: String::new(),
            pages: vec![],
            page_refs: vec![],
            fps: 0.0,
            waveform: String::new(),
            clips,
        });
    }
    if tool == "trim" || tool == "snapshot" {
        let media = runner::probe_media(&src).ok_or("could not read the file")?;
        let preview = if media.has_video {
            data_url(&runner::frame_png(&src, 0.0, SCRUB_MAX).map_err(|e| e.to_string())?)
        } else {
            String::new()
        };
        let waveform = if media.has_audio {
            runner::waveform_png(&src, WAVE_W, WAVE_H).map(|png| data_url(&png)).unwrap_or_default()
        } else {
            String::new()
        };
        return Ok(EditorState {
            paths,
            tool,
            video: media.has_video,
            width: media.width,
            height: media.height,
            duration: media.duration,
            preview,
            pages: vec![],
            page_refs: vec![],
            fps: if media.fps > 0.0 { media.fps } else { 25.0 },
            waveform,
            clips: vec![],
        });
    }
    let video = format::category_of(&src) == Some(format::Category::Video);
    let (width, height) = runner::dimensions(&src).ok_or("could not read the size")?;
    let duration = if video { runner::probe_media(&src).map_or(0.0, |m| m.duration) } else { 0.0 };
    let png = runner::preview_png(&src, PREVIEW_MAX).map_err(|e| e.to_string())?;
    let preview = data_url(&png);
    *EDITOR_PREVIEW.lock().unwrap_or_else(|e| e.into_inner()) = png;
    Ok(EditorState {
        paths,
        tool,
        video,
        width,
        height,
        duration,
        preview,
        pages: vec![],
        page_refs: vec![],
        fps: 0.0,
        waveform: String::new(),
        clips: vec![],
    })
}

#[derive(Serialize)]
struct Box4 {
    x: u32,
    y: u32,
    w: u32,
    h: u32,
}

/// Text lines in the editor's preview, in preview pixels (redact → "Detect text").
#[tauri::command]
async fn detect_text(lang: String) -> Result<Vec<Box4>, String> {
    let png = EDITOR_PREVIEW.lock().unwrap_or_else(|e| e.into_inner()).clone();
    if png.is_empty() {
        return Err("no preview".into());
    }
    let boxes = runner::detect_text_png(&png, &lang).map_err(|e| e.to_string())?;
    Ok(boxes.into_iter().map(|r| Box4 { x: r.x, y: r.y, w: r.w, h: r.h }).collect())
}

/// The frame at `t` seconds of the file open in the editor (trim / save-frame timeline).
#[tauri::command]
async fn frame_at(t: f64) -> Result<String, String> {
    let (paths, _) = EDITOR.lock().unwrap_or_else(|e| e.into_inner()).clone().ok_or("no file open")?;
    let png = runner::frame_png(Path::new(&paths[0]), t, SCRUB_MAX).map_err(|e| e.to_string())?;
    Ok(format!("data:image/png;base64,{}", b64::encode(&png)))
}

/// Faces in the editor's preview, in preview pixels (redact → "Detect faces").
#[tauri::command]
async fn detect_faces() -> Result<Vec<Box4>, String> {
    let png = EDITOR_PREVIEW.lock().unwrap_or_else(|e| e.into_inner()).clone();
    if png.is_empty() {
        return Err("no preview".into());
    }
    let boxes = runner::detect_faces_png(&png).map_err(|e| e.to_string())?;
    Ok(boxes.into_iter().map(|r| Box4 { x: r.x, y: r.y, w: r.w, h: r.h }).collect())
}

/// Composites the annotation layer (transparent PNG at preview size) onto the original at
/// full resolution, as a normal job.
#[tauri::command]
fn save_annotation(app: AppHandle, src: String, data: String, width: u32, height: u32) -> Result<(), String> {
    let png = b64::decode(&data).ok_or("bad image data")?;
    if !png.starts_with(b"\x89PNG") {
        return Err("not a PNG".into());
    }
    if width == 0 || height == 0 {
        return Err("unknown image size".into());
    }
    let overlay = std::env::temp_dir().join(format!(
        "zeidex-overlay-{}-{}.png",
        std::process::id(),
        NEXT_JOB.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::write(&overlay, png).map_err(|e| e.to_string())?;
    let op = Op::Overlay { png: overlay.clone(), width, height };
    spawn_job(app, op, vec![PathBuf::from(src)], "annotate".into(), Some(overlay));
    Ok(())
}

#[tauri::command]
fn close_editor(app: AppHandle) {
    *EDITOR.lock().unwrap_or_else(|e| e.into_inner()) = None;
    EDITOR_PREVIEW.lock().unwrap_or_else(|e| e.into_inner()).clear();
    if let Some(w) = app.get_webview_window("editor") {
        let _ = w.hide();
    }
}

#[derive(Clone, Serialize, Deserialize)]
struct Recipe {
    name: String,
    steps: String,
}

/// Process files arriving in `dir` with `steps` (one step or a recipe).
#[derive(Clone, Serialize, Deserialize)]
struct WatchRule {
    dir: String,
    #[serde(default)]
    exts: String,
    steps: String,
    #[serde(default = "yes")]
    enabled: bool,
}

fn yes() -> bool {
    true
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
struct Settings {
    convert: Trigger,
    tools: Trigger,
    actions: Trigger,
    reveal: bool,
    /// Ctrl+Shift+Space opens the wheel on the current selection.
    hotkey: bool,
    recipes: Vec<Recipe>,
    watch: Vec<WatchRule>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            convert: Trigger::Shift,
            tools: Trigger::Control,
            actions: Trigger::Alt,
            reveal: true,
            hotkey: true,
            recipes: vec![Recipe {
                name: "Web image".into(),
                steps: "resize max=1600 | compress percent=60 | strip".into(),
            }],
            watch: vec![],
        }
    }
}

/// Bumped on every settings save; watcher threads of an older generation stop.
static WATCH_GEN: AtomicU64 = AtomicU64::new(0);
/// Seconds between folder polls.
const WATCH_POLL: std::time::Duration = std::time::Duration::from_secs(2);

fn start_watchers(app: &AppHandle, rules: &[WatchRule]) {
    let generation = WATCH_GEN.fetch_add(1, Ordering::SeqCst) + 1;
    for rule in rules.iter().filter(|r| r.enabled) {
        let Ok(op) = ops::parse_step(&rule.steps) else { continue };
        let dir = PathBuf::from(&rule.dir);
        let exts: Vec<String> = rule
            .exts
            .split(',')
            .map(|e| e.trim().trim_start_matches('.').to_ascii_lowercase())
            .filter(|e| !e.is_empty())
            .collect();
        let app = app.clone();
        let title = format!("watch: {}", file_name(&dir));
        std::thread::spawn(move || {
            let mut w = zeidex_core::watch::Watcher::new(&dir, &exts);
            while WATCH_GEN.load(Ordering::SeqCst) == generation {
                for file in w.poll() {
                    let job = NEXT_JOB.fetch_add(1, Ordering::Relaxed);
                    for out in runner::run(&op, &[file], |e| {
                        let _ = app.emit("job", job_event(job, &title, e));
                    })
                    .into_iter()
                    .flatten()
                    {
                        w.mark_done(&out);
                    }
                }
                std::thread::sleep(WATCH_POLL);
            }
        });
    }
}

fn settings_path(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_config_dir().ok().map(|d| d.join("settings.json"))
}

fn apply(s: &Settings) {
    hook::set_keys([s.convert, s.tools, s.actions]);
    REVEAL.store(s.reveal, Ordering::Relaxed);
    hook::set_hotkey(s.hotkey);
}

#[tauri::command]
fn get_settings(app: AppHandle) -> Settings {
    settings_path(&app)
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

#[tauri::command]
fn set_settings(app: AppHandle, settings: Settings) -> Result<(), String> {
    let keys = [settings.convert, settings.tools, settings.actions];
    if keys[0] == keys[1] || keys[1] == keys[2] || keys[0] == keys[2] {
        return Err("each wheel needs its own key".into());
    }
    for r in &settings.recipes {
        if r.name.trim().is_empty() {
            return Err("every recipe needs a name".into());
        }
        ops::parse_recipe(&r.steps).map_err(|e| format!("{}: {e}", r.name))?;
    }
    for w in &settings.watch {
        if !Path::new(&w.dir).is_dir() {
            return Err(format!("{} is not a folder", w.dir));
        }
        ops::parse_step(&w.steps).map_err(|e| format!("{}: {e}", w.dir))?;
    }
    let path = settings_path(&app).ok_or("no config directory")?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(&path, serde_json::to_vec_pretty(&settings).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    apply(&settings);
    start_watchers(&app, &settings.watch);
    Ok(())
}

#[tauri::command]
async fn pick_folder(app: AppHandle) -> Option<String> {
    app.dialog()
        .file()
        .blocking_pick_folder()
        .and_then(|f| f.into_path().ok())
        .map(|p| p.to_string_lossy().into_owned())
}

#[tauri::command]
fn hide_wheel(app: AppHandle) {
    hook::hide(&app);
}

/// The dropped tool needs options: keep the wheel open and let it take keyboard focus.
#[tauri::command]
fn keep_wheel(app: AppHandle) {
    hook::keep();
    if let Some(w) = app.get_webview_window("wheel") {
        let _ = w.set_focus();
    }
}

/// Files dragged onto the orb: open the wheel around it so the drag can continue onto a segment.
#[tauri::command]
fn wheel_at_shelf(app: AppHandle) {
    let Some(shelf) = app.get_webview_window("shelf") else { return };
    let (Ok(pos), Ok(size)) = (shelf.outer_position(), shelf.outer_size()) else { return };
    let (cx, cy) = (pos.x + size.width as i32 / 2, pos.y + size.height as i32 / 2);
    hook::show_at(&app, "convert", (cx, cy));
    if let Some(w) = app.get_webview_window("wheel") {
        hook::center(&w, cx, cy);
    }
}

#[tauri::command]
fn reveal(path: String) -> Result<(), String> {
    actions::reveal(Path::new(&path)).map_err(|e| e.to_string())
}

#[tauri::command]
fn doctor() -> Vec<ToolStatus> {
    tools::Tool::ALL
        .iter()
        .map(|t| ToolStatus {
            name: t.label(),
            path: tools::locate(*t).map(|p| p.to_string_lossy().into_owned()),
            hint: t.install_hint(),
        })
        .collect()
}

#[tauri::command]
fn hook_ok() -> bool {
    hook::ok()
}

#[tauri::command]
fn integrate_shell() -> Result<Vec<String>, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    integrate::install(&exe)
        .map(|v| v.into_iter().map(|p| p.to_string_lossy().into_owned()).collect())
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn open_url(url: String) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd").args(["/c", "start", "", &url]).spawn().map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open").arg(&url).spawn().map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open").arg(&url).spawn().map_err(|e| e.to_string())?;
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        let _ = url;
    }
    Ok(())
}

#[tauri::command]
fn download_dragged_url(url: String) -> Result<String, String> {
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err("invalid URL".into());
    }
    let file_name = url.rsplit('/').next().unwrap_or("download").split('?').next().unwrap_or("download");
    let ext = file_name.rsplit_once('.').map(|(_, e)| e).unwrap_or("png");
    let tmp_dir = std::env::temp_dir().join("zeidex_browser_drag");
    let _ = std::fs::create_dir_all(&tmp_dir);
    let target = tmp_dir.join(format!(
        "dragged_{}.{ext}",
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0)
    ));
    let status = std::process::Command::new("curl")
        .args(["-fsSL", &url, "-o"])
        .arg(&target)
        .status()
        .map_err(|e| format!("curl execution failed: {e}"))?;
    if !status.success() {
        return Err("download failed".into());
    }
    Ok(target.to_string_lossy().into_owned())
}

#[tauri::command]
async fn pick_files(app: AppHandle) -> Vec<String> {
    app.dialog()
        .file()
        .blocking_pick_files()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|f| f.into_path().ok())
        .map(|p| p.to_string_lossy().into_owned())
        .collect()
}

#[tauri::command]
fn take_pending() -> Vec<String> {
    std::mem::take(&mut *PENDING.lock().unwrap_or_else(|e| e.into_inner()))
}

/// Files dropped on the orb itself (not on the wheel): open them in the home window.
#[tauri::command]
fn open_in_home(app: AppHandle, paths: Vec<String>) {
    PENDING.lock().unwrap_or_else(|e| e.into_inner()).extend(paths);
    hook::hide(&app);
    let _ = app.emit("open-files", ());
    show(&app, "home");
}

#[tauri::command]
fn show_home(app: AppHandle) {
    show(&app, "home");
}

fn show(app: &AppHandle, label: &str) {
    if let Some(w) = app.get_webview_window(label) {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

/// Ctrl+Shift+Space: open the wheel for the file manager's selection (Explorer on Windows)
/// or for files copied to the clipboard (Ctrl+C in Nautilus, Dolphin, Nemo...).
fn open_selection(app: &AppHandle) {
    let mut paths = zeidex_core::selection::explorer_selection();
    if paths.is_empty() {
        if let Ok(text) = app.clipboard().read_text() {
            paths = zeidex_core::selection::parse_uri_list(&text);
        }
    }
    if paths.is_empty() {
        let _ = app.emit("selection-empty", ());
        return;
    }
    hook::show_at(app, "convert", (0, 0));
    hook::keep();
    if let Some(w) = app.get_webview_window("wheel") {
        let _ = w.set_focus();
    }
    let _ = app.emit("wheel-open", paths.iter().map(|p| p.to_string_lossy().into_owned()).collect::<Vec<_>>());
}

/// Handles `--open <files...>`; relative names (Nautilus scripts pass them) resolve against `cwd`.
fn open_args(app: &AppHandle, args: &[String], cwd: &Path) {
    let files: Vec<String> = args
        .iter()
        .skip_while(|a| a.as_str() != "--open")
        .skip(1)
        .map(|a| cwd.join(a).to_string_lossy().into_owned())
        .collect();
    if !files.is_empty() {
        PENDING.lock().unwrap_or_else(|e| e.into_inner()).extend(files);
        let _ = app.emit("open-files", ());
    }
    show(app, "home");
}

pub fn run() {
    tauri::Builder::default()
        // Must be registered first so a second launch forwards its files and exits.
        .plugin(tauri_plugin_single_instance::init(|app, args, cwd| open_args(app, &args, Path::new(&cwd))))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_drag::init())
        .invoke_handler(tauri::generate_handler![
            describe,
            run_job,
            hide_wheel,
            keep_wheel,
            wheel_at_shelf,
            reveal,
            doctor,
            hook_ok,
            integrate_shell,
            pick_files,
            take_pending,
            open_in_home,
            show_home,
            open_editor,
            editor_state,
            save_annotation,
            close_editor,
            get_settings,
            set_settings,
            detect_text,
            float_add,
            float_list,
            float_clear,
            drag_icon,
            pick_folder,
            frame_at,
            detect_faces,
            open_url,
            download_dragged_url,
            localdrop_cancel
        ])
        .on_window_event(|window, event| {
            // Closing a window keeps the app alive in the tray, like a menu-bar app.
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .setup(|app| {
            if let Ok(res) = app.path().resource_dir() {
                tools::set_bundle_dirs(vec![res.join("engines")]);
            }
            let handle = app.handle().clone();
            let settings = get_settings(handle.clone());
            apply(&settings);
            start_watchers(&handle, &settings.watch);

            let menu = Menu::with_items(
                app,
                &[
                    &MenuItem::with_id(app, "home", "Open Zeidex", true, None::<&str>)?,
                    &MenuItem::with_id(app, "shelf", "Show / hide drop orb", true, None::<&str>)?,
                    &MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?,
                ],
            )?;
            let mut tray = TrayIconBuilder::new().tooltip("Zeidex Convert").menu(&menu).on_menu_event(|app, event| {
                match event.id.as_ref() {
                    "home" => show(app, "home"),
                    "shelf" => {
                        if let Some(w) = app.get_webview_window("shelf") {
                            let _ = if w.is_visible().unwrap_or(false) { w.hide() } else { w.show() };
                        }
                    }
                    "quit" => app.exit(0),
                    _ => {}
                }
            });
            if let Some(icon) = app.default_window_icon() {
                tray = tray.icon(icon.clone());
            }
            tray.build(app)?;

            // Park the orb near the bottom-right corner of the primary screen.
            if let Some(shelf) = app.get_webview_window("shelf") {
                if let Ok(Some(m)) = shelf.primary_monitor() {
                    let (p, s) = (m.position(), m.size());
                    let _ = shelf
                        .set_position(PhysicalPosition::new(p.x + s.width as i32 - 160, p.y + s.height as i32 - 220));
                }
            }

            // macOS: the same rule through AppKit. `focusable(false)` makes tao's NSWindow answer
            // NO to canBecomeKeyWindow/canBecomeMainWindow (what an NSPanel with
            // .nonactivatingPanel gives), so the wheel never steals focus from Finder mid-drag; and
            // the overlays follow the user across Spaces and full-screen apps.
            #[cfg(target_os = "macos")]
            for label in ["wheel", "shelf"] {
                if let Some(w) = app.get_webview_window(label) {
                    let _ = w.set_focusable(false);
                    let _ = w.set_visible_on_all_workspaces(true);
                }
            }

            // Windows: overlays must never take focus or show up in Alt+Tab (`WS_EX_NOACTIVATE`
            // and `WS_EX_TOOLWINDOW`), and must stay above every normal window.
            #[cfg(windows)]
            for label in ["wheel", "shelf"] {
                if let Some(w) = app.get_webview_window(label) {
                    if let Ok(hwnd) = w.hwnd() {
                        win_drag_hook::apply_overlay_styles(hwnd.0 as isize);
                    }
                }
            }

            // Windows: the studio window gets the best backdrop this build supports (Mica,
            // then Acrylic, then blur). The config asks for Mica at creation; this is the
            // ladder for the builds where that silently fails.
            #[cfg(windows)]
            if let Some(home) = app.get_webview_window("home") {
                win_studio::apply_backdrop(&home);
            }

            hook::start(handle.clone(), open_selection);
            let args: Vec<String> = std::env::args().collect();
            let cwd = std::env::current_dir().unwrap_or_default();
            open_args(&handle, &args, &cwd);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Zeidex Convert");
}
