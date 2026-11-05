//! Which H.264 encoder `prepare` uses. Linux distributions ship FFmpeg with
//! `libx264`; the LGPL FFmpeg that Wallflow bundles on Windows and macOS has no
//! `libx264` (it is GPL), so there the system's own encoder does the work.

use anyhow::{Context, Result, bail};
use std::process::Stdio;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encoder {
    /// Best quality per bit; CRF keeps the quality even across clips.
    X264,
    /// macOS VideoToolbox (hardware, software fallback allowed).
    VideoToolbox,
    /// Windows Media Foundation (the built-in software encoder at worst).
    MediaFoundation,
}

impl Encoder {
    /// In order of preference.
    const ALL: [Encoder; 3] = [
        Encoder::X264,
        Encoder::VideoToolbox,
        Encoder::MediaFoundation,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Encoder::X264 => "libx264",
            Encoder::VideoToolbox => "h264_videotoolbox",
            Encoder::MediaFoundation => "h264_mf",
        }
    }

    /// The pixel format the encoder takes without converting again itself.
    pub fn pix_fmt(self) -> &'static str {
        match self {
            Encoder::X264 => "yuv420p",
            Encoder::VideoToolbox | Encoder::MediaFoundation => "nv12",
        }
    }

    /// Codec options for an output of `size` at `fps`.
    pub fn args(self, size: (u32, u32), fps: f64, threads: usize) -> Vec<String> {
        let mut args: Vec<String> = vec!["-c:v".into(), self.name().into()];
        match self {
            // `faster` at CRF 20 still looks clean.
            Encoder::X264 => args.extend(
                ["-preset", "faster", "-crf", "20", "-threads"]
                    .map(String::from)
                    .into_iter()
                    .chain([threads.to_string()]),
            ),
            // Hardware encoders ignore CRF: aim for a bitrate instead.
            Encoder::VideoToolbox | Encoder::MediaFoundation => {
                let rate = bitrate(size, fps);
                args.extend([
                    "-b:v".into(),
                    rate.to_string(),
                    "-maxrate".into(),
                    (rate * 3 / 2).to_string(),
                    "-bufsize".into(),
                    (rate * 2).to_string(),
                ]);
                if self == Encoder::VideoToolbox {
                    args.extend(["-allow_sw", "1", "-profile:v", "high"].map(String::from));
                }
            }
        }
        args
    }
}

/// About 0.1 bit per pixel per frame — 1080p30 ≈ 6 Mbit/s — which keeps slow
/// wallpaper motion clean; bounded so tiny or huge clips stay sensible.
pub fn bitrate(size: (u32, u32), fps: f64) -> u64 {
    let bits = size.0 as f64 * size.1 as f64 * fps.max(1.0) * 0.1;
    (bits as u64).clamp(2_000_000, 20_000_000)
}

/// Picks from the output of `ffmpeg -hide_banner -encoders`.
pub fn pick(encoders: &str) -> Option<Encoder> {
    let names: Vec<&str> = encoders
        .lines()
        .filter_map(|line| {
            let mut words = line.split_whitespace();
            // " V....D libx264   libx264 H.264 …": capabilities, then the name.
            let caps = words.next()?;
            (caps.len() == 6 && caps.starts_with('V'))
                .then(|| words.next())
                .flatten()
        })
        .collect();
    Encoder::ALL
        .into_iter()
        .find(|encoder| names.contains(&encoder.name()))
}

pub fn detect(ffmpeg: &str) -> Result<Encoder> {
    let output = crate::process::command(ffmpeg)
        .args(["-hide_banner", "-encoders"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .with_context(|| format!("running `{ffmpeg}` (is FFmpeg installed?)"))?;
    match pick(&String::from_utf8_lossy(&output.stdout)) {
        Some(encoder) => Ok(encoder),
        None => bail!(
            "this FFmpeg has no H.264 encoder (looked for {})",
            Encoder::ALL.map(Encoder::name).join(", ")
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LINUX: &str = "Encoders:
 V..... = Video
 ------
 V....D libx264              libx264 H.264 / AVC / MPEG-4 AVC / MPEG-4 part 10 (codec h264)
 V....D h264_vaapi           H.264/AVC (VAAPI) (codec h264)
 V....D mjpeg                MJPEG (Motion JPEG)
";
    const WINDOWS_LGPL: &str = "Encoders:
 V..... = Video
 ------
 V....D h264_mf              H264 via MediaFoundation (codec h264)
 V....D mjpeg                MJPEG (Motion JPEG)
 A....D aac                  AAC (Advanced Audio Coding)
";
    const MAC_LGPL: &str = "Encoders:
 ------
 V....D h264_videotoolbox    VideoToolbox H.264 Encoder (codec h264)
 V....D mjpeg                MJPEG (Motion JPEG)
";

    #[test]
    fn prefers_x264_then_the_systems_own_encoder() {
        assert_eq!(pick(LINUX), Some(Encoder::X264));
        assert_eq!(pick(WINDOWS_LGPL), Some(Encoder::MediaFoundation));
        assert_eq!(pick(MAC_LGPL), Some(Encoder::VideoToolbox));
    }

    #[test]
    fn no_h264_encoder_is_none() {
        assert_eq!(pick(" V....D mjpeg   MJPEG\n"), None);
        // The legend line is not an encoder called "=".
        assert_eq!(pick(" V..... = Video\n"), None);
    }

    #[test]
    fn bitrate_scales_with_pixels_and_stays_bounded() {
        assert_eq!(bitrate((1920, 1080), 30.0), 6_220_800);
        assert_eq!(bitrate((320, 240), 24.0), 2_000_000);
        assert_eq!(bitrate((7680, 4320), 30.0), 20_000_000);
    }

    #[test]
    fn hardware_encoders_get_a_bitrate_not_crf() {
        let args = Encoder::MediaFoundation.args((1920, 1080), 30.0, 4);
        assert!(args.windows(2).any(|w| w == ["-b:v", "6220800"]));
        assert!(!args.iter().any(|a| a == "-crf"));
        let args = Encoder::X264.args((1920, 1080), 30.0, 4);
        assert!(args.windows(2).any(|w| w == ["-threads", "4"]));
    }
}
