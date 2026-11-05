//! `wallflow-live prepare`: converts a video once, when it is added, into the
//! cheapest form to loop forever — no bigger than the screen, at most 30 fps,
//! H.264 4:2:0 without audio — and saves a poster frame for the static wallpaper
//! and the library thumbnail.
//!
//! A 1440p60 clip on a 1080p monitor otherwise costs four times the decoding and
//! upload work on every frame, for detail nobody can see.

use crate::encoder::{self, Encoder};
use crate::probe::{self, VideoInfo};
use anyhow::{Context, Result, bail};
use serde::Serialize;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::Stdio;

/// Desktop motion looks smooth at 30 fps; more only burns power.
pub const MAX_FPS: f64 = 30.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Plan {
    /// Output size; `None` keeps the source size.
    pub size: Option<(u32, u32)>,
    /// Output frame rate; `None` keeps the source rate.
    pub fps: Option<f64>,
}

/// Downscale (never upscale) so the video still covers a `screen`-sized monitor,
/// and cap the frame rate.
pub fn plan(video: (u32, u32), fps: f64, screen: (u32, u32), max_fps: f64) -> Plan {
    let (vw, vh) = (video.0.max(1) as f64, video.1.max(1) as f64);
    let (sw, sh) = (screen.0.max(1) as f64, screen.1.max(1) as f64);
    // The smallest scale at which the video still fills the screen in Fill mode.
    let cover = (sw / vw).max(sh / vh);
    let size = (cover < 1.0).then(|| {
        let even = |value: f64| ((value * cover / 2.0).round() as u32 * 2).max(2);
        (even(vw), even(vh))
    });
    let fps = (fps > max_fps + 0.5).then_some(max_fps);
    Plan { size, fps }
}

pub struct Args {
    pub input: PathBuf,
    pub output: PathBuf,
    pub poster: PathBuf,
    pub screen: (u32, u32),
    pub ffmpeg: String,
    pub ffprobe: String,
}

/// One JSON object per stdout line, for the app's progress bar.
#[derive(Serialize)]
#[serde(tag = "event", rename_all = "camelCase")]
enum Report<'a> {
    Started {
        width: u32,
        height: u32,
        fps: f64,
        duration: Option<f64>,
    },
    Progress {
        fraction: f64,
    },
    Done {
        video: &'a Path,
        poster: &'a Path,
    },
}

fn report(event: &Report) {
    if let Ok(line) = serde_json::to_string(event) {
        println!("{line}");
    }
}

