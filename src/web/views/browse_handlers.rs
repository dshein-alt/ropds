use super::*;

pub async fn home(
    State(state): State<AppState>,
    jar: CookieJar,
) -> Result<Html<String>, StatusCode> {
    let mut ctx = build_context(&state, &jar, "home").await;

    if state.config.reader.enable {
        if let Some(user_id) = session_user_id(&state, &jar) {
            let recent = reading_positions::get_recent(&state.db, user_id, 8)
                .await
                .unwrap_or_default();
            let continue_reading: Vec<ContinueReadingItem> = recent
                .into_iter()
                .map(|item| ContinueReadingItem {
                    book_id: item.book_id,
                    title: item.title,
                    format: item.format,
                    progress_pct: (item.progress.clamp(0.0, 1.0) * 100.0).round() as i32,
                    updated_at: item.updated_at,
                })
                .collect();

            ctx.insert("continue_reading", &continue_reading);
        }
    }

    render(&state.tera, "web/home.html", &ctx)
}

pub async fn recent_books(
    State(state): State<AppState>,
    jar: CookieJar,
    Query(params): Query<RecentBooksParams>,
) -> Result<Html<String>, StatusCode> {
    let mut ctx = build_context(&state, &jar, "recent").await;
    let page = params.page.max(0);
    let max_items = state.config.opds.max_items as i32;
    let offset = page * max_items;
    let hide_doubles = state.config.opds.hide_doubles;
    let locale = jar
        .get("lang")
        .map(|c| c.value().to_string())
        .unwrap_or_else(|| state.config.web.language.clone());

    let raw_books = books::get_recent_added(&state.db, max_items, offset, hide_doubles)
        .await
        .unwrap_or_default();
    let total = books::count_recent_added(&state.db, hide_doubles)
        .await
        .unwrap_or(0);

    let user_id = session_user_id(&state, &jar);
    let shelf_ids = if let Some(uid) = user_id {
        bookshelf::get_book_ids_for_user(&state.db, uid).await.ok()
    } else {
        None
    };
    let raw_book_ids: Vec<i64> = raw_books.iter().map(|book| book.id).collect();
    let read_progress = if let Some(uid) = user_id {
        reading_positions::get_progress_map(&state.db, uid, &raw_book_ids)
            .await
            .unwrap_or_default()
    } else {
        std::collections::HashMap::new()
    };

    let mut book_views = Vec::with_capacity(raw_books.len());
    for book in raw_books {
        let book_id = book.id;
        book_views.push(
            enrich_book(
                &state,
                book,
                hide_doubles,
                shelf_ids.as_ref(),
                read_progress.get(&book_id).copied(),
                &locale,
            )
            .await,
        );
    }

    let t = i18n::get_locale(&state.translations, &locale);
    let recent_label = t
        .get("nav")
        .and_then(|nav| nav.get("recent"))
        .and_then(|value| value.as_str())
        .unwrap_or("Recently added");

    ctx.insert("books", &book_views);
    ctx.insert("search_label", recent_label);
    ctx.insert("pagination", &Pagination::new(page, max_items, total));
    ctx.insert("pagination_qs", "");
    ctx.insert("current_path", &format!("/web/recent?page={page}"));

    render(&state.tera, "web/books.html", &ctx)
}

