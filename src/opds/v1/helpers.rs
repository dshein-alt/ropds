use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};

use crate::db::queries::{authors, genres};
use crate::state::AppState;

use super::xml::{self, FeedBuilder};

pub const DEFAULT_UPDATED: &str = "2024-01-01T00:00:00Z";

pub fn atom_response(body: Vec<u8>) -> Response {
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, xml::ATOM_XML)],
        body,
    )
        .into_response()
}

pub fn error_response(status: StatusCode, msg: &str) -> Response {
    (status, msg.to_string()).into_response()
}

pub fn normalize_locale_code(locale: &str) -> Option<String> {
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

pub fn write_language_facets_for_href(
    fb: &mut FeedBuilder,
    state: &AppState,
    selected_lang: &str,
    target_href: &str,
) {
    for locale in locale_choices(state) {
        let facet_href = add_lang_query(target_href, &locale);
        let label = locale_label(state, &locale);
        let _ = fb.write_facet_link(
            &facet_href,
            xml::NAV_TYPE,
            &label,
            "Language",
            locale == selected_lang,
        );
    }
}

pub fn write_language_facets_as_root_lang_paths(
    fb: &mut FeedBuilder,
    state: &AppState,
    selected_lang: &str,
) {
    for locale in locale_choices(state) {
        let href = format!("/opds/lang/{}/", urlencoding::encode(&locale));
        let label = locale_label(state, &locale);
        let _ = fb.write_facet_link(
            &href,
            xml::NAV_TYPE,
            &label,
            "Language",
            locale == selected_lang,
        );
    }
}

/// Write a book acquisition entry.
pub async fn write_book_entry(
    fb: &mut FeedBuilder,
    state: &AppState,
    book: &crate::db::models::Book,
    lang: &str,
) {
    let _ = fb.begin_entry(&format!("b:{}", book.id), &book.title, &book.reg_date);

    // Download link (alternate)
    let dl_href = format!("/opds/download/{}/0/", book.id);
    let alternate_link = xml::Link {
        href: dl_href,
        rel: "alternate".to_string(),
        link_type: xml::mime_for_format(&book.format).to_string(),
        title: None,
    };
    let _ = fb.write_link_obj(&alternate_link);

    // Acquisition links
    let _ = fb.write_acquisition_links(book.id, &book.format, book.cover != 0);

    // Content: book description HTML
    let mut html = format!("<b>Title: </b>{}<br/>", book.title);
    if !book.format.is_empty() {
        html.push_str(&format!("<b>Format: </b>{}<br/>", book.format));
    }
    html.push_str(&format!("<b>Size: </b>{} KB<br/>", book.size / 1024));
    if !book.lang.is_empty() {
        html.push_str(&format!("<b>Language: </b>{}<br/>", book.lang));
    }
    if !book.docdate.is_empty() {
        html.push_str(&format!("<b>Date: </b>{}<br/>", book.docdate));
    }
    if !book.annotation.is_empty() {
        html.push_str(&format!("<p class='book'>{}</p>", book.annotation));
    }
    let _ = fb.write_content_html(&html);

    // Authors
    if let Ok(book_authors) = authors::get_for_book(&state.db, book.id).await {
        for author in &book_authors {
            let author_elem = xml::Author {
                name: author.full_name.clone(),
            };
            let _ = fb.write_author_obj(&author_elem);

            let author_href = format!("/opds/search/books/a/{}/", author.id);
            let related_link = xml::Link {
                href: author_href,
                rel: "related".to_string(),
                link_type: xml::ACQ_TYPE.to_string(),
                title: Some(format!("All books by {}", author.full_name)),
            };
            let _ = fb.write_link_obj(&related_link);
        }
    }

    // Genres
    if let Ok(book_genres) = genres::get_for_book(&state.db, book.id, lang).await {
        for genre in &book_genres {
            let category = xml::Category {
                term: genre.code.clone(),
                label: genre.subsection.clone(),
            };
            let _ = fb.write_category_obj(&category);
        }
    }

    let _ = fb.end_entry();
}

/// Generate the language/script selection feed.
pub async fn lang_selection_feed(
    state: &AppState,
    headers: &HeaderMap,
    query_lang: Option<&str>,
    nav_key: &str,
    fallback_title: &str,
    base_href: &str,
) -> Response {
    let lang = detect_opds_lang(headers, &state.config, query_lang);
    let title = tr(state, &lang, "nav", nav_key, fallback_title);
    let all_label = tr(state, &lang, "browse", "all_languages", "All");
    let cyrillic_label = tr(state, &lang, "browse", "cyrillic", "Cyrillic");
    let latin_label = tr(state, &lang, "browse", "latin", "Latin");
    let digits_label = tr(state, &lang, "browse", "digits", "Digits");
    let other_label = tr(state, &lang, "browse", "other", "Other");

    let mut fb = FeedBuilder::new();
    let self_href = add_lang_query(base_href, &lang);
    let _ = fb.begin_feed(
        &format!("tag:lang:{title}"),
        &title,
        "",
        DEFAULT_UPDATED,
        &self_href,
        &add_lang_query("/opds/", &lang),
    );
    let _ = fb.write_search_links(
        &add_lang_query("/opds/search/", &lang),
        &add_lang_query("/opds/search/{searchTerms}/", &lang),
    );
    write_language_facets_for_href(&mut fb, state, &lang, base_href);

    let entries = [
        (
            "l:0",
            all_label,
            add_lang_query(&format!("{base_href}0/"), &lang),
        ),
        (
            "l:1",
            cyrillic_label,
            add_lang_query(&format!("{base_href}1/"), &lang),
        ),
        (
            "l:2",
            latin_label,
            add_lang_query(&format!("{base_href}2/"), &lang),
        ),
        (
            "l:3",
            digits_label,
            add_lang_query(&format!("{base_href}3/"), &lang),
        ),
        (
            "l:9",
            other_label,
            add_lang_query(&format!("{base_href}9/"), &lang),
        ),
    ];
    for (id, label, href) in &entries {
        let _ = fb.write_nav_entry(id, label, href, "", DEFAULT_UPDATED);
    }

    match fb.finish() {
        Ok(body) => atom_response(body),
        Err(_) => error_response(StatusCode::INTERNAL_SERVER_ERROR, "XML error"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use axum::http::HeaderMap;

    fn test_config(default_lang: &str) -> crate::config::Config {
        let cfg = format!(
            r#"
[server]
session_secret = "s"
[library]
root_path = "/tmp"
[database]
[opds]
[scanner]
[web]
language = "{default_lang}"
"#
        );
        toml::from_str(&cfg).unwrap()
    }

    #[test]
    fn test_detect_opds_lang_parses_primary_language() {
        let cfg = test_config("en");
        let mut headers = HeaderMap::new();
        headers.insert(
            "accept-language",
            "fr-CA,fr;q=0.9,en;q=0.8".parse().unwrap(),
        );
        assert_eq!(detect_opds_lang(&headers, &cfg, None), "fr");

        headers.insert("accept-language", "RU;q=0.8,en".parse().unwrap());
        assert_eq!(detect_opds_lang(&headers, &cfg, None), "ru");
    }

    #[test]
    fn test_detect_opds_lang_fallback_to_config() {
        let cfg = test_config("de");
        let headers = HeaderMap::new();
        assert_eq!(detect_opds_lang(&headers, &cfg, None), "de");
    }

    #[test]
    fn test_detect_opds_lang_prefers_query_lang() {
        let cfg = test_config("en");
        let mut headers = HeaderMap::new();
        headers.insert("accept-language", "fr".parse().unwrap());
        assert_eq!(detect_opds_lang(&headers, &cfg, Some("ru")), "ru");
    }

    #[tokio::test]
    async fn test_atom_and_error_response() {
        let atom = atom_response(b"<feed/>".to_vec());
        assert_eq!(atom.status(), StatusCode::OK);
        assert_eq!(
            atom.headers().get(header::CONTENT_TYPE).unwrap(),
            xml::ATOM_XML
        );
        let atom_body = to_bytes(atom.into_body(), usize::MAX).await.unwrap();
        assert_eq!(atom_body.as_ref(), b"<feed/>");

        let err = error_response(StatusCode::BAD_REQUEST, "bad");
        assert_eq!(err.status(), StatusCode::BAD_REQUEST);
        let err_body = to_bytes(err.into_body(), usize::MAX).await.unwrap();
        assert_eq!(err_body.as_ref(), b"bad");
    }

    #[tokio::test]
    async fn test_lang_selection_feed_contains_expected_entries() {
        let cfg = test_config("en");
        let db = crate::db::create_test_pool().await;
        let tera = tera::Tera::default();
        let mut translations = crate::web::i18n::Translations::new();
        translations.insert(
            "en".to_string(),
            serde_json::json!({
                "nav": { "authors": "Authors" },
                "browse": {
                    "all_languages": "All languages",
                    "cyrillic": "Cyrillic",
                    "latin": "Latin",
                    "digits": "Digits",
                    "other": "Other"
                },
                "lang": { "en": "English", "ru": "Русский" }
            }),
        );
        translations.insert(
            "ru".to_string(),
            serde_json::json!({
                "nav": { "authors": "Авторы" },
                "browse": {
                    "all_languages": "Все языки",
                    "cyrillic": "Кириллица",
                    "latin": "Латиница",
                    "digits": "Цифры",
                    "other": "Другие"
                },
                "lang": { "en": "English", "ru": "Русский" }
            }),
        );
        let state = AppState::new(cfg, db, tera, translations, false, false);
        let mut headers = HeaderMap::new();
        headers.insert("accept-language", "ru".parse().unwrap());

        let response = lang_selection_feed(
            &state,
            &headers,
            Some("ru"),
            "authors",
            "Authors",
            "/opds/authors/",
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE).unwrap(),
            xml::ATOM_XML
        );

        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let xml = String::from_utf8(bytes.to_vec()).unwrap();
        assert!(xml.contains("Авторы"));
        assert!(xml.contains("/opds/authors/1/?lang=ru"));
        assert!(xml.contains("Кириллица"));
        assert!(xml.contains("Цифры"));
    }

    #[test]
    fn test_add_lang_query_helper() {
        assert_eq!(
            add_lang_query("/opds/genres/", "ru"),
            "/opds/genres/?lang=ru"
        );
        assert_eq!(
            add_lang_query("/opds/genres/?page=1", "en"),
            "/opds/genres/?page=1&lang=en"
        );
    }
}
