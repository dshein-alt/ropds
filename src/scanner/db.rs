use super::*;
use crate::db::DbBackend;

/// Ensure a catalog row exists for the given path, creating it if needed.
pub async fn ensure_catalog(
    pool: &DbPool,
    path: &str,
    cat_type: CatType,
) -> Result<i64, ScanError> {
    if let Some(cat) = catalogs::find_by_path(pool, path).await? {
        return Ok(cat.id);
    }

    // Determine parent catalog
    let parent_path = Path::new(path).parent();
    let parent_id = match parent_path {
        Some(p) if !p.as_os_str().is_empty() => {
            let pp = p.to_string_lossy().to_string();
            // Recursively ensure parent exists
            Some(Box::pin(ensure_catalog(pool, &pp, cat_type)).await?)
        }
        _ => None,
    };

    let cat_name = Path::new(path)
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    let id = catalogs::insert(pool, parent_id, path, &cat_name, cat_type, 0, "").await?;
    Ok(id)
}

/// Ensure a catalog for an archive exists and update its archive metadata.
pub(super) async fn ensure_archive_catalog(
    pool: &DbPool,
    path: &str,
    cat_type: CatType,
    cat_size: i64,
    cat_mtime: &str,
) -> Result<i64, ScanError> {
    if let Some(cat) = catalogs::find_by_path(pool, path).await? {
        if cat.cat_type != cat_type as i32 || cat.cat_size != cat_size || cat.cat_mtime != cat_mtime
        {
            catalogs::update_archive_meta(pool, cat.id, cat_type, cat_size, cat_mtime).await?;
        }
        return Ok(cat.id);
    }

    // Determine parent catalog
    let parent_path = Path::new(path).parent();
    let parent_id = match parent_path {
        Some(p) if !p.as_os_str().is_empty() => {
            let pp = p.to_string_lossy().to_string();
            Some(Box::pin(ensure_catalog(pool, &pp, CatType::Normal)).await?)
        }
        _ => None,
    };

    let cat_name = Path::new(path)
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    let id = catalogs::insert(
        pool, parent_id, path, &cat_name, cat_type, cat_size, cat_mtime,
    )
    .await?;
    Ok(id)
}

/// Find or create an author by name.
pub async fn ensure_author(pool: &DbPool, full_name: &str) -> Result<i64, ScanError> {
    if let Some(a) = authors::find_by_name(pool, full_name).await? {
        return Ok(a.id);
    }
    let search = full_name.to_uppercase();
    let lang_code = detect_lang_code(full_name);
    let id = authors::insert(pool, full_name, &search, lang_code).await?;
    Ok(id)
}

/// Find or create a series by name.
pub async fn ensure_series(pool: &DbPool, ser_name: &str) -> Result<i64, ScanError> {
    if let Some(s) = series::find_by_name(pool, ser_name).await? {
        return Ok(s.id);
    }
    let search = ser_name.to_uppercase();
    let lang_code = detect_lang_code(ser_name);
    let id = series::insert(pool, ser_name, &search, lang_code).await?;
    Ok(id)
}

pub(super) async fn cached_ensure_catalog(
    ctx: &ScanContext,
    path: &str,
    cat_type: CatType,
) -> Result<i64, ScanError> {
    if let Some(id) = ctx.catalog_cache.get(path) {
        return Ok(*id);
    }
    let id = ensure_catalog(&ctx.pool, path, cat_type).await?;
    ctx.catalog_cache.insert(path.to_string(), id);
    Ok(id)
}

async fn cached_ensure_author(ctx: &ScanContext, full_name: &str) -> Result<i64, ScanError> {
    if let Some(id) = ctx.author_cache.get(full_name) {
        return Ok(*id);
    }
    let id = ensure_author(&ctx.pool, full_name).await?;
    ctx.author_cache.insert(full_name.to_string(), id);
    Ok(id)
}

async fn cached_ensure_series(ctx: &ScanContext, ser_name: &str) -> Result<i64, ScanError> {
    if let Some(id) = ctx.series_cache.get(ser_name) {
        return Ok(*id);
    }
    let id = ensure_series(&ctx.pool, ser_name).await?;
    ctx.series_cache.insert(ser_name.to_string(), id);
    Ok(id)
}

