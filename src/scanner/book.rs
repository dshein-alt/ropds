use super::*;
use std::io::{BufReader, Cursor};

/// Process a single book file on disk.
pub(super) async fn process_file(
    ctx: &ScanContext,
    path: &Path,
    rel_path: &str,
    filename: &str,
    extension: &str,
    size: i64,
) -> Result<(), ScanError> {
    if let Some(existing_id) = ctx.existing_book_id(rel_path, filename) {
        ctx.mark_existing_book_confirmed(existing_id);
        ctx.stats.books_skipped.fetch_add(1, Ordering::Relaxed);
        return Ok(());
    }

    if books::find_by_path_and_filename(&ctx.pool, rel_path, filename)
        .await?
        .is_some()
    {
        // This fallback path means another worker inserted this row in the
        // current scan run. Pending inserts are written with avail=Confirmed,
        // so no additional confirmation tracking is required here.
        ctx.stats.books_skipped.fetch_add(1, Ordering::Relaxed);
        return Ok(());
    }

    // Skip books suppressed by admin
    if crate::db::queries::suppressed::is_suppressed(&ctx.pool, rel_path, filename).await? {
        ctx.stats.books_skipped.fetch_add(1, Ordering::Relaxed);
        return Ok(());
    }

    if !ctx.try_mark_pending_new_book(rel_path, filename) {
        ctx.stats.books_skipped.fetch_add(1, Ordering::Relaxed);
        return Ok(());
    }

    // Parse metadata
    let meta = {
        let _permit = acquire_scan_permit(ctx).await?;
        tokio::task::spawn_blocking({
            let path = path.to_path_buf();
            let ext = extension.to_string();
            let cover_cfg = ctx.cover_image_cfg;
            move || parse_book_file(&path, &ext, cover_cfg)
        })
        .await
        .map_err(|e| ScanError::Internal(e.to_string()))??
    };

    let pending = build_pending_book_insert(
        ctx,
        filename,
        rel_path,
        extension,
        size,
        CatType::Normal,
        &meta,
    )
    .await?;
    enqueue_pending_book(ctx, pending).await?;
    Ok(())
}

/// Parse a book file from disk by extension.
pub fn parse_book_file(
    path: &Path,
    ext: &str,
    cover_cfg: CoverImageConfig,
) -> Result<BookMeta, ScanError> {
    let file = fs::File::open(path)?;
    let reader = BufReader::new(file);
    match ext {
        "fb2" => parsers::fb2::parse(reader).map_err(|e| ScanError::Parse(e.to_string())),
        "epub" => {
            // EPUB needs Read + Seek, reopen as file
            let file = fs::File::open(path)?;
            parsers::epub::parse(file).map_err(|e| ScanError::Parse(e.to_string()))
        }
        "mobi" => parsers::mobi::parse(reader).map_err(|e| ScanError::Parse(e.to_string())),
        "pdf" => {
            let fallback_title = path
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            let mut meta = BookMeta {
                title: fallback_title.clone(),
                ..Default::default()
            };

            match crate::pdf::extract_metadata_from_path(path) {
                Ok(pdf_meta) => {
                    if let Some(title) = pdf_meta.title {
                        meta.title = title;
                    }
                    if let Some(author) = pdf_meta.author {
                        meta.authors = vec![author];
                    }
                }
                Err(e) => {
                    warn!(
                        "Failed to extract PDF metadata for {}: {}",
                        path.display(),
                        e
                    );
                }
            }

            if meta.title.trim().is_empty() {
                meta.title = fallback_title;
            }

            match crate::pdf::render_first_page_jpeg_from_path(path, cover_cfg) {
                Ok(cover) => {
                    meta.cover_data = Some(cover);
                    meta.cover_type = "image/jpeg".to_string();
                }
                Err(e) => {
                    warn!("Failed to render PDF cover for {}: {}", path.display(), e);
                }
            }

            Ok(meta)
        }
        "djvu" => {
            let fallback_title = path
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            let mut meta = BookMeta {
                title: fallback_title,
                ..Default::default()
            };

            match crate::djvu::render_first_page_jpeg_from_path(path, cover_cfg) {
                Ok(cover) => {
                    meta.cover_data = Some(cover);
                    meta.cover_type = "image/jpeg".to_string();
                }
                Err(e) => {
                    warn!("Failed to render DJVU cover for {}: {}", path.display(), e);
                }
            }

            Ok(meta)
        }
        _ => {
            // For unsupported formats, return minimal metadata from filename
            Ok(BookMeta {
                title: path
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string(),
                ..Default::default()
            })
        }
    }
}

