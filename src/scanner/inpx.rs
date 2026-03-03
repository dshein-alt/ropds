use super::*;
use std::io::BufReader;
use tokio::sync::Mutex as TokioMutex;

/// Process an INPX index file.
pub(super) async fn process_inpx(
    ctx: Arc<ScanContext>,
    inpx_path: &Path,
    rel_path: &str,
    mtime: &str,
) -> Result<(), ScanError> {
    let inpx_size = fs::metadata(inpx_path)?.len() as i64;
    let inpx_dir = Path::new(rel_path)
        .parent()
        .unwrap_or(Path::new(""))
        .to_string_lossy()
        .to_string();

    if try_skip_inpx_archive(
        &ctx.pool,
        rel_path,
        &inpx_dir,
        inpx_size,
        ctx.skip_unchanged,
        mtime,
    )
    .await?
    {
        ctx.stats.archives_skipped.fetch_add(1, Ordering::Relaxed);
        return Ok(());
    }

    ensure_archive_catalog(&ctx.pool, rel_path, CatType::Inpx, inpx_size, mtime).await?;

    // Keep a moderate buffer so parser throughput is less sensitive to
    // temporary DB-side stalls in worker tasks.
    let (batch_tx, batch_rx) =
        mpsc::channel::<(String, Vec<parsers::inpx::InpxRecord>)>(ctx.workers_num.max(1) * 8);

    let inpx_path_buf = inpx_path.to_path_buf();
    let inpx_dir_for_parse = inpx_dir.clone();
    let parser_task = tokio::task::spawn_blocking(move || -> Result<u64, ScanError> {
        let file = fs::File::open(&inpx_path_buf)?;
        let reader = BufReader::new(file);
        parsers::inpx::parse_grouped_streaming(reader, move |folder, records| {
            let book_path = if inpx_dir_for_parse.is_empty() {
                folder
            } else {
                format!("{inpx_dir_for_parse}/{folder}")
            };

            batch_tx.blocking_send((book_path, records)).map_err(|_| {
                parsers::inpx::InpxError::Io(std::io::Error::new(
                    std::io::ErrorKind::BrokenPipe,
                    "INPX batch receiver dropped",
                ))
            })
        })
        .map_err(|e| ScanError::Internal(e.to_string()))
    });

    let worker_count = ctx.workers_num.max(1);
    let batch_rx = Arc::new(TokioMutex::new(batch_rx));
    let mut workers = tokio::task::JoinSet::new();
    for _ in 0..worker_count {
        let ctx = Arc::clone(&ctx);
        let batch_rx = Arc::clone(&batch_rx);
        workers.spawn(async move {
            loop {
                let next = {
                    let mut rx = batch_rx.lock().await;
                    rx.recv().await
                };
                let Some((book_path, zip_records)) = next else {
                    break;
                };

                if let Err(e) = process_inpx_zip_group(&ctx, &book_path, zip_records).await {
                    warn!("INPX group processing failed for '{}': {}", book_path, e);
                    ctx.stats.errors.fetch_add(1, Ordering::Relaxed);
                }
            }
        });
    }

    let parsed_records = parser_task
        .await
        .map_err(|e| ScanError::Internal(e.to_string()))??;

    while let Some(join_result) = workers.join_next().await {
        if let Err(e) = join_result {
            warn!("INPX worker join failure: {e}");
            ctx.stats.errors.fetch_add(1, Ordering::Relaxed);
        }
    }

    info!("INPX: parsed {} records from {}", parsed_records, rel_path);
    ctx.stats.archives_scanned.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

async fn process_inpx_zip_group(
    ctx: &ScanContext,
    book_path: &str,
    zip_records: Vec<parsers::inpx::InpxRecord>,
) -> Result<(), ScanError> {
    let mut pending = Vec::new();
    for record in zip_records {
        if let Some(existing_id) = ctx.existing_book_id(book_path, &record.filename) {
            ctx.mark_existing_book_confirmed(existing_id);
            ctx.stats.books_skipped.fetch_add(1, Ordering::Relaxed);
            continue;
        }

        if books::find_by_path_and_filename(&ctx.pool, book_path, &record.filename)
            .await?
            .is_some()
        {
            // This fallback path means another worker inserted this row in the
            // current scan run. Pending inserts are written with avail=Confirmed,
            // so no additional confirmation tracking is required here.
            ctx.stats.books_skipped.fetch_add(1, Ordering::Relaxed);
            continue;
        }

        // Skip books suppressed by admin
        if crate::db::queries::suppressed::is_suppressed(&ctx.pool, book_path, &record.filename)
            .await?
        {
            ctx.stats.books_skipped.fetch_add(1, Ordering::Relaxed);
            continue;
        }

        if !ctx.try_mark_pending_new_book(book_path, &record.filename) {
            ctx.stats.books_skipped.fetch_add(1, Ordering::Relaxed);
            continue;
        }

        pending.push(record);
    }

    if pending.is_empty() {
        return Ok(());
    }

    // Best-effort metadata enrichment from referenced ZIP entries.
    let needed_filenames: HashSet<String> = pending.iter().map(|r| r.filename.clone()).collect();
    let zip_abs_path = ctx.root.join(book_path);
    let mut parsed_meta = if !zip_abs_path.exists() {
        warn!(
            "INPX referenced ZIP archive is missing: {}",
            zip_abs_path.display()
        );
        HashMap::new()
    } else {
        let zip_abs_path_for_parse = zip_abs_path.clone();
        let exts = ctx.extensions.clone();
        let test_files = ctx.test_files;
        let cover_cfg = ctx.cover_image_cfg;

        let parsed_meta = {
            let _permit = acquire_scan_permit(ctx).await?;
            tokio::task::spawn_blocking(move || {
                super::zip::read_selected_zip_entries_meta(
                    &zip_abs_path_for_parse,
                    &exts,
                    &needed_filenames,
                    test_files,
                    cover_cfg,
                )
            })
            .await
            .map_err(|e| ScanError::Internal(e.to_string()))?
        };

        match parsed_meta {
            Ok(meta) => meta,
            Err(e) => {
                warn!(
                    "INPX metadata enrichment failed for {}: {}",
                    zip_abs_path.display(),
                    e
                );
                HashMap::new()
            }
        }
    };

    for record in pending {
        let mut meta = record.meta;
        if let Some(parsed) = parsed_meta.remove(&record.filename) {
            if !parsed.annotation.trim().is_empty() {
                meta.annotation = parsed.annotation;
            }
            if let Some(cover_data) = parsed.cover_data {
                meta.cover_data = Some(cover_data);
                meta.cover_type = parsed.cover_type;
            }
        }

        let pending = match build_pending_book_insert(
            ctx,
            &record.filename,
            book_path,
            &record.format,
            record.size,
            CatType::Inpx,
            &meta,
        )
        .await
        {
            Ok(pending) => pending,
            Err(e) => {
                warn!(
                    "Failed to prepare INPX book '{}::{}': {}",
                    book_path, record.filename, e
                );
                ctx.stats.errors.fetch_add(1, Ordering::Relaxed);
                continue;
            }
        };

        if let Err(e) = enqueue_pending_book(ctx, pending).await {
            warn!(
                "Failed to queue INPX book '{}::{}': {}",
                book_path, record.filename, e
            );
            ctx.stats.errors.fetch_add(1, Ordering::Relaxed);
        }
    }

    Ok(())
}

/// Try to skip scanning an unchanged INPX archive.
async fn try_skip_inpx_archive(
    pool: &DbPool,
    rel_inpx: &str,
    inpx_dir: &str,
    inpx_size: i64,
    skip_unchanged: bool,
    mtime: &str,
) -> Result<bool, ScanError> {
    let Some(cat) = catalogs::find_by_path(pool, rel_inpx).await? else {
        return Ok(false);
    };
    if CatType::try_from(cat.cat_type).ok() != Some(CatType::Inpx) || cat.cat_size != inpx_size {
        return Ok(false);
    }
    if skip_unchanged && !mtime.is_empty() && !cat.cat_mtime.is_empty() && cat.cat_mtime != mtime {
        return Ok(false);
    }
    let updated = books::set_avail_for_inpx_dir(pool, inpx_dir, AvailStatus::Confirmed).await?;
    // Require at least one touched row to skip full reprocessing. If nothing
    // was confirmed, fall back to parsing so missing DB rows can be restored.
    Ok(updated > 0)
}
