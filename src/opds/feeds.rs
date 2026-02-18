use axum::extract::{Path, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};

use crate::db::queries::{authors, books, catalogs, genres, series};
use crate::state::AppState;

use super::xml::{self, FeedBuilder};

const DEFAULT_UPDATED: &str = "2024-01-01T00:00:00Z";

/// Extract primary language from Accept-Language header, fallback to config default.
fn detect_opds_lang(headers: &axum::http::HeaderMap, config: &crate::config::Config) -> String {
    if let Some(accept_lang) = headers.get("accept-language").and_then(|v| v.to_str().ok()) {
        let primary = accept_lang.split(',').next().unwrap_or("en");
        let lang = primary.split(&['-', ';'][..]).next().unwrap_or("en");
        return lang.to_lowercase();
    }
    config.web.language.clone()
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
pub async fn root_feed(State(state): State<AppState>, headers: axum::http::HeaderMap) -> Response {
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

    let entries = [
        (
            "m:1",
            "By Catalogs",
            "/opds/catalogs/",
            "Browse by directory tree",
        ),
        ("m:2", "By Authors", "/opds/authors/", "Browse by author"),
        ("m:3", "By Genres", "/opds/genres/", "Browse by genre"),
        ("m:4", "By Series", "/opds/series/", "Browse by series"),
        ("m:5", "By Title", "/opds/books/", "Browse by book title"),
    ];
    for (id, title, href, content) in &entries {
        let _ = fb.write_nav_entry(id, title, href, content, DEFAULT_UPDATED);
    }

    if state.config.opds.auth_required {
        if let Some(user_id) = super::auth::get_user_id_from_headers(&state.db, &headers).await {
            let count = crate::db::queries::bookshelf::count_by_user(&state.db, user_id)
                .await
                .unwrap_or(0);
            let content = format!("Books read: {count}");
            let _ = fb.write_nav_entry(
                "m:6",
                "Book shelf",
                "/opds/bookshelf/",
                &content,
                DEFAULT_UPDATED,
            );
        }
    }

    match fb.finish() {
        Ok(body) => atom_response(body),
        Err(_) => error_response(StatusCode::INTERNAL_SERVER_ERROR, "XML error"),
    }
}

/// GET /opds/catalogs/
/// GET /opds/catalogs/:cat_id/
/// GET /opds/catalogs/:cat_id/:page/
pub async fn catalogs_feed(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    path: Option<Path<CatalogsParams>>,
) -> Response {
    let lang = detect_opds_lang(&headers, &state.config);
    let (cat_id, page) = match path {
        Some(Path(p)) => (p.cat_id, p.page.unwrap_or(1).max(1)),
        None => (0, 1),
    };
    let max_items = state.config.opds.max_items as i32;
    let offset = (page - 1) * max_items;

    let mut fb = FeedBuilder::new();
    let self_href = if cat_id == 0 {
        "/opds/catalogs/".to_string()
    } else {
        format!("/opds/catalogs/{cat_id}/{page}/")
    };
    let _ = fb.begin_feed(
        &format!("tag:catalogs:{cat_id}:{page}"),
        "Catalogs",
        "",
        DEFAULT_UPDATED,
        &self_href,
        "/opds/",
    );
    let _ = fb.write_search_links("/opds/search/", "/opds/search/{searchTerms}/");

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
            let href = format!("/opds/catalogs/{}/", cat.id);
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
            Some(format!("/opds/catalogs/{cat_id}/{}/", page - 1))
        } else {
            None
        };
        let next_href = if has_next {
            Some(format!("/opds/catalogs/{cat_id}/{}/", page + 1))
        } else {
            None
        };
        let _ = fb.write_pagination(prev_href.as_deref(), next_href.as_deref());

        for book in &book_list {
            write_book_entry(&mut fb, &state, book, &lang).await;
        }
    }

    match fb.finish() {
        Ok(body) => atom_response(body),
        Err(_) => error_response(StatusCode::INTERNAL_SERVER_ERROR, "XML error"),
    }
}

