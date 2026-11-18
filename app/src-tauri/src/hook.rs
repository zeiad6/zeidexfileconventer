//! Global "tap a key while dragging" detection (Tangerine's Shift wheel).
//!
//! Windows uses a 60 Hz `GetAsyncKeyState` poll (`win_drag_hook`): no low-level mouse hook for
//! antivirus to flag, and it works even when the app started after the drag began. Other X11
//! systems use `rdev`, which sees raw mouse and keyboard events system-wide. Wayland blocks
//! global input hooks by design; there `ok()` turns false and the UI points users to the orb.

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, WebviewWindow};

pub const WHEEL_LOGICAL_PX: u32 = 380;
/// Grace period after Shift (or trigger key) is released before mouse-up so dropping a file
/// while lifting Shift does not abort the native OS drop.
pub const SHIFT_RELEASE_GRACE: Duration = Duration::from_millis(350);

/// Modifier that opens a wheel mode (Settings lets users remap them, as in Tangerine).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Trigger {
    Shift,
    Alt,
    Control,
    Meta,
}

#[cfg(not(windows))]
impl Trigger {
    fn of(key: rdev::Key) -> Option<Trigger> {
        Some(match key {
            rdev::Key::ShiftLeft | rdev::Key::ShiftRight => Trigger::Shift,
            rdev::Key::Alt | rdev::Key::AltGr => Trigger::Alt,
            rdev::Key::ControlLeft | rdev::Key::ControlRight => Trigger::Control,
            rdev::Key::MetaLeft | rdev::Key::MetaRight => Trigger::Meta,
            _ => return None,
        })
    }
}

/// Wheel modes, in ring order: convert / tools / actions.
pub(crate) const MODES: [&str; 3] = ["convert", "tools", "actions"];

/// Keys for convert, tools, actions (Settings can remap them).
pub(crate) static KEYS: Mutex<[Trigger; 3]> = Mutex::new([Trigger::Shift, Trigger::Control, Trigger::Alt]);

pub fn set_keys(keys: [Trigger; 3]) {
    *KEYS.lock().unwrap_or_else(|e| e.into_inner()) = keys;
}

/// Which mode a set of held modifiers selects, honouring the remapped keys.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn mode_for(
    keys: &[Trigger; 3],
    shift: Option<Trigger>,
    alt: Option<Trigger>,
    ctrl: Option<Trigger>,
    meta: Option<Trigger>,
) -> Option<&'static str> {
    for t in [shift, ctrl, alt, meta].into_iter().flatten() {
        if let Some(i) = keys.iter().position(|k| *k == t) {
            return Some(MODES[i]);
        }
    }
    None
}

static OK: AtomicBool = AtomicBool::new(true);
/// Ctrl+Shift+Space shortcut enabled (Settings).
static HOTKEY: AtomicBool = AtomicBool::new(true);
static VISIBLE: AtomicBool = AtomicBool::new(false);
/// Set when the wheel must survive the mouse release (a tool needs its options typed in, or
/// micro-progress/toast is displayed after a drop).
static KEEP: AtomicBool = AtomicBool::new(false);
static CLICK_MODE: AtomicBool = AtomicBool::new(false);
static DROPPED_AT: Mutex<Option<Instant>> = Mutex::new(None);
static SHIFT_UP_AT: Mutex<Option<Instant>> = Mutex::new(None);
static NATIVE_DRAG_PATHS: Mutex<Vec<String>> = Mutex::new(Vec::new());
static WHEEL_FILES: Mutex<Vec<String>> = Mutex::new(Vec::new());
static CURRENT_MODE: Mutex<String> = Mutex::new(String::new());

#[derive(Clone, serde::Serialize)]
pub(crate) struct HoverPoint {
    pub x: i32,
    pub y: i32,
}

#[derive(Clone, serde::Serialize)]
pub(crate) struct CommitPayload {
    pub position: HoverPoint,
    pub paths: Vec<String>,
}

pub fn set_native_drag_paths(paths: &[std::path::PathBuf]) {
    let next: Vec<String> = paths.iter().map(|p| p.to_string_lossy().into_owned()).collect();
    if !next.is_empty() {
        *NATIVE_DRAG_PATHS.lock().unwrap_or_else(|e| e.into_inner()) = next;
    }
}

pub fn set_native_drag_path_strings(paths: Vec<String>) {
    if !paths.is_empty() {
        *NATIVE_DRAG_PATHS.lock().unwrap_or_else(|e| e.into_inner()) = paths;
    }
}

