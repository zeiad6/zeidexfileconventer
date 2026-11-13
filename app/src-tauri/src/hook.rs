//! Global "tap a key while dragging" detection (Tangerine's Shift wheel).
//!
//! Windows uses a 60 Hz `GetAsyncKeyState` poll (`win_drag_hook`): no low-level mouse hook for
//! antivirus to flag, and it works even when the app started after the drag began. Other X11
//! systems use `rdev`, which sees raw mouse and keyboard events system-wide. Wayland blocks
//! global input hooks by design; there `ok()` turns false and the UI points users to the orb.

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, WebviewWindow};

#[cfg(not(windows))]
use std::time::Duration;

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
/// Set when the wheel must survive the mouse release (a tool needs its options typed in).
static KEEP: AtomicBool = AtomicBool::new(false);

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

#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn set_visible(on: bool) {
    VISIBLE.store(on, Ordering::Relaxed);
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
        let result = rdev::listen(move |ev| {
            use rdev::{Button, EventType, Key};
            match ev.event_type {
                EventType::KeyPress(Key::ControlLeft | Key::ControlRight) => ctrl = true,
                EventType::KeyRelease(Key::ControlLeft | Key::ControlRight) => ctrl = false,
                EventType::KeyPress(Key::ShiftLeft | Key::ShiftRight) => shift = true,
                EventType::KeyRelease(Key::ShiftLeft | Key::ShiftRight) => shift = false,
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
                }
                EventType::ButtonPress(Button::Left) => {
                    down = true;
                    dragging = false;
                    origin = last;
                }
                EventType::ButtonRelease(Button::Left) => {
                    down = false;
                    if std::mem::take(&mut dragging) && VISIBLE.load(Ordering::Relaxed) {
                        let app = handle.clone();
                        std::thread::spawn(move || {
                            std::thread::sleep(RELEASE_GRACE);
                            if !KEEP.load(Ordering::Relaxed) {
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
                }
                _ => {}
            }
        });
        if let Err(e) = result {
            OK.store(false, Ordering::Relaxed);
            let _ = app.emit("hook-error", format!("{e:?}"));
        }
    });
}

/// Shows the wheel centred on the cursor (first call) and switches it to `mode`.
pub fn show_at(app: &AppHandle, mode: &str, fallback: (i32, i32)) {
    let Some(w) = app.get_webview_window("wheel") else { return };
    if !VISIBLE.swap(true, Ordering::Relaxed) {
        KEEP.store(false, Ordering::Relaxed);
        let (x, y) = w.cursor_position().map(|p| (p.x as i32, p.y as i32)).unwrap_or(fallback);
        center(&w, x, y);
        let _ = w.show();
    }
    let _ = app.emit("wheel-mode", mode);
}

pub fn center(w: &WebviewWindow, x: i32, y: i32) {
    let s = w.outer_size().unwrap_or(PhysicalSize::new(480, 480));
    let _ = w.set_position(PhysicalPosition::new(x - s.width as i32 / 2, y - s.height as i32 / 2));
}

pub fn hide(app: &AppHandle) {
    VISIBLE.store(false, Ordering::Relaxed);
    KEEP.store(false, Ordering::Relaxed);
    if let Some(w) = app.get_webview_window("wheel") {
        let _ = w.hide();
    }
    let _ = app.emit("wheel-hidden", ());
}