/// Parse book metadata from in-memory bytes.
pub fn parse_book_bytes(
    data: &[u8],
    ext: &str,
    filename: &str,
    cover_cfg: CoverImageConfig,
) -> Result<BookMeta, ScanError> {
    match ext {
        "fb2" => {
            let reader = BufReader::new(Cursor::new(data));
            parsers::fb2::parse(reader).map_err(|e| ScanError::Parse(e.to_string()))
        }
        "epub" => {
            let cursor = Cursor::new(data);
            parsers::epub::parse(cursor).map_err(|e| ScanError::Parse(e.to_string()))
        }
        "mobi" => parsers::mobi::parse_bytes(data).map_err(|e| ScanError::Parse(e.to_string())),
        "pdf" => {
            let fallback_title = Path::new(filename)
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();

            let mut meta = BookMeta {
                title: fallback_title.clone(),
                ..Default::default()
            };

            match crate::pdf::extract_metadata_from_bytes(data) {
                Ok(pdf_meta) => {
                    if let Some(title) = pdf_meta.title {
                        meta.title = title;
                    }
                    if let Some(author) = pdf_meta.author {
                        meta.authors = vec![author];
                    }
                }
                Err(e) => {
                    warn!("Failed to extract PDF metadata from archive bytes: {}", e);
                }
            }

            if meta.title.trim().is_empty() {
                meta.title = fallback_title;
            }

            match crate::pdf::render_first_page_jpeg_from_bytes(data, cover_cfg) {
                Ok(cover) => {
                    meta.cover_data = Some(cover);
                    meta.cover_type = "image/jpeg".to_string();
                }
                Err(e) => {
                    warn!("Failed to render PDF cover from archive bytes: {}", e);
                }
            }
            Ok(meta)
        }
        "djvu" => {
            let fallback_title = Path::new(filename)
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();

            let mut meta = BookMeta {
                title: fallback_title,
                ..Default::default()
            };

            match crate::djvu::render_first_page_jpeg_from_bytes(data, cover_cfg) {
                Ok(cover) => {
                    meta.cover_data = Some(cover);
                    meta.cover_type = "image/jpeg".to_string();
                }
                Err(e) => {
                    warn!("Failed to render DJVU cover from archive bytes: {}", e);
                }
            }

            Ok(meta)
        }
        _ => Ok(BookMeta {
            title: Path::new(filename)
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string(),
            ..Default::default()
        }),
    }
}

/// Insert a book record and link authors, genres, series.
/// Saves cover image to `covers_path` if present.
/// (Public API — used by upload handler.)
#[allow(clippy::too_many_arguments)]
pub async fn insert_book_with_meta(
    pool: &DbPool,
    catalog_id: i64,
    filename: &str,
    path: &str,
    format: &str,
    size: i64,
    cat_type: CatType,
    meta: &BookMeta,
    covers_path: &Path,
    cover_cfg: CoverImageConfig,
) -> Result<i64, ScanError> {
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
    let lang = &meta.lang;
    let lang_code = detect_lang_code(&title);
    let has_cover = if meta.cover_data.is_some() { 1 } else { 0 };

    // Strip high Unicode from annotation (MySQL 3-byte UTF8 compat)
    let annotation: String = meta
        .annotation
        .chars()
        .filter(|c| (*c as u32) < 0x10000)
        .collect();

    let book_id = books::insert(
        pool,
        catalog_id,
        filename,
        path,
        format,
        &title,
        &search_title,
        &annotation,
        &meta.docdate,
        lang,
        lang_code,
        size,
        cat_type,
        has_cover,
        &meta.cover_type,
    )
    .await?;

    // Save cover to disk
    if let Some(ref cover_data) = meta.cover_data
        && let Err(e) = save_cover(
            covers_path,
            book_id,
            cover_data,
            &meta.cover_type,
            cover_cfg,
        )
    {
        warn!("Failed to save cover for book {book_id}: {e}");
    }

    // Link authors
    if meta.authors.is_empty() {
        let author_id = ensure_author(pool, "Unknown").await?;
        authors::link_book(pool, book_id, author_id).await?;
    } else {
        for author_name in &meta.authors {
            let name = normalise_author_name(author_name);
            if name.is_empty() {
                continue;
            }
            let author_id = ensure_author(pool, &name).await?;
            authors::link_book(pool, book_id, author_id).await?;
        }
    }
    books::update_author_key(pool, book_id).await?;

    // Link genres
    for genre_code in &meta.genres {
        genres::link_book_by_code(pool, book_id, genre_code).await?;
    }

    // Link series
    if let Some(ref ser_title) = meta.series_title
        && !ser_title.is_empty()
    {
        let series_id = ensure_series(pool, ser_title).await?;
        series::link_book(pool, book_id, series_id, meta.series_index).await?;
    }

    Ok(book_id)
}
