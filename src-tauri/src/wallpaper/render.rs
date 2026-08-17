use crate::config::FillMode;
use anyhow::{Context, Result};
use image::{imageops, Rgba, RgbaImage};
use std::path::Path;

/// Matches GNOME’s wallpaper fade closely enough that a 5s minimum interval
/// still has plenty of headroom after the scheduler’s blocking apply.
#[cfg_attr(not(any(windows, target_os = "macos")), allow(dead_code))]
pub const FADE_MS: u32 = 700;
#[cfg_attr(not(any(windows, target_os = "macos")), allow(dead_code))]
pub const FADE_FRAME_MS: u32 = 16;

#[cfg_attr(not(any(windows, target_os = "macos")), allow(dead_code))]
pub fn ease_in_out_cubic(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    if t < 0.5 {
        4.0 * t * t * t
    } else {
        1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
    }
}

#[cfg_attr(not(any(windows, target_os = "macos")), allow(dead_code))]
pub fn compose(
    path: &Path,
    mode: FillMode,
    width: u32,
    height: u32,
    letterbox: [u8; 4],
) -> Result<RgbaImage> {
    let source = image::open(path)
        .with_context(|| format!("decoding {}", path.display()))?
        .to_rgba8();
    Ok(compose_image(&source, mode, width, height, letterbox))
}

