use crate::config::FillMode;
use anyhow::{bail, Result};
use serde::Serialize;
use std::path::{Path, PathBuf};

pub const POSTERS_DIR: &str = "posters";

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum LiveSupport {
    Full,
    NeedsGnomeExtension,
    None,
}

impl LiveSupport {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::NeedsGnomeExtension => "needsGnomeExtension",
            Self::None => "none",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum GnomeExtensionStatus {
    NotApplicable,
    NotInstalled,
    NeedsLogout,
    Active,
    #[allow(dead_code)]
    Failed,
}

impl GnomeExtensionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotApplicable => "notApplicable",
            Self::NotInstalled => "notInstalled",
            Self::NeedsLogout => "needsLogout",
            Self::Active => "active",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct LiveOpts {
    pub muted: bool,
    pub fill_mode: FillMode,
    #[allow(dead_code)]
    pub pause_on_fullscreen: bool,
    #[allow(dead_code)]
    pub pause_on_battery: bool,
}

impl LiveOpts {
    pub fn from_config(config: &crate::config::Config) -> Self {
        Self {
            muted: config.live_muted,
            fill_mode: config.fill_mode,
            pause_on_fullscreen: config.pause_live_on_fullscreen,
            pause_on_battery: config.pause_live_on_battery,
        }
    }
}

/// Destination rectangle inside a window, plus an optional source crop, for
/// mapping a video frame onto a monitor.
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
pub struct Blit {
    pub src_x: i32,
    pub src_y: i32,
    pub src_w: i32,
    pub src_h: i32,
    pub dst_x: i32,
    pub dst_y: i32,
    pub dst_w: i32,
    pub dst_h: i32,
}

#[allow(dead_code)]
pub fn blit_for(mode: FillMode, video_w: u32, video_h: u32, win_w: u32, win_h: u32) -> Blit {
    let vw = video_w.max(1) as f64;
    let vh = video_h.max(1) as f64;
    let ww = win_w.max(1) as f64;
    let wh = win_h.max(1) as f64;
    let video_ar = vw / vh;
    let win_ar = ww / wh;

    match mode {
        FillMode::Stretch => Blit {
            src_x: 0,
            src_y: 0,
            src_w: video_w as i32,
            src_h: video_h as i32,
            dst_x: 0,
            dst_y: 0,
            dst_w: win_w as i32,
            dst_h: win_h as i32,
        },
        FillMode::Fit | FillMode::Center | FillMode::Tile => {
            let (dw, dh) = if video_ar > win_ar {
                (ww, ww / video_ar)
            } else {
                (wh * video_ar, wh)
            };
            let dw = dw.min(ww);
            let dh = dh.min(wh);
            Blit {
                src_x: 0,
                src_y: 0,
                src_w: video_w as i32,
                src_h: video_h as i32,
                dst_x: ((ww - dw) / 2.0).round() as i32,
                dst_y: ((wh - dh) / 2.0).round() as i32,
                dst_w: dw.round().max(1.0) as i32,
                dst_h: dh.round().max(1.0) as i32,
            }
        }
        FillMode::Fill => {
            let (sw, sh) = if video_ar > win_ar {
                (vh * win_ar, vh)
            } else {
                (vw, vw / win_ar)
            };
            Blit {
                src_x: ((vw - sw) / 2.0).round().max(0.0) as i32,
                src_y: ((vh - sh) / 2.0).round().max(0.0) as i32,
                src_w: sw.round().max(1.0) as i32,
                src_h: sh.round().max(1.0) as i32,
                dst_x: 0,
                dst_y: 0,
                dst_w: win_w as i32,
                dst_h: win_h as i32,
            }
        }
    }
}

pub fn poster_path(cache_dir: &Path, source: &Path) -> Result<PathBuf> {
    let dir = cache_dir.join(POSTERS_DIR);
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join(format!("{}.jpg", crate::thumbs::cache_key(source)?)))
}

#[allow(dead_code)]
pub fn save_rgb_jpeg(path: &Path, width: u32, height: u32, rgb: &[u8]) -> Result<()> {
    let image = image::RgbImage::from_raw(width, height, rgb.to_vec())
        .ok_or_else(|| anyhow::anyhow!("could not build a poster frame"))?;
    image
        .save_with_format(path, image::ImageFormat::Jpeg)
        .map_err(|err| anyhow::anyhow!("writing {}: {err}", path.display()))
}

pub fn file_uri(path: &Path) -> String {
    use percent_encoding::{utf8_percent_encode, AsciiSet, CONTROLS};
    const URI_ESCAPE: &AsciiSet = &CONTROLS
        .add(b' ')
        .add(b'"')
        .add(b'<')
        .add(b'>')
        .add(b'`')
        .add(b'#')
        .add(b'?')
        .add(b'{')
        .add(b'}')
        .add(b'%')
        .add(b'[')
        .add(b']')
        .add(b'\\')
        .add(b'^')
        .add(b'|')
        .add(b'\'');
    format!(
        "file://{}",
        utf8_percent_encode(&path.to_string_lossy(), URI_ESCAPE)
    )
}

/// Used when a platform cannot decode a poster; the UI still has a cache file
/// to point at rather than failing the whole apply.
pub fn write_placeholder_poster(path: &Path) -> Result<()> {
    let image = image::RgbImage::from_pixel(16, 9, image::Rgb([20, 20, 24]));
    image
        .save_with_format(path, image::ImageFormat::Jpeg)
        .map_err(|err| anyhow::anyhow!("writing {}: {err}", path.display()))
}

pub fn unsupported() -> Result<()> {
    bail!("Live wallpaper is not supported on this desktop.")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stretch_fills_the_window() {
        let blit = blit_for(FillMode::Stretch, 100, 50, 200, 200);
        assert_eq!(blit.dst_x, 0);
        assert_eq!(blit.dst_y, 0);
        assert_eq!(blit.dst_w, 200);
        assert_eq!(blit.dst_h, 200);
    }

    #[test]
    fn fit_letterboxes_a_wide_video() {
        let blit = blit_for(FillMode::Fit, 200, 50, 200, 200);
        assert_eq!(blit.dst_x, 0);
        assert!(blit.dst_y > 0);
        assert_eq!(blit.dst_w, 200);
    }
}
