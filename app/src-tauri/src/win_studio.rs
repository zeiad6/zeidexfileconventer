//! Backdrop for the studio window: Mica on Windows 11 22H2 and newer, Acrylic on Windows 10
//! 1809 and newer, plain blur before that — each with rounded corners and a dark frame.
//!
//! `tauri.conf.json` asks for Mica at creation time, where a failure is silent. This module is
//! the explicit ladder: it reads the real build number from `RtlGetVersion` (which ignores the
//! compatibility shims `GetVersionExW` is subject to) and applies the best backdrop the build
//! actually supports. The macOS and Linux builds keep the window as configured; the CSS in
//! `style.css` is a readable dark surface on its own if no backdrop is available.

use std::ffi::c_void;

use tauri::WebviewWindow;
use windows_sys::Win32::Foundation::{BOOL, HWND};
use windows_sys::Win32::Graphics::Dwm::{
    DwmSetWindowAttribute, DWMSBT_MAINWINDOW, DWMWA_SYSTEMBACKDROP_TYPE, DWMWA_USE_IMMERSIVE_DARK_MODE,
    DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
};

/// Windows 11 22H2: the build that understands `DWMWA_SYSTEMBACKDROP_TYPE` (Mica).
const BUILD_MICA: u32 = 22_621;
/// Windows 10 1809: the build that understands the acrylic accent policy.
const BUILD_ACRYLIC: u32 = 17_763;

/// Tint behind the acrylic blur, in DWM's 0xAABBGGRR order: 80% opaque near-black.
const ACRYLIC_TINT_DARK: u32 = 0xCC1A1A1A;
/// The same tint for the light theme, so the blur does not look like a stuck dark mode.
const ACRYLIC_TINT_LIGHT: u32 = 0xCCF2EEE9;
/// `ACCENT_FLAG_DRAW_ALL_BORDERS`, the only flag that renders a full-window acrylic surface.
const ACCENT_FLAG_DRAW_ALL_BORDERS: u32 = 2;

#[repr(C)]
struct OsVersionInfoW {
    size: u32,
    major: u32,
    minor: u32,
    build: u32,
    platform: u32,
    csd: [u16; 128],
}

#[repr(C)]
struct AccentPolicy {
    state: u32,
    flags: u32,
    color: u32,
    animation: u32,
}

#[repr(C)]
struct WindowCompositionAttributeData {
    attribute: u32,
    data: *mut c_void,
    size: usize,
}

const WCA_ACCENT_POLICY: u32 = 19;
const ACCENT_ENABLE_BLURBEHIND: u32 = 3;
const ACCENT_ENABLE_ACRYLICBLURBEHIND: u32 = 4;

#[link(name = "ntdll")]
extern "system" {
    fn RtlGetVersion(info: *mut OsVersionInfoW) -> i32;
}

#[link(name = "user32")]
extern "system" {
    fn SetWindowCompositionAttribute(hwnd: HWND, data: *mut WindowCompositionAttributeData) -> BOOL;
}

/// The real OS build, or 0 when the query fails (treated as "no backdrop").
fn build_number() -> u32 {
    let mut info = OsVersionInfoW {
        size: std::mem::size_of::<OsVersionInfoW>() as u32,
        major: 0,
        minor: 0,
        build: 0,
        platform: 0,
        csd: [0; 128],
    };
    let status = unsafe { RtlGetVersion(&mut info) };
    if status == 0 {
        info.build
    } else {
        0
    }
}

fn set_attribute(hwnd: HWND, attribute: i32, value: &[u32]) {
    unsafe {
        DwmSetWindowAttribute(
            hwnd,
            attribute as u32,
            value.as_ptr() as *const c_void,
            std::mem::size_of_val(value) as u32,
        );
    }
}

/// Applies the best available backdrop. Safe to call once, after the window exists.
pub fn apply_backdrop(window: &WebviewWindow) {
    let Ok(hwnd) = window.hwnd() else { return };
    let hwnd: HWND = hwnd.0;
    let build = build_number();
    // Follow the window theme so the frame and the tint match the app's own palette.
    let dark = matches!(window.theme(), Ok(tauri::Theme::Dark));

    // Dark frame and rounded corners on every Windows 10/11 build; both attributes are
    // ignored by older composition engines instead of failing the call.
    let dark_flag: BOOL = if dark { 1 } else { 0 };
    set_attribute(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE, &[dark_flag as u32]);
    set_attribute(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE, &[DWMWCP_ROUND as u32]);

    if build >= BUILD_MICA {
        set_attribute(hwnd, DWMWA_SYSTEMBACKDROP_TYPE, &[DWMSBT_MAINWINDOW as u32]);
        return;
    }

    let state = if build >= BUILD_ACRYLIC { ACCENT_ENABLE_ACRYLICBLURBEHIND } else { ACCENT_ENABLE_BLURBEHIND };
    let mut policy = AccentPolicy {
        state,
        flags: ACCENT_FLAG_DRAW_ALL_BORDERS,
        color: if dark { ACRYLIC_TINT_DARK } else { ACRYLIC_TINT_LIGHT },
        animation: 0,
    };
    let mut data = WindowCompositionAttributeData {
        attribute: WCA_ACCENT_POLICY,
        data: &mut policy as *mut AccentPolicy as *mut c_void,
        size: std::mem::size_of::<AccentPolicy>(),
    };
    unsafe {
        SetWindowCompositionAttribute(hwnd, &mut data);
    }
}
