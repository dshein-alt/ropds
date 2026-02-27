use axum::extract::{Path, Query, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};

use crate::db::queries::{authors, books, catalogs, genres, series};
use crate::state::AppState;

use super::xml::{self, FeedBuilder};

const DEFAULT_UPDATED: &str = "2024-01-01T00:00:00Z";

/// Extract primary language from Accept-Language header, fallback to config default.
fn detect_opds_lang(
    headers: &axum::http::HeaderMap,
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

fn locale_label(state: &AppState, locale: &str) -> String {
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

fn tr(state: &AppState, lang: &str, section: &str, key: &str, fallback: &str) -> String {
    let locale = crate::web::i18n::get_locale(state.translations.as_ref(), lang);
    locale
        .get(section)
        .and_then(|v| v.get(key))
        .and_then(|v| v.as_str())
        .unwrap_or(fallback)
        .to_string()
}

fn locale_choices(state: &AppState) -> Vec<String> {
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

fn add_lang_query(href: &str, lang: &str) -> String {
    let encoded = urlencoding::encode(lang);
    if href.contains('?') {
        format!("{href}&lang={encoded}")
    } else {
        format!("{href}?lang={encoded}")
    }
}

fn write_language_facets_for_href(
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

fn write_language_facets_as_root_lang_paths(
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

fn atom_response(body: Vec<u8>) -> Response {
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, xml::ATOM_XML)],
        body,
    )
        .into_response()
}

fn error_response(status: StatusCode, msg: &str) -> Response {
    (status, msg.to_string()).into_response()
}

/// GET /opds/ — Root navigation feed.
pub async fn root_feed(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Query(q): Query<LangQuery>,
) -> Response {
    build_root_feed(&state, &headers, q.lang.as_deref()).await
}

/// GET /opds/lang/:locale/ — Root feed forced to a specific locale.
pub async fn root_feed_for_locale(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path((locale,)): Path<(String,)>,
) -> Response {
    build_root_feed(&state, &headers, Some(locale.as_str())).await
}

async fn build_root_feed(
    state: &AppState,
    headers: &axum::http::HeaderMap,
    query_lang: Option<&str>,
) -> Response {
    let lang = detect_opds_lang(headers, &state.config, query_lang);
    let by_catalogs = tr(state, &lang, "opds", "root_by_catalogs", "By Catalogs");
    let by_authors = tr(state, &lang, "opds", "root_by_authors", "By Authors");
    let by_genres = tr(state, &lang, "opds", "root_by_genres", "By Genres");
    let by_series = tr(state, &lang, "opds", "root_by_series", "By Series");
    let by_title = tr(state, &lang, "opds", "root_by_title", "By Title");
    let by_recent = tr(state, &lang, "opds", "root_by_recent", "Recently Added");
    let language_facets = tr(
        state,
        &lang,
        "opds",
        "root_language_facets",
        "Language facets",
    );
    let by_catalogs_content = tr(
        state,
        &lang,
        "opds",
        "root_content_catalogs",
        "Browse by directory tree",
    );
    let by_authors_content = tr(
        state,
        &lang,
        "opds",
        "root_content_authors",
        "Browse by author",
    );
    let by_genres_content = tr(
        state,
        &lang,
        "opds",
        "root_content_genres",
        "Browse by genre",
    );
    let by_series_content = tr(
        state,
        &lang,
        "opds",
        "root_content_series",
        "Browse by series",
    );
    let by_title_content = tr(
        state,
        &lang,
        "opds",
        "root_content_title",
        "Browse by book title",
    );
    let by_recent_content = tr(
        state,
        &lang,
        "opds",
        "root_content_recent",
        "Browse newly scanned books",
    );
    let language_facets_content = tr(
        state,
        &lang,
        "opds",
        "root_content_language_facets",
        "Switch OPDS language facet",
    );
    let title = &state.config.opds.title;
    let subtitle = &state.config.opds.subtitle;

    let mut fb = FeedBuilder::new();
    if fb
        .begin_feed(
            "tag:root",
            title,
            subtitle,
            DEFAULT_UPDATED,
            "/opds/",
            "/opds/",
        )
        .is_err()
    {
        return error_response(StatusCode::INTERNAL_SERVER_ERROR, "XML error");
    }
    let _ = fb.write_search_links("/opds/search/", "/opds/search/{searchTerms}/");
    write_language_facets_as_root_lang_paths(&mut fb, state, &lang);

    let entries: Vec<(&str, String, String, String)> = vec![
        (
            "m:1",
            by_catalogs,
            add_lang_query("/opds/catalogs/", &lang),
            by_catalogs_content,
        ),
        (
            "m:2",
            by_authors,
            add_lang_query("/opds/authors/", &lang),
            by_authors_content,
        ),
        (
            "m:3",
            by_genres,
            add_lang_query("/opds/genres/", &lang),
            by_genres_content,
        ),
        (
            "m:4",
            by_series,
            add_lang_query("/opds/series/", &lang),
            by_series_content,
        ),
        (
            "m:5",
            by_title,
            add_lang_query("/opds/books/", &lang),
            by_title_content,
        ),
        (
            "m:8",
            by_recent,
            add_lang_query("/opds/recent/", &lang),
            by_recent_content,
        ),
        (
            "m:7",
            language_facets,
            add_lang_query("/opds/facets/languages/", &lang),
            language_facets_content,
        ),
    ];
    for (id, title, href, content) in &entries {
        let _ = fb.write_nav_entry(id, title, href, content, DEFAULT_UPDATED);
    }

    if state.config.opds.auth_required
        && let Some(user_id) = super::auth::get_user_id_from_headers(&state.db, headers).await
    {
        let count = crate::db::queries::bookshelf::count_by_user(&state.db, user_id)
            .await
            .unwrap_or(0);
        let books_read_prefix = tr(state, &lang, "opds", "books_read_prefix", "Books read");
        let content = format!("{books_read_prefix}: {count}");
        let _ = fb.write_nav_entry(
            "m:6",
            &tr(state, &lang, "opds", "root_bookshelf", "Book shelf"),
            &add_lang_query("/opds/bookshelf/", &lang),
            &content,
            DEFAULT_UPDATED,
        );
    }

    match fb.finish() {
        Ok(body) => atom_response(body),
        Err(_) => error_response(StatusCode::INTERNAL_SERVER_ERROR, "XML error"),
    }
}

/// GET /opds/catalogs/
pub async fn catalogs_root(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Query(q): Query<LangQuery>,
) -> Response {
    build_catalogs_feed(&state, &headers, q.lang.as_deref(), 0, 1).await
}

/// GET /opds/catalogs/:cat_id/
/// GET /opds/catalogs/:cat_id/:page/
pub async fn catalogs_feed(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(p): Path<CatalogsParams>,
    Query(q): Query<LangQuery>,
) -> Response {
    build_catalogs_feed(
        &state,
        &headers,
        q.lang.as_deref(),
        p.cat_id,
        p.page.unwrap_or(1).max(1),
    )
    .await
}

async fn build_catalogs_feed(
    state: &AppState,
    headers: &axum::http::HeaderMap,
    query_lang: Option<&str>,
    cat_id: i64,
    page: i32,
) -> Response {
    let lang = detect_opds_lang(headers, &state.config, query_lang);
    let max_items = state.config.opds.max_items as i32;
    let offset = (page - 1) * max_items;

    let mut fb = FeedBuilder::new();
    let self_href = if cat_id == 0 {
        add_lang_query("/opds/catalogs/", &lang)
    } else {
        add_lang_query(&format!("/opds/catalogs/{cat_id}/{page}/"), &lang)
    };
    let _ = fb.begin_feed(
        &format!("tag:catalogs:{cat_id}:{page}"),
        "Catalogs",
        "",
        DEFAULT_UPDATED,
        &self_href,
        &add_lang_query("/opds/", &lang),
    );
    let _ = fb.write_search_links(
        &add_lang_query("/opds/search/", &lang),
        &add_lang_query("/opds/search/{searchTerms}/", &lang),
    );
    write_language_facets_for_href(&mut fb, state, &lang, "/opds/catalogs/");

    // Child catalogs (only on page 1 — subcatalogs are not paginated)
    if page == 1 {
        let cats = if cat_id == 0 {
            catalogs::get_root_catalogs(&state.db)
                .await
                .unwrap_or_default()
        } else {
            catalogs::get_children(&state.db, cat_id)
                .await
                .unwrap_or_default()
        };

        for cat in &cats {
            let href = add_lang_query(&format!("/opds/catalogs/{}/", cat.id), &lang);
            let _ = fb.write_nav_entry(
                &format!("c:{}", cat.id),
                &cat.cat_name,
                &href,
                "",
                DEFAULT_UPDATED,
            );
        }
    }

    // Books in this catalog (paginated)
    if cat_id > 0 {
        let hide_doubles = state.config.opds.hide_doubles;
        let book_list = books::get_by_catalog(&state.db, cat_id, max_items, offset, hide_doubles)
            .await
            .unwrap_or_default();

        // Pagination links
        let has_next = book_list.len() as i32 >= max_items;
        let has_prev = page > 1;
        let prev_href = if has_prev {
            Some(add_lang_query(
                &format!("/opds/catalogs/{cat_id}/{}/", page - 1),
                &lang,
            ))
        } else {
            None
        };
        let next_href = if has_next {
            Some(add_lang_query(
                &format!("/opds/catalogs/{cat_id}/{}/", page + 1),
                &lang,
            ))
        } else {
            None
        };
        let _ = fb.write_pagination(prev_href.as_deref(), next_href.as_deref());

        for book in &book_list {
            write_book_entry(&mut fb, state, book, &lang).await;
        }
    }

    match fb.finish() {
        Ok(body) => atom_response(body),
        Err(_) => error_response(StatusCode::INTERNAL_SERVER_ERROR, "XML error"),
    }
}

/// GET /opds/authors/ — Language/script selection for authors.
pub async fn authors_root(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Query(q): Query<LangQuery>,
) -> Response {
    lang_selection_feed(
        &state,
        &headers,
        q.lang.as_deref(),
        "authors",
        "Authors",
        "/opds/authors/",
    )
    .await
}

/// GET /opds/authors/:lang_code/ — Alphabet drill-down for authors.
/// GET /opds/authors/:lang_code/:prefix/ — Drill down by prefix.
pub async fn authors_feed(
    State(state): State<AppState>,
    Path(params): Path<AuthorsParams>,
) -> Response {
    let lang_code = params.lang_code;
    let prefix = params.prefix.unwrap_or_default();
    let split_items = state.config.opds.split_items as i64;

    let mut fb = FeedBuilder::new();
    let self_href = if prefix.is_empty() {
        format!("/opds/authors/{lang_code}/")
    } else {
        format!(
            "/opds/authors/{lang_code}/{}/",
            urlencoding::encode(&prefix)
        )
    };
    let _ = fb.begin_feed(
        &format!("tag:authors:{lang_code}:{prefix}"),
        "Authors",
        "",
        DEFAULT_UPDATED,
        &self_href,
        "/opds/",
    );
    let _ = fb.write_search_links("/opds/search/", "/opds/search/{searchTerms}/");

    let groups = authors::get_name_prefix_groups(&state.db, lang_code, &prefix.to_uppercase())
        .await
        .unwrap_or_default();

    for (prefix_str, count) in &groups {
        if *count >= split_items {
            let href = format!(
                "/opds/authors/{lang_code}/{}/",
                urlencoding::encode(prefix_str)
            );
            let _ = fb.write_nav_entry(
                &format!("ap:{prefix_str}"),
                prefix_str,
                &href,
                &format!("{count}"),
                DEFAULT_UPDATED,
            );
        } else {
            let href = format!(
                "/opds/authors/{lang_code}/{}/list/",
                urlencoding::encode(prefix_str)
            );
            let _ = fb.write_nav_entry(
                &format!("ap:{prefix_str}"),
                prefix_str,
                &href,
                &format!("{count}"),
                DEFAULT_UPDATED,
            );
        }
    }

    match fb.finish() {
        Ok(body) => atom_response(body),
        Err(_) => error_response(StatusCode::INTERNAL_SERVER_ERROR, "XML error"),
    }
}

/// GET /opds/authors/:lang_code/:prefix/list/ — Paginated author listing for a prefix.
/// GET /opds/authors/:lang_code/:prefix/list/:page/
pub async fn authors_list(
    State(state): State<AppState>,
    Path(params): Path<AuthorsListParams>,
) -> Response {
    let max_items = state.config.opds.max_items as i32;
    let lang_code = params.lang_code;
    let prefix = params.prefix;
    let page = params.page.unwrap_or(1).max(1);
    let offset = (page - 1) * max_items;

    let mut fb = FeedBuilder::new();
    let self_href = format!(
        "/opds/authors/{lang_code}/{}/list/{page}/",
        urlencoding::encode(&prefix)
    );
    let _ = fb.begin_feed(
        &format!("tag:authors:{lang_code}:{prefix}:list:{page}"),
        &format!("Authors: {prefix}"),
        "",
        DEFAULT_UPDATED,
        &self_href,
        "/opds/",
    );
    let _ = fb.write_search_links("/opds/search/", "/opds/search/{searchTerms}/");

    let author_list = authors::get_by_lang_code_prefix(
        &state.db,
        lang_code,
        &prefix.to_uppercase(),
        max_items,
        offset,
    )
    .await
    .unwrap_or_default();

    let has_next = author_list.len() as i32 >= max_items;
    let has_prev = page > 1;
    let encoded_prefix = urlencoding::encode(&prefix);
    let prev_href = if has_prev {
        Some(format!(
            "/opds/authors/{lang_code}/{encoded_prefix}/list/{}/",
            page - 1
        ))
    } else {
        None
    };
    let next_href = if has_next {
        Some(format!(
            "/opds/authors/{lang_code}/{encoded_prefix}/list/{}/",
            page + 1
        ))
    } else {
        None
    };
    let _ = fb.write_pagination(prev_href.as_deref(), next_href.as_deref());

    for author in &author_list {
        let href = format!("/opds/search/books/a/{}/", author.id);
        let _ = fb.write_nav_entry(
            &format!("a:{}", author.id),
            &author.full_name,
            &href,
            "",
            DEFAULT_UPDATED,
        );
    }

    match fb.finish() {
        Ok(body) => atom_response(body),
        Err(_) => error_response(StatusCode::INTERNAL_SERVER_ERROR, "XML error"),
    }
}

/// GET /opds/series/ — Language/script selection for series.
pub async fn series_root(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Query(q): Query<LangQuery>,
) -> Response {
    lang_selection_feed(
        &state,
        &headers,
        q.lang.as_deref(),
        "series",
        "Series",
        "/opds/series/",
    )
    .await
}

/// GET /opds/series/:lang_code/ — Alphabet drill-down for series.
/// GET /opds/series/:lang_code/:prefix/ — Drill down by prefix.
pub async fn series_feed(
    State(state): State<AppState>,
    Path(params): Path<AuthorsParams>,
) -> Response {
    let lang_code = params.lang_code;
    let prefix = params.prefix.unwrap_or_default();
    let split_items = state.config.opds.split_items as i64;

    let mut fb = FeedBuilder::new();
    let self_href = if prefix.is_empty() {
        format!("/opds/series/{lang_code}/")
    } else {
        format!("/opds/series/{lang_code}/{}/", urlencoding::encode(&prefix))
    };
    let _ = fb.begin_feed(
        &format!("tag:series:{lang_code}:{prefix}"),
        "Series",
        "",
        DEFAULT_UPDATED,
        &self_href,
        "/opds/",
    );
    let _ = fb.write_search_links("/opds/search/", "/opds/search/{searchTerms}/");

    let groups = series::get_name_prefix_groups(&state.db, lang_code, &prefix.to_uppercase())
        .await
        .unwrap_or_default();

    for (prefix_str, count) in &groups {
        if *count >= split_items {
            let href = format!(
                "/opds/series/{lang_code}/{}/",
                urlencoding::encode(prefix_str)
            );
            let _ = fb.write_nav_entry(
                &format!("sp:{prefix_str}"),
                prefix_str,
                &href,
                &format!("{count}"),
                DEFAULT_UPDATED,
            );
        } else {
            let href = format!(
                "/opds/series/{lang_code}/{}/list/",
                urlencoding::encode(prefix_str)
            );
            let _ = fb.write_nav_entry(
                &format!("sp:{prefix_str}"),
                prefix_str,
                &href,
                &format!("{count}"),
                DEFAULT_UPDATED,
            );
        }
    }

    match fb.finish() {
        Ok(body) => atom_response(body),
        Err(_) => error_response(StatusCode::INTERNAL_SERVER_ERROR, "XML error"),
    }
}

/// GET /opds/series/:lang_code/:prefix/list/ — Paginated series listing for a prefix.
/// GET /opds/series/:lang_code/:prefix/list/:page/
pub async fn series_list(
    State(state): State<AppState>,
    Path(params): Path<AuthorsListParams>,
) -> Response {
    let max_items = state.config.opds.max_items as i32;
    let lang_code = params.lang_code;
    let prefix = params.prefix;
    let page = params.page.unwrap_or(1).max(1);
    let offset = (page - 1) * max_items;

    let mut fb = FeedBuilder::new();
    let self_href = format!(
        "/opds/series/{lang_code}/{}/list/{page}/",
        urlencoding::encode(&prefix)
    );
    let _ = fb.begin_feed(
        &format!("tag:series:{lang_code}:{prefix}:list:{page}"),
        &format!("Series: {prefix}"),
        "",
        DEFAULT_UPDATED,
        &self_href,
        "/opds/",
    );
    let _ = fb.write_search_links("/opds/search/", "/opds/search/{searchTerms}/");

    let series_list = series::get_by_lang_code_prefix(
        &state.db,
        lang_code,
        &prefix.to_uppercase(),
        max_items,
        offset,
    )
    .await
    .unwrap_or_default();

    let has_next = series_list.len() as i32 >= max_items;
    let has_prev = page > 1;
    let encoded_prefix = urlencoding::encode(&prefix);
    let prev_href = if has_prev {
        Some(format!(
            "/opds/series/{lang_code}/{encoded_prefix}/list/{}/",
            page - 1
        ))
    } else {
        None
    };
    let next_href = if has_next {
        Some(format!(
            "/opds/series/{lang_code}/{encoded_prefix}/list/{}/",
            page + 1
        ))
    } else {
        None
    };
    let _ = fb.write_pagination(prev_href.as_deref(), next_href.as_deref());

    for ser in &series_list {
        let href = format!("/opds/search/books/s/{}/", ser.id);
        let _ = fb.write_nav_entry(
            &format!("s:{}", ser.id),
            &ser.ser_name,
            &href,
            "",
            DEFAULT_UPDATED,
        );
    }

    match fb.finish() {
        Ok(body) => atom_response(body),
        Err(_) => error_response(StatusCode::INTERNAL_SERVER_ERROR, "XML error"),
    }
}

/// GET /opds/genres/ — Genre sections.
pub async fn genres_root(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Query(q): Query<LangQuery>,
) -> Response {
    let lang = detect_opds_lang(&headers, &state.config, q.lang.as_deref());
    let mut fb = FeedBuilder::new();

    let _ = fb.begin_feed(
        "tag:genres",
        "Genres",
        "",
        DEFAULT_UPDATED,
        &add_lang_query("/opds/genres/", &lang),
        &add_lang_query("/opds/", &lang),
    );
    let _ = fb.write_search_links(
        &add_lang_query("/opds/search/", &lang),
        &add_lang_query("/opds/search/{searchTerms}/", &lang),
    );
    write_language_facets_for_href(&mut fb, &state, &lang, "/opds/genres/");

    let sections = genres::get_sections(&state.db, &lang)
        .await
        .unwrap_or_default();
    for (i, (code, name)) in sections.iter().enumerate() {
        let href = add_lang_query(
            &format!("/opds/genres/{}/", urlencoding::encode(code)),
            &lang,
        );
        let _ = fb.write_nav_entry(&format!("gs:{i}"), name, &href, "", DEFAULT_UPDATED);
    }

    match fb.finish() {
        Ok(body) => atom_response(body),
        Err(_) => error_response(StatusCode::INTERNAL_SERVER_ERROR, "XML error"),
    }
}

/// GET /opds/genres/:section/ — Genres in section.
pub async fn genres_by_section(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path((section_code,)): Path<(String,)>,
    Query(q): Query<LangQuery>,
) -> Response {
    let lang = detect_opds_lang(&headers, &state.config, q.lang.as_deref());
    let mut fb = FeedBuilder::new();

    let self_href = add_lang_query(
        &format!("/opds/genres/{}/", urlencoding::encode(&section_code)),
        &lang,
    );

    let genre_list = genres::get_by_section(&state.db, &section_code, &lang)
        .await
        .unwrap_or_default();

    let section_title = genre_list
        .first()
        .map(|g| g.section.clone())
        .unwrap_or_else(|| section_code.clone());

    let _ = fb.begin_feed(
        &format!("tag:genres:{section_code}"),
        &section_title,
        "",
        DEFAULT_UPDATED,
        &self_href,
        &add_lang_query("/opds/", &lang),
    );
    let _ = fb.write_search_links(
        &add_lang_query("/opds/search/", &lang),
        &add_lang_query("/opds/search/{searchTerms}/", &lang),
    );
    write_language_facets_for_href(
        &mut fb,
        &state,
        &lang,
        &format!("/opds/genres/{}/", urlencoding::encode(&section_code)),
    );

    for genre in &genre_list {
        let href = add_lang_query(&format!("/opds/search/books/g/{}/", genre.id), &lang);
        let _ = fb.write_nav_entry(
            &format!("g:{}", genre.id),
            &genre.subsection,
            &href,
            &genre.code,
            DEFAULT_UPDATED,
        );
    }

    match fb.finish() {
        Ok(body) => atom_response(body),
        Err(_) => error_response(StatusCode::INTERNAL_SERVER_ERROR, "XML error"),
    }
}

/// GET /opds/facets/languages/ — OPDS language facet links.
pub async fn language_facets_feed(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Query(q): Query<LangQuery>,
) -> Response {
    let lang = detect_opds_lang(&headers, &state.config, q.lang.as_deref());
    let self_href = add_lang_query("/opds/facets/languages/", &lang);
    let facets_title = tr(&state, &lang, "opds", "facet_title", "Language facets");
    let browse_prefix = tr(
        &state,
        &lang,
        "opds",
        "facet_browse_catalog_in",
        "Browse OPDS catalog in",
    );

    let mut fb = FeedBuilder::new();
    let _ = fb.begin_feed(
        "tag:facets:languages",
        &facets_title,
        "",
        DEFAULT_UPDATED,
        &self_href,
        &add_lang_query("/opds/", &lang),
    );
    let _ = fb.write_search_links(
        &add_lang_query("/opds/search/", &lang),
        &add_lang_query("/opds/search/{searchTerms}/", &lang),
    );

    // Dedicated language facet feed should provide robust client-compatible
    // links without relying on query string support.
    write_language_facets_as_root_lang_paths(&mut fb, &state, &lang);

    for locale in locale_choices(&state) {
        let href = format!("/opds/lang/{}/", urlencoding::encode(&locale));
        let label = locale_label(&state, &locale);
        let content = format!("{browse_prefix} {label}");
        let _ = fb.write_nav_entry(
            &format!("lf:{locale}"),
            &label,
            &href,
            &content,
            DEFAULT_UPDATED,
        );
    }

    match fb.finish() {
        Ok(body) => atom_response(body),
        Err(_) => error_response(StatusCode::INTERNAL_SERVER_ERROR, "XML error"),
    }
}

/// GET /opds/books/ — Language selection for books by title.
pub async fn books_root(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Query(q): Query<LangQuery>,
) -> Response {
    lang_selection_feed(
        &state,
        &headers,
        q.lang.as_deref(),
        "books",
        "Books",
        "/opds/books/",
    )
    .await
}

/// GET /opds/books/:lang_code/
/// GET /opds/books/:lang_code/:prefix/
pub async fn books_feed(
    State(state): State<AppState>,
    Path(params): Path<AuthorsParams>,
) -> Response {
    let lang_code = params.lang_code;
    let prefix = params.prefix.unwrap_or_default();
    let split_items = state.config.opds.split_items as i64;

    let mut fb = FeedBuilder::new();
    let self_href = if prefix.is_empty() {
        format!("/opds/books/{lang_code}/")
    } else {
        format!("/opds/books/{lang_code}/{}/", urlencoding::encode(&prefix))
    };
    let _ = fb.begin_feed(
        &format!("tag:books:{lang_code}:{prefix}"),
        "Books",
        "",
        DEFAULT_UPDATED,
        &self_href,
        "/opds/",
    );
    let _ = fb.write_search_links("/opds/search/", "/opds/search/{searchTerms}/");

    let groups = books::get_title_prefix_groups(&state.db, lang_code, &prefix.to_uppercase())
        .await
        .unwrap_or_default();

    for (prefix_str, count) in &groups {
        if *count >= split_items {
            let href = format!(
                "/opds/books/{lang_code}/{}/",
                urlencoding::encode(prefix_str)
            );
            let _ = fb.write_nav_entry(
                &format!("bp:{prefix_str}"),
                prefix_str,
                &href,
                &format!("{count}"),
                DEFAULT_UPDATED,
            );
        } else {
            let href = format!("/opds/search/books/b/{}/", urlencoding::encode(prefix_str));
            let _ = fb.write_nav_entry(
                &format!("bp:{prefix_str}"),
                prefix_str,
                &href,
                &format!("{count}"),
                DEFAULT_UPDATED,
            );
        }
    }

    match fb.finish() {
        Ok(body) => atom_response(body),
        Err(_) => error_response(StatusCode::INTERNAL_SERVER_ERROR, "XML error"),
    }
}

/// GET /opds/recent/
pub async fn recent_root(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Query(q): Query<LangQuery>,
) -> Response {
    build_recent_feed(&state, &headers, q.lang.as_deref(), 1).await
}

/// GET /opds/recent/:page/
pub async fn recent_feed(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path((page,)): Path<(i32,)>,
    Query(q): Query<LangQuery>,
) -> Response {
    build_recent_feed(&state, &headers, q.lang.as_deref(), page.max(1)).await
}

async fn build_recent_feed(
    state: &AppState,
    headers: &axum::http::HeaderMap,
    query_lang: Option<&str>,
    page: i32,
) -> Response {
    let lang = detect_opds_lang(headers, &state.config, query_lang);
    let max_items = state.config.opds.max_items as i32;
    let offset = (page - 1) * max_items;
    let hide_doubles = state.config.opds.hide_doubles;

    let mut fb = FeedBuilder::new();
    let self_href = add_lang_query(&format!("/opds/recent/{page}/"), &lang);
    let _ = fb.begin_feed(
        &format!("tag:recent:{page}"),
        &tr(state, &lang, "opds", "root_by_recent", "Recently Added"),
        "",
        DEFAULT_UPDATED,
        &self_href,
        &add_lang_query("/opds/", &lang),
    );
    let _ = fb.write_search_links(
        &add_lang_query("/opds/search/", &lang),
        &add_lang_query("/opds/search/{searchTerms}/", &lang),
    );
    write_language_facets_for_href(&mut fb, state, &lang, "/opds/recent/");

    let book_list = books::get_recent_added(&state.db, max_items, offset, hide_doubles)
        .await
        .unwrap_or_default();

    let has_next = book_list.len() as i32 >= max_items;
    let has_prev = page > 1;
    let prev_href = if has_prev {
        Some(add_lang_query(
            &format!("/opds/recent/{}/", page - 1),
            &lang,
        ))
    } else {
        None
    };
    let next_href = if has_next {
        Some(add_lang_query(
            &format!("/opds/recent/{}/", page + 1),
            &lang,
        ))
    } else {
        None
    };
    let _ = fb.write_pagination(prev_href.as_deref(), next_href.as_deref());

    for book in &book_list {
        write_book_entry(&mut fb, state, book, &lang).await;
    }

    match fb.finish() {
        Ok(body) => atom_response(body),
        Err(_) => error_response(StatusCode::INTERNAL_SERVER_ERROR, "XML error"),
    }
}

/// GET /opds/search/:terms/ — Search type selection.
pub async fn search_types_feed(
    State(_state): State<AppState>,
    Path((terms,)): Path<(String,)>,
) -> Response {
    let mut fb = FeedBuilder::new();
    let self_href = format!("/opds/search/{}/", urlencoding::encode(&terms));
    let _ = fb.begin_feed(
        &format!("tag:search:{terms}"),
        &format!("Search: {terms}"),
        "",
        DEFAULT_UPDATED,
        &self_href,
        "/opds/",
    );

    let entries = [
        (
            "st:1",
            "Search by title",
            format!("/opds/search/books/m/{}/", urlencoding::encode(&terms)),
        ),
        (
            "st:2",
            "Search by author",
            format!("/opds/search/authors/m/{}/", urlencoding::encode(&terms)),
        ),
        (
            "st:3",
            "Search by series",
            format!("/opds/search/series/m/{}/", urlencoding::encode(&terms)),
        ),
    ];
    for (id, title, href) in &entries {
        let _ = fb.write_nav_entry(id, title, href, "", DEFAULT_UPDATED);
    }

    match fb.finish() {
        Ok(body) => atom_response(body),
        Err(_) => error_response(StatusCode::INTERNAL_SERVER_ERROR, "XML error"),
    }
}

/// GET /opds/search/books/:search_type/:terms/
/// GET /opds/search/books/:search_type/:terms/:page/
///
/// Search types: b=begins, m=contains, e=exact, a=by author id, s=by series id, g=by genre id
pub async fn search_books_feed(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(params): Path<SearchBooksParams>,
    Query(q): Query<LangQuery>,
) -> Response {
    let lang = detect_opds_lang(&headers, &state.config, q.lang.as_deref());
    let max_items = state.config.opds.max_items as i32;
    let page = params.page.unwrap_or(1).max(1);
    let offset = (page - 1) * max_items;
    let search_type = &params.search_type;
    let terms = &params.terms;

    let mut fb = FeedBuilder::new();
    let self_href = add_lang_query(
        &format!(
            "/opds/search/books/{}/{}/{}/",
            search_type,
            urlencoding::encode(terms),
            page
        ),
        &lang,
    );
    let _ = fb.begin_feed(
        &format!("tag:search:books:{search_type}:{terms}:{page}"),
        &format!("Search: {terms}"),
        "",
        DEFAULT_UPDATED,
        &self_href,
        &add_lang_query("/opds/", &lang),
    );
    let _ = fb.write_search_links(
        &add_lang_query("/opds/search/", &lang),
        &add_lang_query("/opds/search/{searchTerms}/", &lang),
    );

    let hide_doubles = state.config.opds.hide_doubles;
    let book_list = match search_type.as_str() {
        "a" => {
            // By author ID
            let author_id: i64 = terms.parse().unwrap_or(0);
            books::get_by_author(&state.db, author_id, max_items, offset, hide_doubles)
                .await
                .unwrap_or_default()
        }
        "s" => {
            // By series ID
            let series_id: i64 = terms.parse().unwrap_or(0);
            books::get_by_series(&state.db, series_id, max_items, offset, hide_doubles)
                .await
                .unwrap_or_default()
        }
        "g" => {
            // By genre ID
            let genre_id: i64 = terms.parse().unwrap_or(0);
            books::get_by_genre(&state.db, genre_id, max_items, offset, hide_doubles)
                .await
                .unwrap_or_default()
        }
        _ => {
            // Title search: m=contains, b=begins, e=exact
            let search_term = terms.to_uppercase();
            books::search_by_title(&state.db, &search_term, max_items, offset, hide_doubles)
                .await
                .unwrap_or_default()
        }
    };

    // Pagination
    let has_next = book_list.len() as i32 >= max_items;
    let has_prev = page > 1;
    let prev_href = if has_prev {
        Some(format!(
            "/opds/search/books/{}/{}/{}/",
            search_type,
            urlencoding::encode(terms),
            page - 1
        ))
        .map(|href| add_lang_query(&href, &lang))
    } else {
        None
    };
    let next_href = if has_next {
        Some(format!(
            "/opds/search/books/{}/{}/{}/",
            search_type,
            urlencoding::encode(terms),
            page + 1
        ))
        .map(|href| add_lang_query(&href, &lang))
    } else {
        None
    };
    let _ = fb.write_pagination(prev_href.as_deref(), next_href.as_deref());

    for book in &book_list {
        write_book_entry(&mut fb, &state, book, &lang).await;
    }

    match fb.finish() {
        Ok(body) => atom_response(body),
        Err(_) => error_response(StatusCode::INTERNAL_SERVER_ERROR, "XML error"),
    }
}

/// GET /opds/search/authors/:search_type/:terms/
/// GET /opds/search/authors/:search_type/:terms/:page/
pub async fn search_authors_feed(
    State(state): State<AppState>,
    Path(params): Path<SearchBooksParams>,
) -> Response {
    let max_items = state.config.opds.max_items as i32;
    let page = params.page.unwrap_or(1).max(1);
    let offset = (page - 1) * max_items;
    let terms = &params.terms;

    let mut fb = FeedBuilder::new();
    let self_href = format!(
        "/opds/search/authors/m/{}/{}/",
        urlencoding::encode(terms),
        page
    );
    let _ = fb.begin_feed(
        &format!("tag:search:authors:{terms}:{page}"),
        &format!("Authors: {terms}"),
        "",
        DEFAULT_UPDATED,
        &self_href,
        "/opds/",
    );
    let _ = fb.write_search_links("/opds/search/", "/opds/search/{searchTerms}/");

    let search_term = terms.to_uppercase();
    let author_list = authors::search_by_name(&state.db, &search_term, max_items, offset)
        .await
        .unwrap_or_default();

    let has_next = author_list.len() as i32 >= max_items;
    let has_prev = page > 1;
    let prev_href = if has_prev {
        Some(format!(
            "/opds/search/authors/m/{}/{}/",
            urlencoding::encode(terms),
            page - 1
        ))
    } else {
        None
    };
    let next_href = if has_next {
        Some(format!(
            "/opds/search/authors/m/{}/{}/",
            urlencoding::encode(terms),
            page + 1
        ))
    } else {
        None
    };
    let _ = fb.write_pagination(prev_href.as_deref(), next_href.as_deref());

    for author in &author_list {
        let href = format!("/opds/search/books/a/{}/", author.id);
        let _ = fb.write_nav_entry(
            &format!("a:{}", author.id),
            &author.full_name,
            &href,
            "",
            DEFAULT_UPDATED,
        );
    }

    match fb.finish() {
        Ok(body) => atom_response(body),
        Err(_) => error_response(StatusCode::INTERNAL_SERVER_ERROR, "XML error"),
    }
}

/// GET /opds/search/series/:search_type/:terms/
/// GET /opds/search/series/:search_type/:terms/:page/
pub async fn search_series_feed(
    State(state): State<AppState>,
    Path(params): Path<SearchBooksParams>,
) -> Response {
    let max_items = state.config.opds.max_items as i32;
    let page = params.page.unwrap_or(1).max(1);
    let offset = (page - 1) * max_items;
    let terms = &params.terms;

    let mut fb = FeedBuilder::new();
    let self_href = format!(
        "/opds/search/series/m/{}/{}/",
        urlencoding::encode(terms),
        page
    );
    let _ = fb.begin_feed(
        &format!("tag:search:series:{terms}:{page}"),
        &format!("Series: {terms}"),
        "",
        DEFAULT_UPDATED,
        &self_href,
        "/opds/",
    );
    let _ = fb.write_search_links("/opds/search/", "/opds/search/{searchTerms}/");

    let search_term = terms.to_uppercase();
    let series_list = series::search_by_name(&state.db, &search_term, max_items, offset)
        .await
        .unwrap_or_default();

    let has_next = series_list.len() as i32 >= max_items;
    let has_prev = page > 1;
    let prev_href = if has_prev {
        Some(format!(
            "/opds/search/series/m/{}/{}/",
            urlencoding::encode(terms),
            page - 1
        ))
    } else {
        None
    };
    let next_href = if has_next {
        Some(format!(
            "/opds/search/series/m/{}/{}/",
            urlencoding::encode(terms),
            page + 1
        ))
    } else {
        None
    };
    let _ = fb.write_pagination(prev_href.as_deref(), next_href.as_deref());

    for ser in &series_list {
        let href = format!("/opds/search/books/s/{}/", ser.id);
        let _ = fb.write_nav_entry(
            &format!("s:{}", ser.id),
            &ser.ser_name,
            &href,
            "",
            DEFAULT_UPDATED,
        );
    }

    match fb.finish() {
        Ok(body) => atom_response(body),
        Err(_) => error_response(StatusCode::INTERNAL_SERVER_ERROR, "XML error"),
    }
}

/// GET /opds/bookshelf/
pub async fn bookshelf_root(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Query(q): Query<LangQuery>,
) -> Response {
    build_bookshelf_feed(&state, &headers, q.lang.as_deref(), 1).await
}

/// GET /opds/bookshelf/:page/
pub async fn bookshelf_feed(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path((page,)): Path<(i32,)>,
    Query(q): Query<LangQuery>,
) -> Response {
    build_bookshelf_feed(&state, &headers, q.lang.as_deref(), page.max(1)).await
}

async fn build_bookshelf_feed(
    state: &AppState,
    headers: &axum::http::HeaderMap,
    query_lang: Option<&str>,
    page: i32,
) -> Response {
    let lang = detect_opds_lang(headers, &state.config, query_lang);
    let user_id = match super::auth::get_user_id_from_headers(&state.db, headers).await {
        Some(uid) => uid,
        None => return error_response(StatusCode::UNAUTHORIZED, "Authentication required"),
    };

    let max_items = state.config.opds.max_items as i32;
    let offset = (page - 1) * max_items;

    let mut fb = FeedBuilder::new();
    let self_href = add_lang_query(&format!("/opds/bookshelf/{page}/"), &lang);
    let _ = fb.begin_feed(
        &format!("tag:bookshelf:{page}"),
        "Book shelf",
        "",
        DEFAULT_UPDATED,
        &self_href,
        &add_lang_query("/opds/", &lang),
    );
    let _ = fb.write_search_links(
        &add_lang_query("/opds/search/", &lang),
        &add_lang_query("/opds/search/{searchTerms}/", &lang),
    );
    write_language_facets_for_href(&mut fb, state, &lang, "/opds/bookshelf/");

    let book_list = crate::db::queries::bookshelf::get_by_user(
        &state.db,
        user_id,
        &crate::db::queries::bookshelf::SortColumn::Date,
        false,
        max_items,
        offset,
    )
    .await
    .unwrap_or_default();

    // Pagination
    let has_next = book_list.len() as i32 >= max_items;
    let has_prev = page > 1;
    let prev_href = if has_prev {
        Some(add_lang_query(
            &format!("/opds/bookshelf/{}/", page - 1),
            &lang,
        ))
    } else {
        None
    };
    let next_href = if has_next {
        Some(add_lang_query(
            &format!("/opds/bookshelf/{}/", page + 1),
            &lang,
        ))
    } else {
        None
    };
    let _ = fb.write_pagination(prev_href.as_deref(), next_href.as_deref());

    for book in &book_list {
        write_book_entry(&mut fb, state, book, &lang).await;
    }

    match fb.finish() {
        Ok(body) => atom_response(body),
        Err(_) => error_response(StatusCode::INTERNAL_SERVER_ERROR, "XML error"),
    }
}

/// GET /opds/search/ — OpenSearch description.
pub async fn opensearch(_state: State<AppState>) -> Response {
    let xml = r#"<?xml version="1.0" encoding="utf-8"?>
<OpenSearchDescription xmlns="http://a9.com/-/spec/opensearch/1.1/">
    <ShortName>ropds</ShortName>
    <LongName>Rust OPDS Server</LongName>
    <Description>Search the OPDS catalog</Description>
    <Url type="application/atom+xml" template="/opds/search/{searchTerms}/" />
    <SyndicationRight>open</SyndicationRight>
    <AdultContent>false</AdultContent>
    <Language>*</Language>
    <OutputEncoding>UTF-8</OutputEncoding>
    <InputEncoding>UTF-8</InputEncoding>
</OpenSearchDescription>"#;

    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, xml::OPENSEARCH_TYPE)],
        xml,
    )
        .into_response()
}

