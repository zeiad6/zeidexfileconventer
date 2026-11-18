//! Hardware acceleration detection and mode selection for FFmpeg video encoding.
//!
//! Defaults to `Auto`: detects an available GPU encoder once per session (`h264_nvenc`,
//! `h264_qsv`, `h264_amf`, `h264_vaapi`, or `h264_videotoolbox`) and lets the runner fall back
//! cleanly to CPU (`libx264`) if the GPU driver fails at runtime.

use crate::{Error, Result};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum HwAccelMode {
    #[default]
    Auto = 0,
    On = 1,
    Off = 2,
}

impl HwAccelMode {
    pub fn parse(s: &str) -> Result<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "auto" => Ok(HwAccelMode::Auto),
            "on" | "true" | "1" | "yes" => Ok(HwAccelMode::On),
            "off" | "false" | "0" | "no" => Ok(HwAccelMode::Off),
            other => Err(Error::BadOption(format!("hwaccel={other} (expected auto, on, or off)"))),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            HwAccelMode::Auto => "auto",
            HwAccelMode::On => "on",
            HwAccelMode::Off => "off",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HwEncoder {
    Nvenc,
    Qsv,
    Amf,
    Vaapi,
    VideoToolbox,
}

impl HwEncoder {
    pub fn h264_codec(self) -> &'static str {
        match self {
            HwEncoder::Nvenc => "h264_nvenc",
            HwEncoder::Qsv => "h264_qsv",
            HwEncoder::Amf => "h264_amf",
            HwEncoder::Vaapi => "h264_vaapi",
            HwEncoder::VideoToolbox => "h264_videotoolbox",
        }
    }

    pub fn hevc_codec(self) -> &'static str {
        match self {
            HwEncoder::Nvenc => "hevc_nvenc",
            HwEncoder::Qsv => "hevc_qsv",
            HwEncoder::Amf => "hevc_amf",
            HwEncoder::Vaapi => "hevc_vaapi",
            HwEncoder::VideoToolbox => "hevc_videotoolbox",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            HwEncoder::Nvenc => "NVIDIA NVENC",
            HwEncoder::Qsv => "Intel Quick Sync (QSV)",
            HwEncoder::Amf => "AMD AMF",
            HwEncoder::Vaapi => "Linux VA-API",
            HwEncoder::VideoToolbox => "Apple VideoToolbox",
        }
    }
}

static MODE: AtomicU8 = AtomicU8::new(HwAccelMode::Auto as u8);
static DETECTED: OnceLock<Option<HwEncoder>> = OnceLock::new();

pub fn set_mode(mode: HwAccelMode) {
    MODE.store(mode as u8, Ordering::Relaxed);
}

pub fn mode() -> HwAccelMode {
    match MODE.load(Ordering::Relaxed) {
        1 => HwAccelMode::On,
        2 => HwAccelMode::Off,
        _ => HwAccelMode::Auto,
    }
}

/// Parses `ffmpeg -hide_banner -encoders` output and selects the best available GPU H.264
/// encoder for the current platform.
pub fn parse_encoders_output(stdout: &str, has_vaapi_device: bool) -> Option<HwEncoder> {
    let has = |name: &str| stdout.lines().any(|line| line.split_whitespace().nth(1) == Some(name));
    if cfg!(target_os = "macos") {
        if has("h264_videotoolbox") {
            return Some(HwEncoder::VideoToolbox);
        }
        return None;
    }
    if cfg!(windows) {
        if has("h264_nvenc") {
            return Some(HwEncoder::Nvenc);
        }
        if has("h264_qsv") {
            return Some(HwEncoder::Qsv);
        }
        if has("h264_amf") {
            return Some(HwEncoder::Amf);
        }
        return None;
    }
    // Linux: prefer NVENC when CUDA/NVENC is present, else VA-API when a DRI render node exists,
    // else QSV.
    if has("h264_nvenc") {
        return Some(HwEncoder::Nvenc);
    }
    if has_vaapi_device && has("h264_vaapi") {
        return Some(HwEncoder::Vaapi);
    }
    if has("h264_qsv") {
        return Some(HwEncoder::Qsv);
    }
    None
}

/// Probes `ffmpeg -hide_banner -encoders` once per session to check what GPU encoders are built
/// into the active FFmpeg binary (respects `HwAccelMode::Off`).
pub fn detect_for_ffmpeg(ffmpeg_bin: &Path) -> Option<HwEncoder> {
    if mode() == HwAccelMode::Off {
        return None;
    }
    *DETECTED.get_or_init(|| probe_uncached(ffmpeg_bin))
}

fn probe_uncached(ffmpeg_bin: &Path) -> Option<HwEncoder> {
    let bin = crate::tools::normalize_engine_path(ffmpeg_bin);
    let mut cmd = Command::new(bin);
    cmd.args(["-hide_banner", "-encoders"]).stdin(Stdio::null()).stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let out = cmd.output().ok()?;
    if !out.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    let has_vaapi = Path::new("/dev/dri/renderD128").exists();
    parse_encoders_output(&stdout, has_vaapi)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hwaccel_mode_parses_values() {
        assert_eq!(HwAccelMode::parse("auto").unwrap(), HwAccelMode::Auto);
        assert_eq!(HwAccelMode::parse("ON").unwrap(), HwAccelMode::On);
        assert_eq!(HwAccelMode::parse("off").unwrap(), HwAccelMode::Off);
        assert!(HwAccelMode::parse("bogus").is_err());
    }

    #[test]
    fn encoder_detection_parses_ffmpeg_encoders_list() {
        let sample = "\
Encoders:
 V..... libx264              libx264 H.264 / AVC
 V....D h264_nvenc           NVIDIA NVENC H.264 encoder
 V....D h264_vaapi           H.264/AVC (VAAPI)
 V....D h264_qsv             H.264 / AVC / MPEG-4 AVC / MPEG-2 part 10 (Intel Quick Sync Video)
 V....D h264_amf             AMD AMF H.264 Encoder
 V....D h264_videotoolbox    VideoToolbox H.264 Encoder
";
        let picked = parse_encoders_output(sample, true);
        if cfg!(target_os = "macos") {
            assert_eq!(picked, Some(HwEncoder::VideoToolbox));
        } else {
            assert_eq!(picked, Some(HwEncoder::Nvenc));
        }

        let vaapi_only = " V....D h264_vaapi           H.264/AVC (VAAPI)\n";
        if cfg!(target_os = "linux") {
            assert_eq!(parse_encoders_output(vaapi_only, false), None);
            assert_eq!(parse_encoders_output(vaapi_only, true), Some(HwEncoder::Vaapi));
        }
    }
}