pub fn native_drag_paths() -> Vec<String> {
    NATIVE_DRAG_PATHS.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

pub fn clear_native_drag_paths() {
    NATIVE_DRAG_PATHS.lock().unwrap_or_else(|e| e.into_inner()).clear();
}

pub fn set_wheel_files(paths: Vec<String>) {
    if !paths.is_empty() {
        *WHEEL_FILES.lock().unwrap_or_else(|e| e.into_inner()) = paths;
    }
}

pub fn wheel_files() -> Vec<String> {
    WHEEL_FILES.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

pub fn clear_wheel_files() {
    WHEEL_FILES.lock().unwrap_or_else(|e| e.into_inner()).clear();
}

pub fn current_mode() -> String {
    let m = CURRENT_MODE.lock().unwrap_or_else(|e| e.into_inner()).clone();
    if m.is_empty() {
        "convert".into()
    } else {
        m
    }
}

pub fn set_click_mode(on: bool) {
    CLICK_MODE.store(on, Ordering::Relaxed);
}

pub fn click_mode() -> bool {
    CLICK_MODE.load(Ordering::Relaxed)
}

pub fn note_drop() {
    *DROPPED_AT.lock().unwrap_or_else(|e| e.into_inner()) = Some(Instant::now());
}

pub fn in_drop() -> bool {
    DROPPED_AT.lock().unwrap_or_else(|e| e.into_inner()).is_some_and(|t| t.elapsed() < Duration::from_millis(1200))
}

pub fn note_shift_release() {
    *SHIFT_UP_AT.lock().unwrap_or_else(|e| e.into_inner()) = Some(Instant::now());
}

#[cfg_attr(not(windows), allow(dead_code))]
pub fn in_shift_grace() -> bool {
    SHIFT_UP_AT.lock().unwrap_or_else(|e| e.into_inner()).is_some_and(|t| t.elapsed() < SHIFT_RELEASE_GRACE)
}

pub fn set_hotkey(on: bool) {
    HOTKEY.store(on, Ordering::Relaxed);
}

#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn hotkey_enabled() -> bool {
    HOTKEY.load(Ordering::Relaxed)
}

pub fn ok() -> bool {
    OK.load(Ordering::Relaxed)
}

pub fn keep() {
    KEEP.store(true, Ordering::Relaxed);
}

#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn kept() -> bool {
    KEEP.load(Ordering::Relaxed)
}

#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn visible() -> bool {
    VISIBLE.load(Ordering::Relaxed)
}

pub(crate) fn wheel_local_coords(app: &AppHandle, x: i32, y: i32) -> Option<(i32, i32)> {
    let w = app.get_webview_window("wheel")?;
    let pos = w.outer_position().ok()?;
    let size = w.outer_size().ok()?;
    if x >= pos.x && y >= pos.y && x < pos.x + size.width as i32 && y < pos.y + size.height as i32 {
        Some((x - pos.x, y - pos.y))
    } else {
        None
    }
}

/// Time the webview gets to handle the drop before a release elsewhere hides the wheel.
#[cfg(not(windows))]
const RELEASE_GRACE: Duration = Duration::from_millis(700);
/// Pointer travel (px) before a held button counts as a drag.
#[cfg(not(windows))]
const DRAG_THRESHOLD: f64 = 12.0;

/// `on_hotkey` runs (on its own thread) when Ctrl+Shift+Space is pressed outside a drag: the
/// keyboard way to open the wheel for files selected in the file manager.
pub fn start(app: AppHandle, on_hotkey: fn(&AppHandle)) {
    #[cfg(windows)]
    {
        // The keyboard shortcut is registered as a Tauri global shortcut on Windows, so the
        // poll loop never needs the callback; keep the parameter for a common signature.
        crate::win_drag_hook::start(app, on_hotkey);
    }
    #[cfg(not(windows))]
    start_rdev(app, on_hotkey);
}

