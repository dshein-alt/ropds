#[cfg(debug_assertions)]
use std::path::Path as FsPath;

#[cfg(not(debug_assertions))]
use std::collections::HashMap;
#[cfg(not(debug_assertions))]
use std::sync::LazyLock;

use axum::body::Body;
#[cfg(not(debug_assertions))]
use axum::body::Bytes;
use axum::extract::Path;
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
#[cfg(not(debug_assertions))]
use include_dir::{Dir, include_dir};
use sha2::{Digest, Sha256};

const STATIC_CACHE_CONTROL: &str = "public, max-age=3600";

#[cfg(not(debug_assertions))]
static EMBEDDED_ASSETS: Dir<'_> = include_dir!("$OUT_DIR/embedded_assets");

#[cfg(not(debug_assertions))]
mod embedded_static_metadata {
    include!(concat!(env!("OUT_DIR"), "/embedded_static_metadata.rs"));
}

#[cfg(not(debug_assertions))]
static EMBEDDED_STATIC_FILES: LazyLock<HashMap<String, EmbeddedStaticFile>> = LazyLock::new(|| {
    let mut files = HashMap::new();
    if let Some(static_dir) = EMBEDDED_ASSETS.get_dir("static") {
        collect_embedded_static_files(static_dir, "", &mut files);
    }
    files
});

#[cfg(not(debug_assertions))]
struct EmbeddedStaticFile {
    bytes: Bytes,
    content_type: String,
    etag: String,
}

#[cfg(debug_assertions)]
pub fn load_templates() -> Result<tera::Tera, tera::Error> {
    tera::Tera::new("templates/**/*.html")
}

#[cfg(not(debug_assertions))]
pub fn load_templates() -> Result<tera::Tera, tera::Error> {
    let templates_dir = EMBEDDED_ASSETS
        .get_dir("templates")
        .ok_or_else(|| tera::Error::msg("embedded templates directory is missing"))?;

    let mut templates = Vec::new();
    collect_templates(templates_dir, "", &mut templates)?;
    templates.sort_by(|a, b| a.0.cmp(&b.0));

    let mut tera = tera::Tera::default();
    let refs: Vec<(&str, &str)> = templates
        .iter()
        .map(|(name, content)| (name.as_str(), content.as_str()))
        .collect();
    tera.add_raw_templates(refs)?;
    Ok(tera)
}

#[cfg(not(debug_assertions))]
fn collect_templates(
    dir: &Dir<'_>,
    prefix: &str,
    out: &mut Vec<(String, String)>,
) -> Result<(), tera::Error> {
    for file in dir.files() {
        let file_name = file
            .path()
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| tera::Error::msg("embedded template has invalid UTF-8 file name"))?;

        let template_name = if prefix.is_empty() {
            file_name.to_string()
        } else {
            format!("{prefix}/{file_name}")
        };

        let content = std::str::from_utf8(file.contents())
            .map_err(|e| tera::Error::msg(format!("template {template_name} is not UTF-8: {e}")))?
            .to_string();
        out.push((template_name, content));
    }

    for child in dir.dirs() {
        let child_name = child
            .path()
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| tera::Error::msg("embedded template dir has invalid UTF-8 name"))?;
        let child_prefix = if prefix.is_empty() {
            child_name.to_string()
        } else {
            format!("{prefix}/{child_name}")
        };
        collect_templates(child, &child_prefix, out)?;
    }

    Ok(())
}

pub async fn static_asset(Path(path): Path<String>, headers: HeaderMap) -> Response {
    #[cfg(debug_assertions)]
    {
        debug_static_asset(path, headers).await
    }

    #[cfg(not(debug_assertions))]
    {
        embedded_static_asset(path, headers)
    }
}

