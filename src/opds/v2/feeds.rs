use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use serde_json::{Value, json};

use crate::db::queries::{authors, books, bookshelf, catalogs, genres, series};
use crate::state::AppState;

use super::helpers::*;
use super::{
    AuthorsListParams, AuthorsParams, CatalogsParams, LangQuery, SearchBooksParams,
};

pub async fn root_feed(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<LangQuery>,
) -> Response {
    build_root_feed(&state, &headers, q.lang.as_deref()).await
}

pub async fn root_feed_for_locale(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((locale,)): Path<(String,)>,
) -> Response {
    build_root_feed(&state, &headers, Some(locale.as_str())).await
}

async fn build_root_feed(
    state: &AppState,
    headers: &HeaderMap,
    query_lang: Option<&str>,
) -> Response {
    let lang = detect_opds_lang(headers, &state.config, query_lang);
    let by_catalogs = tr(state, &lang, "opds", "root_by_catalogs", "By Catalogs");
    let by_authors = tr(state, &lang, "opds", "root_by_authors", "By Authors");
    let by_genres = tr(state, &lang, "opds", "root_by_genres", "By Genres");
    let by_series = tr(state, &lang, "opds", "root_by_series", "By Series");
    let by_recent = tr(state, &lang, "opds", "root_by_recent", "Recently Added");
    let language_facets = tr(
        state,
        &lang,
        "opds",
        "root_language_facets",
        "Language facets",
    );

    let mut navigation = vec![
        nav_link(by_catalogs, add_lang_query("/opds/v2/catalogs/", &lang)),
        nav_link(by_authors, add_lang_query("/opds/v2/authors/", &lang)),
        nav_link(by_genres, add_lang_query("/opds/v2/genres/", &lang)),
        nav_link(by_series, add_lang_query("/opds/v2/series/", &lang)),
        nav_link(by_recent, add_lang_query("/opds/v2/recent/", &lang)),
        nav_link(
            language_facets,
            add_lang_query("/opds/v2/facets/languages/", &lang),
        ),
    ];

    if state.config.opds.auth_required
        && let Some(user_id) = crate::opds::auth::get_user_id_from_headers(&state.db, headers).await
    {
        let count = bookshelf::count_by_user(&state.db, user_id)
            .await
            .unwrap_or(0);
        let bookshelf_title = tr(state, &lang, "opds", "root_bookshelf", "Book shelf");
        navigation.push(nav_link(
            format!("{bookshelf_title}: {count}"),
            add_lang_query("/opds/v2/bookshelf/", &lang),
        ));
    }

    opds2_response(json!({
        "metadata": {
            "title": state.config.opds.title,
            "modified": DEFAULT_MODIFIED,
            "numberOfItems": navigation.len()
        },
        "links": feed_links(
            add_lang_query("/opds/v2/", &lang),
            add_lang_query("/opds/v2/", &lang),
            &lang
        ),
        "navigation": navigation
    }))
}

pub async fn catalogs_root(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<LangQuery>,
) -> Response {
    build_catalogs_feed(&state, &headers, q.lang.as_deref(), 0, 1).await
}

