use crate::scanner;
use anyhow::{bail, Context, Result};
use futures_util::StreamExt;
use reqwest::Url;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::io::AsyncWriteExt;

const SEARCH_URL: &str = "https://wallhaven.cc/api/v1/search";
const DOWNLOAD_HOST: &str = "w.wallhaven.cc";
const PURITY_SFW: &str = "100";
const DEFAULT_RATIOS: &str = "landscape";
const DEFAULT_CATEGORIES: &str = "111";
const USER_AGENT: &str = concat!(
    "Wallflow/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/efadhemon/wallflow)"
);

pub const DOWNLOAD_PROGRESS_EVENT: &str = "wallpaper://download-progress";

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const SEARCH_TIMEOUT: Duration = Duration::from_secs(20);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(120);
const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);
const PROGRESS_BYTES: u64 = 128 * 1024;
const MAGIC_LEN: usize = 16;

const ALLOWED_RATIOS: &[&str] = &["landscape", "portrait", "square"];
/// Wallhaven has no "ultrawide" keyword, so it is spelled out as ratio buckets.
const ULTRAWIDE_RATIOS: &str = "21x9,32x9,48x9";
const ALLOWED_SORTING: &[&str] = &[
    "date_added",
    "relevance",
    "random",
    "views",
    "favorites",
    "toplist",
    "hot",
];
const ALLOWED_TOP_RANGES: &[&str] = &["1d", "3d", "1w", "1M", "3M", "6M", "1y"];
const DEFAULT_TOP_RANGE: &str = "1M";

