use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde_json::{Value, json};

use crate::db::models::Book;
use crate::db::queries::{authors, genres};
use crate::state::AppState;

pub const OPDS2_JSON: &str = "application/opds+json; charset=utf-8";
pub const OPDS2_TYPE: &str = "application/opds+json";
pub const DEFAULT_MODIFIED: &str = "2024-01-01T00:00:00Z";
pub const REL_ACQUISITION: &str = "http://opds-spec.org/acquisition/open-access";

pub fn opds2_response(body: Value) -> Response {
    match serde_json::to_vec(&body) {
        Ok(bytes) => (StatusCode::OK, [(header::CONTENT_TYPE, OPDS2_JSON)], bytes).into_response(),
        Err(_) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "JSON serialization error",
        ),
    }
}

pub fn error_response(status: StatusCode, msg: &str) -> Response {
    (status, msg.to_string()).into_response()
}

fn normalize_locale_code(locale: &str) -> Option<String> {
    let normalized = locale.trim().to_lowercase();
    if normalized.is_empty() {
        return None;
    }
    if normalized
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-')
    {
        Some(normalized)
    } else {
        None
    }
}

pub fn detect_opds_lang(
    headers: &HeaderMap,
    config: &crate::config::Config,
    query_lang: Option<&str>,
) -> String {
    if let Some(lang) = query_lang.and_then(normalize_locale_code) {
        return lang;
    }
    if let Some(accept_lang) = headers.get("accept-language").and_then(|v| v.to_str().ok()) {
        let primary = accept_lang.split(',').next().unwrap_or("en");
        let lang = primary.split(&['-', ';'][..]).next().unwrap_or("en").trim();
        if let Some(lang) = normalize_locale_code(lang) {
            return lang;
        }
    }
    normalize_locale_code(&config.web.language).unwrap_or_else(|| "en".to_string())
}

pub fn tr(state: &AppState, lang: &str, section: &str, key: &str, fallback: &str) -> String {
    let locale = crate::web::i18n::get_locale(state.translations.as_ref(), lang);
    locale
        .get(section)
        .and_then(|v| v.get(key))
        .and_then(|v| v.as_str())
        .unwrap_or(fallback)
        .to_string()
}

pub fn locale_label(state: &AppState, locale: &str) -> String {
    if let Some(v) = state.translations.get(locale)
        && let Some(label) = v
            .get("lang")
            .and_then(|s| s.get(locale))
            .and_then(|s| s.as_str())
    {
        return label.to_string();
    }
    match locale {
        "en" => "English".to_string(),
        "ru" => "Русский".to_string(),
        _ => locale.to_uppercase(),
    }
}

pub fn locale_choices(state: &AppState) -> Vec<String> {
    let mut locales: Vec<String> = state
        .translations
        .keys()
        .filter_map(|l| normalize_locale_code(l))
        .collect();
    if locales.is_empty() {
        locales.push(
            normalize_locale_code(&state.config.web.language).unwrap_or_else(|| "en".to_string()),
        );
    }
    locales.sort();
    locales.dedup();
    locales
}

pub fn add_lang_query(href: &str, lang: &str) -> String {
    let encoded = urlencoding::encode(lang);
    if href.contains('?') {
        format!("{href}&lang={encoded}")
    } else {
        format!("{href}?lang={encoded}")
    }
}

pub fn nav_link(title: String, href: String) -> Value {
    json!({
        "title": title,
        "href": href,
        "type": OPDS2_TYPE
    })
}

pub fn feed_links(self_href: String, start_href: String, lang: &str) -> Vec<Value> {
    vec![
        json!({
            "rel": "self",
            "href": self_href,
            "type": OPDS2_TYPE
        }),
        json!({
            "rel": "start",
            "href": start_href,
            "type": OPDS2_TYPE
        }),
        json!({
            "rel": "search",
            "href": add_lang_query("/opds/v2/search/{searchTerms}/", lang),
            "type": OPDS2_TYPE,
            "templated": true
        }),
    ]
}

pub async fn book_publication(state: &AppState, book: &Book, lang: &str) -> Value {
    let mut metadata = serde_json::Map::new();
    metadata.insert("identifier".to_string(), json!(format!("b:{}", book.id)));
    metadata.insert("title".to_string(), json!(book.title));
    metadata.insert("modified".to_string(), json!(book.reg_date));
    if !book.lang.is_empty() {
        metadata.insert("language".to_string(), json!([book.lang.clone()]));
    }
    if !book.docdate.is_empty() {
        metadata.insert("published".to_string(), json!(book.docdate));
    }
    if !book.annotation.is_empty() {
        metadata.insert("description".to_string(), json!(book.annotation));
    }

    if let Ok(book_authors) = authors::get_for_book(&state.db, book.id).await
        && !book_authors.is_empty()
    {
        let author_list: Vec<Value> = book_authors
            .iter()
            .map(|a| json!({ "name": a.full_name }))
            .collect();
        metadata.insert("author".to_string(), Value::Array(author_list));
    }

    if let Ok(book_genres) = genres::get_for_book(&state.db, book.id, lang).await
        && !book_genres.is_empty()
    {
        let subjects: Vec<Value> = book_genres
            .iter()
            .map(|g| {
                json!({
                    "name": g.subsection,
                    "code": g.code
                })
            })
            .collect();
        metadata.insert("subject".to_string(), Value::Array(subjects));
    }

    let mut links = vec![json!({
        "rel": REL_ACQUISITION,
        "href": format!("/opds/download/{}/0/", book.id),
        "type": super::super::v1::xml::mime_for_format(&book.format)
    })];

    if !super::super::v1::xml::is_nozip_format(&book.format) {
        links.push(json!({
            "rel": REL_ACQUISITION,
            "href": format!("/opds/download/{}/1/", book.id),
            "type": super::super::v1::xml::mime_for_zip(&book.format)
        }));
    }

    let mut images = Vec::new();
    if book.cover != 0 {
        images.push(json!({
            "href": format!("/opds/cover/{}/", book.id),
            "type": "image/jpeg"
        }));
        images.push(json!({
            "href": format!("/opds/thumb/{}/", book.id),
            "type": "image/jpeg",
            "width": 200,
            "height": 200
        }));
    }

    let mut pub_obj = serde_json::Map::new();
    pub_obj.insert("metadata".to_string(), Value::Object(metadata));
    pub_obj.insert("links".to_string(), Value::Array(links));
    if !images.is_empty() {
        pub_obj.insert("images".to_string(), Value::Array(images));
    }
    Value::Object(pub_obj)
}
