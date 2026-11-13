//! Motion photo and Live Photo video extraction (pure Rust std).
//!
//! Handles:
//! 1. Google Motion Photo (Pixel/Samsung): embedded MP4 video at the tail of the JPEG.
//! 2. MicroVideo JPEG: older offset from end of file.
//! 3. Paired Apple Live Photo: `foo.mov` beside `foo.heic` or `foo.jpg`.
//! 4. Apple `.livp` ZIP archives containing a `.mov` video.

use crate::{Error, Result};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq)]
pub enum MotionKind {
    EmbeddedJpeg { offset: u64, length: u64 },
    PairedMov(PathBuf),
    LivpPackage,
}

#[derive(Clone, Debug)]
pub struct MotionInfo {
    pub kind: MotionKind,
}

pub fn probe_motion(path: &Path) -> Option<MotionInfo> {
    // 1. Check if it's a livp file
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        if ext.eq_ignore_ascii_case("livp") {
            return Some(MotionInfo { kind: MotionKind::LivpPackage });
        }
    }

    // 2. Check for paired MOV file beside HEIC/JPG
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        if ext.eq_ignore_ascii_case("heic") || ext.eq_ignore_ascii_case("jpg") || ext.eq_ignore_ascii_case("jpeg") {
            let mov = path.with_extension("mov");
            if mov.exists() && mov != path {
                return Some(MotionInfo { kind: MotionKind::PairedMov(mov) });
            }
            let mov_upper = path.with_extension("MOV");
            if mov_upper.exists() && mov_upper != path {
                return Some(MotionInfo { kind: MotionKind::PairedMov(mov_upper) });
            }
        }
    }

    // 3. Check for embedded JPEG motion photo
    if let Ok(mut f) = File::open(path) {
        if let Ok((offset, length)) = find_jpeg_embedded_video(&mut f) {
            return Some(MotionInfo { kind: MotionKind::EmbeddedJpeg { offset, length } });
        }
    }

    None
}

/// Parses JPEG APP1 (XMP) markers and locates the embedded MP4 video at the end of the file.
fn find_jpeg_embedded_video(f: &mut File) -> std::io::Result<(u64, u64)> {
    let file_len = f.seek(SeekFrom::End(0))?;
    if file_len < 16 {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "file too small"));
    }
    f.seek(SeekFrom::Start(0))?;

    let mut header = [0u8; 2];
    f.read_exact(&mut header)?;
    if header != [0xFF, 0xD8] {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "not a JPEG"));
    }

    const MAX_METADATA: u64 = 8 * 1024 * 1024;
    let mut cur = 2u64;

    while cur < file_len.min(MAX_METADATA) {
        let mut marker_buf = [0u8; 2];
        if f.read_exact(&mut marker_buf).is_err() {
            break;
        }
        if marker_buf[0] != 0xFF {
            break;
        }
        let marker = marker_buf[1];
        if marker == 0xDA || marker == 0xD9 {
            // SOS (Start of Scan) or EOI
            break;
        }
        if marker == 0xFF {
            // Fill byte
            f.seek(SeekFrom::Current(-1))?;
            cur += 1;
            continue;
        }

        let mut len_buf = [0u8; 2];
        if f.read_exact(&mut len_buf).is_err() {
            break;
        }
        let seg_len = u16::from_be_bytes(len_buf) as u64;
        if seg_len < 2 {
            break;
        }
        let payload_len = seg_len - 2;

        if marker == 0xE1 {
            // APP1 segment: check for XMP
            let mut payload = vec![0u8; payload_len as usize];
            if f.read_exact(&mut payload).is_ok() {
                const XMP_SIG: &[u8] = b"http://ns.adobe.com/xap/1.0/\0";
                if payload.starts_with(XMP_SIG) {
                    let xmp_text = String::from_utf8_lossy(&payload[XMP_SIG.len()..]);
                    if let Some(video_len) = extract_xmp_video_length(&xmp_text) {
                        if video_len > 0 && video_len < file_len {
                            let video_offset = file_len - video_len;
                            // Verify MP4 box at video_offset
                            f.seek(SeekFrom::Start(video_offset))?;
                            let mut box_header = [0u8; 8];
                            if f.read_exact(&mut box_header).is_ok() {
                                let box_type = &box_header[4..8];
                                if box_type == b"ftyp" {
                                    return Ok((video_offset, video_len));
                                }
                            }
                        }
                    }
                }
            }
        } else {
            f.seek(SeekFrom::Current(payload_len as i64))?;
        }
        cur += 2 + seg_len;
    }

    Err(std::io::Error::new(std::io::ErrorKind::NotFound, "no motion photo video found"))
}