/// GET /opds/authors/ — Language/script selection for authors.
/// GET /opds/authors/:lang_code/ — Authors starting with letter prefix.
/// GET /opds/authors/:lang_code/:prefix/ — Drill down by prefix.
pub async fn authors_feed(
    State(state): State<AppState>,
    path: Option<Path<AuthorsParams>>,
) -> Response {
    let max_items = state.config.opds.max_items as i32;

    match path {
        None => {
            // Language selection
            lang_selection_feed("Authors", "/opds/authors/").await
        }
        Some(Path(params)) => {
            let lang_code = params.lang_code;
            let prefix = params.prefix.unwrap_or_default();

            let mut fb = FeedBuilder::new();
            let self_href = if prefix.is_empty() {
                format!("/opds/authors/{lang_code}/")
            } else {
                format!("/opds/authors/{lang_code}/{prefix}/")
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

            let author_list = authors::get_by_lang_code_prefix(
                &state.db,
                lang_code,
                &prefix.to_uppercase(),
                max_items,
                0,
            )
            .await
            .unwrap_or_default();

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
    }
}

/// GET /opds/series/ — Language/script selection for series.
/// GET /opds/series/:lang_code/
/// GET /opds/series/:lang_code/:prefix/
pub async fn series_feed(
    State(state): State<AppState>,
    path: Option<Path<AuthorsParams>>,
) -> Response {
    let max_items = state.config.opds.max_items as i32;

    match path {
        None => lang_selection_feed("Series", "/opds/series/").await,
        Some(Path(params)) => {
            let lang_code = params.lang_code;
            let prefix = params.prefix.unwrap_or_default();

            let mut fb = FeedBuilder::new();
            let self_href = if prefix.is_empty() {
                format!("/opds/series/{lang_code}/")
            } else {
                format!("/opds/series/{lang_code}/{prefix}/")
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

            let series_list = series::get_by_lang_code_prefix(
                &state.db,
                lang_code,
                &prefix.to_uppercase(),
                max_items,
                0,
            )
            .await
            .unwrap_or_default();

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
    }
}

/// GET /opds/genres/ — Genre sections.
/// GET /opds/genres/:section/ — Genres in section.
pub async fn genres_feed(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    path: Option<Path<(String,)>>,
) -> Response {
    let lang = detect_opds_lang(&headers, &state.config);
    let mut fb = FeedBuilder::new();

    match path {
        None => {
            // Show genre sections
            let _ = fb.begin_feed(
                "tag:genres",
                "Genres",
                "",
                DEFAULT_UPDATED,
                "/opds/genres/",
                "/opds/",
            );
            let _ = fb.write_search_links("/opds/search/", "/opds/search/{searchTerms}/");

            let sections = genres::get_sections(&state.db, &lang)
                .await
                .unwrap_or_default();
            for (i, (code, name)) in sections.iter().enumerate() {
                let href = format!("/opds/genres/{}/", urlencoding::encode(code));
                let _ = fb.write_nav_entry(&format!("gs:{i}"), name, &href, "", DEFAULT_UPDATED);
            }
        }
        Some(Path((section_code,))) => {
            let self_href = format!("/opds/genres/{}/", urlencoding::encode(&section_code));

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
                "/opds/",
            );
            let _ = fb.write_search_links("/opds/search/", "/opds/search/{searchTerms}/");

            for genre in &genre_list {
                let href = format!("/opds/search/books/g/{}/", genre.id);
                let _ = fb.write_nav_entry(
                    &format!("g:{}", genre.id),
                    &genre.subsection,
                    &href,
                    &genre.code,
                    DEFAULT_UPDATED,
                );
            }
        }
    }

    match fb.finish() {
        Ok(body) => atom_response(body),
        Err(_) => error_response(StatusCode::INTERNAL_SERVER_ERROR, "XML error"),
    }
}

/// GET /opds/books/ — Language selection for books by title.
/// GET /opds/books/:lang_code/
pub async fn books_feed(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    path: Option<Path<(i32,)>>,
) -> Response {
    let lang = detect_opds_lang(&headers, &state.config);
    match path {
        None => lang_selection_feed("Books", "/opds/books/").await,
        Some(Path((lang_code,))) => {
            let max_items = state.config.opds.max_items as i32;
            let mut fb = FeedBuilder::new();
            let self_href = format!("/opds/books/{lang_code}/");
            let _ = fb.begin_feed(
                &format!("tag:books:{lang_code}"),
                "Books",
                "",
                DEFAULT_UPDATED,
                &self_href,
                "/opds/",
            );
            let _ = fb.write_search_links("/opds/search/", "/opds/search/{searchTerms}/");

            // TODO: alphabet drill-down like authors/series
            // For now, show first page of books matching the lang_code
            let hide_doubles = state.config.opds.hide_doubles;
            let book_list = books::search_by_title(&state.db, "", max_items, 0, hide_doubles)
                .await
                .unwrap_or_default();
            for book in &book_list {
                write_book_entry(&mut fb, &state, book, &lang).await;
            }

            match fb.finish() {
                Ok(body) => atom_response(body),
                Err(_) => error_response(StatusCode::INTERNAL_SERVER_ERROR, "XML error"),
            }
        }
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
) -> Response {
    let lang = detect_opds_lang(&headers, &state.config);
    let max_items = state.config.opds.max_items as i32;
    let page = params.page.unwrap_or(1).max(1);
    let offset = (page - 1) * max_items;
    let search_type = &params.search_type;
    let terms = &params.terms;

    let mut fb = FeedBuilder::new();
    let self_href = format!(
        "/opds/search/books/{}/{}/{}/",
        search_type,
        urlencoding::encode(terms),
        page
    );
    let _ = fb.begin_feed(
        &format!("tag:search:books:{search_type}:{terms}:{page}"),
        &format!("Search: {terms}"),
        "",
        DEFAULT_UPDATED,
        &self_href,
        "/opds/",
    );
    let _ = fb.write_search_links("/opds/search/", "/opds/search/{searchTerms}/");

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
/// GET /opds/bookshelf/:page/
pub async fn bookshelf_feed(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    path: Option<Path<(i32,)>>,
) -> Response {
    let lang = detect_opds_lang(&headers, &state.config);
    let user_id = match super::auth::get_user_id_from_headers(&state.db, &headers).await {
        Some(uid) => uid,
        None => return error_response(StatusCode::UNAUTHORIZED, "Authentication required"),
    };

    let max_items = state.config.opds.max_items as i32;
    let page = path.map(|Path((p,))| p).unwrap_or(1).max(1);
    let offset = (page - 1) * max_items;

    let mut fb = FeedBuilder::new();
    let self_href = format!("/opds/bookshelf/{page}/");
    let _ = fb.begin_feed(
        &format!("tag:bookshelf:{page}"),
        "Book shelf",
        "",
        DEFAULT_UPDATED,
        &self_href,
        "/opds/",
    );
    let _ = fb.write_search_links("/opds/search/", "/opds/search/{searchTerms}/");

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
        Some(format!("/opds/bookshelf/{}/", page - 1))
    } else {
        None
    };
    let next_href = if has_next {
        Some(format!("/opds/bookshelf/{}/", page + 1))
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

/// GET /opds/search/ — OpenSearch description.
pub async fn opensearch(_state: State<AppState>) -> Response {
    let xml = r#"<?xml version="1.0" encoding="utf-8"?>
<OpenSearchDescription xmlns="http://a9.com/-/spec/opensearch/1.1/">
    <ShortName>ropds</ShortName>
    <LongName>Rust OPDS Server</LongName>
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

#[derive(serde::Deserialize)]
pub struct AuthorsParams {
    pub lang_code: i32,
    pub prefix: Option<String>,
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
async fn lang_selection_feed(title: &str, base_href: &str) -> Response {
    let mut fb = FeedBuilder::new();
    let _ = fb.begin_feed(
        &format!("tag:lang:{title}"),
        title,
        "",
        DEFAULT_UPDATED,
        base_href,
        "/opds/",
    );
    let _ = fb.write_search_links("/opds/search/", "/opds/search/{searchTerms}/");

    let entries = [
        ("l:0", "All", format!("{base_href}0/")),
        ("l:1", "Cyrillic", format!("{base_href}1/")),
        ("l:2", "Latin", format!("{base_href}2/")),
        ("l:3", "Digits", format!("{base_href}3/")),
        ("l:9", "Other", format!("{base_href}9/")),
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
