//! Global "tap a key while dragging" detection (Tangerine's Shift wheel).
//!
//! rdev sees raw mouse/keyboard events system-wide on Windows and X11. Wayland blocks global
//! input hooks by design; there `ok()` turns false and the UI points users to the drop orb.

use rdev::{listen, Button, EventType, Key};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, WebviewWindow};

/// Modifier that opens a wheel mode (Settings lets users remap them, as in Tangerine).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Trigger {
    Shift,
    Alt,
    Control,
    Meta,
}

impl Trigger {
    fn of(key: Key) -> Option<Trigger> {
        Some(match key {
            Key::ShiftLeft | Key::ShiftRight => Trigger::Shift,
            Key::Alt | Key::AltGr => Trigger::Alt,
            Key::ControlLeft | Key::ControlRight => Trigger::Control,
            Key::MetaLeft | Key::MetaRight => Trigger::Meta,
            _ => return None,
        })
    }
}

/// Keys for convert, tools, actions.
static KEYS: Mutex<[Trigger; 3]> = Mutex::new([Trigger::Shift, Trigger::Alt, Trigger::Control]);

pub fn set_keys(keys: [Trigger; 3]) {
    *KEYS.lock().unwrap_or_else(|e| e.into_inner()) = keys;
}

static OK: AtomicBool = AtomicBool::new(true);
/// Ctrl+Shift+Space shortcut enabled (Settings).
static HOTKEY: AtomicBool = AtomicBool::new(true);

pub fn set_hotkey(on: bool) {
    HOTKEY.store(on, Ordering::Relaxed);
}
static VISIBLE: AtomicBool = AtomicBool::new(false);
/// Set when the wheel must survive the mouse release (a tool needs its options typed in).
static KEEP: AtomicBool = AtomicBool::new(false);

/// Pointer travel (px) before a held button counts as a drag.
const DRAG_THRESHOLD: f64 = 12.0;
/// Time the webview gets to handle the drop before a release elsewhere hides the wheel.
const RELEASE_GRACE: Duration = Duration::from_millis(700);

pub fn ok() -> bool {
    OK.load(Ordering::Relaxed)
}

pub fn keep() {
    KEEP.store(true, Ordering::Relaxed);
}

/// `on_hotkey` runs (on its own thread) when Ctrl+Shift+Space is pressed outside a drag: the
/// keyboard way to open the wheel for files selected in the file manager.
pub fn start(app: AppHandle, on_hotkey: fn(&AppHandle)) {
    std::thread::spawn(move || {
        let handle = app.clone();
        let (mut down, mut dragging) = (false, false);
        let (mut ctrl, mut shift) = (false, false);
        let (mut origin, mut last) = ((0.0f64, 0.0f64), (0.0f64, 0.0f64));
        let result = listen(move |ev| {
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
                    let mode = ["convert", "tools", "actions"][i];
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
    let s = w.outer_size().unwrap_or(PhysicalSize::new(400, 400));
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