// ---- Helper types and functions ----

#[derive(serde::Deserialize, Default)]
pub struct LangQuery {
    pub lang: Option<String>,
}

#[derive(serde::Deserialize)]
pub struct AuthorsParams {
    pub lang_code: i32,
    pub prefix: Option<String>,
}

#[derive(serde::Deserialize)]
pub struct AuthorsListParams {
    pub lang_code: i32,
    pub prefix: String,
    pub page: Option<i32>,
}

#[derive(serde::Deserialize)]
pub struct CatalogsParams {
    pub cat_id: i64,
    pub page: Option<i32>,
}

#[derive(serde::Deserialize)]
pub struct SearchBooksParams {
    pub search_type: String,
    pub terms: String,
    pub page: Option<i32>,
}

/// Generate the language/script selection feed.
async fn lang_selection_feed(
    state: &AppState,
    headers: &axum::http::HeaderMap,
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

/// Write a book acquisition entry.
async fn write_book_entry(
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::create_test_pool;
    use crate::web::i18n::Translations;
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
        let db = create_test_pool().await;
        let tera = tera::Tera::default();
        let mut translations = Translations::new();
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

    #[tokio::test]
    async fn test_root_feed_contains_recent_nav_entry() {
        let cfg = test_config("en");
        let db = create_test_pool().await;
        let tera = tera::Tera::default();
        let mut translations = Translations::new();
        translations.insert(
            "en".to_string(),
            serde_json::json!({
                "opds": {
                    "root_by_catalogs": "By Catalogs",
                    "root_by_authors": "By Authors",
                    "root_by_genres": "By Genres",
                    "root_by_series": "By Series",
                    "root_by_title": "By Title",
                    "root_by_recent": "Recently Added",
                    "root_language_facets": "Language",
                    "root_content_catalogs": "Browse by directory tree",
                    "root_content_authors": "Browse by author",
                    "root_content_genres": "Browse by genre",
                    "root_content_series": "Browse by series",
                    "root_content_title": "Browse by book title",
                    "root_content_recent": "Browse newly scanned books",
                    "root_content_language_facets": "Switch OPDS language facet"
                },
                "lang": { "en": "English", "ru": "Русский" }
            }),
        );
        let state = AppState::new(cfg, db, tera, translations, false, false);
        let headers = HeaderMap::new();

        let response = build_root_feed(&state, &headers, Some("en")).await;
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let xml = String::from_utf8(bytes.to_vec()).unwrap();
        assert!(xml.contains("/opds/recent/"));
    }
}
