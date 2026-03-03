use super::*;
use std::io::BufReader;

pub(super) struct ZipBookEntry {
    pub(super) filename: String,
    pub(super) extension: String,
    pub(super) size: i64,
    pub(super) data: Vec<u8>,
}

/// Process a ZIP archive containing book files.
pub(super) async fn process_zip(
    ctx: &ScanContext,
    zip_path: &Path,
    rel_dir: &str,
    mtime: &str,
) -> Result<(), ScanError> {
    let zip_filename = zip_path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    let rel_zip = if rel_dir.is_empty() {
        zip_filename.clone()
    } else {
        format!("{rel_dir}/{zip_filename}")
    };

    let zip_size = fs::metadata(zip_path)?.len() as i64;
    if try_skip_zip_archive(&ctx.pool, &rel_zip, zip_size, ctx.skip_unchanged, mtime).await? {
        ctx.stats.archives_skipped.fetch_add(1, Ordering::Relaxed);
        return Ok(());
    }

    // Validate ZIP integrity if enabled
    if ctx.test_zip {
        let zip_path_buf = zip_path.to_path_buf();
        let valid = {
            let _permit = acquire_scan_permit(ctx).await?;
            tokio::task::spawn_blocking(move || validate_zip_integrity(&zip_path_buf))
                .await
                .map_err(|e| ScanError::Internal(e.to_string()))??
        };
        if !valid {
            warn!("ZIP integrity check failed: {}", zip_path.display());
            ctx.stats.errors.fetch_add(1, Ordering::Relaxed);
            return Ok(());
        }
    }

    ensure_archive_catalog(&ctx.pool, &rel_zip, CatType::Zip, zip_size, mtime).await?;

    // Read ZIP contents in a blocking task
    let zip_path_buf = zip_path.to_path_buf();
    let extensions_clone = ctx.extensions.clone();
    let test_files = ctx.test_files;

    let zip_entries = {
        let _permit = acquire_scan_permit(ctx).await?;
        tokio::task::spawn_blocking(move || {
            read_zip_entries(&zip_path_buf, &extensions_clone, test_files)
        })
        .await
        .map_err(|e| ScanError::Internal(e.to_string()))??
    };

    for ze in zip_entries {
        if let Some(existing_id) = ctx.existing_book_id(&rel_zip, &ze.filename) {
            ctx.mark_existing_book_confirmed(existing_id);
            ctx.stats.books_skipped.fetch_add(1, Ordering::Relaxed);
            continue;
        }

        if books::find_by_path_and_filename(&ctx.pool, &rel_zip, &ze.filename)
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
        if crate::db::queries::suppressed::is_suppressed(&ctx.pool, &rel_zip, &ze.filename).await? {
            ctx.stats.books_skipped.fetch_add(1, Ordering::Relaxed);
            continue;
        }

        if !ctx.try_mark_pending_new_book(&rel_zip, &ze.filename) {
            ctx.stats.books_skipped.fetch_add(1, Ordering::Relaxed);
            continue;
        }

        // Parse metadata from in-memory data
        let meta = {
            let data = ze.data.clone();
            let ext = ze.extension.clone();
            let filename = ze.filename.clone();
            let cover_cfg = ctx.cover_image_cfg;
            // Keep per-entry parse under the shared budget so ZIP parsing and
            // INPX enrichment parsing draw from the same global limit.
            let _permit = acquire_scan_permit(ctx).await?;
            tokio::task::spawn_blocking(move || parse_book_bytes(&data, &ext, &filename, cover_cfg))
                .await
                .map_err(|e| ScanError::Internal(e.to_string()))?
        };

        let meta = match meta {
            Ok(m) => m,
            Err(e) => {
                debug!("Failed to parse {} in {}: {e}", ze.filename, zip_filename);
                ctx.stats.errors.fetch_add(1, Ordering::Relaxed);
                continue;
            }
        };

        let pending = build_pending_book_insert(
            ctx,
            &ze.filename,
            &rel_zip,
            &ze.extension,
            ze.size,
            CatType::Zip,
            &meta,
        )
        .await?;
        enqueue_pending_book(ctx, pending).await?;
    }

    ctx.stats.archives_scanned.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

/// Iterate ZIP entries, read matching files into memory, validate size if
/// requested, and hand data to a callback.
fn for_each_matching_zip_entry<S, H>(
    path: &Path,
    extensions: &HashSet<String>,
    test_files: bool,
    mut should_take: S,
    mut handle: H,
) -> Result<(), ScanError>
where
    S: FnMut(&str, &str, u64) -> bool,
    H: FnMut(String, String, u64, Vec<u8>),
{
    let file = fs::File::open(path)?;
    let reader = BufReader::new(file);
    let mut archive = ::zip::ZipArchive::new(reader)?;

    for i in 0..archive.len() {
        let mut entry = match archive.by_index(i) {
            Ok(entry) => entry,
            Err(e) => {
                warn!("Failed to read ZIP entry #{i} in {}: {}", path.display(), e);
                continue;
            }
        };
        if !entry.is_file() {
            continue;
        }

        let entry_name = entry.name().to_string();
        let filename = Path::new(&entry_name)
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let ext = Path::new(&filename)
            .extension()
            .unwrap_or_default()
            .to_string_lossy()
            .to_lowercase();
        if !extensions.contains(&ext) {
            continue;
        }

        let declared_size = entry.size();
        if !should_take(&entry_name, &filename, declared_size) {
            continue;
        }

        let mut data = Vec::new();
        std::io::Read::read_to_end(&mut entry, &mut data)?;
        if test_files && declared_size > 0 && declared_size != data.len() as u64 {
            warn!(
                "ZIP entry size mismatch in {}: {} declared={}, read={}",
                path.display(),
                entry_name,
                declared_size,
                data.len()
            );
            continue;
        }

        handle(filename, ext, declared_size, data);
    }

    Ok(())
}

/// Read all matching book files from a ZIP archive.
/// When `test_files` is enabled, entries whose extracted size does not match
/// the declared size are skipped.
pub(super) fn read_zip_entries(
    path: &Path,
    extensions: &HashSet<String>,
    test_files: bool,
) -> Result<Vec<ZipBookEntry>, ScanError> {
    let mut entries = Vec::new();

    for_each_matching_zip_entry(
        path,
        extensions,
        test_files,
        |_, _, _| true,
        |filename, ext, declared_size, data| {
            entries.push(ZipBookEntry {
                filename,
                extension: ext,
                size: declared_size as i64,
                data,
            });
        },
    )?;

    Ok(entries)
}

/// Read selected book files from a ZIP archive and parse metadata for each
/// matched entry, keyed by basename filename.
pub(super) fn read_selected_zip_entries_meta(
    path: &Path,
    extensions: &HashSet<String>,
    needed_filenames: &HashSet<String>,
    test_files: bool,
    cover_cfg: CoverImageConfig,
) -> Result<HashMap<String, BookMeta>, ScanError> {
    let mut out = HashMap::new();

    for_each_matching_zip_entry(
        path,
        extensions,
        test_files,
        |_, filename, _| needed_filenames.contains(filename),
        |filename, ext, _, data| {
            if out.contains_key(&filename) {
                warn!(
                    "Duplicate basename '{}' in ZIP {}; keeping first matched entry",
                    filename,
                    path.display()
                );
                return;
            }

            if let Ok(meta) = parse_book_bytes(&data, &ext, &filename, cover_cfg) {
                out.insert(filename, meta);
            }
        },
    )?;

    Ok(out)
}

/// Validate ZIP archive integrity by reading every entry (triggers CRC check).
/// Returns `false` if any entry is corrupt.
pub(super) fn validate_zip_integrity(path: &Path) -> Result<bool, ScanError> {
    let file = fs::File::open(path)?;
    let reader = BufReader::new(file);
    let mut archive = ::zip::ZipArchive::new(reader)?;
    for i in 0..archive.len() {
        let mut entry = match archive.by_index(i) {
            Ok(e) => e,
            Err(_) => return Ok(false),
        };
        let mut buf = Vec::new();
        if std::io::Read::read_to_end(&mut entry, &mut buf).is_err() {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Try to skip scanning an unchanged ZIP archive.
/// With `skip_unchanged` enabled, also checks mtime (backward-compatible with
/// empty mtime in old DB records).
pub(super) async fn try_skip_zip_archive(
    pool: &DbPool,
    rel_zip: &str,
    zip_size: i64,
    skip_unchanged: bool,
    mtime: &str,
) -> Result<bool, ScanError> {
    let Some(cat) = catalogs::find_by_path(pool, rel_zip).await? else {
        return Ok(false);
    };
    if CatType::try_from(cat.cat_type).ok() != Some(CatType::Zip) || cat.cat_size != zip_size {
        return Ok(false);
    }
    // When skip_unchanged is enabled, also compare mtime (skip this check if
    // either side is empty for backward compatibility with pre-mtime records).
    if skip_unchanged && !mtime.is_empty() && !cat.cat_mtime.is_empty() && cat.cat_mtime != mtime {
        return Ok(false);
    }
    let updated = books::set_avail_by_path(pool, rel_zip, AvailStatus::Confirmed).await?;
    Ok(updated > 0)
}