pub fn run(args: &Args) -> Result<()> {
    let info = probe::probe(&args.ffprobe, &args.input)?;
    let encoder = encoder::detect(&args.ffmpeg)?;
    log::info!("encoding with {}", encoder.name());
    let plan = plan((info.width, info.height), info.fps, args.screen, MAX_FPS);
    let (width, height) = plan.size.unwrap_or((info.width, info.height));
    report(&Report::Started {
        width,
        height,
        fps: plan.fps.unwrap_or(info.fps),
        duration: info.duration,
    });

    let part = with_suffix(&args.output, ".part.mp4");
    encode(args, &info, &plan, encoder, &part)?;
    std::fs::rename(&part, &args.output)
        .with_context(|| format!("saving {}", args.output.display()))?;

    let poster_part = with_suffix(&args.poster, ".part.jpg");
    poster(&args.ffmpeg, &args.output, &poster_part, info.duration)?;
    std::fs::rename(&poster_part, &args.poster)
        .with_context(|| format!("saving {}", args.poster.display()))?;

    report(&Report::Done {
        video: &args.output,
        poster: &args.poster,
    });
    Ok(())
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

fn encode(args: &Args, info: &VideoInfo, plan: &Plan, encoder: Encoder, out: &Path) -> Result<()> {
    let mut filters = Vec::new();
    if let Some((w, h)) = plan.size {
        filters.push(format!("scale={w}:{h}:flags=lanczos"));
    }
    if let Some(fps) = plan.fps {
        filters.push(format!("fps={fps}"));
    }
    filters.push(format!("format={}", encoder.pix_fmt()));
    let fps = plan.fps.unwrap_or(info.fps);
    let gop = (fps * 2.0).round().max(1.0) as u32;
    let size = plan.size.unwrap_or((info.width, info.height));

    let mut child = crate::process::command(&args.ffmpeg)
        .args(["-hide_banner", "-loglevel", "error", "-nostdin", "-y", "-i"])
        .arg(&args.input)
        .args([
            "-map",
            "0:v:0",
            "-an",
            "-sn",
            "-dn",
            "-vf",
            &filters.join(","),
        ])
        // Half the cores at most: importing a wallpaper must not make the
        // desktop itself stutter.
        .args(encoder.args(size, fps, encode_threads()))
        .args(["-g", &gop.to_string(), "-movflags", "+faststart"])
        .args(["-progress", "pipe:1", "-nostats", "-f", "mp4"])
        .arg(out)
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .with_context(|| format!("running `{}` (is FFmpeg installed?)", args.ffmpeg))?;

    let stdout = child.stdout.take().context("ffmpeg has no stdout")?;
    let mut last = -1.0;
    for line in BufReader::new(stdout).lines().map_while(Result::ok) {
        let (Some(total), Some(micros)) = (
            info.duration,
            line.strip_prefix("out_time_us=")
                .and_then(|v| v.parse::<f64>().ok()),
        ) else {
            continue;
        };
        let fraction = (micros / 1e6 / total).clamp(0.0, 1.0);
        if fraction - last >= 0.01 {
            last = fraction;
            report(&Report::Progress {
                fraction: (fraction * 100.0).round() / 100.0,
            });
        }
    }

    let status = child.wait()?;
    if !status.success() {
        let _ = std::fs::remove_file(out);
        bail!("ffmpeg could not convert {}", args.input.display());
    }
    Ok(())
}

fn encode_threads() -> usize {
    std::thread::available_parallelism()
        .map(|cores| (cores.get() / 2).max(1))
        .unwrap_or(2)
}

/// A frame a second in (or the middle of a short clip), so fades from black
/// do not become the poster.
fn poster(ffmpeg: &str, video: &Path, out: &Path, duration: Option<f64>) -> Result<()> {
    let at = duration.map_or(1.0, |secs| (secs / 2.0).min(1.0));
    let status = crate::process::command(ffmpeg)
        .args(["-hide_banner", "-loglevel", "error", "-nostdin", "-y"])
        .args(["-ss", &format!("{at:.2}"), "-i"])
        .arg(video)
        .args(["-frames:v", "1", "-q:v", "2", "-f", "image2"])
        .arg(out)
        .status()
        .with_context(|| format!("running `{ffmpeg}`"))?;
    if !status.success() {
        bail!("ffmpeg could not save a poster frame");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_1440p60_clip_shrinks_to_cover_1080p_at_30fps() {
        let plan = plan((2560, 1440), 60.0, (1920, 1080), MAX_FPS);
        assert_eq!(plan.size, Some((1920, 1080)));
        assert_eq!(plan.fps, Some(30.0));
    }

    #[test]
    fn small_or_slow_clips_are_left_alone() {
        let plan = plan((1280, 720), 24.0, (1920, 1080), MAX_FPS);
        assert_eq!(
            plan,
            Plan {
                size: None,
                fps: None
            }
        );
        // 29.97 is not worth resampling.
        assert_eq!(plan_fps(29.97), None);
    }

    #[test]
    fn a_wide_clip_keeps_enough_height_to_fill_the_screen() {
        // 21:9 4K on a 16:9 1080p screen: height decides, width overflows.
        let plan = plan((5120, 2160), 30.0, (1920, 1080), MAX_FPS);
        assert_eq!(plan.size, Some((2560, 1080)));
    }

    #[test]
    fn output_sizes_are_even() {
        let (w, h) = plan((1001, 1001), 30.0, (333, 333), MAX_FPS).size.unwrap();
        assert_eq!((w % 2, h % 2), (0, 0));
    }

    fn plan_fps(fps: f64) -> Option<f64> {
        plan((100, 100), fps, (100, 100), MAX_FPS).fps
    }
}
