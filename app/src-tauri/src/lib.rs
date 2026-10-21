//! Tauri shell: windows, tray, IPC commands. All file work lives in zeidex-core.

mod hook;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, WindowEvent};
use tauri_plugin_clipboard_manager::ClipboardExt;
use tauri_plugin_dialog::DialogExt;
use zeidex_core::ops::{self, Op};
use zeidex_core::runner::{self, Event};
use zeidex_core::{actions, b64, format, integrate, tools};
use hook::Trigger;

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
}

#[derive(Serialize)]
struct Described {
    category: Option<&'static str>,
    convert: Vec<Choice>,
    tools: Vec<Choice>,
    actions: Vec<Choice>,
}

const ACTIONS: [(&str, &str, &str); 4] = [
    ("reveal", "Show in folder", "إظهار في المجلد"),
    ("copy-path", "Copy path", "نسخ المسار"),
    ("move", "Move to…", "نقل إلى…"),
    ("terminal", "Terminal", "الطرفية"),
];

#[derive(Clone, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum JobEvent {
    Start { job: u64, index: usize, total: usize, file: String, title: String },
    Progress { job: u64, index: usize, fraction: f32 },
    Done { job: u64, index: usize, output: String },
    Failed { job: u64, index: usize, error: String },
    Text { job: u64, title: String, text: String },
}

#[derive(Serialize)]
struct ToolStatus {
    name: &'static str,
    path: Option<String>,
    hint: &'static str,
}

static NEXT_JOB: AtomicU64 = AtomicU64::new(1);
static REVEAL: AtomicBool = AtomicBool::new(true);
/// Files and tool the editor window was opened for.
static EDITOR: Mutex<Option<(Vec<String>, String)>> = Mutex::new(None);
/// Files handed over by `--open` before the home window was ready to receive them.
static PENDING: Mutex<Vec<String>> = Mutex::new(Vec::new());

#[tauri::command]
fn describe(paths: Vec<String>) -> Described {
    let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
    let cat = format::common_category(&paths);
    let convert = format::common_targets(&paths)
        .into_iter()
        .map(|t| Choice { kind: "convert", editor: false, id: t.into(), label: t.to_uppercase(), label_ar: t.to_uppercase(), params: vec![] })
        .collect();
    let tools = ops::tools_for(cat, paths.len())
        .into_iter()
        .map(|t| Choice {
            kind: "tool",
            editor: ops::EDITOR_TOOLS.contains(&t.id),
            id: t.id.into(),
            label: t.label.into(),
            label_ar: t.label_ar.into(),
            params: t
                .params
                .iter()
                .map(|p| ParamOut { key: p.key, label: p.label, label_ar: p.label_ar, default: p.default })
                .collect(),
        })
        .collect();
    let actions = ACTIONS
        .iter()
        .map(|(id, label, ar)| Choice { kind: "action", editor: false, id: (*id).into(), label: (*label).into(), label_ar: (*ar).into(), params: vec![] })
        .collect();
    Described { category: cat.map(|c| c.name()), convert, tools, actions }
}

