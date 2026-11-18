//! Windows drag detection without a low-level mouse hook.
//!
//! `rdev` installs a `WH_MOUSE_LL` hook, which antivirus products flag and which Windows
//! throttles under load. Instead this module polls the button state 60 times a second with
//! `GetAsyncKeyState(VK_LBUTTON)`, measures how far the pointer travelled since the press, and
//! treats the gesture as a drag once it passes the threshold. Modifier keys are read with the
//! same call, so a drag that starts before the app is even running still works.
//!
//! `CF_HDROP` is read from the clipboard as a best-effort probe of what is being dragged.
//! Explorer does **not** publish it during an OLE drag, so that probe usually comes up empty;
//! the file-manager selection is used as the fallback (`crate::drag_paths`) and Tauri's OLE drop
//! target on the wheel window supplies the authoritative list once the pointer moves.

use std::sync::atomic::{AtomicIsize, Ordering};
use std::time::Duration;
use tauri::{AppHandle, Emitter};
use windows_sys::Win32::Foundation::{HWND, POINT};
use windows_sys::Win32::System::DataExchange::{
    CloseClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, VK_CONTROL, VK_LBUTTON, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT, VK_SPACE,
};
use windows_sys::Win32::UI::Shell::{DragQueryFileW, HDROP};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetAncestor, GetClassNameW, GetCursorPos, GetForegroundWindow, GetWindowLongPtrW, SetWindowLongPtrW, SetWindowPos,
    WindowFromPoint, GA_ROOT, GWL_EXSTYLE, HWND_TOPMOST, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT,
};

use crate::hook::{
    hide, in_drop, in_shift_grace, mode_for, native_drag_paths, note_shift_release, set_wheel_files, show_at_center,
    visible, wheel_local_coords, CommitPayload, HoverPoint, Trigger, KEYS, SHIFT_RELEASE_GRACE,
};

/// Pointer travel (px) before a held left button counts as a drag.
const DRAG_THRESHOLD: f64 = 12.0;
/// 60 Hz while a drag is active or the wheel is open, so hover/dwell stays instant.
const ACTIVE_TICK: Duration = Duration::from_millis(16);
/// ~15 Hz while idle in the background, cutting timer wakeups and CPU usage by 4x.
const IDLE_TICK: Duration = Duration::from_millis(64);
/// Time the webview gets to handle a drop before a release elsewhere hides the wheel.
const RELEASE_GRACE: Duration = Duration::from_millis(700);
/// `CF_HDROP`: the clipboard format Explorer publishes during a file drag.
const CF_HDROP: u32 = 15;

static ORIGIN_HWND: AtomicIsize = AtomicIsize::new(0);

fn root_window_at(p: POINT) -> isize {
    unsafe {
        let hwnd = WindowFromPoint(p);
        if hwnd.is_null() {
            return 0;
        }
        let root = GetAncestor(hwnd, GA_ROOT);
        (if root.is_null() { hwnd } else { root }) as isize
    }
}

fn is_desktop_hwnd(hwnd: isize) -> bool {
    if hwnd == 0 {
        return false;
    }
    let mut buf = [0u16; 64];
    let len = unsafe { GetClassNameW(hwnd as HWND, buf.as_mut_ptr(), buf.len() as i32) };
    if len <= 0 {
        return false;
    }
    let class = String::from_utf16_lossy(&buf[..len as usize]);
    class == "Progman" || class == "WorkerW"
}

/// Candidate root window HWNDs (drag origin + foreground) and whether either is the Desktop.
pub fn probe_context() -> (Vec<isize>, bool) {
    let origin = ORIGIN_HWND.load(Ordering::Relaxed);
    let fg = unsafe { GetForegroundWindow() } as isize;
    let mut hwnds = Vec::with_capacity(2);
    if origin != 0 {
        hwnds.push(origin);
    }
    if fg != 0 && fg != origin {
        hwnds.push(fg);
    }
    let desktop = hwnds.iter().copied().any(is_desktop_hwnd);
    (hwnds, desktop)
}

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
        // Never leave WS_EX_TRANSPARENT set: WebView2 needs hit-testing to receive native OLE drops.
        let style = (current & !(WS_EX_TRANSPARENT as isize))
            | WS_EX_NOACTIVATE as isize
            | WS_EX_TOOLWINDOW as isize
            | WS_EX_TOPMOST as isize;
        SetWindowLongPtrW(hwnd as HWND, GWL_EXSTYLE, style);
        SetWindowPos(
            hwnd as HWND,
            HWND_TOPMOST,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_FRAMECHANGED,
        );
    }
}