fn extract_xmp_video_length(xmp: &str) -> Option<u64> {
    // Check for GCamera:MotionPhoto="1" or MicroVideo="1"
    let is_motion = xmp.contains("MotionPhoto=\"1\"")
        || xmp.contains("MotionPhoto>1<")
        || xmp.contains("MicroVideo=\"1\"")
        || xmp.contains("MicroVideo>1<");
    if !is_motion {
        return None;
    }

    // Try MicroVideoOffset="(\d+)"
    if let Some(pos) = xmp.find("MicroVideoOffset=\"") {
        let rest = &xmp[pos + 18..];
        if let Some(end) = rest.find('"') {
            if let Ok(val) = rest[..end].trim().parse::<u64>() {
                return Some(val);
            }
        }
    }
    if let Some(pos) = xmp.find("MicroVideoOffset>") {
        let rest = &xmp[pos + 17..];
        if let Some(end) = rest.find('<') {
            if let Ok(val) = rest[..end].trim().parse::<u64>() {
                return Some(val);
            }
        }
    }

    // Try Item:Length="(\d+)" with MotionPhoto / video/mp4
    if let Some(pos) = xmp.find("Item:Length=\"") {
        let rest = &xmp[pos + 13..];
        if let Some(end) = rest.find('"') {
            if let Ok(val) = rest[..end].trim().parse::<u64>() {
                return Some(val);
            }
        }
    }
    if xmp.contains("Length=\"") {
        // Find last Length="..."
        let mut last_val = None;
        for part in xmp.split("Length=\"").skip(1) {
            if let Some(end) = part.find('"') {
                if let Ok(val) = part[..end].trim().parse::<u64>() {
                    last_val = Some(val);
                }
            }
        }
        if let Some(val) = last_val {
            return Some(val);
        }
    }

    None
}

/// Extracts the motion video from `src` and writes it to `dst`.
pub fn extract_motion_video(src: &Path, dst: &Path) -> Result<()> {
    let info = probe_motion(src)
        .ok_or_else(|| Error::Unsupported(format!("{} is not a recognized motion or live photo", src.display())))?;

    match info.kind {
        MotionKind::EmbeddedJpeg { offset, length } => {
            let mut f = File::open(src)?;
            f.seek(SeekFrom::Start(offset))?;
            let mut out = File::create(dst)?;
            let mut remaining = length;
            let mut buf = [0u8; 64 * 1024];
            while remaining > 0 {
                let to_read = (remaining as usize).min(buf.len());
                let n = f.read(&mut buf[..to_read])?;
                if n == 0 {
                    break;
                }
                out.write_all(&buf[..n])?;
                remaining -= n as u64;
            }
            Ok(())
        }
        MotionKind::PairedMov(mov_path) => {
            std::fs::copy(&mov_path, dst)?;
            Ok(())
        }
        MotionKind::LivpPackage => {
            // A .livp file is a zip archive.
            Err(Error::Unsupported("livp extraction requires archive unpacker".into()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_motion_photo_xmp_microvideo() {
        let xmp = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF><rdf:Description GCamera:MicroVideo="1" GCamera:MicroVideoOffset="123456"/></rdf:RDF></x:xmpmeta>"#;
        assert_eq!(extract_xmp_video_length(xmp), Some(123456));
    }

    #[test]
    fn parse_motion_photo_xmp_container() {
        let xmp = r#"<x:xmpmeta><rdf:Description GCamera:MotionPhoto="1"><Container:Directory><Container:Item Item:Semantic="MotionPhoto" Item:Length="654321"/></Container:Directory></rdf:Description></x:xmpmeta>"#;
        assert_eq!(extract_xmp_video_length(xmp), Some(654321));
    }

    #[test]
    fn non_motion_xmp_returns_none() {
        let xmp = r#"<x:xmpmeta><rdf:Description dc:title="test"/></x:xmpmeta>"#;
        assert_eq!(extract_xmp_video_length(xmp), None);
    }
}