pub async fn catalogs_feed(
    State(state): State<AppState>,
    headers: HeaderMap,
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
    headers: &HeaderMap,
    query_lang: Option<&str>,
    cat_id: i64,
    page: i32,
) -> Response {
    let lang = detect_opds_lang(headers, &state.config, query_lang);
    let max_items = state.config.opds.max_items as i32;
    let offset = (page - 1) * max_items;

    let self_href = if cat_id == 0 {
        add_lang_query("/opds/v2/catalogs/", &lang)
    } else {
        add_lang_query(&format!("/opds/v2/catalogs/{cat_id}/{page}/"), &lang)
    };
    let mut links = feed_links(self_href, add_lang_query("/opds/v2/", &lang), &lang);
    let mut navigation = Vec::new();
    let mut publications = Vec::new();

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
        for cat in cats {
            navigation.push(nav_link(
                cat.cat_name,
                add_lang_query(&format!("/opds/v2/catalogs/{}/", cat.id), &lang),
            ));
        }
    }

    if cat_id > 0 {
        let hide_doubles = state.config.opds.hide_doubles;
        let book_list = books::get_by_catalog(&state.db, cat_id, max_items, offset, hide_doubles)
            .await
            .unwrap_or_default();

        let has_next = book_list.len() as i32 >= max_items;
        let has_prev = page > 1;
        if has_prev {
            links.push(json!({
                "rel": "prev",
                "href": add_lang_query(&format!("/opds/v2/catalogs/{cat_id}/{}/", page - 1), &lang),
                "type": OPDS2_TYPE
            }));
        }
        if has_next {
            links.push(json!({
                "rel": "next",
                "href": add_lang_query(&format!("/opds/v2/catalogs/{cat_id}/{}/", page + 1), &lang),
                "type": OPDS2_TYPE
            }));
        }

        for book in &book_list {
            publications.push(book_publication(state, book, &lang).await);
        }
    }

    let mut body = serde_json::Map::new();
    body.insert(
        "metadata".to_string(),
        json!({
            "title": "Catalogs",
            "modified": DEFAULT_MODIFIED,
            "numberOfItems": navigation.len() + publications.len()
        }),
    );
    body.insert("links".to_string(), Value::Array(links));
    if !navigation.is_empty() {
        body.insert("navigation".to_string(), Value::Array(navigation));
    }
    if !publications.is_empty() {
        body.insert("publications".to_string(), Value::Array(publications));
    }
    opds2_response(Value::Object(body))
}

async fn lang_selection_feed(
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

    let navigation = vec![
        nav_link(all_label, add_lang_query(&format!("{base_href}0/"), &lang)),
        nav_link(
            cyrillic_label,
            add_lang_query(&format!("{base_href}1/"), &lang),
        ),
        nav_link(
            latin_label,
            add_lang_query(&format!("{base_href}2/"), &lang),
        ),
        nav_link(
            digits_label,
            add_lang_query(&format!("{base_href}3/"), &lang),
        ),
        nav_link(
            other_label,
            add_lang_query(&format!("{base_href}9/"), &lang),
        ),
    ];

    opds2_response(json!({
        "metadata": {
            "title": title,
            "modified": DEFAULT_MODIFIED,
            "numberOfItems": navigation.len()
        },
        "links": feed_links(
            add_lang_query(base_href, &lang),
            add_lang_query("/opds/v2/", &lang),
            &lang
        ),
        "navigation": navigation
    }))
}

pub async fn authors_root(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<LangQuery>,
) -> Response {
    lang_selection_feed(
        &state,
        &headers,
        q.lang.as_deref(),
        "authors",
        "Authors",
        "/opds/v2/authors/",
    )
    .await
}

pub async fn authors_feed(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(params): Path<AuthorsParams>,
    Query(q): Query<LangQuery>,
) -> Response {
    let lang = detect_opds_lang(&headers, &state.config, q.lang.as_deref());
    let split_items = state.config.opds.split_items as i64;
    let prefix = params.prefix.unwrap_or_default();

    let groups =
        authors::get_name_prefix_groups(&state.db, params.lang_code, &prefix.to_uppercase())
            .await
            .unwrap_or_default();

    let mut navigation = Vec::with_capacity(groups.len());
    for (prefix_str, count) in &groups {
        let href = if *count >= split_items {
            format!(
                "/opds/v2/authors/{}/{}/",
                params.lang_code,
                urlencoding::encode(prefix_str)
            )
        } else {
            format!(
                "/opds/v2/authors/{}/{}/list/",
                params.lang_code,
                urlencoding::encode(prefix_str)
            )
        };
        navigation.push(nav_link(prefix_str.clone(), add_lang_query(&href, &lang)));
    }

    let self_href = if prefix.is_empty() {
        add_lang_query(&format!("/opds/v2/authors/{}/", params.lang_code), &lang)
    } else {
        add_lang_query(
            &format!(
                "/opds/v2/authors/{}/{}/",
                params.lang_code,
                urlencoding::encode(&prefix)
            ),
            &lang,
        )
    };

    opds2_response(json!({
        "metadata": {
            "title": tr(&state, &lang, "nav", "authors", "Authors"),
            "modified": DEFAULT_MODIFIED,
            "numberOfItems": navigation.len()
        },
        "links": feed_links(self_href, add_lang_query("/opds/v2/", &lang), &lang),
        "navigation": navigation
    }))
}