#[cfg(not(windows))]
fn start_rdev(app: AppHandle, on_hotkey: fn(&AppHandle)) {
    std::thread::spawn(move || {
        let handle = app.clone();
        let (mut down, mut dragging) = (false, false);
        let (mut ctrl, mut shift) = (false, false);
        let (mut origin, mut last) = ((0.0f64, 0.0f64), (0.0f64, 0.0f64));
        let mut last_hover = Instant::now();
        let mut last_alive = Instant::now();
        let result = rdev::listen(move |ev| {
            use rdev::{Button, EventType, Key};
            match ev.event_type {
                EventType::KeyPress(Key::ControlLeft | Key::ControlRight) => ctrl = true,
                EventType::KeyRelease(Key::ControlLeft | Key::ControlRight) => ctrl = false,
                EventType::KeyPress(Key::ShiftLeft | Key::ShiftRight) => shift = true,
                EventType::KeyRelease(Key::ShiftLeft | Key::ShiftRight) => {
                    shift = false;
                    if dragging && VISIBLE.load(Ordering::Relaxed) {
                        note_shift_release();
                    }
                }
                EventType::KeyPress(Key::Space) if ctrl && shift && !dragging && HOTKEY.load(Ordering::Relaxed) => {
                    let app = handle.clone();
                    std::thread::spawn(move || on_hotkey(&app));
                }
                _ => {}
            }
            match ev.event_type {
                EventType::MouseMove { x, y } => {
                    last = (x, y);
                    if down && !dragging && (x - origin.0).hypot(y - origin.1) > DRAG_THRESHOLD {
                        dragging = true;
                    }
                    if dragging && VISIBLE.load(Ordering::Relaxed) && !KEEP.load(Ordering::Relaxed) {
                        if last_alive.elapsed() >= Duration::from_millis(480) {
                            last_alive = Instant::now();
                            let _ = handle.emit("wheel-alive", ());
                        }
                        if last_hover.elapsed() >= Duration::from_millis(16) {
                            last_hover = Instant::now();
                            if let Some(w) = handle.get_webview_window("wheel") {
                                if let Ok(cp) = w.cursor_position() {
                                    if let Some((lx, ly)) = wheel_local_coords(&handle, cp.x as i32, cp.y as i32) {
                                        let _ = handle.emit("wheel-hover", HoverPoint { x: lx, y: ly });
                                    }
                                }
                            }
                        }
                    }
                }
                EventType::ButtonPress(Button::Left) => {
                    down = true;
                    dragging = false;
                    origin = last;
                }
                EventType::ButtonRelease(Button::Left) => {
                    down = false;
                    if std::mem::take(&mut dragging) && VISIBLE.load(Ordering::Relaxed) {
                        let extra = if in_shift_grace() { SHIFT_RELEASE_GRACE } else { Duration::ZERO };
                        let local = handle
                            .get_webview_window("wheel")
                            .and_then(|w| w.cursor_position().ok())
                            .and_then(|cp| wheel_local_coords(&handle, cp.x as i32, cp.y as i32));
                        let app = handle.clone();
                        std::thread::spawn(move || {
                            if let Some((lx, ly)) = local {
                                std::thread::sleep(Duration::from_millis(180));
                                if !in_drop() && visible() && !kept() {
                                    let paths = native_drag_paths();
                                    let _ = app.emit(
                                        "wheel-commit",
                                        CommitPayload { position: HoverPoint { x: lx, y: ly }, paths },
                                    );
                                }
                                std::thread::sleep(Duration::from_millis(520));
                                if !kept() && !in_drop() && visible() {
                                    hide(&app);
                                }
                                return;
                            }
                            std::thread::sleep(RELEASE_GRACE + extra);
                            if !KEEP.load(Ordering::Relaxed) && !in_drop() {
                                hide(&app);
                            }
                        });
                    }
                }
                EventType::KeyPress(key) if dragging => {
                    let Some(t) = Trigger::of(key) else { return };
                    let keys = *KEYS.lock().unwrap_or_else(|e| e.into_inner());
                    let Some(i) = keys.iter().position(|k| *k == t) else { return };
                    let mode = MODES[i];
                    show_at(&handle, mode, (last.0 as i32, last.1 as i32));
                    let app = handle.clone();
                    std::thread::spawn(move || {
                        let files = crate::drag_probe_paths();
                        if !files.is_empty() && visible() && native_drag_paths().is_empty() {
                            set_wheel_files(files.clone());
                            let _ = app.emit("wheel-files", files);
                        }
                    });
                }
                _ => {}
            }
        });
        if let Err(e) = result {
            OK.store(false, Ordering::Relaxed);
            // No global keys: the orb is the only way into the wheel, so it comes back even when
            // the user left it off. The UI explains the situation with the same event.
            if let Some(shelf) = app.get_webview_window("shelf") {
                let _ = shelf.show();
            }
            let _ = app.emit("hook-error", format!("{e:?}"));
        }
    });
}

/// Shows the wheel centred on the cursor (first call) and switches it to `mode`.
pub fn show_at(app: &AppHandle, mode: &str, fallback: (i32, i32)) {
    let pos = app
        .get_webview_window("wheel")
        .and_then(|w| w.cursor_position().ok())
        .map(|p| (p.x as i32, p.y as i32))
        .unwrap_or(fallback);
    show_at_center(app, mode, pos);
}

/// Shows the wheel centred at the given physical screen coordinates `(x, y)` and switches it to `mode`.
pub fn show_at_center(app: &AppHandle, mode: &str, at: (i32, i32)) {
    let Some(w) = app.get_webview_window("wheel") else { return };
    if !VISIBLE.swap(true, Ordering::Relaxed) {
        KEEP.store(false, Ordering::Relaxed);
        CLICK_MODE.store(false, Ordering::Relaxed);
        clear_native_drag_paths();
        clear_wheel_files();
        *DROPPED_AT.lock().unwrap_or_else(|e| e.into_inner()) = None;
        *SHIFT_UP_AT.lock().unwrap_or_else(|e| e.into_inner()) = None;
        let _ = w.set_ignore_cursor_events(false);
        #[cfg(target_os = "macos")]
        let _ = w.set_focusable(false);
        #[cfg(windows)]
        if let Ok(hwnd) = w.hwnd() {
            crate::win_drag_hook::apply_overlay_styles(hwnd.0 as isize);
        }
        center(&w, at.0, at.1);
        let _ = w.show();
    }
    *CURRENT_MODE.lock().unwrap_or_else(|e| e.into_inner()) = mode.to_string();
    let _ = app.emit("wheel-mode", mode);
}

