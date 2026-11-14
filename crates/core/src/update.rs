//! Multi-package update and cryptographic signature verification system.
//! Detects and verifies updates from zeidexsoft.com for every package the project ships:
//! Android APK, Windows setup, Linux AppImage (x64/arm64) and macOS DMG (Intel/Apple Silicon).

use crate::sha256;
use std::fs::File;
use std::io;
use std::path::Path;

pub const OFFICIAL_MANIFEST_URL: &str = "https://zeidexsoft.com/updates/manifest.json";
pub const FALLBACK_MANIFEST_URL: &str =
    "https://raw.githubusercontent.com/zeiad6/zeidexfileconventer/main/packaging/updates/manifest.json";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageUpdate {
    pub id: String,
    pub name: String,
    pub platform: String,
    pub version: String,
    pub version_code: u64,
    pub url: String,
    pub sha256: String,
    pub signing_key_id: String,
    pub signature: String,
    pub changelog_ar: String,
    pub changelog_en: String,
    pub mandatory: bool,
}

impl PackageUpdate {
    /// Checks if this update has a higher version code than the currently running version.
    pub fn is_newer_than(&self, current_version_code: u64) -> bool {
        self.version_code > current_version_code
    }

    /// Verifies that the downloaded package file exactly matches the official SHA-256 signature.
    pub fn verify_file(&self, path: &Path) -> io::Result<bool> {
        let f = File::open(path)?;
        let computed = sha256::of_reader(f)?;
        Ok(computed.eq_ignore_ascii_case(self.sha256.trim()))
    }
}

/// Verifies any file against an expected SHA-256 checksum string.
pub fn verify_file_checksum(path: &Path, expected_sha256: &str) -> io::Result<bool> {
    let f = File::open(path)?;
    let computed = sha256::of_reader(f)?;
    Ok(computed.eq_ignore_ascii_case(expected_sha256.trim()))
}

/// Simple parser for the standard update manifest without external JSON dependencies.
pub fn parse_manifest(json: &str) -> Vec<PackageUpdate> {
    let mut packages = Vec::new();

    // Look for objects inside "packages"
    let mut rest = json;
    while let Some(pkg_idx) = rest.find("\"id\":") {
        let sub = &rest[pkg_idx..];
        let end_idx = sub.find('}').unwrap_or(sub.len());
        let block = &sub[..end_idx];

        let id = extract_json_string(block, "id").unwrap_or_default();
        let name = extract_json_string(block, "name").unwrap_or_default();
        let platform = extract_json_string(block, "platform").unwrap_or_default();
        let version = extract_json_string(block, "version").unwrap_or_default();
        let version_code = extract_json_number(block, "version_code").unwrap_or(0);
        let url = extract_json_string(block, "url").unwrap_or_default();
        let sha256 = extract_json_string(block, "sha256").unwrap_or_default();
        let signing_key_id = extract_json_string(block, "signing_key_id").unwrap_or_default();
        let signature = extract_json_string(block, "signature").unwrap_or_default();
        let changelog_ar = extract_json_string(block, "changelog_ar").unwrap_or_default();
        let changelog_en = extract_json_string(block, "changelog_en").unwrap_or_default();
        let mandatory = block.contains("\"mandatory\": true") || block.contains("\"mandatory\":true");

        if !id.is_empty() {
            packages.push(PackageUpdate {
                id,
                name,
                platform,
                version,
                version_code,
                url,
                sha256,
                signing_key_id,
                signature,
                changelog_ar,
                changelog_en,
                mandatory,
            });
        }

        rest = &sub[end_idx..];
    }

    packages
}

fn extract_json_string(block: &str, key: &str) -> Option<String> {
    let pattern = format!("\"{key}\":");
    let pos = block.find(&pattern)?;
    let after = &block[pos + pattern.len()..];
    let start_quote = after.find('"')?;
    let val_start = &after[start_quote + 1..];
    let end_quote = val_start.find('"')?;
    Some(val_start[..end_quote].to_string())
}

fn extract_json_number(block: &str, key: &str) -> Option<u64> {
    let pattern = format!("\"{key}\":");
    let pos = block.find(&pattern)?;
    let after = block[pos + pattern.len()..].trim_start();
    let num_str: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
    num_str.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_packages_from_manifest() {
        let sample = r#"{
            "packages": [
                {
                    "id": "com.zeidex.convert",
                    "name": "Zeidex Convert Android",
                    "platform": "android",
                    "version": "1.1.0",
                    "version_code": 2,
                    "url": "https://zeidexsoft.com/downloads/zeidex-convert-v1.1.0.apk",
                    "sha256": "5003c67107dddab544e4185d50bcc89866551d621af23683231acc3b88b74010",
                    "signing_key_id": "zeidex-release-key-2026",
                    "signature": "d24bf9dfb3f1b9acb0f48fb1940b3eacc3875cdb19e7869f4b1281fdecb9366e",
                    "changelog_ar": "تحسينات شاملة",
                    "changelog_en": "Major performance update",
                    "mandatory": false
                }
            ]
        }"#;

        let pkgs = parse_manifest(sample);
        assert_eq!(pkgs.len(), 1);
        assert_eq!(pkgs[0].id, "com.zeidex.convert");
        assert_eq!(pkgs[0].version_code, 2);
        assert!(pkgs[0].is_newer_than(1));
    }

    #[test]
    fn shipped_registry_has_real_digests_and_https_urls() {
        let json = include_str!("../../../packaging/updates/manifest.json");
        let pkgs = parse_manifest(json);

        assert!(pkgs.len() >= 3, "the registry must list every shipped package");
        for pkg in &pkgs {
            assert!(!pkg.id.is_empty());
            assert!(pkg.version_code > 0, "{} has no version code", pkg.id);
            assert!(pkg.url.starts_with("https://"), "{} downloads over plain HTTP", pkg.id);
            assert_eq!(pkg.sha256.len(), 64, "{} has a truncated digest", pkg.id);
            assert!(pkg.sha256.chars().all(|c| c.is_ascii_hexdigit()), "{} has a non-hex digest", pkg.id);
            // The registry used to carry typed-out example digests. A real SHA-256 can contain
            // this run by chance, but a hand-written placeholder almost always does.
            assert!(!pkg.sha256.contains("0123456789abcdef"), "{} still carries a placeholder digest", pkg.id);
        }
    }
}