pub async fn authors_list(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(params): Path<AuthorsListParams>,
    Query(q): Query<LangQuery>,
) -> Response {
    let lang = detect_opds_lang(&headers, &state.config, q.lang.as_deref());
    let max_items = state.config.opds.max_items as i32;
    let page = params.page.unwrap_or(1).max(1);
    let offset = (page - 1) * max_items;

    let author_list = authors::get_by_lang_code_prefix(
        &state.db,
        params.lang_code,
        &params.prefix.to_uppercase(),
        max_items,
        offset,
    )
    .await
    .unwrap_or_default();

    let mut links = feed_links(
        add_lang_query(
            &format!(
                "/opds/v2/authors/{}/{}/list/{}/",
                params.lang_code,
                urlencoding::encode(&params.prefix),
                page
            ),
            &lang,
        ),
        add_lang_query("/opds/v2/", &lang),
        &lang,
    );
    if page > 1 {
        links.push(json!({
            "rel": "prev",
            "href": add_lang_query(
                &format!(
                    "/opds/v2/authors/{}/{}/list/{}/",
                    params.lang_code,
                    urlencoding::encode(&params.prefix),
                    page - 1
                ),
                &lang
            ),
            "type": OPDS2_TYPE
        }));
    }
    if author_list.len() as i32 >= max_items {
        links.push(json!({
            "rel": "next",
            "href": add_lang_query(
                &format!(
                    "/opds/v2/authors/{}/{}/list/{}/",
                    params.lang_code,
                    urlencoding::encode(&params.prefix),
                    page + 1
                ),
                &lang
            ),
            "type": OPDS2_TYPE
        }));
    }

    let navigation: Vec<Value> = author_list
        .iter()
        .map(|author| {
            nav_link(
                author.full_name.clone(),
                add_lang_query(&format!("/opds/v2/search/books/a/{}/", author.id), &lang),
            )
        })
        .collect();

    opds2_response(json!({
        "metadata": {
            "title": format!("{}: {}", tr(&state, &lang, "nav", "authors", "Authors"), params.prefix),
            "modified": DEFAULT_MODIFIED,
            "numberOfItems": navigation.len()
        },
        "links": links,
        "navigation": navigation
    }))
}

pub async fn series_root(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<LangQuery>,
) -> Response {
    lang_selection_feed(
        &state,
        &headers,
        q.lang.as_deref(),
        "series",
        "Series",
        "/opds/v2/series/",
    )
    .await
}

pub async fn series_feed(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(params): Path<AuthorsParams>,
    Query(q): Query<LangQuery>,
) -> Response {
    let lang = detect_opds_lang(&headers, &state.config, q.lang.as_deref());
    let split_items = state.config.opds.split_items as i64;
    let prefix = params.prefix.unwrap_or_default();

    let groups =
        series::get_name_prefix_groups(&state.db, params.lang_code, &prefix.to_uppercase())
            .await
            .unwrap_or_default();

    let mut navigation = Vec::with_capacity(groups.len());
    for (prefix_str, count) in &groups {
        let href = if *count >= split_items {
            format!(
                "/opds/v2/series/{}/{}/",
                params.lang_code,
                urlencoding::encode(prefix_str)
            )
        } else {
            format!(
                "/opds/v2/series/{}/{}/list/",
                params.lang_code,
                urlencoding::encode(prefix_str)
            )
        };
        navigation.push(nav_link(prefix_str.clone(), add_lang_query(&href, &lang)));
    }

    let self_href = if prefix.is_empty() {
        add_lang_query(&format!("/opds/v2/series/{}/", params.lang_code), &lang)
    } else {
        add_lang_query(
            &format!(
                "/opds/v2/series/{}/{}/",
                params.lang_code,
                urlencoding::encode(&prefix)
            ),
            &lang,
        )
    };

    opds2_response(json!({
        "metadata": {
            "title": tr(&state, &lang, "nav", "series", "Series"),
            "modified": DEFAULT_MODIFIED,
            "numberOfItems": navigation.len()
        },
        "links": feed_links(self_href, add_lang_query("/opds/v2/", &lang), &lang),
        "navigation": navigation
    }))
}