#[derive(Debug, Clone, Default)]
pub struct SearchParams {
    pub query: Option<String>,
    pub ratios: Option<String>,
    pub categories: Option<String>,
    pub sorting: Option<String>,
    pub page: u32,
    pub atleast: Option<String>,
    pub top_range: Option<String>,
    pub colors: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GalleryPhoto {
    pub id: String,
    pub url: String,
    pub resolution: String,
    pub file_size: u64,
    pub thumb: String,
    /// Wider thumbnail shown while the full image loads in the preview.
    pub thumb_large: String,
    pub color: Option<String>,
    pub colors: Vec<String>,
    pub path: String,
    pub views: u64,
    pub favorites: u64,
    pub category: String,
    pub file_type: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GalleryPage {
    pub photos: Vec<GalleryPhoto>,
    pub page: u32,
    pub last_page: u32,
    pub total: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadProgress {
    pub url: String,
    pub received: u64,
    pub total: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct DownloadedImage {
    pub path: PathBuf,
    pub added: bool,
}

#[derive(Debug, Deserialize)]
struct WallhavenSearchResponse {
    data: Vec<WallhavenPhoto>,
    meta: WallhavenMeta,
}

#[derive(Debug, Deserialize)]
struct WallhavenMeta {
    current_page: u32,
    last_page: u32,
    total: u64,
}

#[derive(Debug, Deserialize)]
struct WallhavenPhoto {
    id: String,
    url: String,
    resolution: String,
    file_size: u64,
    path: String,
    colors: Vec<String>,
    thumbs: WallhavenThumbs,
    #[serde(default)]
    views: u64,
    #[serde(default)]
    favorites: u64,
    #[serde(default)]
    category: String,
    #[serde(default)]
    file_type: String,
}

#[derive(Debug, Deserialize)]
struct WallhavenThumbs {
    small: String,
    large: String,
}

/// Join a parent keyword with a subquery. Custom search passes an empty parent.
#[cfg(test)]
fn compose_query(parent: &str, subquery: &str) -> String {
    let parent = parent.trim();
    let subquery = subquery.trim();
    match (parent.is_empty(), subquery.is_empty()) {
        (true, true) => String::new(),
        (true, false) => subquery.to_string(),
        (false, true) => parent.to_string(),
        (false, false) => format!("{parent} {subquery}"),
    }
}

pub fn build_search_url(params: &SearchParams) -> Result<Url> {
    let mut url = Url::parse(SEARCH_URL).context("invalid Wallhaven search URL")?;
    {
        let mut pairs = url.query_pairs_mut();
        pairs.append_pair("purity", PURITY_SFW);

        let query = params
            .query
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        if let Some(query) = query {
            pairs.append_pair("q", query);
        }

        let ratios = params
            .ratios
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        match ratios {
            None => {
                pairs.append_pair("ratios", DEFAULT_RATIOS);
            }
            Some("any") => {}
            Some("ultrawide") => {
                pairs.append_pair("ratios", ULTRAWIDE_RATIOS);
            }
            Some(value) if ALLOWED_RATIOS.contains(&value) => {
                pairs.append_pair("ratios", value);
            }
            Some(value) => bail!("unsupported orientation: {value}"),
        }

        let categories = params
            .categories
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(DEFAULT_CATEGORIES);
        if !is_category_bits(categories) {
            bail!("unsupported categories: {categories}");
        }
        pairs.append_pair("categories", categories);

        let sorting = params
            .sorting
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let sorting = match sorting {
            Some(value) if ALLOWED_SORTING.contains(&value) => value,
            Some(value) => bail!("unsupported sort: {value}"),
            None if query.is_some() => "relevance",
            None => "toplist",
        };
        pairs.append_pair("sorting", sorting);

        if sorting == "toplist" {
            let range = params
                .top_range
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or(DEFAULT_TOP_RANGE);
            if !ALLOWED_TOP_RANGES.contains(&range) {
                bail!("unsupported top range: {range}");
            }
            pairs.append_pair("topRange", range);
        }

        if let Some(colors) = params
            .colors
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            if !colors.split(',').all(is_hex_color) {
                bail!("unsupported color: {colors}");
            }
            pairs.append_pair("colors", colors);
        }

        let page = params.page.max(1);
        pairs.append_pair("page", &page.to_string());

        if let Some(atleast) = params
            .atleast
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty() && *value != "any")
        {
            if !is_resolution(atleast) {
                bail!("unsupported minimum resolution: {atleast}");
            }
            pairs.append_pair("atleast", atleast);
        }
    }

    Ok(url)
}

pub fn validate_download_url(url: &str) -> Result<Url> {
    let parsed = Url::parse(url).context("invalid image URL")?;
    if parsed.scheme() != "https" {
        bail!("only https image URLs are allowed");
    }
    if parsed.host_str() != Some(DOWNLOAD_HOST) {
        bail!("that image is not from Wallhaven");
    }
    if !parsed.path().starts_with("/full/") {
        bail!("that image is not from Wallhaven");
    }
    Ok(parsed)
}

pub async fn search(params: SearchParams) -> Result<GalleryPage> {
    let url = build_search_url(&params)?;
    let response = http_client()?
        .get(url)
        .timeout(SEARCH_TIMEOUT)
        .header(reqwest::header::ACCEPT, "application/json")
        .send()
        .await
        .context("could not reach Wallhaven")?;

    let status = response.status();
    if status.as_u16() == 429 {
        bail!("Wallhaven is rate-limiting requests. Try again in a minute.");
    }
    if !status.is_success() {
        bail!("Wallhaven returned {status}");
    }

    let body: WallhavenSearchResponse = response
        .json()
        .await
        .context("Wallhaven sent a response that could not be read")?;

    Ok(GalleryPage {
        photos: body.data.into_iter().map(GalleryPhoto::from).collect(),
        page: body.meta.current_page,
        last_page: body.meta.last_page,
        total: body.meta.total,
    })
}

pub async fn download_and_save(
    url: &str,
    images_dir: &Path,
    expected_size: Option<u64>,
    mut on_progress: impl FnMut(DownloadProgress),
) -> Result<DownloadedImage> {
    let parsed = validate_download_url(url)?;
    match download_once(
        url,
        parsed.clone(),
        images_dir,
        expected_size,
        &mut on_progress,
    )
    .await
    {
        Ok(saved) => Ok(saved),
        Err(err) if is_retryable(&err) => {
            log::warn!("download interrupted, retrying once: {err:#}");
            download_once(url, parsed, images_dir, expected_size, &mut on_progress).await
        }
        Err(err) => Err(err),
    }
}

async fn download_once(
    url: &str,
    parsed: Url,
    images_dir: &Path,
    expected_size: Option<u64>,
    on_progress: &mut impl FnMut(DownloadProgress),
) -> Result<DownloadedImage> {
    let response = http_client()?
        .get(parsed)
        .timeout(DOWNLOAD_TIMEOUT)
        .header(reqwest::header::ACCEPT, "image/*")
        .header(reqwest::header::REFERER, "https://wallhaven.cc/")
        .send()
        .await
        .context("could not download the wallpaper")?;

    let status = response.status();
    if status.as_u16() == 429 {
        bail!("Wallhaven is rate-limiting requests. Try again in a minute.");
    }
    if !status.is_success() {
        bail!("Wallhaven returned {status}");
    }

    let total = response
        .content_length()
        .filter(|len| *len > 0)
        .or_else(|| expected_size.filter(|len| *len > 0));

    emit_progress(on_progress, url, 0, total);

    tokio::fs::create_dir_all(images_dir)
        .await
        .with_context(|| format!("could not create {}", images_dir.display()))?;

    let temp_path = temp_download_path(images_dir);
    let mut temp = TempFile::new(temp_path);
    let mut file = tokio::fs::File::create(&temp.path)
        .await
        .with_context(|| format!("could not create {}", temp.path.display()))?;

    let mut hasher = Sha256::new();
    let mut header = Vec::with_capacity(MAGIC_LEN);
    let mut received = 0u64;
    let mut last_emit = Instant::now();
    let mut last_emit_bytes = 0u64;
    let mut stream = response.bytes_stream();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.context("the download was interrupted")?;
        if header.len() < MAGIC_LEN {
            let take = (MAGIC_LEN - header.len()).min(chunk.len());
            header.extend_from_slice(&chunk[..take]);
            if header.len() >= MAGIC_LEN && scanner::extension_for_bytes(&header).is_none() {
                bail!("That file is not a supported image (JPG, PNG, WebP or BMP).");
            }
        }
        hasher.update(&chunk);
        file.write_all(&chunk)
            .await
            .context("could not save the image")?;
        received += chunk.len() as u64;

        if last_emit.elapsed() >= PROGRESS_INTERVAL || received - last_emit_bytes >= PROGRESS_BYTES
        {
            emit_progress(on_progress, url, received, total);
            last_emit = Instant::now();
            last_emit_bytes = received;
        }
    }

    file.flush().await.context("could not save the image")?;
    drop(file);

    if received == 0 {
        bail!("the download was interrupted");
    }

    let Some(ext) = scanner::extension_for_bytes(&header) else {
        bail!("That file is not a supported image (JPG, PNG, WebP or BMP).");
    };

    let dest = images_dir.join(format!("{}.{ext}", hex_lower(&hasher.finalize())));
    emit_progress(on_progress, url, received, total.or(Some(received)));

    if dest.exists() {
        return Ok(DownloadedImage {
            path: dest,
            added: false,
        });
    }

    match tokio::fs::rename(&temp.path, &dest).await {
        Ok(()) => {
            temp.persist();
            Ok(DownloadedImage {
                path: dest,
                added: true,
            })
        }
        Err(err) if err.kind() == ErrorKind::AlreadyExists || dest.exists() => {
            Ok(DownloadedImage {
                path: dest,
                added: false,
            })
        }
        Err(err) => {
            Err(err).with_context(|| format!("could not save the image to {}", dest.display()))
        }
    }
}

fn emit_progress(
    on_progress: &mut impl FnMut(DownloadProgress),
    url: &str,
    received: u64,
    total: Option<u64>,
) {
    on_progress(DownloadProgress {
        url: url.to_string(),
        received,
        total,
    });
}

fn is_retryable(err: &anyhow::Error) -> bool {
    let message = format!("{err:#}");
    if message.contains("rate-limiting") || message.contains("Wallhaven returned") {
        return false;
    }
    if message.contains("not a supported image") {
        return false;
    }
    err.chain().any(|cause| {
        cause.downcast_ref::<reqwest::Error>().is_some_and(|error| {
            error.is_timeout() || error.is_connect() || error.is_body() || error.is_request()
        }) || cause.downcast_ref::<std::io::Error>().is_some()
    }) || message.contains("interrupted")
}

fn temp_download_path(images_dir: &Path) -> PathBuf {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    images_dir.join(format!(".download-{nanos}-{n}.part"))
}

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn http_client() -> Result<&'static reqwest::Client> {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    if let Some(client) = CLIENT.get() {
        return Ok(client);
    }
    let client = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(DOWNLOAD_TIMEOUT)
        .pool_idle_timeout(Duration::from_secs(90))
        .build()
        .context("could not create HTTP client")?;
    Ok(CLIENT.get_or_init(|| client))
}