pub async fn catalogs(
    State(state): State<AppState>,
    jar: CookieJar,
    Query(params): Query<CatalogsParams>,
) -> Result<Html<String>, StatusCode> {
    let mut ctx = build_context(&state, &jar, "catalogs").await;
    let max_items = state.config.opds.max_items as i32;
    let cat_id = params.cat_id.unwrap_or(0);
    let offset = params.page * max_items;

    let subcatalogs = if cat_id == 0 {
        catalogs::get_root_catalogs(&state.db)
            .await
            .unwrap_or_default()
    } else {
        catalogs::get_children(&state.db, cat_id)
            .await
            .unwrap_or_default()
    };

    let hide_doubles = state.config.opds.hide_doubles;
    let (catalog_books, book_total) = if cat_id > 0 {
        let bks = books::get_by_catalog(&state.db, cat_id, max_items, offset, hide_doubles)
            .await
            .unwrap_or_default();
        let cnt = books::count_by_catalog(&state.db, cat_id, hide_doubles)
            .await
            .unwrap_or(0);
        (bks, cnt)
    } else {
        (vec![], 0)
    };

    let mut entries: Vec<CatalogEntry> = subcatalogs
        .iter()
        .map(|c| CatalogEntry {
            id: c.id,
            cat_name: c.cat_name.clone(),
            cat_type: c.cat_type,
            is_catalog: true,
            title: None,
            format: None,
            authors_str: None,
        })
        .collect();

    for book in &catalog_books {
        let book_authors = authors::get_for_book(&state.db, book.id)
            .await
            .unwrap_or_default();
        let authors_str = book_authors
            .iter()
            .map(|a| a.full_name.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        entries.push(CatalogEntry {
            id: book.id,
            cat_name: String::new(),
            cat_type: 0,
            is_catalog: false,
            title: Some(book.title.clone()),
            format: Some(book.format.clone()),
            authors_str: Some(authors_str),
        });
    }

    ctx.insert("entries", &entries);
    ctx.insert("cat_id", &cat_id);
    ctx.insert("pagination_qs", &format!("cat_id={}&", cat_id));

    if cat_id > 0 {
        let crumbs = build_breadcrumbs(&state, cat_id).await;
        if let Some(last) = crumbs.last() {
            ctx.insert("current_cat_name", &last.name);
        }
        if crumbs.len() > 1 {
            let parent = &crumbs[crumbs.len() - 2];
            ctx.insert(
                "parent_url",
                &format!("/web/catalogs?cat_id={}", parent.cat_id.unwrap_or(0)),
            );
            ctx.insert("parent_name", &parent.name);
        } else {
            ctx.insert("parent_url", "/web/catalogs");
        }
        ctx.insert("breadcrumbs", &crumbs);
    }

    let pagination = Pagination::new(params.page, max_items, book_total);
    ctx.insert("pagination", &pagination);

    render(&state.tera, "web/catalogs.html", &ctx)
}

pub async fn search_books(
    State(state): State<AppState>,
    jar: CookieJar,
    Query(params): Query<SearchBooksParams>,
) -> Result<Html<String>, StatusCode> {
    let mut ctx = build_context(&state, &jar, "books").await;
    let locale = jar
        .get("lang")
        .map(|c| c.value().to_string())
        .unwrap_or_else(|| state.config.web.language.clone());
    let search_target = match params.search_type.as_str() {
        "a" => "author",
        "s" => "series",
        _ => "title",
    };
    ctx.insert("search_target", search_target);
    let max_items = state.config.opds.max_items as i32;
    let offset = params.page * max_items;

    let hide_doubles = state.config.opds.hide_doubles;
    let (raw_books, total) = match params.search_type.as_str() {
        "a" => {
            let id: i64 = params.q.parse().unwrap_or(0);
            let bks = books::get_by_author(&state.db, id, max_items, offset, hide_doubles)
                .await
                .unwrap_or_default();
            let cnt = books::count_by_author(&state.db, id, hide_doubles)
                .await
                .unwrap_or(0);
            if let Ok(Some(author)) = authors::get_by_id(&state.db, id).await {
                ctx.insert("search_label", &author.full_name);
            }
            let t = i18n::get_locale(&state.translations, &locale);
            let label = t["nav"]["authors"].as_str().unwrap_or("Authors");
            ctx.insert("back_label", label);
            if let Some(src_q) = params.src_q.as_deref().filter(|s| !s.trim().is_empty()) {
                ctx.insert(
                    "back_url",
                    &format!(
                        "/web/search/authors?type=b&q={}",
                        urlencoding::encode(src_q)
                    ),
                );
            } else {
                ctx.insert("back_url", "/web/authors");
            }
            (bks, cnt)
        }
        "s" => {
            let id: i64 = params.q.parse().unwrap_or(0);
            let bks = books::get_by_series(&state.db, id, max_items, offset, hide_doubles)
                .await
                .unwrap_or_default();
            let cnt = books::count_by_series(&state.db, id, hide_doubles)
                .await
                .unwrap_or(0);
            if let Ok(Some(ser)) = series::get_by_id(&state.db, id).await {
                ctx.insert("search_label", &ser.ser_name);
            }
            let t = i18n::get_locale(&state.translations, &locale);
            let label = t["nav"]["series"].as_str().unwrap_or("Series");
            ctx.insert("back_label", label);
            if let Some(src_q) = params.src_q.as_deref().filter(|s| !s.trim().is_empty()) {
                ctx.insert(
                    "back_url",
                    &format!("/web/search/series?type=b&q={}", urlencoding::encode(src_q)),
                );
            } else {
                ctx.insert("back_url", "/web/series");
            }
            (bks, cnt)
        }
        "g" => {
            let id: i64 = params.q.parse().unwrap_or(0);
            let bks = books::get_by_genre(&state.db, id, max_items, offset, hide_doubles)
                .await
                .unwrap_or_default();
            let cnt = books::count_by_genre(&state.db, id, hide_doubles)
                .await
                .unwrap_or(0);
            if let Ok(Some(genre)) = genres::get_by_id(&state.db, id, &locale).await {
                ctx.insert("search_label", &genre.subsection);
                // Back navigation to the genre's section
                if let Some(section_id) = genre.section_id
                    && let Ok(Some(code)) = genres::get_section_code(&state.db, section_id).await
                {
                    ctx.insert(
                        "back_url",
                        &format!("/web/genres?section={}", urlencoding::encode(&code)),
                    );
                    ctx.insert("back_label", &genre.section);
                }
            }
            (bks, cnt)
        }
        "b" => {
            let term = params.q.to_uppercase();
            let bks =
                books::search_by_title_prefix(&state.db, &term, max_items, offset, hide_doubles)
                    .await
                    .unwrap_or_default();
            let cnt = books::count_by_title_prefix(&state.db, &term, hide_doubles)
                .await
                .unwrap_or(0);
            ctx.insert("search_label", &params.q);
            let t = i18n::get_locale(&state.translations, &locale);
            let label = t["nav"]["books"].as_str().unwrap_or("Books");
            ctx.insert("back_label", label);
            ctx.insert("back_url", "/web/books");
            (bks, cnt)
        }
        "i" => {
            let id: i64 = params.q.parse().unwrap_or(0);
            let bks = books::get_by_id(&state.db, id)
                .await
                .ok()
                .flatten()
                .map(|b| vec![b])
                .unwrap_or_default();
            let cnt = bks.len() as i64;
            (bks, cnt)
        }
        _ => {
            let term = params.q.to_uppercase();
            let bks = books::search_by_title(&state.db, &term, max_items, offset, hide_doubles)
                .await
                .unwrap_or_default();
            let cnt = books::count_by_title_search(&state.db, &term, hide_doubles)
                .await
                .unwrap_or(0);
            ctx.insert("search_label", &params.q);
            (bks, cnt)
        }
    };

    let user_id = session_user_id(&state, &jar);
    let shelf_ids = if let Some(user_id) = user_id {
        crate::db::queries::bookshelf::get_book_ids_for_user(&state.db, user_id)
            .await
            .ok()
    } else {
        None
    };
    let raw_book_ids: Vec<i64> = raw_books.iter().map(|book| book.id).collect();
    let read_progress = if let Some(user_id) = user_id {
        reading_positions::get_progress_map(&state.db, user_id, &raw_book_ids)
            .await
            .unwrap_or_default()
    } else {
        std::collections::HashMap::new()
    };

    let mut book_views = Vec::with_capacity(raw_books.len());
    for book in raw_books {
        let progress = read_progress.get(&book.id).copied();
        book_views.push(
            enrich_book(
                &state,
                book,
                hide_doubles,
                shelf_ids.as_ref(),
                progress,
                &locale,
            )
            .await,
        );
    }

    let pagination = Pagination::new(params.page, max_items, total);

    let display_query = match params.search_type.as_str() {
        // Preserve original typed query for grouped author/series flows.
        "a" | "s" => params
            .src_q
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(&params.q)
            .to_string(),
        // ID-based lookups (genre, direct book jump) should not prefill the search box.
        "g" | "i" => String::new(),
        _ => params.q.clone(),
    };

    let mut pagination_qs = format!(
        "type={}&q={}&",
        params.search_type,
        urlencoding::encode(&params.q)
    );
    if let Some(src_q) = params.src_q.as_deref().filter(|s| !s.trim().is_empty()) {
        pagination_qs.push_str(&format!("src_q={}&", urlencoding::encode(src_q)));
    }

    let current_url = format!("/web/search/books?{}", pagination_qs);
    ctx.insert("current_path", &current_url);
    ctx.insert("books", &book_views);
    ctx.insert("pagination", &pagination);
    ctx.insert("search_type", &params.search_type);
    ctx.insert("search_terms", &display_query);
    ctx.insert("pagination_qs", &pagination_qs);

    render(&state.tera, "web/books.html", &ctx)
}

pub async fn books_browse(
    State(state): State<AppState>,
    jar: CookieJar,
    Query(params): Query<BrowseParams>,
) -> Result<Html<String>, StatusCode> {
    let mut ctx = build_context(&state, &jar, "books").await;
    let split_items = state.config.opds.split_items as i64;

    let prefix = params.chars.to_uppercase();
    let groups = books::get_title_prefix_groups(&state.db, params.lang, &prefix)
        .await
        .unwrap_or_default();

    let prefix_groups: Vec<PrefixGroup> = groups
        .into_iter()
        .map(|(p, cnt)| PrefixGroup {
            prefix: p,
            count: cnt,
            drill_deeper: cnt >= split_items,
        })
        .collect();

    ctx.insert("groups", &prefix_groups);
    ctx.insert("lang", &params.lang);
    ctx.insert("chars", &prefix);
    ctx.insert("browse_type", "books");
    ctx.insert("search_url", "/web/search/books");
    ctx.insert("browse_url", "/web/books");
    ctx.insert("search_type_param", "b");

    render(&state.tera, "web/browse.html", &ctx)
}

pub async fn authors_browse(
    State(state): State<AppState>,
    jar: CookieJar,
    Query(params): Query<BrowseParams>,
) -> Result<Html<String>, StatusCode> {
    let mut ctx = build_context(&state, &jar, "authors").await;
    let split_items = state.config.opds.split_items as i64;

    let prefix = params.chars.to_uppercase();
    let groups = authors::get_name_prefix_groups(&state.db, params.lang, &prefix)
        .await
        .unwrap_or_default();

    let prefix_groups: Vec<PrefixGroup> = groups
        .into_iter()
        .map(|(p, cnt)| PrefixGroup {
            prefix: p,
            count: cnt,
            drill_deeper: cnt >= split_items,
        })
        .collect();

    ctx.insert("groups", &prefix_groups);
    ctx.insert("lang", &params.lang);
    ctx.insert("chars", &prefix);
    ctx.insert("browse_type", "authors");
    ctx.insert("search_url", "/web/search/authors");
    ctx.insert("browse_url", "/web/authors");
    ctx.insert("search_type_param", "b");

    render(&state.tera, "web/browse.html", &ctx)
}

pub async fn series_browse(
    State(state): State<AppState>,
    jar: CookieJar,
    Query(params): Query<BrowseParams>,
) -> Result<Html<String>, StatusCode> {
    let mut ctx = build_context(&state, &jar, "series").await;
    let split_items = state.config.opds.split_items as i64;

    let prefix = params.chars.to_uppercase();
    let groups = series::get_name_prefix_groups(&state.db, params.lang, &prefix)
        .await
        .unwrap_or_default();

    let prefix_groups: Vec<PrefixGroup> = groups
        .into_iter()
        .map(|(p, cnt)| PrefixGroup {
            prefix: p,
            count: cnt,
            drill_deeper: cnt >= split_items,
        })
        .collect();

    ctx.insert("groups", &prefix_groups);
    ctx.insert("lang", &params.lang);
    ctx.insert("chars", &prefix);
    ctx.insert("browse_type", "series");
    ctx.insert("search_url", "/web/search/series");
    ctx.insert("browse_url", "/web/series");
    ctx.insert("search_type_param", "b");

    render(&state.tera, "web/browse.html", &ctx)
}

pub async fn genres(
    State(state): State<AppState>,
    jar: CookieJar,
    Query(params): Query<GenresParams>,
) -> Result<Html<String>, StatusCode> {
    let mut ctx = build_context(&state, &jar, "genres").await;
    let locale = jar
        .get("lang")
        .map(|c| c.value().to_string())
        .unwrap_or_else(|| state.config.web.language.clone());

    match params.section {
        None => {
            let sections = genres::get_sections_with_counts(&state.db, &locale)
                .await
                .unwrap_or_default();
            ctx.insert("sections", &sections);
            ctx.insert("is_top_level", &true);
        }
        Some(ref section_code) => {
            let subsections = genres::get_by_section_with_counts(&state.db, section_code, &locale)
                .await
                .unwrap_or_default();
            // Extract translated section name from the first genre
            let section_name = subsections
                .first()
                .map(|(g, _)| g.section.clone())
                .unwrap_or_else(|| section_code.clone());
            let items: Vec<serde_json::Value> = subsections
                .into_iter()
                .map(|(g, cnt)| {
                    serde_json::json!({
                        "id": g.id,
                        "subsection": g.subsection,
                        "code": g.code,
                        "count": cnt,
                    })
                })
                .collect();
            ctx.insert("subsections", &items);
            ctx.insert("is_top_level", &false);
            ctx.insert("section_code", section_code);
            ctx.insert("section_name", &section_name);
        }
    }

    render(&state.tera, "web/genres.html", &ctx)
}

pub async fn search_authors(
    State(state): State<AppState>,
    jar: CookieJar,
    Query(params): Query<SearchListParams>,
) -> Result<Html<String>, StatusCode> {
    let mut ctx = build_context(&state, &jar, "authors").await;
    ctx.insert("search_target", "author");
    let max_items = state.config.opds.max_items as i32;
    let offset = params.page * max_items;

    let term = params.q.to_uppercase();
    let items = authors::search_by_name(&state.db, &term, max_items, offset)
        .await
        .unwrap_or_default();
    let total = authors::count_by_name_search(&state.db, &term)
        .await
        .unwrap_or(0);

    let hide_doubles = state.config.opds.hide_doubles;
    let mut enriched: Vec<serde_json::Value> = Vec::new();
    for author in &items {
        let book_count = books::count_by_author(&state.db, author.id, hide_doubles)
            .await
            .unwrap_or(0);
        enriched.push(serde_json::json!({
            "id": author.id,
            "full_name": author.full_name,
            "book_count": book_count,
        }));
    }

    let pagination = Pagination::new(params.page, max_items, total);
    let search_terms_encoded = urlencoding::encode(&params.q).to_string();

    ctx.insert("authors", &enriched);
    ctx.insert("pagination", &pagination);
    ctx.insert("search_terms", &params.q);
    ctx.insert("search_terms_encoded", &search_terms_encoded);
    ctx.insert("back_url", "/web/authors");
    ctx.insert(
        "pagination_qs",
        &format!(
            "type={}&q={}&",
            params.search_type,
            urlencoding::encode(&params.q)
        ),
    );

    render(&state.tera, "web/authors.html", &ctx)
}

pub async fn search_series(
    State(state): State<AppState>,
    jar: CookieJar,
    Query(params): Query<SearchListParams>,
) -> Result<Html<String>, StatusCode> {
    let mut ctx = build_context(&state, &jar, "series").await;
    ctx.insert("search_target", "series");
    let max_items = state.config.opds.max_items as i32;
    let offset = params.page * max_items;

    let term = params.q.to_uppercase();
    let items = series::search_by_name(&state.db, &term, max_items, offset)
        .await
        .unwrap_or_default();
    let total = series::count_by_name_search(&state.db, &term)
        .await
        .unwrap_or(0);

    let hide_doubles = state.config.opds.hide_doubles;
    let mut enriched: Vec<serde_json::Value> = Vec::new();
    for ser in &items {
        let book_count = books::count_by_series(&state.db, ser.id, hide_doubles)
            .await
            .unwrap_or(0);
        enriched.push(serde_json::json!({
            "id": ser.id,
            "ser_name": ser.ser_name,
            "book_count": book_count,
        }));
    }

    let pagination = Pagination::new(params.page, max_items, total);
    let search_terms_encoded = urlencoding::encode(&params.q).to_string();

    ctx.insert("series_list", &enriched);
    ctx.insert("pagination", &pagination);
    ctx.insert("search_terms", &params.q);
    ctx.insert("search_terms_encoded", &search_terms_encoded);
    ctx.insert("back_url", "/web/series");
    ctx.insert(
        "pagination_qs",
        &format!(
            "type={}&q={}&",
            params.search_type,
            urlencoding::encode(&params.q)
        ),
    );

    render(&state.tera, "web/series.html", &ctx)
}

pub async fn set_language(
    jar: CookieJar,
    Query(params): Query<SetLanguageParams>,
) -> (CookieJar, Redirect) {
    let cookie = Cookie::build(("lang", params.lang))
        .path("/")
        .max_age(time::Duration::days(365))
        .build();
    let jar = jar.add(cookie);
    let redirect = sanitize_internal_redirect(params.redirect.as_deref());
    (jar, Redirect::to(redirect))
}
