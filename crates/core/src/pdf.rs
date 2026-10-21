//! Minimal PDF writer: one page per JPEG, embedded as-is (DCTDecode, no re-encoding).
//! libvips cannot write PDF, and pulling in LibreOffice just for "images → PDF" is overkill.

use std::io::{self, Write};

pub struct JpegInfo {
    pub width: u32,
    pub height: u32,
    pub components: u8,
}

/// Reads the size from the first SOF marker.
pub fn jpeg_info(data: &[u8]) -> Option<JpegInfo> {
    if data.get(..2)? != [0xFF, 0xD8] {
        return None;
    }
    let mut i = 2;
    while i + 4 <= data.len() {
        if data[i] != 0xFF {
            i += 1;
            continue;
        }
        let marker = data[i + 1];
        if marker == 0xFF {
            i += 1;
            continue;
        }
        let len = u16::from_be_bytes([data[i + 2], data[i + 3]]) as usize;
        // SOF0..SOF15 except DHT (C4), JPG (C8) and DAC (CC).
        if (0xC0..=0xCF).contains(&marker) && ![0xC4, 0xC8, 0xCC].contains(&marker) {
            let s = data.get(i + 4..i + 10)?;
            return Some(JpegInfo {
                height: u16::from_be_bytes([s[1], s[2]]) as u32,
                width: u16::from_be_bytes([s[3], s[4]]) as u32,
                components: s[5],
            });
        }
        i += 2 + len;
    }
    None
}

/// Writes a PDF with one page per JPEG. Pages are sized at 96 dpi so screens show them 1:1.
pub fn jpegs_to_pdf(jpegs: &[Vec<u8>], out: &mut impl Write) -> io::Result<()> {
    let mut buf: Vec<u8> = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets: Vec<usize> = Vec::new();
    let n = jpegs.len();
    // Object ids: 1 catalog, 2 pages, then per image i: page 3+3i, content 4+3i, image 5+3i.
    let mut obj = |buf: &mut Vec<u8>, body: &[u8]| {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n", offsets.len()).as_bytes());
        buf.extend_from_slice(body);
        buf.extend_from_slice(b"\nendobj\n");
    };
    obj(&mut buf, b"<< /Type /Catalog /Pages 2 0 R >>");
    let kids: Vec<String> = (0..n).map(|i| format!("{} 0 R", 3 + 3 * i)).collect();
    obj(&mut buf, format!("<< /Type /Pages /Kids [{}] /Count {n} >>", kids.join(" ")).as_bytes());
    for (i, data) in jpegs.iter().enumerate() {
        let info = jpeg_info(data).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "not a JPEG"))?;
        let (w, h) = (info.width as f64 * 0.75, info.height as f64 * 0.75);
        let (content, image) = (4 + 3 * i, 5 + 3 * i);
        obj(
            &mut buf,
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {w:.2} {h:.2}] /Contents {content} 0 R /Resources << /XObject << /Im0 {image} 0 R >> >> >>"
            )
            .as_bytes(),
        );
        let draw = format!("q {w:.2} 0 0 {h:.2} 0 0 cm /Im0 Do Q");
        obj(&mut buf, format!("<< /Length {} >>\nstream\n{draw}\nendstream", draw.len()).as_bytes());
        let (cs, decode) = match info.components {
            1 => ("/DeviceGray", ""),
            // Adobe-style CMYK JPEGs are stored inverted.
            4 => ("/DeviceCMYK", " /Decode [1 0 1 0 1 0 1 0]"),
            _ => ("/DeviceRGB", ""),
        };
        let mut body = format!(
            "<< /Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace {cs} /BitsPerComponent 8 /Filter /DCTDecode{decode} /Length {} >>\nstream\n",
            info.width,
            info.height,
            data.len()
        )
        .into_bytes();
        body.extend_from_slice(data);
        body.extend_from_slice(b"\nendstream");
        obj(&mut buf, &body);
    }
    let xref = buf.len();
    buf.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len() + 1).as_bytes());
    for off in &offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", offsets.len() + 1).as_bytes(),
    );
    out.write_all(&buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SOI + APP0 stub + SOF0 for a `w`x`h` 3-component image + EOI. Enough for the writer.
    fn fake_jpeg(w: u16, h: u16) -> Vec<u8> {
        let mut d = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x04, 0x00, 0x00, 0xFF, 0xC0, 0x00, 0x11, 0x08];
        d.extend_from_slice(&h.to_be_bytes());
        d.extend_from_slice(&w.to_be_bytes());
        d.extend_from_slice(&[3, 1, 0x22, 0, 2, 0x11, 1, 3, 0x11, 1, 0xFF, 0xD9]);
        d
    }

    #[test]
    fn reads_sof() {
        let i = jpeg_info(&fake_jpeg(640, 480)).unwrap();
        assert_eq!((i.width, i.height, i.components), (640, 480, 3));
        assert!(jpeg_info(b"\x89PNG").is_none());
    }

    #[test]
    fn xref_offsets_point_at_objects() {
        let mut out = Vec::new();
        jpegs_to_pdf(&[fake_jpeg(100, 50), fake_jpeg(10, 20)], &mut out).unwrap();
        let text = String::from_utf8_lossy(&out);
        assert!(text.starts_with("%PDF-1.4") && text.contains("/Count 2"));
        let xref_at: usize = text.rsplit("startxref\n").next().unwrap().lines().next().unwrap().parse().unwrap();
        // Byte offsets: index `out`, not the lossy text (the binary header comment shifts it).
        let tail = std::str::from_utf8(&out[xref_at..]).unwrap();
        assert!(tail.starts_with("xref"));
        let entries: Vec<usize> =
            tail.lines().skip(3).take_while(|l| l.ends_with(" n ")).map(|l| l[..10].parse().unwrap()).collect();
        assert_eq!(entries.len(), 8);
        for (i, off) in entries.iter().enumerate() {
            assert!(out[*off..].starts_with(format!("{} 0 obj", i + 1).as_bytes()), "object {}", i + 1);
        }
        assert!(text.contains("/MediaBox [0 0 75.00 37.50]"));
    }
}