pub async fn series_list(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(params): Path<AuthorsListParams>,
    Query(q): Query<LangQuery>,
) -> Response {
    let lang = detect_opds_lang(&headers, &state.config, q.lang.as_deref());
    let max_items = state.config.opds.max_items as i32;
    let page = params.page.unwrap_or(1).max(1);
    let offset = (page - 1) * max_items;

    let series_list = series::get_by_lang_code_prefix(
        &state.db,
        params.lang_code,
        &params.prefix.to_uppercase(),
        max_items,
        offset,
    )
    .await
    .unwrap_or_default();

    let mut links = feed_links(
        add_lang_query(
            &format!(
                "/opds/v2/series/{}/{}/list/{}/",
                params.lang_code,
                urlencoding::encode(&params.prefix),
                page
            ),
            &lang,
        ),
        add_lang_query("/opds/v2/", &lang),
        &lang,
    );
    if page > 1 {
        links.push(json!({
            "rel": "prev",
            "href": add_lang_query(
                &format!(
                    "/opds/v2/series/{}/{}/list/{}/",
                    params.lang_code,
                    urlencoding::encode(&params.prefix),
                    page - 1
                ),
                &lang
            ),
            "type": OPDS2_TYPE
        }));
    }
    if series_list.len() as i32 >= max_items {
        links.push(json!({
            "rel": "next",
            "href": add_lang_query(
                &format!(
                    "/opds/v2/series/{}/{}/list/{}/",
                    params.lang_code,
                    urlencoding::encode(&params.prefix),
                    page + 1
                ),
                &lang
            ),
            "type": OPDS2_TYPE
        }));
    }

    let navigation: Vec<Value> = series_list
        .iter()
        .map(|ser| {
            nav_link(
                ser.ser_name.clone(),
                add_lang_query(&format!("/opds/v2/search/books/s/{}/", ser.id), &lang),
            )
        })
        .collect();

    opds2_response(json!({
        "metadata": {
            "title": format!("{}: {}", tr(&state, &lang, "nav", "series", "Series"), params.prefix),
            "modified": DEFAULT_MODIFIED,
            "numberOfItems": navigation.len()
        },
        "links": links,
        "navigation": navigation
    }))
}

pub async fn genres_root(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<LangQuery>,
) -> Response {
    let lang = detect_opds_lang(&headers, &state.config, q.lang.as_deref());
    let sections = genres::get_sections(&state.db, &lang)
        .await
        .unwrap_or_default();
    let navigation: Vec<Value> = sections
        .iter()
        .map(|(code, name)| {
            nav_link(
                name.clone(),
                add_lang_query(
                    &format!("/opds/v2/genres/{}/", urlencoding::encode(code)),
                    &lang,
                ),
            )
        })
        .collect();

    opds2_response(json!({
        "metadata": {
            "title": tr(&state, &lang, "nav", "genres", "Genres"),
            "modified": DEFAULT_MODIFIED,
            "numberOfItems": navigation.len()
        },
        "links": feed_links(
            add_lang_query("/opds/v2/genres/", &lang),
            add_lang_query("/opds/v2/", &lang),
            &lang
        ),
        "navigation": navigation
    }))
}

pub async fn genres_by_section(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((section_code,)): Path<(String,)>,
    Query(q): Query<LangQuery>,
) -> Response {
    let lang = detect_opds_lang(&headers, &state.config, q.lang.as_deref());
    let genre_list = genres::get_by_section(&state.db, &section_code, &lang)
        .await
        .unwrap_or_default();
    let title = genre_list
        .first()
        .map(|g| g.section.clone())
        .unwrap_or_else(|| section_code.clone());

    let navigation: Vec<Value> = genre_list
        .iter()
        .map(|g| {
            nav_link(
                g.subsection.clone(),
                add_lang_query(&format!("/opds/v2/search/books/g/{}/", g.id), &lang),
            )
        })
        .collect();

    opds2_response(json!({
        "metadata": {
            "title": title,
            "modified": DEFAULT_MODIFIED,
            "numberOfItems": navigation.len()
        },
        "links": feed_links(
            add_lang_query(
                &format!("/opds/v2/genres/{}/", urlencoding::encode(&section_code)),
                &lang
            ),
            add_lang_query("/opds/v2/", &lang),
            &lang
        ),
        "navigation": navigation
    }))
}

