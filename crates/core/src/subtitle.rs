//! SRT / WebVTT / plain text conversion. Pure Rust: subtitles are too small to need an engine.

#[derive(Debug, PartialEq)]
pub struct Cue {
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
}

/// Parses SRT or WebVTT (detected from content, not extension).
pub fn parse(src: &str) -> Vec<Cue> {
    let src = src.trim_start_matches('\u{feff}').replace("\r\n", "\n");
    let mut cues = Vec::new();
    for block in src.split("\n\n") {
        let mut lines = block.lines().skip_while(|l| !l.contains("-->"));
        let Some(timing) = lines.next() else { continue };
        let Some((a, b)) = timing.split_once("-->") else { continue };
        // VTT cue settings follow the end time ("00:01.000 align:start").
        let b = b.split_whitespace().next().unwrap_or("");
        let (Some(start_ms), Some(end_ms)) = (parse_ts(a.trim()), parse_ts(b)) else { continue };
        let text = lines.collect::<Vec<_>>().join("\n");
        cues.push(Cue { start_ms, end_ms, text });
    }
    cues
}

fn parse_ts(s: &str) -> Option<u64> {
    let s = s.replace(',', ".");
    let (hms, ms) = s.split_once('.').unwrap_or((&s, "0"));
    let mut secs = 0u64;
    let parts: Vec<&str> = hms.split(':').collect();
    if parts.is_empty() || parts.len() > 3 {
        return None;
    }
    for p in parts {
        secs = secs * 60 + p.parse::<u64>().ok()?;
    }
    let ms: u64 = format!("{ms:0<3}")[..3].parse().ok()?;
    Some(secs * 1000 + ms)
}

fn fmt_ts(ms: u64, sep: char) -> String {
    format!("{:02}:{:02}:{:02}{sep}{:03}", ms / 3_600_000, ms / 60_000 % 60, ms / 1000 % 60, ms % 1000)
}

pub fn write(cues: &[Cue], to: &str) -> String {
    let mut out = String::new();
    match to {
        "vtt" => {
            out.push_str("WEBVTT\n\n");
            for c in cues {
                out += &format!("{} --> {}\n{}\n\n", fmt_ts(c.start_ms, '.'), fmt_ts(c.end_ms, '.'), c.text);
            }
        }
        "srt" => {
            for (i, c) in cues.iter().enumerate() {
                out += &format!("{}\n{} --> {}\n{}\n\n", i + 1, fmt_ts(c.start_ms, ','), fmt_ts(c.end_ms, ','), c.text);
            }
        }
        _ => {
            for c in cues {
                out += &c.text;
                out.push('\n');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRT: &str = "1\r\n00:00:01,500 --> 00:00:04,000\r\nHello\r\nworld\r\n\r\n2\r\n00:01:00,000 --> 00:01:02,250\r\nمرحبا\r\n";

    #[test]
    fn srt_vtt_roundtrip() {
        let cues = parse(SRT);
        assert_eq!(cues.len(), 2);
        assert_eq!(cues[0], Cue { start_ms: 1500, end_ms: 4000, text: "Hello\nworld".into() });
        let vtt = write(&cues, "vtt");
        assert!(vtt.starts_with("WEBVTT\n\n00:00:01.500 --> 00:00:04.000\nHello"));
        assert_eq!(parse(&vtt), cues);
        assert_eq!(write(&parse(&write(&cues, "srt")), "srt"), write(&cues, "srt"));
        assert_eq!(write(&cues, "txt"), "Hello\nworld\nمرحبا\n");
    }

    #[test]
    fn vtt_short_times_and_settings() {
        let cues = parse("WEBVTT\n\nintro\n01:02.5 --> 01:04.000 align:start\nHi\n");
        assert_eq!(cues, vec![Cue { start_ms: 62_500, end_ms: 64_000, text: "Hi".into() }]);
    }
}