/// Pure screen-edge clamping helper for positioning the Wheel inside a target monitor's
/// physical rectangle (works across mixed-DPI multi-monitor setups).
#[allow(clippy::too_many_arguments)]
pub fn clamp_wheel_position(
    cursor_x: i32,
    cursor_y: i32,
    win_w: i32,
    win_h: i32,
    mon_x: i32,
    mon_y: i32,
    mon_w: i32,
    mon_h: i32,
) -> (i32, i32) {
    let raw_x = cursor_x - win_w / 2;
    let raw_y = cursor_y - win_h / 2;
    let max_x = (mon_x + mon_w - win_w).max(mon_x);
    let max_y = (mon_y + mon_h - win_h).max(mon_y);
    (raw_x.clamp(mon_x, max_x), raw_y.clamp(mon_y, max_y))
}

pub fn center(w: &WebviewWindow, x: i32, y: i32) {
    let monitors = w.available_monitors().unwrap_or_default();
    let target_mon = monitors
        .into_iter()
        .find(|m| {
            let p = m.position();
            let s = m.size();
            x >= p.x && y >= p.y && x < p.x + s.width as i32 && y < p.y + s.height as i32
        })
        .or_else(|| w.current_monitor().ok().flatten())
        .or_else(|| w.primary_monitor().ok().flatten());

    if let Some(m) = target_mon {
        let scale = m.scale_factor();
        let expected_px = (f64::from(WHEEL_LOGICAL_PX) * scale).round() as i32;
        let outer = w.outer_size().unwrap_or(PhysicalSize::new(expected_px as u32, expected_px as u32));
        let win_w = if outer.width > 0 { (outer.width as i32).max(expected_px) } else { expected_px };
        let win_h = if outer.height > 0 { (outer.height as i32).max(expected_px) } else { expected_px };
        let mp = m.position();
        let ms = m.size();
        let (cx, cy) = clamp_wheel_position(x, y, win_w, win_h, mp.x, mp.y, ms.width as i32, ms.height as i32);
        let _ = w.set_position(PhysicalPosition::new(cx, cy));
    } else {
        let s = w.outer_size().unwrap_or(PhysicalSize::new(WHEEL_LOGICAL_PX, WHEEL_LOGICAL_PX));
        let _ = w.set_position(PhysicalPosition::new(x - s.width as i32 / 2, y - s.height as i32 / 2));
    }
}

pub fn hide(app: &AppHandle) {
    VISIBLE.store(false, Ordering::Relaxed);
    KEEP.store(false, Ordering::Relaxed);
    CLICK_MODE.store(false, Ordering::Relaxed);
    clear_native_drag_paths();
    clear_wheel_files();
    CURRENT_MODE.lock().unwrap_or_else(|e| e.into_inner()).clear();
    if let Some(w) = app.get_webview_window("wheel") {
        #[cfg(target_os = "macos")]
        let _ = w.set_focusable(false);
        #[cfg(windows)]
        if let Ok(hwnd) = w.hwnd() {
            crate::win_drag_hook::apply_overlay_styles(hwnd.0 as isize);
        }
        let _ = w.hide();
    }
    let _ = app.emit("wheel-hidden", ());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamps_wheel_across_mixed_dpi_monitors() {
        // Primary 1920x1080 @ 100% (380x380 wheel): cursor at top-left corner (10, 10)
        assert_eq!(clamp_wheel_position(10, 10, 380, 380, 0, 0, 1920, 1080), (0, 0));
        // Primary bottom-right corner (1910, 1070)
        assert_eq!(clamp_wheel_position(1910, 1070, 380, 380, 0, 0, 1920, 1080), (1540, 700));
        // Secondary 2560x1440 @ 150% DPI (570x570 physical px) positioned to the left (-2560..0)
        assert_eq!(clamp_wheel_position(-2550, 50, 570, 570, -2560, 0, 2560, 1440), (-2560, 0));
        assert_eq!(clamp_wheel_position(-20, 1420, 570, 570, -2560, 0, 2560, 1440), (-570, 870));
        // Center of secondary monitor
        assert_eq!(clamp_wheel_position(-1280, 720, 570, 570, -2560, 0, 2560, 1440), (-1565, 435));
    }
}
