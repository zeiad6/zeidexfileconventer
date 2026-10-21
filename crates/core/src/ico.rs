//! Windows .ico writer with PNG-compressed entries (supported since Windows Vista).

use std::io::{self, Write};

/// `pngs` are (size in px, PNG bytes), each square. 256 px is stored as 0 per the format.
pub fn write_ico(pngs: &[(u32, Vec<u8>)], out: &mut impl Write) -> io::Result<()> {
    if pngs.is_empty() || pngs.iter().any(|(s, d)| *s == 0 || *s > 256 || !d.starts_with(b"\x89PNG")) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "need square PNGs of 1-256 px"));
    }
    let count = pngs.len() as u16;
    let mut buf = Vec::new();
    buf.extend_from_slice(&[0, 0, 1, 0]);
    buf.extend_from_slice(&count.to_le_bytes());
    let mut offset = 6 + 16 * pngs.len() as u32;
    for (size, data) in pngs {
        let dim = if *size == 256 { 0 } else { *size as u8 };
        buf.extend_from_slice(&[dim, dim, 0, 0]);
        buf.extend_from_slice(&1u16.to_le_bytes()); // colour planes
        buf.extend_from_slice(&32u16.to_le_bytes()); // bits per pixel
        buf.extend_from_slice(&(data.len() as u32).to_le_bytes());
        buf.extend_from_slice(&offset.to_le_bytes());
        offset += data.len() as u32;
    }
    for (_, data) in pngs {
        buf.extend_from_slice(data);
    }
    out.write_all(&buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn directory_points_at_images() {
        let a = b"\x89PNG-small".to_vec();
        let b = b"\x89PNG-large-image".to_vec();
        let mut out = Vec::new();
        write_ico(&[(16, a.clone()), (256, b.clone())], &mut out).unwrap();
        assert_eq!(&out[..6], &[0, 0, 1, 0, 2, 0]);
        let entry = |i: usize| &out[6 + 16 * i..6 + 16 * (i + 1)];
        assert_eq!(entry(0)[0], 16);
        assert_eq!(entry(1)[0], 0, "256 px is stored as 0");
        let off = |i: usize| u32::from_le_bytes(entry(i)[12..16].try_into().unwrap()) as usize;
        assert_eq!(&out[off(0)..off(0) + a.len()], &a[..]);
        assert_eq!(&out[off(1)..], &b[..]);
        assert!(write_ico(&[(300, a)], &mut Vec::new()).is_err());
    }
}
