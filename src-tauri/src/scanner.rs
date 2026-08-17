use crate::config::Folder;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

pub const SUPPORTED_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "webp", "bmp"];

pub fn is_supported(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
        .is_some_and(|ext| SUPPORTED_EXTENSIONS.contains(&ext.as_str()))
}

/// File extension to use when copying a recognised image into the managed directory.
pub fn extension_for_bytes(bytes: &[u8]) -> Option<&'static str> {
    match image::guess_format(bytes).ok()? {
        image::ImageFormat::Jpeg => Some("jpg"),
        image::ImageFormat::Png => Some("png"),
        image::ImageFormat::WebP => Some("webp"),
        image::ImageFormat::Bmp => Some("bmp"),
        _ => None,
    }
}

fn is_hidden(entry: &walkdir::DirEntry) -> bool {
    entry.depth() > 0
        && entry
            .file_name()
            .to_str()
            .is_some_and(|name| name.starts_with('.'))
}

/// Every supported image across all configured folders, sorted and de-duplicated
/// so the sequential order is stable and overlapping folders do not double up.
pub fn scan(folders: &[Folder]) -> Vec<PathBuf> {
    let mut images = Vec::new();

    for folder in folders {
        if !folder.path.is_dir() {
            log::warn!("skipping missing folder {}", folder.path.display());
            continue;
        }

        let walker = WalkDir::new(&folder.path)
            .max_depth(if folder.recursive { usize::MAX } else { 1 })
            .follow_links(false)
            .into_iter()
            .filter_entry(|entry| !is_hidden(entry));

        for entry in walker.filter_map(Result::ok) {
            if entry.file_type().is_file() && is_supported(entry.path()) {
                images.push(entry.into_path());
            }
        }
    }

    images.sort();
    images.dedup();
    images
}

/// Number of images a single folder contributes, used for the folder list in the UI.
pub fn count(folder: &Folder) -> usize {
    scan(std::slice::from_ref(folder)).len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extension_matching_is_case_insensitive() {
        assert!(is_supported(Path::new("/a/b.JPG")));
        assert!(is_supported(Path::new("/a/b.webp")));
        assert!(!is_supported(Path::new("/a/b.txt")));
        assert!(!is_supported(Path::new("/a/b")));
    }

    #[test]
    fn extension_follows_magic_bytes_not_the_filename() {
        assert_eq!(extension_for_bytes(&[0xFF, 0xD8, 0xFF, 0xE0]), Some("jpg"));
        assert_eq!(
            extension_for_bytes(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]),
            Some("png")
        );
        assert_eq!(extension_for_bytes(b"BM"), Some("bmp"));
        assert_eq!(extension_for_bytes(b"not an image"), None);
    }
}
