//! Tauri build script.
//!
//! A native build runs `tauri-build`, which embeds the icon, version block and application
//! manifest into a Windows resource. That step shells out to a resource compiler and
//! preprocesses the generated `.rc` against the Windows SDK, so it cannot work when a Linux or
//! macOS host cross-checks the Windows target (`cargo check --target x86_64-pc-windows-msvc`,
//! used to type-check `win_drag_hook.rs`). There is no switch for it in `tauri-build`, so the
//! cross-check emits the same `cfg` directives and skips only the resources. The real Windows
//! build still goes through `tauri-build` unchanged.

fn main() {
    if cross_checking_windows() {
        println!("cargo:rerun-if-env-changed=TAURI_CONFIG");
        // `tauri-build` aliases `desktop`/`mobile` from the target OS; repeat them here so the
        // cross-check compiles the same `#[cfg(desktop)]` code the native build does.
        println!("cargo:rustc-check-cfg=cfg(desktop)");
        println!("cargo:rustc-check-cfg=cfg(mobile)");
        println!("cargo:rustc-cfg=desktop");
        println!("cargo:warning=cross-checking the Windows target: icon and manifest resources skipped");
        return;
    }
    tauri_build::build()
}

fn cross_checking_windows() -> bool {
    let host = std::env::var("HOST").unwrap_or_default();
    let target = std::env::var("TARGET").unwrap_or_default();
    target.contains("windows") && !host.contains("windows")
}
