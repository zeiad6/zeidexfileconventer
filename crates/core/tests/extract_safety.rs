//! Extracting an archive must never write outside the folder the archive lives in.
//!
//! A ZIP is written by hand here (stored entries, no compression) so the fixture is exactly the
//! hostile archive we want: one normal file plus entries that try to climb out with `..` and an
//! absolute path. The op then runs through the real `7z` on the machine, and the whole scratch
//! tree is walked afterwards. Anything named `escape.txt` that ended up outside the extracted
//! folder is a failure.
//!
//! Skipped (with a line on stderr) when no 7-Zip binary is installed, so `cargo test` still
//! works on a machine without the engines.

use std::fs;
use std::path::{Path, PathBuf};
use zeidex_core::ops::Op;
use zeidex_core::plan::{self, Ctx};
use zeidex_core::runner;

const NAME: &str = "zeidex-escape.txt";
const PAYLOAD: &str = "climbed out";

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

/// A minimal stored-entry ZIP: local header + data per entry, then a central directory.
fn zip(entries: &[(&str, &str)]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut directory = Vec::new();
    for (name, body) in entries {
        let offset = out.len() as u32;
        let crc = crc32(body.as_bytes());
        let size = body.len() as u32;
        out.extend_from_slice(&0x0403_4B50u32.to_le_bytes()); // local file header
        out.extend_from_slice(&20u16.to_le_bytes()); // version needed
        out.extend_from_slice(&0u16.to_le_bytes()); // flags
        out.extend_from_slice(&0u16.to_le_bytes()); // stored
        out.extend_from_slice(&0u16.to_le_bytes()); // time
        out.extend_from_slice(&0u16.to_le_bytes()); // date
        out.extend_from_slice(&crc.to_le_bytes());
        out.extend_from_slice(&size.to_le_bytes()); // compressed
        out.extend_from_slice(&size.to_le_bytes()); // uncompressed
        out.extend_from_slice(&(name.len() as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes()); // extra length
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(body.as_bytes());

        directory.extend_from_slice(&0x0201_4B50u32.to_le_bytes()); // central header
        directory.extend_from_slice(&20u16.to_le_bytes()); // made by
        directory.extend_from_slice(&20u16.to_le_bytes()); // needed
        directory.extend_from_slice(&0u16.to_le_bytes());
        directory.extend_from_slice(&0u16.to_le_bytes());
        directory.extend_from_slice(&0u16.to_le_bytes());
        directory.extend_from_slice(&0u16.to_le_bytes());
        directory.extend_from_slice(&crc.to_le_bytes());
        directory.extend_from_slice(&size.to_le_bytes());
        directory.extend_from_slice(&size.to_le_bytes());
        directory.extend_from_slice(&(name.len() as u16).to_le_bytes());
        directory.extend_from_slice(&0u16.to_le_bytes()); // extra
        directory.extend_from_slice(&0u16.to_le_bytes()); // comment
        directory.extend_from_slice(&0u16.to_le_bytes()); // disk
        directory.extend_from_slice(&0u16.to_le_bytes()); // internal attrs
        directory.extend_from_slice(&0u32.to_le_bytes()); // external attrs
        directory.extend_from_slice(&offset.to_le_bytes());
        directory.extend_from_slice(name.as_bytes());
    }
    let dir_offset = out.len() as u32;
    let dir_size = directory.len() as u32;
    out.extend_from_slice(&directory);
    out.extend_from_slice(&0x0605_4B50u32.to_le_bytes()); // end of central directory
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&dir_size.to_le_bytes());
    out.extend_from_slice(&dir_offset.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}

/// Every file under `dir`, depth first.
fn tree(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(read) = fs::read_dir(dir) else { return };
    for entry in read.flatten() {
        let path = entry.path();
        if path.is_dir() {
            tree(&path, out);
        } else {
            out.push(path);
        }
    }
}

fn seven_zip_available() -> bool {
    ["7z", "7zz", "7za"].iter().any(|bin| {
        std::process::Command::new(bin)
            .arg("i")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok()
    })
}

#[test]
fn hostile_archive_entries_cannot_escape_the_output_folder() {
    if !seven_zip_available() {
        eprintln!("skipped: no 7-Zip binary on PATH");
        return;
    }

    let root = std::env::temp_dir().join(format!("zeidex-extract-safety-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let inbox = root.join("in");
    fs::create_dir_all(&inbox).unwrap();

    let archive = inbox.join("hostile.zip");
    fs::write(
        &archive,
        zip(&[
            ("ok.txt", "fine"),
            ("../../zeidex-escape.txt", PAYLOAD),
            ("/zeidex-escape.txt", PAYLOAD),
            ("nested/../../../zeidex-escape.txt", PAYLOAD),
        ]),
    )
    .unwrap();

    let tmp = inbox.join(".zeidex-test-tmp");
    fs::create_dir_all(&tmp).unwrap();
    let plan = plan::plan(&Op::Extract { password: None }, std::slice::from_ref(&archive), &Ctx::default(), &tmp)
        .expect("extract plans");
    assert!(!plan.steps.is_empty(), "extract must run 7z");

    let results = runner::run(&Op::Extract { password: None }, std::slice::from_ref(&archive), |_| {});
    for result in &results {
        // A refusal is acceptable (7z may reject the whole archive); silently writing outside
        // the folder is not.
        let _ = result;
    }

    let mut files = Vec::new();
    tree(&root, &mut files);
    let escaped: Vec<&PathBuf> = files
        .iter()
        .filter(|p| p.file_name().and_then(|n| n.to_str()) == Some(NAME))
        .filter(|p| !p.starts_with(inbox.join("hostile")))
        .collect();
    assert!(escaped.is_empty(), "these files escaped the extraction folder: {escaped:?}\nfull tree: {files:?}");

    // And the normal entry is still extracted, so the guard above is not passing because
    // nothing ran at all.
    let extracted: Vec<&PathBuf> = files.iter().filter(|p| p.starts_with(inbox.join("hostile"))).collect();
    assert!(!extracted.is_empty(), "nothing was extracted at all: {results:?}");

    let _ = fs::remove_dir_all(&root);
}
