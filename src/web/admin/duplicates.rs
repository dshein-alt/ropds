use super::*;

use crate::db::queries::{authors, books};
use crate::web::pagination::Pagination;

#[derive(Deserialize)]
pub struct DuplicatesParams {
    #[serde(default)]
    pub page: i32,
}

const ITEMS_PER_PAGE: i32 = 20;

/// GET /web/admin/duplicates — show duplicate book groups.
pub async fn duplicates_page(
    State(state): State<AppState>,
    jar: CookieJar,
    Query(params): Query<DuplicatesParams>,
) -> Result<Html<String>, StatusCode> {
    let mut ctx = build_context(&state, &jar, "admin").await;
    let page = params.page.max(0);
    let offset = page * ITEMS_PER_PAGE;

    let groups = books::get_duplicate_groups(&state.db, ITEMS_PER_PAGE, offset)
        .await
        .unwrap_or_default();

    let total = books::count_duplicate_groups(&state.db).await.unwrap_or(0);

    let mut group_views: Vec<serde_json::Value> = Vec::with_capacity(groups.len());

    for group in &groups {
        let group_books =
            books::get_books_in_group(&state.db, &group.search_title, &group.author_key)
                .await
                .unwrap_or_default();

        // Get author names from the first book in the group
        let author_names: Vec<String> = if let Some(first_book) = group_books.first() {
            authors::get_for_book(&state.db, first_book.id)
                .await
                .unwrap_or_default()
                .into_iter()
                .map(|a| a.full_name)
                .collect()
        } else {
            vec![]
        };

        let book_list: Vec<serde_json::Value> = group_books
            .iter()
            .map(|b| {
                serde_json::json!({
                    "id": b.id,
                    "title": b.title,
                    "format": b.format,
                    "size": b.size,
                    "lang": b.lang,
                    "filename": b.filename,
                    "path": b.path,
                })
            })
            .collect();

        // Use the display title from the first book (preserves original case)
        let display_title = group_books
            .first()
            .map(|b| b.title.clone())
            .unwrap_or_else(|| group.search_title.clone());

        group_views.push(serde_json::json!({
            "search_title": display_title,
            "authors": author_names.join(", "),
            "cnt": group.cnt,
            "books": book_list,
        }));
    }

    let pagination = Pagination::new(page, ITEMS_PER_PAGE, total);

    ctx.insert("groups", &group_views);
    ctx.insert("pagination", &pagination);
    ctx.insert("pagination_qs", "");
    ctx.insert("total_groups", &total);

    match state.tera.render("web/duplicates.html", &ctx) {
        Ok(html) => Ok(Html(html)),
        Err(e) => {
            tracing::error!("Template error: {e}");
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}