#[cfg(debug_assertions)]
async fn debug_static_asset(path: String, headers: HeaderMap) -> Response {
    let if_none_match = headers
        .get(header::IF_NONE_MATCH)
        .and_then(|value| value.to_str().ok());

    let Some(normalized) = normalize_static_path(&path) else {
        return StatusCode::NOT_FOUND.into_response();
    };

    let full_path = FsPath::new("static").join(&normalized);
    let bytes = match tokio::fs::read(&full_path).await {
        Ok(data) => data,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return StatusCode::NOT_FOUND.into_response();
        }
        Err(error) => {
            tracing::error!(
                "Failed to read static asset {}: {}",
                full_path.display(),
                error
            );
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let etag = build_etag(&bytes);
    if matches_if_none_match(if_none_match, &etag) {
        return not_modified_response(&etag);
    }

    let content_type = mime_guess::from_path(&normalized)
        .first_or_octet_stream()
        .essence_str()
        .to_string();
    let content_length = bytes.len();

    ok_response(Body::from(bytes), &content_type, &etag, content_length)
}

#[cfg(not(debug_assertions))]
fn embedded_static_asset(path: String, headers: HeaderMap) -> Response {
    let if_none_match = headers
        .get(header::IF_NONE_MATCH)
        .and_then(|value| value.to_str().ok());

    let Some(normalized) = normalize_static_path(&path) else {
        return StatusCode::NOT_FOUND.into_response();
    };

    let Some(asset) = EMBEDDED_STATIC_FILES.get(&normalized) else {
        return StatusCode::NOT_FOUND.into_response();
    };

    if matches_if_none_match(if_none_match, &asset.etag) {
        return not_modified_response(&asset.etag);
    }

    let content_length = asset.bytes.len();
    ok_response(
        Body::from(asset.bytes.clone()),
        asset.content_type.as_str(),
        asset.etag.as_str(),
        content_length,
    )
}

#[cfg(not(debug_assertions))]
fn collect_embedded_static_files(
    dir: &'static Dir<'static>,
    prefix: &str,
    out: &mut HashMap<String, EmbeddedStaticFile>,
) {
    for file in dir.files() {
        let Some(file_name) = file.path().file_name().and_then(|name| name.to_str()) else {
            continue;
        };

        let relative = if prefix.is_empty() {
            file_name.to_string()
        } else {
            format!("{prefix}/{file_name}")
        };

        let bytes = Bytes::from_static(file.contents());
        let content_type = mime_guess::from_path(&relative)
            .first_or_octet_stream()
            .essence_str()
            .to_string();
        let etag = embedded_static_metadata::etag_for_path(&relative)
            .map(|value| value.to_string())
            .unwrap_or_else(|| {
                tracing::warn!(
                    "Missing generated ETag metadata for embedded static asset {}",
                    relative
                );
                build_etag(&bytes)
            });

        out.insert(
            relative,
            EmbeddedStaticFile {
                bytes,
                content_type,
                etag,
            },
        );
    }

    for child in dir.dirs() {
        let Some(dir_name) = child.path().file_name().and_then(|name| name.to_str()) else {
            continue;
        };

        let child_prefix = if prefix.is_empty() {
            dir_name.to_string()
        } else {
            format!("{prefix}/{dir_name}")
        };

        collect_embedded_static_files(child, &child_prefix, out);
    }
}

fn not_modified_response(etag: &str) -> Response {
    let mut response = Response::new(Body::empty());
    *response.status_mut() = StatusCode::NOT_MODIFIED;
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static(STATIC_CACHE_CONTROL),
    );
    if let Ok(value) = HeaderValue::from_str(etag) {
        response.headers_mut().insert(header::ETAG, value);
    }
    response
}

fn ok_response(body: Body, content_type: &str, etag: &str, content_length: usize) -> Response {
    let mut response = Response::new(body);
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static(STATIC_CACHE_CONTROL),
    );
    if let Ok(value) = HeaderValue::from_str(content_type) {
        response.headers_mut().insert(header::CONTENT_TYPE, value);
    }
    if let Ok(value) = HeaderValue::from_str(etag) {
        response.headers_mut().insert(header::ETAG, value);
    }
    if let Ok(value) = HeaderValue::from_str(&content_length.to_string()) {
        response.headers_mut().insert(header::CONTENT_LENGTH, value);
    }
    response
}

pub(crate) fn normalize_static_path(path: &str) -> Option<String> {
    let trimmed = path.trim_matches('/');
    if trimmed.is_empty() || trimmed.contains('\\') {
        return None;
    }

    let mut normalized = Vec::new();
    for segment in trimmed.split('/') {
        if segment.is_empty() || segment == "." || segment == ".." {
            return None;
        }
        normalized.push(segment);
    }

    Some(normalized.join("/"))
}

fn build_etag(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("\"{}\"", hex::encode(hasher.finalize()))
}

pub(crate) fn matches_if_none_match(if_none_match: Option<&str>, etag: &str) -> bool {
    let expected = strip_weak_etag(etag);
    if_none_match.is_some_and(|header_value| {
        header_value.split(',').any(|candidate| {
            let candidate = candidate.trim();
            candidate == "*" || strip_weak_etag(candidate) == expected
        })
    })
}

fn strip_weak_etag(value: &str) -> &str {
    value.strip_prefix("W/").unwrap_or(value).trim()
}

#[cfg(test)]
mod tests {
    use super::{build_etag, matches_if_none_match, normalize_static_path};

    #[test]
    fn normalize_static_path_accepts_valid_segments() {
        assert_eq!(
            normalize_static_path("js/ropds.js").as_deref(),
            Some("js/ropds.js")
        );
        assert_eq!(
            normalize_static_path("/css/ropds.css/").as_deref(),
            Some("css/ropds.css")
        );
    }

    #[test]
    fn normalize_static_path_rejects_traversal_or_invalid_segments() {
        assert_eq!(normalize_static_path(""), None);
        assert_eq!(normalize_static_path("../js/ropds.js"), None);
        assert_eq!(normalize_static_path("js/../ropds.js"), None);
        assert_eq!(normalize_static_path("js//ropds.js"), None);
        assert_eq!(normalize_static_path(r"js\ropds.js"), None);
    }

    #[test]
    fn etag_matching_handles_strong_and_weak_values() {
        let etag = build_etag(b"asset-data");
        assert!(matches_if_none_match(Some(etag.as_str()), &etag));
        assert!(matches_if_none_match(Some(&format!("W/{etag}")), &etag));
        assert!(matches_if_none_match(Some("*"), &etag));
        assert!(!matches_if_none_match(Some("\"different\""), &etag));
    }
}
