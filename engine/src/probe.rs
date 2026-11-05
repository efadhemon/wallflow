use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::path::Path;
use std::process::Command;

/// What the renderer needs to know about a video before the first frame arrives.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VideoInfo {
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    /// BT.709 for HD material, BT.601 for SD — picks the YUV→RGB matrix.
    pub bt709: bool,
    /// Full-range ("pc") YUV instead of the usual studio range.
    pub full_range: bool,
    /// Seconds, when the container knows it.
    pub duration: Option<f64>,
}

#[derive(Deserialize)]
struct Probe {
    streams: Vec<Stream>,
    format: Option<Format>,
}

#[derive(Deserialize)]
struct Format {
    duration: Option<String>,
}

#[derive(Deserialize)]
struct Stream {
    width: u32,
    height: u32,
    avg_frame_rate: Option<String>,
    r_frame_rate: Option<String>,
    color_space: Option<String>,
    color_range: Option<String>,
}

pub fn probe(ffprobe: &str, path: &Path) -> Result<VideoInfo> {
    let output = Command::new(ffprobe)
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=width,height,avg_frame_rate,r_frame_rate,color_space,color_range:format=duration",
            "-of",
            "json",
        ])
        .arg(path)
        .output()
        .with_context(|| format!("running `{ffprobe}` (is FFmpeg installed?)"))?;
    if !output.status.success() {
        bail!(
            "ffprobe could not read {}: {}",
            path.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    parse(&output.stdout)
}

fn parse(json: &[u8]) -> Result<VideoInfo> {
    let probe: Probe = serde_json::from_slice(json).context("ffprobe sent unreadable JSON")?;
    let duration = probe
        .format
        .and_then(|format| format.duration)
        .and_then(|raw| raw.parse::<f64>().ok())
        .filter(|secs| secs.is_finite() && *secs > 0.0);
    let Some(stream) = probe.streams.into_iter().next() else {
        bail!("the file has no video stream");
    };
    if stream.width == 0 || stream.height == 0 {
        bail!("the video has no size");
    }

    let fps = [&stream.avg_frame_rate, &stream.r_frame_rate]
        .into_iter()
        .flatten()
        .find_map(|rate| parse_rate(rate))
        .unwrap_or(30.0);

    let bt709 = match stream.color_space.as_deref() {
        Some(space) if space.starts_with("bt709") => true,
        Some(space) if space.starts_with("bt470") || space.starts_with("smpte170") => false,
        _ => stream.height >= 720,
    };

    Ok(VideoInfo {
        width: stream.width,
        height: stream.height,
        fps,
        bt709,
        full_range: stream.color_range.as_deref() == Some("pc"),
        duration,
    })
}

/// "30000/1001" → 29.97. Zero or nonsense rates are rejected.
fn parse_rate(raw: &str) -> Option<f64> {
    let (num, den) = raw.split_once('/').unwrap_or((raw, "1"));
    let rate = num.parse::<f64>().ok()? / den.parse::<f64>().ok()?;
    (rate.is_finite() && rate > 0.0 && rate <= 240.0).then_some(rate)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rates_parse() {
        assert_eq!(parse_rate("30/1"), Some(30.0));
        assert!((parse_rate("30000/1001").unwrap() - 29.97).abs() < 0.01);
        assert_eq!(parse_rate("0/0"), None);
        assert_eq!(parse_rate("25"), Some(25.0));
    }

    #[test]
    fn parses_ffprobe_json() {
        let info = parse(
            br#"{"streams":[{"width":1920,"height":1080,"avg_frame_rate":"0/0",
                "r_frame_rate":"24/1","color_space":"bt709","color_range":"tv"}],
                "format":{"duration":"12.5"}}"#,
        )
        .unwrap();
        assert_eq!(
            info,
            VideoInfo {
                width: 1920,
                height: 1080,
                fps: 24.0,
                bt709: true,
                full_range: false,
                duration: Some(12.5),
            }
        );
    }
}