/// Allows keyboard input inside the wheel window when a parameterized tool form is kept open.
pub fn allow_focus(hwnd: isize) {
    if hwnd == 0 {
        return;
    }
    unsafe {
        let current = GetWindowLongPtrW(hwnd as HWND, GWL_EXSTYLE);
        let style = (current & !(WS_EX_NOACTIVATE as isize) & !(WS_EX_TRANSPARENT as isize))
            | WS_EX_TOOLWINDOW as isize
            | WS_EX_TOPMOST as isize;
        SetWindowLongPtrW(hwnd as HWND, GWL_EXSTYLE, style);
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
        let mut prev_hover = POINT { x: i32::MIN, y: i32::MIN };
        let mut mods = (false, false, false, false);
        let mut was_space = false;
        let mut beats = 0u8;

        loop {
            let down = key_down(VK_LBUTTON);
            let wheel_open = visible();
            let p = if down || wheel_open { pointer() } else { POINT { x: 0, y: 0 } };

            if down && !was_down {
                origin = p;
                ORIGIN_HWND.store(root_window_at(p), Ordering::Relaxed);
                active = false;
                mods = (false, false, false, false);
                beats = 0;
                prev_hover = POINT { x: i32::MIN, y: i32::MIN };
                if visible() && !crate::hook::kept() {
                    // A second press while the wheel is open (and not kept for a form/toast) is a cancel.
                    hide(&app);
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

            // A slow drag must not let the UI's idle timer hide the wheel mid-gesture; also forward
            // window-relative cursor motion so wedges & mode switching respond at 60 Hz even when
            // WebView2 skips OLE DragOver notifications on a newly opened overlay (including orb opens).
            if active && visible() && !crate::hook::kept() {
                beats = beats.wrapping_add(1);
                if beats % 30 == 0 {
                    let _ = app.emit("wheel-alive", ());
                }
                if p.x != prev_hover.x || p.y != prev_hover.y {
                    prev_hover = p;
                    if let Some((lx, ly)) = wheel_local_coords(&app, p.x, p.y) {
                        let _ = app.emit("wheel-hover", HoverPoint { x: lx, y: ly });
                    }
                }
            } else if !visible() {
                prev_hover = POINT { x: i32::MIN, y: i32::MIN };
            }

            if active {
                let now = (
                    key_down(VK_SHIFT),
                    key_down(VK_CONTROL),
                    key_down(VK_MENU),
                    key_down(VK_LWIN) || key_down(VK_RWIN),
                );
                if now != mods {
                    if mods.0 && !now.0 && visible() {
                        note_shift_release();
                    }
                    mods = now;
                    let keys = *KEYS.lock().unwrap_or_else(|e| e.into_inner());
                    let (shift, ctrl, alt, win) = trigger(now.0, now.1, now.2, now.3);
                    if let Some(mode) = mode_for(&keys, shift, alt, ctrl, win) {
                        show(&app, mode, (last.x, last.y));
                    }
                }
            } else if !down {
                mods = (false, false, false, false);
            }

            std::thread::sleep(if down || wheel_open { ACTIVE_TICK } else { IDLE_TICK });
        }
    });
}

fn show(app: &AppHandle, mode: &'static str, at: (i32, i32)) {
    let was_visible = visible();
    show_at_center(app, mode, at);
    if !was_visible {
        // Never inspect Windows Clipboard (`CF_HDROP` or text) during a mouse drag: stale
        // copied files or folders in the clipboard would hijack the wheel. Only probe foreground
        // Explorer selection until native OLE `DragEnter` supplies the true payload.
        let app = app.clone();
        std::thread::spawn(move || {
            let paths = crate::drag_probe_paths();
            if !paths.is_empty() && visible() && native_drag_paths().is_empty() {
                set_wheel_files(paths.clone());
                let _ = app.emit("wheel-files", paths);
            }
        });
    }
}

fn release(app: &AppHandle, p: POINT) {
    if !visible() {
        return;
    }
    // Releasing inside the wheel is a drop: native OLE DragDrop usually fires. If WebView2 missed
    // it, emit `wheel-commit` with any native paths (or empty so the wheel uses its loaded files).
    if let Some((lx, ly)) = wheel_local_coords(app, p.x, p.y) {
        let app = app.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(180));
            if !in_drop() && visible() && !crate::hook::kept() {
                let paths = native_drag_paths();
                let _ = app.emit("wheel-commit", CommitPayload { position: HoverPoint { x: lx, y: ly }, paths });
            }
            std::thread::sleep(Duration::from_millis(520));
            if !crate::hook::kept() && !in_drop() && visible() {
                hide(&app);
            }
        });
        return;
    }
    let extra = if in_shift_grace() { SHIFT_RELEASE_GRACE } else { Duration::ZERO };
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(RELEASE_GRACE + extra);
        if !crate::hook::kept() && !in_drop() {
            hide(&app);
        }
    });
}
