//! Windows drag detection without a low-level mouse hook.
//!
//! `rdev` installs a `WH_MOUSE_LL` hook, which antivirus products flag and which Windows
//! throttles under load. Instead this module polls the button state 60 times a second with
//! `GetAsyncKeyState(VK_LBUTTON)`, measures how far the pointer travelled since the press, and
//! treats the gesture as a drag once it passes the threshold. Modifier keys are read with the
//! same call, so a drag that starts before the app is even running still works.
//!
//! `CF_HDROP` is read from the clipboard as a best-effort probe of what is being dragged
//! (Explorer publishes it while a file drag is in flight). The authoritative file list still
//! comes from Tauri's OLE drop target on the wheel window, which is what actually receives the
//! drop; the probe only lets the wheel show the right segments before the pointer arrives.

use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
use windows_sys::Win32::Foundation::{HWND, POINT};
use windows_sys::Win32::System::DataExchange::{
    CloseClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, VK_CONTROL, VK_LBUTTON, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT, VK_SPACE,
};
use windows_sys::Win32::UI::Shell::{DragQueryFileW, HDROP};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, GetWindowLongPtrW, SetWindowLongPtrW, SetWindowPos, GWL_EXSTYLE, HWND_TOPMOST, SWP_NOACTIVATE,
    SWP_NOMOVE, SWP_NOSIZE, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
};

use crate::hook::{center, hide, mode_for, set_visible, visible, Trigger, KEYS};

/// Pointer travel (px) before a held left button counts as a drag.
const DRAG_THRESHOLD: f64 = 12.0;
/// 60 Hz: the same cadence the compositor uses, cheap enough to stay invisible in Task Manager.
const TICK: Duration = Duration::from_millis(16);
/// Time the webview gets to handle a drop before a release elsewhere hides the wheel.
const RELEASE_GRACE: Duration = Duration::from_millis(700);
/// `CF_HDROP`: the clipboard format Explorer publishes during a file drag.
const CF_HDROP: u32 = 15;

fn key_down(vk: u16) -> bool {
    // The high bit means "currently down"; the low bit is the since-last-call latch.
    unsafe { (GetAsyncKeyState(vk as i32) as u16) & 0x8000 != 0 }
}

fn pointer() -> POINT {
    let mut p = POINT { x: 0, y: 0 };
    unsafe { GetCursorPos(&mut p) };
    p
}

/// Euclidean distance in pixels between two pointer samples.
fn distance(a: POINT, b: POINT) -> f64 {
    let dx = f64::from(a.x - b.x);
    let dy = f64::from(a.y - b.y);
    dx.hypot(dy)
}

/// Files currently offered as `CF_HDROP`, if any. Best effort: the clipboard may be held by the
/// drag source, in which case the probe returns nothing and the wheel waits for the real drop.
pub fn dragged_paths() -> Vec<String> {
    let mut out = Vec::new();
    unsafe {
        if IsClipboardFormatAvailable(CF_HDROP) == 0 || OpenClipboard(std::ptr::null_mut()) == 0 {
            return out;
        }
        let handle = GetClipboardData(CF_HDROP);
        if !handle.is_null() {
            let hdrop: HDROP = handle;
            let count = DragQueryFileW(hdrop, u32::MAX, std::ptr::null_mut(), 0);
            for i in 0..count {
                let len = DragQueryFileW(hdrop, i, std::ptr::null_mut(), 0) as usize;
                if len == 0 {
                    continue;
                }
                let mut buf = vec![0u16; len + 1];
                let written = DragQueryFileW(hdrop, i, buf.as_mut_ptr(), buf.len() as u32);
                if written > 0 {
                    buf.truncate(written as usize);
                    out.push(String::from_utf16_lossy(&buf));
                }
            }
        }
        // The clipboard owns the handle; DragFinish must not be called on it.
        CloseClipboard();
    }
    out
}