pub async fn language_facets_feed(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<LangQuery>,
) -> Response {
    let lang = detect_opds_lang(&headers, &state.config, q.lang.as_deref());
    let navigation: Vec<Value> = locale_choices(&state)
        .iter()
        .map(|locale| {
            nav_link(
                locale_label(&state, locale),
                format!("/opds/v2/lang/{}/", urlencoding::encode(locale)),
            )
        })
        .collect();

    opds2_response(json!({
        "metadata": {
            "title": tr(&state, &lang, "opds", "facet_title", "Language"),
            "modified": DEFAULT_MODIFIED,
            "numberOfItems": navigation.len()
        },
        "links": feed_links(
            add_lang_query("/opds/v2/facets/languages/", &lang),
            add_lang_query("/opds/v2/", &lang),
            &lang
        ),
        "navigation": navigation
    }))
}

pub async fn recent_root(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<LangQuery>,
) -> Response {
    build_recent_feed(&state, &headers, q.lang.as_deref(), 1).await
}

pub async fn recent_feed(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((page,)): Path<(i32,)>,
    Query(q): Query<LangQuery>,
) -> Response {
    build_recent_feed(&state, &headers, q.lang.as_deref(), page.max(1)).await
}

async fn build_recent_feed(
    state: &AppState,
    headers: &HeaderMap,
    query_lang: Option<&str>,
    page: i32,
) -> Response {
    let lang = detect_opds_lang(headers, &state.config, query_lang);
    let max_items = state.config.opds.max_items as i32;
    let offset = (page - 1) * max_items;
    let hide_doubles = state.config.opds.hide_doubles;

    let book_list = books::get_recent_added(&state.db, max_items, offset, hide_doubles)
        .await
        .unwrap_or_default();

    let mut links = feed_links(
        add_lang_query(&format!("/opds/v2/recent/{page}/"), &lang),
        add_lang_query("/opds/v2/", &lang),
        &lang,
    );
    if page > 1 {
        links.push(json!({
            "rel": "prev",
            "href": add_lang_query(&format!("/opds/v2/recent/{}/", page - 1), &lang),
            "type": OPDS2_TYPE
        }));
    }
    if book_list.len() as i32 >= max_items {
        links.push(json!({
            "rel": "next",
            "href": add_lang_query(&format!("/opds/v2/recent/{}/", page + 1), &lang),
            "type": OPDS2_TYPE
        }));
    }

    let mut publications = Vec::with_capacity(book_list.len());
    for book in &book_list {
        publications.push(book_publication(state, book, &lang).await);
    }

    opds2_response(json!({
        "metadata": {
            "title": tr(state, &lang, "opds", "root_by_recent", "Recently Added"),
            "modified": DEFAULT_MODIFIED,
            "numberOfItems": publications.len()
        },
        "links": links,
        "publications": publications
    }))
}

pub async fn bookshelf_root(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<LangQuery>,
) -> Response {
    build_bookshelf_feed(&state, &headers, q.lang.as_deref(), 1).await
}

pub async fn bookshelf_feed(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((page,)): Path<(i32,)>,
    Query(q): Query<LangQuery>,
) -> Response {
    build_bookshelf_feed(&state, &headers, q.lang.as_deref(), page.max(1)).await
}

