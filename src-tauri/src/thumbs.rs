use anyhow::{Context, Result};
use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

/// Wide enough to stay sharp as the dashboard preview, small enough that a grid of
/// them decodes instantly.
const THUMB_MAX_EDGE: u32 = 800;
/// Sharp on a 1440p screen without caching full 8K originals a second time.
const PREVIEW_MAX_EDGE: u32 = 2560;

pub const THUMBS_DIR: &str = "thumbnails";
pub const PREVIEWS_DIR: &str = "previews";
pub const CONVERTED_DIR: &str = "converted";

/// Identifies a file by path, size and mtime so an edited image gets a new cache
/// entry instead of a stale one.
fn cache_key(path: &Path) -> Result<String> {
    let metadata =
        fs::metadata(path).with_context(|| format!("reading metadata of {}", path.display()))?;

    let mut hasher = DefaultHasher::new();
    path.hash(&mut hasher);
    metadata.len().hash(&mut hasher);
    if let Ok(modified) = metadata.modified() {
        if let Ok(since_epoch) = modified.duration_since(std::time::UNIX_EPOCH) {
            since_epoch.as_secs().hash(&mut hasher);
        }
    }
    Ok(format!("{:016x}", hasher.finish()))
}

fn extension(path: &Path) -> String {
    path.extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
}

/// Returns a path the platform can actually render. Formats the desktop cannot
/// display natively (WebP above all) are decoded once and cached as PNG.
pub fn prepare_for_wallpaper(
    cache_dir: &Path,
    source: &Path,
    native_extensions: &[&str],
) -> Result<PathBuf> {
    if native_extensions.contains(&extension(source).as_str()) {
        return Ok(source.to_path_buf());
    }

    let dir = cache_dir.join(CONVERTED_DIR);
    fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;

    let target = dir.join(format!("{}.png", cache_key(source)?));
    if target.is_file() {
        return Ok(target);
    }

    let image = image::open(source).with_context(|| format!("decoding {}", source.display()))?;
    crate::config::write_atomic(&target, |temp| {
        image
            .save_with_format(temp, image::ImageFormat::Png)
            .map_err(Into::into)
    })
    .with_context(|| format!("writing {}", target.display()))?;

    log::info!("converted {} to {}", source.display(), target.display());
    Ok(target)
}

/// Cached, downscaled JPEG preview used everywhere in the UI.
pub fn thumbnail(cache_dir: &Path, source: &Path) -> Result<PathBuf> {
    resized(cache_dir, THUMBS_DIR, THUMB_MAX_EDGE, source)
}

/// Large JPEG for the full-screen preview.
pub fn preview(cache_dir: &Path, source: &Path) -> Result<PathBuf> {
    resized(cache_dir, PREVIEWS_DIR, PREVIEW_MAX_EDGE, source)
}

fn resized(cache_dir: &Path, sub: &str, max_edge: u32, source: &Path) -> Result<PathBuf> {
    let dir = cache_dir.join(sub);
    fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;

    let target = dir.join(format!("{}.jpg", cache_key(source)?));
    if target.is_file() {
        return Ok(target);
    }

    let preview = open_upright(source)
        .with_context(|| format!("decoding {}", source.display()))?
        .thumbnail(max_edge, max_edge)
        .to_rgb8();
    crate::config::write_atomic(&target, |temp| {
        preview
            .save_with_format(temp, image::ImageFormat::Jpeg)
            .map_err(Into::into)
    })
    .with_context(|| format!("writing {}", target.display()))?;

    Ok(target)
}

/// Decodes an image and turns it the way its EXIF orientation says, so photos
/// straight off a phone are not shown on their side.
fn open_upright(source: &Path) -> Result<image::DynamicImage> {
    use image::ImageDecoder;

    let mut decoder = image::ImageReader::open(source)?
        .with_guessed_format()?
        .into_decoder()?;
    let orientation = decoder.orientation()?;
    let mut image = image::DynamicImage::from_decoder(decoder)?;
    image.apply_orientation(orientation);
    Ok(image)
}

/// Drops cache entries whose source image is gone or has been edited, so removing a
/// folder eventually reclaims its disk space.
pub fn prune(cache_dir: &Path, keep: &[PathBuf]) {
    let live: std::collections::HashSet<String> = keep
        .iter()
        .filter_map(|path| cache_key(path).ok())
        .collect();

    for sub in [THUMBS_DIR, PREVIEWS_DIR, CONVERTED_DIR] {
        let dir = cache_dir.join(sub);
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            let stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default();
            // Dot files are writes still in flight.
            if path.is_file() && !stem.starts_with('.') && !live.contains(stem) {
                let _ = fs::remove_file(path);
            }
        }
    }
}
