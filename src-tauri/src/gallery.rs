use anyhow::{bail, Context, Result};
use reqwest::Url;
use serde::{Deserialize, Serialize};
use std::time::Duration;

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

const ALLOWED_RATIOS: &[&str] = &["landscape", "portrait", "square"];
const ALLOWED_SORTING: &[&str] = &[
    "date_added",
    "relevance",
    "random",
    "views",
    "favorites",
    "toplist",
];

#[derive(Debug, Clone, Default)]
pub struct SearchParams {
    pub query: Option<String>,
    pub ratios: Option<String>,
    pub categories: Option<String>,
    pub sorting: Option<String>,
    pub page: u32,
    pub atleast: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GalleryPhoto {
    pub id: String,
    pub url: String,
    pub resolution: String,
    pub file_size: u64,
    pub thumb: String,
    pub color: Option<String>,
    pub path: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GalleryPage {
    pub photos: Vec<GalleryPhoto>,
    pub page: u32,
    pub last_page: u32,
    pub total: u64,
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
}

#[derive(Debug, Deserialize)]
struct WallhavenThumbs {
    small: String,
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
            pairs.append_pair("topRange", "1M");
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
    let response = http_client(Duration::from_secs(20))?
        .get(url)
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

pub async fn download_image(url: &str) -> Result<Vec<u8>> {
    let parsed = validate_download_url(url)?;
    let response = http_client(Duration::from_secs(120))?
        .get(parsed)
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

    let bytes = response
        .bytes()
        .await
        .context("the download was interrupted")?;
    Ok(bytes.to_vec())
}

fn http_client(timeout: Duration) -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(timeout)
        .build()
        .context("could not create HTTP client")
}

fn is_category_bits(value: &str) -> bool {
    value.len() == 3 && value.bytes().all(|byte| byte == b'0' || byte == b'1')
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
            color: photo.colors.into_iter().next(),
            path: photo.path,
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
        assert_eq!(photo.color.as_deref(), Some("#000000"));
        assert_eq!(parsed.meta.total, 848);
    }
}