/// Z-order and focus hygiene for the wheel and the orb: never take focus, never appear in
/// Alt+Tab, always above normal windows.
pub fn apply_overlay_styles(hwnd: isize) {
    if hwnd == 0 {
        return;
    }
    unsafe {
        let current = GetWindowLongPtrW(hwnd as HWND, GWL_EXSTYLE);
        let style = current | WS_EX_NOACTIVATE as isize | WS_EX_TOOLWINDOW as isize | WS_EX_TOPMOST as isize;
        SetWindowLongPtrW(hwnd as HWND, GWL_EXSTYLE, style);
        SetWindowPos(hwnd as HWND, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
    }
}

fn trigger(
    shift: bool,
    ctrl: bool,
    alt: bool,
    win: bool,
) -> (Option<Trigger>, Option<Trigger>, Option<Trigger>, Option<Trigger>) {
    (
        shift.then_some(Trigger::Shift),
        ctrl.then_some(Trigger::Control),
        alt.then_some(Trigger::Alt),
        win.then_some(Trigger::Meta),
    )
}

/// Polls the button and shows the wheel while a drag is in flight. The same loop watches
/// Ctrl+Shift+Space, the keyboard route for a selection made in Explorer.
pub fn start(app: AppHandle, on_hotkey: fn(&AppHandle)) {
    std::thread::spawn(move || {
        let mut was_down = false;
        let mut active = false;
        let mut origin = pointer();
        let mut last = origin;
        let mut mods = (false, false, false, false);
        let mut shown = false;
        let mut was_space = false;
        let mut beats = 0u8;

        loop {
            let down = key_down(VK_LBUTTON);
            let p = pointer();

            if down && !was_down {
                origin = p;
                active = false;
                mods = (false, false, false, false);
                beats = 0;
                if shown {
                    // A second press while the wheel is open is a cancel, not a new drag.
                    hide(&app);
                    shown = false;
                }
            }

            if down {
                last = p;
                if !active && distance(origin, p) > DRAG_THRESHOLD {
                    active = true;
                }
            } else if was_down {
                if active {
                    release(&app, p);
                    shown = false;
                }
                active = false;
            }
            was_down = down;

            // Ctrl+Shift+Space outside a drag: the wheel for the Explorer selection.
            let space = key_down(VK_SPACE);
            if space
                && !was_space
                && !active
                && key_down(VK_CONTROL)
                && key_down(VK_SHIFT)
                && crate::hook::hotkey_enabled()
            {
                let app = app.clone();
                std::thread::spawn(move || on_hotkey(&app));
            }
            was_space = space;

            // A slow drag must not let the UI's idle timer hide the wheel mid-gesture.
            if active && shown {
                beats = beats.wrapping_add(1);
                if beats % 30 == 0 {
                    let _ = app.emit("wheel-alive", ());
                }
            }

            if active {
                let now = (
                    key_down(VK_SHIFT),
                    key_down(VK_CONTROL),
                    key_down(VK_MENU),
                    key_down(VK_LWIN) || key_down(VK_RWIN),
                );
                if now != mods {
                    mods = now;
                    let keys = *KEYS.lock().unwrap_or_else(|e| e.into_inner());
                    let (shift, ctrl, alt, win) = trigger(now.0, now.1, now.2, now.3);
                    if let Some(mode) = mode_for(&keys, shift, alt, ctrl, win) {
                        show(&app, mode, (last.x, last.y));
                        shown = true;
                    }
                }
            } else if !down {
                mods = (false, false, false, false);
            }

            std::thread::sleep(TICK);
        }
    });
}

fn show(app: &AppHandle, mode: &'static str, at: (i32, i32)) {
    if !visible() {
        if let Some(w) = app.get_webview_window("wheel") {
            center(&w, at.0, at.1);
            let _ = w.show();
        }
        set_visible(true);
        let files = dragged_paths();
        if !files.is_empty() {
            // This is a native drag: load the file types, but do not put the wheel in
            // click/keyboard mode. OLE keeps sending drag events to the window.
            let _ = app.emit("wheel-files", files);
        }
    }
    let _ = app.emit("wheel-mode", mode);
}

fn release(app: &AppHandle, p: POINT) {
    if !visible() {
        return;
    }
    // Releasing inside the wheel is a drop: the webview handles it and hides the wheel.
    let inside = app
        .get_webview_window("wheel")
        .and_then(|w| {
            let pos = w.outer_position().ok()?;
            let size = w.outer_size().ok()?;
            Some(p.x >= pos.x && p.y >= pos.y && p.x < pos.x + size.width as i32 && p.y < pos.y + size.height as i32)
        })
        .unwrap_or(false);
    if inside {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(RELEASE_GRACE);
        if !crate::hook::kept() {
            hide(&app);
        }
    });
}