fn file_name(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

#[tauri::command]
fn run_job(app: AppHandle, paths: Vec<String>, kind: String, id: String, opts: HashMap<String, String>) -> Result<(), String> {
    let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
    if paths.is_empty() {
        return Err("no files".into());
    }
    if kind == "action" {
        hook::hide(&app);
        return action(&app, &id, paths);
    }
    // Validate first: on bad options the wheel stays open so the form can show the error.
    let op = if kind == "convert" { Op::Convert { to: id.clone() } } else { Op::parse(&id, &opts).map_err(|e| e.to_string())? };
    hook::hide(&app);
    let job = NEXT_JOB.fetch_add(1, Ordering::Relaxed);
    show(&app, "jobs");
    std::thread::spawn(move || {
        if op == Op::Info {
            for p in &paths {
                let text = runner::info(p).unwrap_or_else(|e| e.to_string());
                let _ = app.emit("job", JobEvent::Text { job, title: file_name(p), text });
            }
            return;
        }
        let title = if kind == "convert" { format!("→ {}", id.to_uppercase()) } else { id.clone() };
        let mut index = 0;
        let results = runner::run(&op, &paths, |e| {
            let ev = match e {
                Event::Start { index: i, total, file } => {
                    index = i;
                    JobEvent::Start { job, index, total, file, title: title.clone() }
                }
                Event::Progress(fraction) => JobEvent::Progress { job, index, fraction },
                Event::Done(p) => JobEvent::Done { job, index, output: p.to_string_lossy().into_owned() },
                Event::Failed { error, .. } => JobEvent::Failed { job, index, error },
            };
            let _ = app.emit("job", ev);
        });
        if REVEAL.load(Ordering::Relaxed) {
            if let Some(Ok(last)) = results.iter().rev().find(|r| r.is_ok()) {
                let _ = actions::reveal(last);
            }
        }
    });
    Ok(())
}

fn action(app: &AppHandle, id: &str, paths: Vec<PathBuf>) -> Result<(), String> {
    match id {
        "reveal" => actions::reveal(&paths[0]).map_err(|e| e.to_string()),
        "terminal" => actions::open_terminal(&paths[0]).map_err(|e| e.to_string()),
        "copy-path" => app.clipboard().write_text(actions::paths_text(&paths)).map_err(|e| e.to_string()),
        "move" => {
            let app = app.clone();
            // The folder picker blocks; keep it off the IPC thread.
            std::thread::spawn(move || {
                let Some(dir) = app.dialog().file().blocking_pick_folder().and_then(|d| d.into_path().ok()) else { return };
                if let Err(e) = actions::move_to(&paths, &dir) {
                    let _ = app.emit("job", JobEvent::Text { job: 0, title: "Move".into(), text: e.to_string() });
                }
            });
            Ok(())
        }
        other => Err(format!("unknown action {other}")),
    }
}

#[derive(Serialize)]
struct EditorState {
    paths: Vec<String>,
    tool: String,
    video: bool,
    width: u32,
    height: u32,
    duration: f64,
    /// PNG data URL; at most `PREVIEW_MAX` px (annotations are saved at this size).
    preview: String,
}

const PREVIEW_MAX: u32 = 1600;
const ANNOTATE_MAX: u32 = 2560;

#[tauri::command]
fn open_editor(app: AppHandle, paths: Vec<String>, tool: String) {
    hook::hide(&app);
    *EDITOR.lock().unwrap_or_else(|e| e.into_inner()) = Some((paths, tool));
    show(&app, "editor");
    let _ = app.emit("editor-open", ());
}

/// Async: decoding the preview can take a moment for large HEIC/RAW-ish images.
#[tauri::command]
async fn editor_state() -> Result<EditorState, String> {
    let (paths, tool) = EDITOR.lock().unwrap_or_else(|e| e.into_inner()).clone().ok_or("no file open")?;
    let src = PathBuf::from(&paths[0]);
    let video = format::category_of(&src) == Some(format::Category::Video);
    let (width, height) = runner::dimensions(&src).ok_or("could not read the size")?;
    let duration = if video { runner::probe_media(&src).map_or(0.0, |m| m.duration) } else { 0.0 };
    let max = if tool == "annotate" { ANNOTATE_MAX } else { PREVIEW_MAX };
    let png = runner::preview_png(&src, max).map_err(|e| e.to_string())?;
    let preview = format!("data:image/png;base64,{}", b64::encode(&png));
    Ok(EditorState { paths, tool, video, width, height, duration, preview })
}

/// Saves the annotated canvas (PNG data URL) beside the original.
#[tauri::command]
fn save_annotation(src: String, data: String) -> Result<String, String> {
    let png = b64::decode(&data).ok_or("bad image data")?;
    if !png.starts_with(b"\x89PNG") {
        return Err("not a PNG".into());
    }
    let src = PathBuf::from(src);
    let name = format!("{}-annotated.png", format::stem_of(&src));
    let out = actions::save_beside(&src, &name, &png).map_err(|e| e.to_string())?;
    if REVEAL.load(Ordering::Relaxed) {
        let _ = actions::reveal(&out);
    }
    Ok(out.to_string_lossy().into_owned())
}

#[tauri::command]
fn close_editor(app: AppHandle) {
    *EDITOR.lock().unwrap_or_else(|e| e.into_inner()) = None;
    if let Some(w) = app.get_webview_window("editor") {
        let _ = w.hide();
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
struct Settings {
    convert: Trigger,
    tools: Trigger,
    actions: Trigger,
    reveal: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { convert: Trigger::Shift, tools: Trigger::Alt, actions: Trigger::Control, reveal: true }
    }
}

fn settings_path(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_config_dir().ok().map(|d| d.join("settings.json"))
}

fn apply(s: &Settings) {
    hook::set_keys([s.convert, s.tools, s.actions]);
    REVEAL.store(s.reveal, Ordering::Relaxed);
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
    let path = settings_path(&app).ok_or("no config directory")?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(&path, serde_json::to_vec_pretty(&settings).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    apply(&settings);
    Ok(())
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
        .map(|t| ToolStatus { name: t.label(), path: tools::locate(*t).map(|p| p.to_string_lossy().into_owned()), hint: t.install_hint() })
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
        .invoke_handler(tauri::generate_handler![
            describe, run_job, hide_wheel, keep_wheel, wheel_at_shelf, reveal, doctor, hook_ok, integrate_shell,
            pick_files, take_pending, open_in_home, show_home, open_editor, editor_state, save_annotation,
            close_editor, get_settings, set_settings
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
            apply(&get_settings(handle.clone()));

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
                    let _ = shelf.set_position(PhysicalPosition::new(p.x + s.width as i32 - 160, p.y + s.height as i32 - 220));
                }
            }

            hook::start(handle.clone());
            let args: Vec<String> = std::env::args().collect();
            let cwd = std::env::current_dir().unwrap_or_default();
            open_args(&handle, &args, &cwd);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Zeidex Convert");
}