async fn cached_genre_id(ctx: &ScanContext, code: &str) -> Result<Option<i64>, ScanError> {
    if let Some(cached) = ctx.genre_cache.get(code) {
        return Ok(*cached);
    }
    if let Some(genre) = genres::get_by_code(&ctx.pool, code).await? {
        ctx.genre_cache.insert(code.to_string(), Some(genre.id));
        return Ok(Some(genre.id));
    }
    ctx.genre_cache.insert(code.to_string(), None);
    Ok(None)
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn build_pending_book_insert(
    ctx: &ScanContext,
    filename: &str,
    path: &str,
    format: &str,
    size: i64,
    cat_type: CatType,
    meta: &BookMeta,
) -> Result<PendingBookInsert, ScanError> {
    let title = if meta.title.is_empty() {
        Path::new(filename)
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string()
    } else {
        meta.title.clone()
    };
    let search_title = title.to_uppercase();
    let lang_code = detect_lang_code(&title);
    let annotation: String = meta
        .annotation
        .chars()
        .filter(|c| (*c as u32) < 0x10000)
        .collect();

    let catalog_id = cached_ensure_catalog(ctx, path, cat_type).await?;

    let mut author_ids = Vec::new();
    if meta.authors.is_empty() {
        author_ids.push(cached_ensure_author(ctx, "Unknown").await?);
    } else {
        for author_name in &meta.authors {
            let name = normalise_author_name(author_name);
            if name.is_empty() {
                continue;
            }
            author_ids.push(cached_ensure_author(ctx, &name).await?);
        }
        if author_ids.is_empty() {
            author_ids.push(cached_ensure_author(ctx, "Unknown").await?);
        }
    }
    author_ids.sort_unstable();
    author_ids.dedup();
    let author_key = author_ids
        .iter()
        .map(|id| id.to_string())
        .collect::<Vec<_>>()
        .join(",");

    let mut genre_ids = Vec::new();
    for genre_code in &meta.genres {
        if let Some(genre_id) = cached_genre_id(ctx, genre_code).await? {
            genre_ids.push(genre_id);
        }
    }
    genre_ids.sort_unstable();
    genre_ids.dedup();

    let series_link = if let Some(ref ser_title) = meta.series_title {
        if ser_title.is_empty() {
            None
        } else {
            Some((
                cached_ensure_series(ctx, ser_title).await?,
                meta.series_index,
            ))
        }
    } else {
        None
    };

    Ok(PendingBookInsert {
        catalog_id,
        filename: filename.to_string(),
        path: path.to_string(),
        format: format.to_string(),
        size,
        cat_type,
        title,
        search_title,
        annotation,
        docdate: meta.docdate.clone(),
        lang: meta.lang.clone(),
        lang_code,
        cover_type: meta.cover_type.clone(),
        cover_data: meta.cover_data.clone(),
        author_ids,
        genre_ids,
        series_link,
        author_key,
    })
}

pub(super) async fn enqueue_pending_book(
    ctx: &ScanContext,
    pending: PendingBookInsert,
) -> Result<(), ScanError> {
    ctx.pending_book_tx
        .send(PendingBookMsg::Insert(Box::new(pending)))
        .await
        .map_err(|e| ScanError::Internal(format!("pending-book queue closed: {e}")))
}

pub(super) async fn run_pending_book_writer(
    ctx: Arc<ScanContext>,
    mut pending_rx: mpsc::Receiver<PendingBookMsg>,
) -> Result<(), ScanError> {
    const BOOK_INSERT_BATCH_SIZE: usize = 512;

    let mut batch = Vec::with_capacity(BOOK_INSERT_BATCH_SIZE);
    while let Some(msg) = pending_rx.recv().await {
        match msg {
            PendingBookMsg::Insert(book) => {
                batch.push(*book);
                if batch.len() >= BOOK_INSERT_BATCH_SIZE {
                    commit_pending_book_batch(&ctx, std::mem::take(&mut batch)).await?;
                }
            }
            PendingBookMsg::Finish => break,
        }
    }
    if !batch.is_empty() {
        commit_pending_book_batch(&ctx, batch).await?;
    }
    Ok(())
}

async fn commit_pending_book_batch(
    ctx: &ScanContext,
    pending_books: Vec<PendingBookInsert>,
) -> Result<(), ScanError> {
    if pending_books.is_empty() {
        return Ok(());
    }
    let inserted_count = pending_books.len();

    let mut tx = ctx.pool.inner().begin().await?;
    let mut covers_to_save = Vec::new();

    let books_insert_sql = ctx.pool.sql(
        "INSERT INTO books (catalog_id, filename, path, format, title, search_title, \
         annotation, docdate, lang, lang_code, size, avail, cat_type, cover, cover_type, author_key) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    );
    let select_inserted_sql = ctx
        .pool
        .sql("SELECT id FROM books WHERE path = ? AND filename = ? ORDER BY id DESC LIMIT 1");
    let link_author_sql = match ctx.pool.backend() {
        DbBackend::Mysql => ctx
            .pool
            .sql("INSERT IGNORE INTO book_authors (book_id, author_id) VALUES (?, ?)"),
        _ => ctx.pool.sql(
            "INSERT INTO book_authors (book_id, author_id) VALUES (?, ?) \
             ON CONFLICT (book_id, author_id) DO NOTHING",
        ),
    };
    let link_genre_sql = match ctx.pool.backend() {
        DbBackend::Mysql => ctx
            .pool
            .sql("INSERT IGNORE INTO book_genres (book_id, genre_id) VALUES (?, ?)"),
        _ => ctx.pool.sql(
            "INSERT INTO book_genres (book_id, genre_id) VALUES (?, ?) \
             ON CONFLICT (book_id, genre_id) DO NOTHING",
        ),
    };
    let link_series_sql = match ctx.pool.backend() {
        DbBackend::Mysql => ctx
            .pool
            .sql("INSERT IGNORE INTO book_series (book_id, series_id, ser_no) VALUES (?, ?, ?)"),
        _ => ctx.pool.sql(
            "INSERT INTO book_series (book_id, series_id, ser_no) VALUES (?, ?, ?) \
             ON CONFLICT (book_id, series_id) DO NOTHING",
        ),
    };

    for pending in pending_books {
        let has_cover = if pending.cover_data.is_some() { 1 } else { 0 };
        let result = sqlx::query(&books_insert_sql)
            .bind(pending.catalog_id)
            .bind(&pending.filename)
            .bind(&pending.path)
            .bind(&pending.format)
            .bind(&pending.title)
            .bind(&pending.search_title)
            .bind(&pending.annotation)
            .bind(&pending.docdate)
            .bind(&pending.lang)
            .bind(pending.lang_code)
            .bind(pending.size)
            .bind(AvailStatus::Confirmed as i32)
            .bind(pending.cat_type as i32)
            .bind(has_cover)
            .bind(&pending.cover_type)
            .bind(&pending.author_key)
            .execute(&mut *tx)
            .await?;

        let book_id = if let Some(id) = result.last_insert_id() {
            id
        } else {
            let row: (i64,) = sqlx::query_as(&select_inserted_sql)
                .bind(&pending.path)
                .bind(&pending.filename)
                .fetch_one(&mut *tx)
                .await?;
            row.0
        };

        for author_id in pending.author_ids {
            sqlx::query(&link_author_sql)
                .bind(book_id)
                .bind(author_id)
                .execute(&mut *tx)
                .await?;
        }
        for genre_id in pending.genre_ids {
            sqlx::query(&link_genre_sql)
                .bind(book_id)
                .bind(genre_id)
                .execute(&mut *tx)
                .await?;
        }
        if let Some((series_id, ser_no)) = pending.series_link {
            sqlx::query(&link_series_sql)
                .bind(book_id)
                .bind(series_id)
                .bind(ser_no)
                .execute(&mut *tx)
                .await?;
        }

        if let Some(cover_data) = pending.cover_data {
            covers_to_save.push((book_id, cover_data, pending.cover_type));
        }
    }

    tx.commit().await?;

    for (book_id, cover_data, cover_type) in covers_to_save {
        if let Err(e) = save_cover(
            &ctx.covers_path,
            book_id,
            &cover_data,
            &cover_type,
            ctx.cover_image_cfg,
        ) {
            warn!("Failed to save cover for book {book_id}: {e}");
        }
    }

    ctx.stats
        .books_added
        .fetch_add(inserted_count as u64, Ordering::Relaxed);
    Ok(())
}
