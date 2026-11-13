//! The product promise is "everything is processed on this machine". Two guards keep it true:
//! no networking crate may be added to a manifest, and a real conversion must not open a
//! socket. Both run offline, in the normal `cargo test` pass.

use std::fs;
use std::path::{Path, PathBuf};

/// Crates that exist to talk to the network. Banning them at the manifest level is the cheapest
/// place to catch a well-meaning "just fetch the update manifest" patch.
const BANNED: &[&str] = &[
    "reqwest",
    "ureq",
    "isahc",
    "surf",
    "awc",
    "curl",
    "curl-sys",
    "hyper",
    "h2",
    "h3",
    "quinn",
    "tokio-tungstenite",
    "tungstenite",
    "rustls",
    "native-tls",
    "openssl",
    "trust-dns-resolver",
    "hickory-resolver",
    "socket2",
    "attohttpc",
    "minreq",
    "ureq",
];

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

/// Direct dependency names declared in one manifest (all `[dependencies]`-style tables).
fn declared_dependencies(manifest: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_deps = false;
    for line in manifest.lines() {
        let t = line.trim();
        if t.starts_with('[') {
            in_deps = t.contains("dependencies");
            continue;
        }
        if !in_deps || t.is_empty() || t.starts_with('#') {
            continue;
        }
        // `name = { ... }`, `name = "1"`, or `[dependencies.name]` handled above.
        if let Some((name, _)) = t.split_once('=') {
            out.push(name.trim().to_string());
        }
    }
    out
}

#[test]
fn no_manifest_pulls_a_networking_crate() {
    let root = workspace_root();
    let manifests = [
        root.join("Cargo.toml"),
        root.join("crates/core/Cargo.toml"),
        root.join("crates/cli/Cargo.toml"),
        root.join("app/src-tauri/Cargo.toml"),
    ];
    let mut checked = 0;
    for path in manifests.iter().filter(|p| p.exists()) {
        let text = fs::read_to_string(path).unwrap();
        let deps = declared_dependencies(&text);
        for dep in &deps {
            assert!(
                !BANNED.contains(&dep.as_str()),
                "{} declares `{dep}`: Zeidex Convert must stay offline",
                path.display()
            );
        }
        checked += 1;
    }
    assert!(checked >= 3, "expected the workspace manifests to be found");
}

/// Socket inodes held by this process, from `/proc/self/fd` (Linux). Comparing inodes rather
/// than "any socket in the namespace" keeps the assertion about *this* process only.
#[cfg(target_os = "linux")]
fn socket_inodes() -> Vec<u64> {
    let Ok(entries) = fs::read_dir("/proc/self/fd") else { return Vec::new() };
    entries
        .flatten()
        .filter_map(|e| fs::read_link(e.path()).ok())
        .filter_map(|target| {
            let s = target.to_string_lossy();
            s.strip_prefix("socket:[").and_then(|r| r.strip_suffix(']')).and_then(|n| n.parse().ok())
        })
        .collect()
}

#[cfg(target_os = "linux")]
#[test]
fn a_real_conversion_opens_no_socket() {
    use zeidex_core::ops::Op;
    use zeidex_core::runner;

    let dir = std::env::temp_dir().join(format!("zeidex-offline-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let src = dir.join("offline.srt");
    fs::write(&src, "1\n00:00:01,000 --> 00:00:02,000\nمرحبا\n").unwrap();

    let before = socket_inodes();
    let results = runner::run(&Op::Convert { to: "vtt".into(), quality: None }, std::slice::from_ref(&src), |_| {});
    let after = socket_inodes();

    let out = results[0].as_ref().expect("subtitle conversion runs without any external engine");
    assert!(fs::read_to_string(out).unwrap().starts_with("WEBVTT"));
    let leaked: Vec<u64> = after.into_iter().filter(|i| !before.contains(i)).collect();
    assert!(leaked.is_empty(), "conversion opened socket(s) {leaked:?}: the app must not use the network");
    fs::remove_dir_all(&dir).unwrap();
}
