use axum::extract::{Path, Query, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};

use crate::db::queries::{authors, books, catalogs, genres, series};
use crate::state::AppState;

use super::helpers::*;
use super::xml::{self, FeedBuilder};
use super::{AuthorsListParams, AuthorsParams, CatalogsParams, LangQuery, SearchBooksParams};

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
        && let Some(user_id) = crate::opds::auth::get_user_id_from_headers(&state.db, headers).await
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
    let user_id = match crate::opds::auth::get_user_id_from_headers(&state.db, headers).await {
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