/// Draw `source` into a `width`×`height` buffer the way the desktop will show it.
pub fn compose_image(
    source: &RgbaImage,
    mode: FillMode,
    width: u32,
    height: u32,
    letterbox: [u8; 4],
) -> RgbaImage {
    let mut dest = RgbaImage::from_pixel(width, height, Rgba(letterbox));
    if width == 0 || height == 0 || source.width() == 0 || source.height() == 0 {
        return dest;
    }

    match mode {
        FillMode::Stretch => {
            let scaled = imageops::resize(source, width, height, imageops::FilterType::Triangle);
            imageops::replace(&mut dest, &scaled, 0, 0);
        }
        FillMode::Fill => {
            let scale =
                (width as f64 / source.width() as f64).max(height as f64 / source.height() as f64);
            let nw = (source.width() as f64 * scale).round().max(1.0) as u32;
            let nh = (source.height() as f64 * scale).round().max(1.0) as u32;
            let scaled = imageops::resize(source, nw, nh, imageops::FilterType::Triangle);
            let x = nw.saturating_sub(width) / 2;
            let y = nh.saturating_sub(height) / 2;
            let cropped =
                imageops::crop_imm(&scaled, x, y, width.min(nw), height.min(nh)).to_image();
            imageops::replace(&mut dest, &cropped, 0, 0);
        }
        FillMode::Fit => {
            let scale =
                (width as f64 / source.width() as f64).min(height as f64 / source.height() as f64);
            let nw = (source.width() as f64 * scale).round().max(1.0) as u32;
            let nh = (source.height() as f64 * scale).round().max(1.0) as u32;
            let scaled = imageops::resize(source, nw, nh, imageops::FilterType::Triangle);
            let x = width.saturating_sub(nw) / 2;
            let y = height.saturating_sub(nh) / 2;
            imageops::replace(&mut dest, &scaled, x.into(), y.into());
        }
        FillMode::Center => {
            let crop_w = source.width().min(width);
            let crop_h = source.height().min(height);
            let sx = source.width().saturating_sub(crop_w) / 2;
            let sy = source.height().saturating_sub(crop_h) / 2;
            let cropped = imageops::crop_imm(source, sx, sy, crop_w, crop_h).to_image();
            let dx = width.saturating_sub(crop_w) / 2;
            let dy = height.saturating_sub(crop_h) / 2;
            imageops::replace(&mut dest, &cropped, dx.into(), dy.into());
        }
        FillMode::Tile => {
            for y in (0..height).step_by(source.height() as usize) {
                for x in (0..width).step_by(source.width() as usize) {
                    imageops::overlay(&mut dest, source, x as i64, y as i64);
                }
            }
        }
    }

    dest
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixel(r: u8, g: u8, b: u8) -> Rgba<u8> {
        Rgba([r, g, b, 255])
    }

    fn unique_grid(width: u32, height: u32) -> RgbaImage {
        RgbaImage::from_fn(width, height, |x, y| pixel(x as u8, y as u8, 99))
    }

    #[test]
    fn ease_starts_and_ends_at_the_edges() {
        assert_eq!(ease_in_out_cubic(0.0), 0.0);
        assert_eq!(ease_in_out_cubic(1.0), 1.0);
        assert!((ease_in_out_cubic(0.5) - 0.5).abs() < 1e-6);
        assert!(ease_in_out_cubic(0.25) < 0.25);
        assert!(ease_in_out_cubic(0.75) > 0.75);
    }

    #[test]
    fn center_places_a_smaller_image_in_the_middle() {
        let source = unique_grid(2, 2);
        let dest = compose_image(&source, FillMode::Center, 4, 4, [7, 8, 9, 255]);
        assert_eq!(dest.get_pixel(0, 0), &pixel(7, 8, 9));
        assert_eq!(dest.get_pixel(1, 1), &pixel(0, 0, 99));
        assert_eq!(dest.get_pixel(2, 2), &pixel(1, 1, 99));
        assert_eq!(dest.get_pixel(3, 3), &pixel(7, 8, 9));
    }

    #[test]
    fn center_crops_a_larger_image() {
        let source = unique_grid(4, 2);
        let dest = compose_image(&source, FillMode::Center, 2, 2, [0, 0, 0, 255]);
        assert_eq!(dest.get_pixel(0, 0), &pixel(1, 0, 99));
        assert_eq!(dest.get_pixel(1, 0), &pixel(2, 0, 99));
        assert_eq!(dest.get_pixel(0, 1), &pixel(1, 1, 99));
        assert_eq!(dest.get_pixel(1, 1), &pixel(2, 1, 99));
    }

    #[test]
    fn fill_crops_the_overflow_from_the_center() {
        let source = unique_grid(4, 2);
        let dest = compose_image(&source, FillMode::Fill, 2, 2, [0, 0, 0, 255]);
        assert_eq!(dest.get_pixel(0, 0), &pixel(1, 0, 99));
        assert_eq!(dest.get_pixel(1, 0), &pixel(2, 0, 99));
        assert_eq!(dest.get_pixel(0, 1), &pixel(1, 1, 99));
        assert_eq!(dest.get_pixel(1, 1), &pixel(2, 1, 99));
    }

    #[test]
    fn fit_letterboxes_the_unused_edges() {
        let source = unique_grid(4, 2);
        let dest = compose_image(&source, FillMode::Fit, 4, 4, [7, 8, 9, 255]);
        assert_eq!(dest.get_pixel(0, 0), &pixel(7, 8, 9));
        assert_eq!(dest.get_pixel(0, 3), &pixel(7, 8, 9));
        assert_eq!(dest.get_pixel(0, 1), &pixel(0, 0, 99));
        assert_eq!(dest.get_pixel(3, 2), &pixel(3, 1, 99));
    }

    #[test]
    fn stretch_fills_the_whole_buffer() {
        let source = RgbaImage::from_pixel(1, 1, pixel(10, 20, 30));
        let dest = compose_image(&source, FillMode::Stretch, 3, 2, [0, 0, 0, 255]);
        assert_eq!(dest.dimensions(), (3, 2));
        for pixel in dest.pixels() {
            assert_eq!(pixel, &Rgba([10, 20, 30, 255]));
        }
    }

    #[test]
    fn tile_repeats_from_the_origin() {
        let source = unique_grid(2, 2);
        let dest = compose_image(&source, FillMode::Tile, 4, 4, [0, 0, 0, 255]);
        assert_eq!(dest.get_pixel(0, 0), &pixel(0, 0, 99));
        assert_eq!(dest.get_pixel(2, 0), &pixel(0, 0, 99));
        assert_eq!(dest.get_pixel(1, 3), &pixel(1, 1, 99));
        assert_eq!(dest.get_pixel(3, 3), &pixel(1, 1, 99));
    }
}