async fn build_bookshelf_feed(
    state: &AppState,
    headers: &HeaderMap,
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
    let book_list = bookshelf::get_by_user(
        &state.db,
        user_id,
        &bookshelf::SortColumn::Date,
        false,
        max_items,
        offset,
    )
    .await
    .unwrap_or_default();

    let mut links = feed_links(
        add_lang_query(&format!("/opds/v2/bookshelf/{page}/"), &lang),
        add_lang_query("/opds/v2/", &lang),
        &lang,
    );
    if page > 1 {
        links.push(json!({
            "rel": "prev",
            "href": add_lang_query(&format!("/opds/v2/bookshelf/{}/", page - 1), &lang),
            "type": OPDS2_TYPE
        }));
    }
    if book_list.len() as i32 >= max_items {
        links.push(json!({
            "rel": "next",
            "href": add_lang_query(&format!("/opds/v2/bookshelf/{}/", page + 1), &lang),
            "type": OPDS2_TYPE
        }));
    }

    let mut publications = Vec::with_capacity(book_list.len());
    for book in &book_list {
        publications.push(book_publication(state, book, &lang).await);
    }

    opds2_response(json!({
        "metadata": {
            "title": tr(state, &lang, "opds", "root_bookshelf", "Book shelf"),
            "modified": DEFAULT_MODIFIED,
            "numberOfItems": publications.len()
        },
        "links": links,
        "publications": publications
    }))
}

pub async fn search_books_default(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((terms,)): Path<(String,)>,
    Query(q): Query<LangQuery>,
) -> Response {
    build_search_books_feed(&state, &headers, q.lang.as_deref(), "m", &terms, 1).await
}

pub async fn search_books_feed(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(params): Path<SearchBooksParams>,
    Query(q): Query<LangQuery>,
) -> Response {
    build_search_books_feed(
        &state,
        &headers,
        q.lang.as_deref(),
        &params.search_type,
        &params.terms,
        params.page.unwrap_or(1).max(1),
    )
    .await
}

async fn build_search_books_feed(
    state: &AppState,
    headers: &HeaderMap,
    query_lang: Option<&str>,
    search_type: &str,
    terms: &str,
    page: i32,
) -> Response {
    let lang = detect_opds_lang(headers, &state.config, query_lang);
    let max_items = state.config.opds.max_items as i32;
    let offset = (page - 1) * max_items;
    let hide_doubles = state.config.opds.hide_doubles;

    let book_list = match search_type {
        "a" => {
            let author_id: i64 = terms.parse().unwrap_or(0);
            books::get_by_author(&state.db, author_id, max_items, offset, hide_doubles)
                .await
                .unwrap_or_default()
        }
        "s" => {
            let series_id: i64 = terms.parse().unwrap_or(0);
            books::get_by_series(&state.db, series_id, max_items, offset, hide_doubles)
                .await
                .unwrap_or_default()
        }
        "g" => {
            let genre_id: i64 = terms.parse().unwrap_or(0);
            books::get_by_genre(&state.db, genre_id, max_items, offset, hide_doubles)
                .await
                .unwrap_or_default()
        }
        _ => {
            let search_term = terms.to_uppercase();
            books::search_by_title(&state.db, &search_term, max_items, offset, hide_doubles)
                .await
                .unwrap_or_default()
        }
    };

    let mut links = feed_links(
        add_lang_query(
            &format!(
                "/opds/v2/search/books/{}/{}/{}/",
                search_type,
                urlencoding::encode(terms),
                page
            ),
            &lang,
        ),
        add_lang_query("/opds/v2/", &lang),
        &lang,
    );
    if page > 1 {
        links.push(json!({
            "rel": "prev",
            "href": add_lang_query(
                &format!(
                    "/opds/v2/search/books/{}/{}/{}/",
                    search_type,
                    urlencoding::encode(terms),
                    page - 1
                ),
                &lang
            ),
            "type": OPDS2_TYPE
        }));
    }
    if book_list.len() as i32 >= max_items {
        links.push(json!({
            "rel": "next",
            "href": add_lang_query(
                &format!(
                    "/opds/v2/search/books/{}/{}/{}/",
                    search_type,
                    urlencoding::encode(terms),
                    page + 1
                ),
                &lang
            ),
            "type": OPDS2_TYPE
        }));
    }

    let mut publications = Vec::with_capacity(book_list.len());
    for book in &book_list {
        publications.push(book_publication(state, book, &lang).await);
    }

    opds2_response(json!({
        "metadata": {
            "title": format!("Search: {terms}"),
            "modified": DEFAULT_MODIFIED,
            "numberOfItems": publications.len()
        },
        "links": links,
        "publications": publications
    }))
}