struct TempFile {
    path: PathBuf,
    persist: bool,
}

impl TempFile {
    fn new(path: PathBuf) -> Self {
        Self {
            path,
            persist: false,
        }
    }

    fn persist(&mut self) {
        self.persist = true;
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        if !self.persist {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

fn is_category_bits(value: &str) -> bool {
    value.len() == 3 && value.bytes().all(|byte| byte == b'0' || byte == b'1')
}

fn is_hex_color(value: &str) -> bool {
    value.len() == 6 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn is_resolution(value: &str) -> bool {
    let mut parts = value.split('x');
    matches!(
        (parts.next(), parts.next(), parts.next()),
        (Some(width), Some(height), None)
            if width.parse::<u32>().is_ok_and(|n| n > 0)
                && height.parse::<u32>().is_ok_and(|n| n > 0)
    )
}

impl From<WallhavenPhoto> for GalleryPhoto {
    fn from(photo: WallhavenPhoto) -> Self {
        Self {
            id: photo.id,
            url: photo.url,
            resolution: photo.resolution,
            file_size: photo.file_size,
            thumb: photo.thumbs.small,
            thumb_large: photo.thumbs.large,
            color: photo.colors.first().cloned(),
            colors: photo.colors,
            path: photo.path,
            views: photo.views,
            favorites: photo.favorites,
            category: photo.category,
            file_type: photo.file_type,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compose_joins_parent_and_subquery() {
        assert_eq!(compose_query("nature", "forest"), "nature forest");
        assert_eq!(compose_query("nature", ""), "nature");
        assert_eq!(compose_query("nature", "  forest  "), "nature forest");
        assert_eq!(
            compose_query("\"fantasy art\"", "castle"),
            "\"fantasy art\" castle"
        );
    }

    #[test]
    fn compose_custom_search_omits_parent() {
        assert_eq!(compose_query("", "mountains at dusk"), "mountains at dusk");
        assert_eq!(compose_query("   ", ""), "");
        assert_eq!(compose_query("", ""), "");
    }

    #[test]
    fn search_url_defaults_to_landscape_sfw() {
        let url = build_search_url(&SearchParams {
            query: Some("nature".into()),
            page: 1,
            ..SearchParams::default()
        })
        .unwrap();
        let query: Vec<(String, String)> = url
            .query_pairs()
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect();

        assert!(query.contains(&("purity".into(), "100".into())));
        assert!(query.contains(&("ratios".into(), "landscape".into())));
        assert!(query.contains(&("categories".into(), "111".into())));
        assert!(query.contains(&("q".into(), "nature".into())));
        assert!(query.contains(&("sorting".into(), "relevance".into())));
        assert!(!query.iter().any(|(key, _)| key == "atleast"));
    }

    #[test]
    fn empty_custom_search_uses_toplist_without_q() {
        let url = build_search_url(&SearchParams {
            page: 1,
            atleast: Some("1920x1080".into()),
            ..SearchParams::default()
        })
        .unwrap();
        let query: Vec<(String, String)> = url
            .query_pairs()
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect();

        assert!(!query.iter().any(|(key, _)| key == "q"));
        assert!(query.contains(&("sorting".into(), "toplist".into())));
        assert!(query.contains(&("topRange".into(), "1M".into())));
        assert!(query.contains(&("atleast".into(), "1920x1080".into())));
        assert!(query.contains(&("purity".into(), "100".into())));
    }

    #[test]
    fn any_orientation_omits_landscape_default() {
        let url = build_search_url(&SearchParams {
            query: Some("city".into()),
            ratios: Some("any".into()),
            page: 1,
            ..SearchParams::default()
        })
        .unwrap();
        let query: Vec<(String, String)> = url
            .query_pairs()
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect();
        assert!(!query.iter().any(|(key, _)| key == "ratios"));
    }

    #[test]
    fn toplist_range_ultrawide_and_colors_are_forwarded() {
        let url = build_search_url(&SearchParams {
            ratios: Some("ultrawide".into()),
            sorting: Some("toplist".into()),
            top_range: Some("1w".into()),
            colors: Some("0066cc".into()),
            page: 1,
            ..SearchParams::default()
        })
        .unwrap();
        let query: Vec<(String, String)> = url
            .query_pairs()
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect();
        assert!(query.contains(&("ratios".into(), "21x9,32x9,48x9".into())));
        assert!(query.contains(&("topRange".into(), "1w".into())));
        assert!(query.contains(&("colors".into(), "0066cc".into())));
    }

    #[test]
    fn hot_sorting_is_allowed_and_bad_values_are_rejected() {
        assert!(build_search_url(&SearchParams {
            sorting: Some("hot".into()),
            page: 1,
            ..SearchParams::default()
        })
        .is_ok());
        assert!(build_search_url(&SearchParams {
            sorting: Some("toplist".into()),
            top_range: Some("5y".into()),
            page: 1,
            ..SearchParams::default()
        })
        .is_err());
        assert!(build_search_url(&SearchParams {
            colors: Some("blue".into()),
            page: 1,
            ..SearchParams::default()
        })
        .is_err());
    }

    #[test]
    fn rejects_unknown_orientation() {
        let err = build_search_url(&SearchParams {
            ratios: Some("wide".into()),
            page: 1,
            ..SearchParams::default()
        })
        .unwrap_err();
        assert!(err.to_string().contains("unsupported orientation"));
    }

    #[test]
    fn download_url_must_be_wallhaven_https() {
        assert!(
            validate_download_url("https://w.wallhaven.cc/full/94/wallhaven-94x38z.jpg").is_ok()
        );
        assert!(
            validate_download_url("http://w.wallhaven.cc/full/94/wallhaven-94x38z.jpg").is_err()
        );
        assert!(
            validate_download_url("https://evil.example/full/94/wallhaven-94x38z.jpg").is_err()
        );
        assert!(
            validate_download_url("https://w.wallhaven.cc.evil.example/full/94/a.jpg").is_err()
        );
        assert!(validate_download_url("https://w.wallhaven.cc/thumbs/94/a.jpg").is_err());
    }

    #[test]
    fn parses_search_json() {
        let raw = r##"{
            "data": [{
                "id": "94x38z",
                "url": "https://wallhaven.cc/w/94x38z",
                "resolution": "6742x3534",
                "file_size": 5070446,
                "path": "https://w.wallhaven.cc/full/94/wallhaven-94x38z.jpg",
                "colors": ["#000000", "#abbcda"],
                "thumbs": {
                    "large": "https://th.wallhaven.cc/lg/94/94x38z.jpg",
                    "original": "https://th.wallhaven.cc/orig/94/94x38z.jpg",
                    "small": "https://th.wallhaven.cc/small/94/94x38z.jpg"
                }
            }],
            "meta": {
                "current_page": 1,
                "last_page": 36,
                "per_page": 24,
                "total": 848
            }
        }"##;

        let parsed: WallhavenSearchResponse = serde_json::from_str(raw).unwrap();
        let photo = GalleryPhoto::from(parsed.data.into_iter().next().unwrap());
        assert_eq!(photo.id, "94x38z");
        assert_eq!(photo.thumb, "https://th.wallhaven.cc/small/94/94x38z.jpg");
        assert_eq!(
            photo.thumb_large,
            "https://th.wallhaven.cc/lg/94/94x38z.jpg"
        );
        assert_eq!(photo.color.as_deref(), Some("#000000"));
        assert_eq!(parsed.meta.total, 848);
    }
}
