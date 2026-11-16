//! The installers promise that every engine a conversion needs travels inside the package.
//! That promise is only worth testing against real artifacts: point `ZEIDEX_BUNDLE_DIR` at the
//! resource directory of an unpacked package (the directory that holds `engines/`) and this
//! test checks that each shipped engine is there and that the app's own lookup resolves it
//! *inside* the package, never from the host's PATH.
//!
//! Unpacking, per platform:
//!   Linux:   `./Zeidex.Convert_*.AppImage --appimage-extract` -> `squashfs-root/usr/lib/Zeidex Convert`
//!   Windows: `7z x Zeidex.Convert_*-setup.exe -onsis`
//!   macOS:   `7zz x Zeidex.Convert_*.dmg -odmg` -> `Zeidex Convert.app/Contents/Resources`
//!
//! With the variable unset the test does nothing, so a plain `cargo test` stays offline.

use std::path::PathBuf;

use zeidex_core::tools::{self, Tool};

/// Engines every desktop package carries; the rest are optional by design.
const BUNDLED: [Tool; 6] = [Tool::Ffmpeg, Tool::Ffprobe, Tool::SevenZip, Tool::Ghostscript, Tool::Qpdf, Tool::Pandoc];

/// What only some packages carry so far: libvips on Windows and Linux, OCR and QR on Linux.
fn platform_extras() -> &'static [Tool] {
    if cfg!(target_os = "linux") {
        &[Tool::Vips, Tool::VipsHeader, Tool::Tesseract, Tool::Qrencode]
    } else if cfg!(windows) {
        &[Tool::Vips, Tool::VipsHeader]
    } else {
        &[]
    }
}

fn resource_dir() -> Option<PathBuf> {
    let dir = PathBuf::from(std::env::var_os("ZEIDEX_BUNDLE_DIR")?);
    assert!(dir.is_dir(), "not a directory: {}", dir.display());
    let engines = dir.join("engines");
    assert!(engines.is_dir(), "package has no engines/ under {}", dir.display());
    tools::set_bundle_dirs(vec![engines]);
    Some(dir)
}

#[test]
fn shipped_package_resolves_its_engines() {
    let Some(resource_dir) = resource_dir() else {
        eprintln!("ZEIDEX_BUNDLE_DIR not set: skipping packaged-engine check");
        return;
    };

    let mut wrong = Vec::new();
    for tool in BUNDLED.into_iter().chain(platform_extras().iter().copied()) {
        match tools::locate(tool) {
            Some(path) if path.starts_with(&resource_dir) && path.is_file() => {}
            Some(path) => wrong.push(format!("{} -> {} (outside the package)", tool.label(), path.display())),
            None => wrong.push(format!("{} -> not found", tool.label())),
        }
    }
    assert!(wrong.is_empty(), "engines not shipped in {}: {}", resource_dir.display(), wrong.join("; "));
}

/// The conda-built Tesseract and libvips only work when their data tree travels beside them:
/// the runner points each one at it, and has nothing to point at if packaging dropped it.
#[test]
fn shipped_conda_engines_carry_their_data() {
    if !cfg!(target_os = "linux") || resource_dir().is_none() {
        return;
    }
    for tool in [Tool::Tesseract, Tool::Vips] {
        let bin = tools::locate(tool).unwrap_or_else(|| panic!("{} -> not found", tool.label()));
        assert!(tools::bundled_env(&bin).is_some(), "{} has no data tree beside {}", tool.label(), bin.display());
    }
}

/// The HWP/HWPX engine travels in every package and appears in the notices, so its presence is
/// checked directly: it has no `Tool` variant yet, which is why the lookup test above cannot
/// cover it.
#[test]
fn shipped_package_carries_the_hwp_engine() {
    let Some(resource_dir) = resource_dir() else {
        eprintln!("ZEIDEX_BUNDLE_DIR not set: skipping packaged-hwp check");
        return;
    };
    let name = if cfg!(windows) { "rhwp.exe" } else { "rhwp" };
    let path = resource_dir.join("engines").join(name);
    let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    assert!(size > 0, "missing or empty: {}", path.display());
}

/// The papers and screenshots that must travel with the binaries.
#[test]
fn shipped_package_carries_notices_and_screens() {
    let Some(resource_dir) = resource_dir() else {
        eprintln!("ZEIDEX_BUNDLE_DIR not set: skipping packaged-notice check");
        return;
    };

    for rel in [
        "legal/LICENSE.txt",
        "legal/THIRD-PARTY-NOTICES.txt",
        "screens/banner.png",
        "screens/wheel.png",
        "screens/demo.gif",
    ] {
        let path = resource_dir.join(rel);
        let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        assert!(size > 0, "missing or empty: {}", path.display());
    }
}
