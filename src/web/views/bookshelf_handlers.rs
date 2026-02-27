use super::*;

pub async fn genres_json(State(state): State<AppState>, jar: CookieJar) -> Response {
    let secret = state.config.server.session_secret.as_bytes();
    if jar
        .get("session")
        .and_then(|c| crate::web::auth::verify_session(c.value(), secret))
        .is_none()
    {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    let locale = jar
        .get("lang")
        .map(|c| c.value().to_string())
        .unwrap_or_else(|| state.config.web.language.clone());
    let all_genres = genres::get_all(&state.db, &locale)
        .await
        .unwrap_or_default();

    let mut sections: std::collections::BTreeMap<String, Vec<serde_json::Value>> =
        std::collections::BTreeMap::new();
    for g in &all_genres {
        sections
            .entry(g.section.clone())
            .or_default()
            .push(serde_json::json!({
                "id": g.id,
                "code": g.code,
                "subsection": g.subsection,
            }));
    }

    axum::Json(serde_json::json!({ "sections": sections })).into_response()
}

// ── Bookshelf toggle handler ────────────────────────────────────────

#[derive(Deserialize)]
pub struct BookshelfToggleForm {
    pub book_id: i64,
    pub csrf_token: String,
    pub redirect: Option<String>,
}

pub async fn bookshelf_toggle(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: axum::http::HeaderMap,
    axum::Form(form): axum::Form<BookshelfToggleForm>,
) -> Response {
    use crate::web::context::validate_csrf;

    let is_ajax = headers
        .get("X-Requested-With")
        .and_then(|v| v.to_str().ok())
        == Some("XMLHttpRequest");

    let secret = state.config.server.session_secret.as_bytes();
    if !validate_csrf(&jar, secret, &form.csrf_token) {
        if is_ajax {
            return axum::Json(serde_json::json!({"ok": false})).into_response();
        }
        return (StatusCode::FORBIDDEN, "Invalid CSRF token").into_response();
    }

    let user_id = match jar
        .get("session")
        .and_then(|c| crate::web::auth::verify_session(c.value(), secret))
    {
        Some(uid) => uid,
        None => {
            if is_ajax {
                return axum::Json(serde_json::json!({"ok": false})).into_response();
            }
            return Redirect::to("/web/login").into_response();
        }
    };

    let on_shelf = bookshelf::is_on_shelf(&state.db, user_id, form.book_id)
        .await
        .unwrap_or(false);
    if on_shelf {
        let _ = bookshelf::delete_one(&state.db, user_id, form.book_id).await;
    } else {
        let _ = bookshelf::upsert(&state.db, user_id, form.book_id).await;
    }

    if is_ajax {
        return axum::Json(serde_json::json!({"ok": true, "on_shelf": !on_shelf})).into_response();
    }

    let redirect = form
        .redirect
        .filter(|r| !r.is_empty() && r.starts_with('/'))
        .unwrap_or_else(|| "/web".to_string());
    Redirect::to(&redirect).into_response()
}

// ── Bookshelf helpers ───────────────────────────────────────────────

pub(super) fn parse_bookshelf_sort(sort: &str, dir: &str) -> (bookshelf::SortColumn, bool) {
    let col = match sort {
        "title" => bookshelf::SortColumn::Title,
        "author" => bookshelf::SortColumn::Author,
        _ => bookshelf::SortColumn::Date,
    };
    let ascending = dir == "asc";
    (col, ascending)
}

const BOOKSHELF_BATCH: i32 = 30;

pub(super) async fn fetch_bookshelf_views(
    state: &AppState,
    user_id: i64,
    sort: &bookshelf::SortColumn,
    ascending: bool,
    limit: i32,
    offset: i32,
    lang: &str,
) -> Vec<BookView> {
    let raw_books = bookshelf::get_by_user(&state.db, user_id, sort, ascending, limit, offset)
        .await
        .unwrap_or_default();
    let read_times = bookshelf::get_read_times(&state.db, user_id)
        .await
        .unwrap_or_default();
    let raw_book_ids: Vec<i64> = raw_books.iter().map(|book| book.id).collect();
    let read_progress = reading_positions::get_progress_map(&state.db, user_id, &raw_book_ids)
        .await
        .unwrap_or_default();

    let shelf_ids: std::collections::HashSet<i64> = raw_books.iter().map(|b| b.id).collect();
    let hide_doubles = state.config.opds.hide_doubles;
    let mut views = Vec::with_capacity(raw_books.len());
    for book in raw_books {
        let bid = book.id;
        let mut v = enrich_book(
            state,
            book,
            hide_doubles,
            Some(&shelf_ids),
            read_progress.get(&bid).copied(),
            lang,
        )
        .await;
        if let Some(rt) = read_times.get(&bid) {
            v.read_time = rt.clone();
        }
        views.push(v);
    }
    views
}

// ── Bookshelf page handler ──────────────────────────────────────────

#[derive(Deserialize)]
pub struct BookshelfPageParams {
    #[serde(default)]
    pub sort: String,
    #[serde(default)]
    pub dir: String,
}

pub async fn bookshelf_page(
    State(state): State<AppState>,
    jar: CookieJar,
    Query(params): Query<BookshelfPageParams>,
) -> Result<Html<String>, StatusCode> {
    let mut ctx = build_context(&state, &jar, "bookshelf").await;
    let locale = jar
        .get("lang")
        .map(|c| c.value().to_string())
        .unwrap_or_else(|| state.config.web.language.clone());

    let user_id = match session_user_id(&state, &jar) {
        Some(uid) => uid,
        None => return Err(StatusCode::UNAUTHORIZED),
    };

    let sort_key = if params.sort.is_empty() {
        "date"
    } else {
        &params.sort
    };
    let dir_key = if params.dir.is_empty() {
        "desc"
    } else {
        &params.dir
    };
    let (sort_col, ascending) = parse_bookshelf_sort(sort_key, dir_key);

    let total = bookshelf::count_by_user(&state.db, user_id)
        .await
        .unwrap_or(0);

    let book_views = fetch_bookshelf_views(
        &state,
        user_id,
        &sort_col,
        ascending,
        BOOKSHELF_BATCH,
        0,
        &locale,
    )
    .await;

    let has_more = (book_views.len() as i64) < total;

    ctx.insert("books", &book_views);
    ctx.insert("current_path", "/web/bookshelf");
    ctx.insert("sort", sort_key);
    ctx.insert("dir", dir_key);
    ctx.insert("has_more", &has_more);
    ctx.insert("batch_size", &BOOKSHELF_BATCH);

    render(&state.tera, "web/bookshelf.html", &ctx)
}

// ── Bookshelf cards API (for infinite scroll) ───────────────────────

#[derive(Deserialize)]
pub struct BookshelfCardsParams {
    #[serde(default)]
    pub offset: i32,
    #[serde(default)]
    pub sort: String,
    #[serde(default)]
    pub dir: String,
}

pub async fn bookshelf_cards(
    State(state): State<AppState>,
    jar: CookieJar,
    Query(params): Query<BookshelfCardsParams>,
) -> Result<axum::Json<serde_json::Value>, StatusCode> {
    let locale = jar
        .get("lang")
        .map(|c| c.value().to_string())
        .unwrap_or_else(|| state.config.web.language.clone());
    let user_id = match session_user_id(&state, &jar) {
        Some(uid) => uid,
        None => return Err(StatusCode::UNAUTHORIZED),
    };

    let sort_key = if params.sort.is_empty() {
        "date"
    } else {
        &params.sort
    };
    let dir_key = if params.dir.is_empty() {
        "desc"
    } else {
        &params.dir
    };
    let (sort_col, ascending) = parse_bookshelf_sort(sort_key, dir_key);

    let total = bookshelf::count_by_user(&state.db, user_id)
        .await
        .unwrap_or(0);

    let book_views = fetch_bookshelf_views(
        &state,
        user_id,
        &sort_col,
        ascending,
        BOOKSHELF_BATCH,
        params.offset,
        &locale,
    )
    .await;

    let loaded = params.offset as i64 + book_views.len() as i64;
    let has_more = loaded < total;

    // Render card fragments
    let mut ctx = build_context(&state, &jar, "bookshelf").await;
    ctx.insert("books", &book_views);
    ctx.insert("current_path", "/web/bookshelf");

    let html = state
        .tera
        .render("web/_bookshelf_cards.html", &ctx)
        .unwrap_or_default();

    Ok(axum::Json(serde_json::json!({
        "html": html,
        "has_more": has_more
    })))
}

// ── Bookshelf clear handler ─────────────────────────────────────────

#[derive(Deserialize)]
pub struct BookshelfClearForm {
    pub csrf_token: String,
}

pub async fn bookshelf_clear(
    State(state): State<AppState>,
    jar: CookieJar,
    axum::Form(form): axum::Form<BookshelfClearForm>,
) -> Response {
    use crate::web::context::validate_csrf;

    let secret = state.config.server.session_secret.as_bytes();
    if !validate_csrf(&jar, secret, &form.csrf_token) {
        return (StatusCode::FORBIDDEN, "Invalid CSRF token").into_response();
    }

    let user_id = match session_user_id(&state, &jar) {
        Some(uid) => uid,
        None => return Redirect::to("/web/login").into_response(),
    };

    let _ = crate::db::queries::bookshelf::clear_all(&state.db, user_id).await;
    Redirect::to("/web/bookshelf").into_response()
}
