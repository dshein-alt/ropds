pub mod parsers;

use std::collections::HashSet;
use std::fs;
use std::io::{BufReader, Cursor};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use tracing::{debug, info, warn};
use walkdir::WalkDir;

use crate::config::Config;
use crate::db::DbPool;
use crate::db::models;
use crate::db::queries::{authors, books, catalogs, counters, genres, series};

use parsers::{BookMeta, detect_lang_code, normalise_author_name};

/// Global scan lock — prevents overlapping scans.
static SCAN_LOCK: AtomicBool = AtomicBool::new(false);

/// Last completed scan result (taken once by the status endpoint).
static LAST_SCAN_RESULT: Mutex<Option<ScanResult>> = Mutex::new(None);

/// Returns `true` if a scan is currently in progress.
pub fn is_scanning() -> bool {
    SCAN_LOCK.load(Ordering::SeqCst)
}

/// Takes the last scan result, leaving `None` in its place.
pub fn take_last_scan_result() -> Option<ScanResult> {
    LAST_SCAN_RESULT.lock().ok().and_then(|mut r| r.take())
}

pub fn store_scan_result(result: ScanResult) {
    if let Ok(mut r) = LAST_SCAN_RESULT.lock() {
        *r = Some(result);
    }
}

/// Outcome of a completed scan (stats or error message).
#[derive(Debug, Clone, serde::Serialize)]
pub struct ScanResult {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stats: Option<ScanStats>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Statistics collected during a scan run.
#[derive(Debug, Default, Clone, serde::Serialize)]
pub struct ScanStats {
    pub books_added: u64,
    pub books_skipped: u64,
    pub books_deleted: u64,
    pub archives_scanned: u64,
    pub archives_skipped: u64,
    pub errors: u64,
}

/// Run a full scan of the library directory.
pub async fn run_scan(pool: &DbPool, config: &Config) -> Result<ScanStats, ScanError> {
    // Acquire scan lock
    if SCAN_LOCK
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Err(ScanError::AlreadyRunning);
    }

    let result = do_scan(pool, config).await;

    // Release lock
    SCAN_LOCK.store(false, Ordering::SeqCst);

    result
}

async fn do_scan(pool: &DbPool, config: &Config) -> Result<ScanStats, ScanError> {
    let root = &config.library.root_path;
    let covers_dir = &config.opds.covers_dir;
    let extensions: HashSet<String> = config
        .library
        .book_extensions
        .iter()
        .map(|e| e.to_lowercase())
        .collect();
    let scan_zip = config.library.scan_zip;
    let inpx_enable = config.library.inpx_enable;

    info!("Starting library scan: {}", root.display());

    let mut stats = ScanStats::default();

    // Step 1: Mark all available books as unverified (avail=1)
    let marked = books::set_avail_all(pool, models::AVAIL_UNVERIFIED).await?;
    info!("Marked {marked} books as unverified");

    // Step 2: Walk filesystem
    let root_path = root.clone();
    let extensions_clone = extensions.clone();
    // Use spawn_blocking for the filesystem walk to avoid blocking Tokio
    let walk_result = tokio::task::spawn_blocking(move || {
        collect_entries(&root_path, &extensions_clone, scan_zip, inpx_enable)
    })
    .await
    .map_err(|e| ScanError::Internal(e.to_string()))?;

    let entries = walk_result?;
    info!("Found {} entries to process", entries.len());

    for entry in entries {
        match entry {
            ScanEntry::File {
                path,
                rel_path,
                filename,
                extension,
                size,
            } => {
                match process_file(
                    pool, root, &path, &rel_path, &filename, &extension, size, &mut stats,
                    covers_dir,
                )
                .await
                {
                    Ok(()) => {}
                    Err(e) => {
                        debug!("Error processing {}: {e}", path.display());
                        stats.errors += 1;
                    }
                }
            }
            ScanEntry::Zip { path, rel_path } => {
                match process_zip(
                    pool,
                    root,
                    &path,
                    &rel_path,
                    &extensions,
                    &mut stats,
                    covers_dir,
                )
                .await
                {
                    Ok(()) => {}
                    Err(e) => {
                        debug!("Error processing ZIP {}: {e}", path.display());
                        stats.errors += 1;
                    }
                }
            }
            ScanEntry::Inpx { path, rel_path } => {
                match process_inpx(pool, &path, &rel_path, &mut stats, covers_dir).await {
                    Ok(()) => {}
                    Err(e) => {
                        debug!("Error processing INPX {}: {e}", path.display());
                        stats.errors += 1;
                    }
                }
            }
        }
    }

    // Step 3: Handle books not found during scan (avail <= 1)
    if config.scanner.delete_logical {
        let deleted = books::logical_delete_unavailable(pool).await?;
        stats.books_deleted = deleted;
        info!("Logically deleted {deleted} unavailable books");
    } else {
        // Get IDs before deletion so we can remove cover files
        let ids = books::get_unavailable_ids(pool).await?;
        let deleted = books::physical_delete_unavailable(pool).await?;
        stats.books_deleted = deleted;
        // Remove cover files from disk
        for id in &ids {
            delete_cover(covers_dir, *id);
        }
        info!(
            "Physically deleted {deleted} unavailable books, removed {} covers",
            ids.len()
        );
    }

    // Step 4: Remove empty catalogs (left after book deletion)
    let cats_deleted = catalogs::delete_empty(pool).await?;
    if cats_deleted > 0 {
        info!("Removed {cats_deleted} empty catalogs");
    }

    // Step 5: Update counters
    counters::update_all(pool).await?;

    info!(
        "Scan complete: added={}, skipped={}, deleted={}, archives_scanned={}, archives_skipped={}, errors={}",
        stats.books_added,
        stats.books_skipped,
        stats.books_deleted,
        stats.archives_scanned,
        stats.archives_skipped,
        stats.errors
    );

    Ok(stats)
}

/// Entries discovered during filesystem walk.
enum ScanEntry {
    File {
        path: PathBuf,
        rel_path: String,
        filename: String,
        extension: String,
        size: i64,
    },
    Zip {
        path: PathBuf,
        rel_path: String,
    },
    Inpx {
        path: PathBuf,
        rel_path: String,
    },
}

/// Walk the filesystem and collect all entries to process.
fn collect_entries(
    root: &Path,
    extensions: &HashSet<String>,
    scan_zip: bool,
    inpx_enable: bool,
) -> Result<Vec<ScanEntry>, ScanError> {
    let mut entries = Vec::new();
    let mut inpx_dirs: HashSet<PathBuf> = HashSet::new();

    // First pass: find directories containing INPX files
    if inpx_enable {
        for entry in WalkDir::new(root).follow_links(true).into_iter().flatten() {
            if entry.file_type().is_file() {
                if let Some(ext) = entry.path().extension() {
                    if ext.to_string_lossy().eq_ignore_ascii_case("inpx") {
                        if let Some(parent) = entry.path().parent() {
                            inpx_dirs.insert(parent.to_path_buf());
                        }
                        let rel = rel_path(root, entry.path());
                        entries.push(ScanEntry::Inpx {
                            path: entry.path().to_path_buf(),
                            rel_path: rel,
                        });
                    }
                }
            }
        }
    }

    // Second pass: collect regular files and ZIPs (skip INPX directories)
    for entry in WalkDir::new(root).follow_links(true).into_iter().flatten() {
        if !entry.file_type().is_file() {
            continue;
        }
        if let Some(parent) = entry.path().parent() {
            if inpx_dirs.contains(parent) {
                continue; // Skip files in INPX directories
            }
        }

        let ext = match entry.path().extension() {
            Some(e) => e.to_string_lossy().to_lowercase(),
            None => continue,
        };

        if ext == "zip" && scan_zip {
            let rel = rel_path(root, entry.path().parent().unwrap_or(entry.path()));
            entries.push(ScanEntry::Zip {
                path: entry.path().to_path_buf(),
                rel_path: rel,
            });
        } else if extensions.contains(&ext) {
            let filename = entry.file_name().to_string_lossy().to_string();
            let rel = rel_path(root, entry.path().parent().unwrap_or(entry.path()));
            let size = entry.metadata().map(|m| m.len() as i64).unwrap_or(0);
            entries.push(ScanEntry::File {
                path: entry.path().to_path_buf(),
                rel_path: rel,
                filename,
                extension: ext,
                size,
            });
        }
    }

    Ok(entries)
}

/// Process a single book file on disk.
async fn process_file(
    pool: &DbPool,
    _root: &Path,
    path: &Path,
    rel_path: &str,
    filename: &str,
    extension: &str,
    size: i64,
    stats: &mut ScanStats,
    covers_dir: &Path,
) -> Result<(), ScanError> {
    // Check if already in DB
    if let Some(_existing) = books::find_by_path_and_filename(pool, rel_path, filename).await? {
        books::set_avail(pool, _existing.id, models::AVAIL_CONFIRMED).await?;
        stats.books_skipped += 1;
        return Ok(());
    }

    // Parse metadata
    let meta = tokio::task::spawn_blocking({
        let path = path.to_path_buf();
        let ext = extension.to_string();
        move || parse_book_file(&path, &ext)
    })
    .await
    .map_err(|e| ScanError::Internal(e.to_string()))??;

    // Ensure catalog exists
    let catalog_id = ensure_catalog(pool, rel_path, models::CAT_NORMAL).await?;

    // Insert book and link metadata
    insert_book_with_meta(
        pool,
        catalog_id,
        filename,
        rel_path,
        extension,
        size,
        models::CAT_NORMAL,
        &meta,
        covers_dir,
    )
    .await?;

    stats.books_added += 1;
    Ok(())
}

/// Process a ZIP archive containing book files.
async fn process_zip(
    pool: &DbPool,
    _root: &Path,
    zip_path: &Path,
    rel_dir: &str,
    extensions: &HashSet<String>,
    stats: &mut ScanStats,
    covers_dir: &Path,
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
    if try_skip_zip_archive(pool, &rel_zip, zip_size).await? {
        stats.archives_skipped += 1;
        return Ok(());
    }

    let catalog_id = ensure_archive_catalog(pool, &rel_zip, models::CAT_ZIP, zip_size).await?;

    // Read ZIP contents in a blocking task
    let zip_path_buf = zip_path.to_path_buf();
    let extensions_clone = extensions.clone();

    let zip_entries =
        tokio::task::spawn_blocking(move || read_zip_entries(&zip_path_buf, &extensions_clone))
            .await
            .map_err(|e| ScanError::Internal(e.to_string()))??;

    for ze in zip_entries {
        // Check if already in DB
        if let Some(existing) =
            books::find_by_path_and_filename(pool, &rel_zip, &ze.filename).await?
        {
            books::set_avail(pool, existing.id, models::AVAIL_CONFIRMED).await?;
            stats.books_skipped += 1;
            continue;
        }

        // Parse metadata from in-memory data
        let meta = {
            let data = ze.data.clone();
            let ext = ze.extension.clone();
            let filename = ze.filename.clone();
            tokio::task::spawn_blocking(move || parse_book_bytes(&data, &ext, &filename))
                .await
                .map_err(|e| ScanError::Internal(e.to_string()))?
        };

        let meta = match meta {
            Ok(m) => m,
            Err(e) => {
                debug!("Failed to parse {} in {}: {e}", ze.filename, zip_filename);
                stats.errors += 1;
                continue;
            }
        };

        insert_book_with_meta(
            pool,
            catalog_id,
            &ze.filename,
            &rel_zip,
            &ze.extension,
            ze.size,
            models::CAT_ZIP,
            &meta,
            covers_dir,
        )
        .await?;

        stats.books_added += 1;
    }

    stats.archives_scanned += 1;
    Ok(())
}

/// Process an INPX index file.
async fn process_inpx(
    pool: &DbPool,
    inpx_path: &Path,
    rel_path: &str,
    stats: &mut ScanStats,
    covers_dir: &Path,
) -> Result<(), ScanError> {
    let inpx_size = fs::metadata(inpx_path)?.len() as i64;
    let inpx_dir = Path::new(rel_path)
        .parent()
        .unwrap_or(Path::new(""))
        .to_string_lossy()
        .to_string();

    if try_skip_inpx_archive(pool, rel_path, &inpx_dir, inpx_size).await? {
        stats.archives_skipped += 1;
        return Ok(());
    }

    ensure_archive_catalog(pool, rel_path, models::CAT_INPX, inpx_size).await?;

    let inpx_path_buf = inpx_path.to_path_buf();
    let records = tokio::task::spawn_blocking(move || {
        let file = fs::File::open(&inpx_path_buf)?;
        let reader = BufReader::new(file);
        parsers::inpx::parse(reader)
    })
    .await
    .map_err(|e| ScanError::Internal(e.to_string()))?
    .map_err(|e| ScanError::Internal(e.to_string()))?;

    info!("INPX: parsed {} records from {}", records.len(), rel_path);

    for record in records {
        let book_path = if inpx_dir.is_empty() {
            record.folder.clone()
        } else {
            format!("{inpx_dir}/{}", record.folder)
        };

        // Check if already in DB
        if let Some(existing) =
            books::find_by_path_and_filename(pool, &book_path, &record.filename).await?
        {
            books::set_avail(pool, existing.id, models::AVAIL_CONFIRMED).await?;
            stats.books_skipped += 1;
            continue;
        }

        let catalog_id = ensure_catalog(pool, &book_path, models::CAT_INPX).await?;

        insert_book_with_meta(
            pool,
            catalog_id,
            &record.filename,
            &book_path,
            &record.format,
            record.size,
            models::CAT_INPX,
            &record.meta,
            covers_dir,
        )
        .await?;

        stats.books_added += 1;
    }

    stats.archives_scanned += 1;
    Ok(())
}

/// Parse a book file from disk by extension.
pub fn parse_book_file(path: &Path, ext: &str) -> Result<BookMeta, ScanError> {
    let file = fs::File::open(path)?;
    let reader = BufReader::new(file);
    match ext {
        "fb2" => parsers::fb2::parse(reader).map_err(|e| ScanError::Parse(e.to_string())),
        "epub" => {
            // EPUB needs Read + Seek, reopen as file
            let file = fs::File::open(path)?;
            parsers::epub::parse(file).map_err(|e| ScanError::Parse(e.to_string()))
        }
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

            match crate::pdf::render_first_page_jpeg_from_path(path) {
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

            match crate::djvu::render_first_page_jpeg_from_path(path) {
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
pub fn parse_book_bytes(data: &[u8], ext: &str, filename: &str) -> Result<BookMeta, ScanError> {
    match ext {
        "fb2" => {
            let reader = BufReader::new(Cursor::new(data));
            parsers::fb2::parse(reader).map_err(|e| ScanError::Parse(e.to_string()))
        }
        "epub" => {
            let cursor = Cursor::new(data);
            parsers::epub::parse(cursor).map_err(|e| ScanError::Parse(e.to_string()))
        }
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

            match crate::pdf::render_first_page_jpeg_from_bytes(data) {
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

            match crate::djvu::render_first_page_jpeg_from_bytes(data) {
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

struct ZipBookEntry {
    filename: String,
    extension: String,
    size: i64,
    data: Vec<u8>,
}

/// Read all matching book files from a ZIP archive.
fn read_zip_entries(
    path: &Path,
    extensions: &HashSet<String>,
) -> Result<Vec<ZipBookEntry>, ScanError> {
    let file = fs::File::open(path)?;
    let reader = BufReader::new(file);
    let mut archive = zip::ZipArchive::new(reader)?;
    let mut entries = Vec::new();

    for i in 0..archive.len() {
        let mut entry = match archive.by_index(i) {
            Ok(e) => e,
            Err(_) => continue,
        };
        if entry.is_dir() {
            continue;
        }
        let name = entry.name().to_string();
        let ext = Path::new(&name)
            .extension()
            .unwrap_or_default()
            .to_string_lossy()
            .to_lowercase();

        if !extensions.contains(&ext) {
            continue;
        }

        let size = entry.size() as i64;
        let filename = Path::new(&name)
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();

        let mut data = Vec::new();
        if let Err(e) = std::io::Read::read_to_end(&mut entry, &mut data) {
            warn!("Failed to read {name} from ZIP: {e}");
            continue;
        }

        entries.push(ZipBookEntry {
            filename,
            extension: ext,
            size,
            data,
        });
    }

    Ok(entries)
}

/// Ensure a catalog row exists for the given path, creating it if needed.
pub async fn ensure_catalog(pool: &DbPool, path: &str, cat_type: i32) -> Result<i64, ScanError> {
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

    let id = catalogs::insert(pool, parent_id, path, &cat_name, cat_type, 0).await?;
    Ok(id)
}

/// Ensure a catalog for an archive exists and update its archive metadata.
async fn ensure_archive_catalog(
    pool: &DbPool,
    path: &str,
    cat_type: i32,
    cat_size: i64,
) -> Result<i64, ScanError> {
    if let Some(cat) = catalogs::find_by_path(pool, path).await? {
        if cat.cat_type != cat_type || cat.cat_size != cat_size {
            catalogs::update_archive_meta(pool, cat.id, cat_type, cat_size).await?;
        }
        return Ok(cat.id);
    }

    // Determine parent catalog
    let parent_path = Path::new(path).parent();
    let parent_id = match parent_path {
        Some(p) if !p.as_os_str().is_empty() => {
            let pp = p.to_string_lossy().to_string();
            Some(Box::pin(ensure_catalog(pool, &pp, models::CAT_NORMAL)).await?)
        }
        _ => None,
    };

    let cat_name = Path::new(path)
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    let id = catalogs::insert(pool, parent_id, path, &cat_name, cat_type, cat_size).await?;
    Ok(id)
}

/// Try to skip scanning an unchanged ZIP archive.
async fn try_skip_zip_archive(
    pool: &DbPool,
    rel_zip: &str,
    zip_size: i64,
) -> Result<bool, ScanError> {
    let Some(cat) = catalogs::find_by_path(pool, rel_zip).await? else {
        return Ok(false);
    };
    if cat.cat_type != models::CAT_ZIP || cat.cat_size != zip_size {
        return Ok(false);
    }
    let updated = books::set_avail_by_path(pool, rel_zip, models::AVAIL_CONFIRMED).await?;
    Ok(updated > 0)
}

/// Try to skip scanning an unchanged INPX archive.
async fn try_skip_inpx_archive(
    pool: &DbPool,
    rel_inpx: &str,
    inpx_dir: &str,
    inpx_size: i64,
) -> Result<bool, ScanError> {
    let Some(cat) = catalogs::find_by_path(pool, rel_inpx).await? else {
        return Ok(false);
    };
    if cat.cat_type != models::CAT_INPX || cat.cat_size != inpx_size {
        return Ok(false);
    }
    let updated = books::set_avail_for_inpx_dir(pool, inpx_dir, models::AVAIL_CONFIRMED).await?;
    Ok(updated > 0)
}

/// Insert a book record and link authors, genres, series.
/// Saves cover image to `covers_dir` if present.
pub async fn insert_book_with_meta(
    pool: &DbPool,
    catalog_id: i64,
    filename: &str,
    path: &str,
    format: &str,
    size: i64,
    cat_type: i32,
    meta: &BookMeta,
    covers_dir: &Path,
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
    if let Some(ref cover_data) = meta.cover_data {
        if let Err(e) = save_cover(covers_dir, book_id, cover_data, &meta.cover_type) {
            warn!("Failed to save cover for book {book_id}: {e}");
        }
    }

    // Link authors
    if meta.authors.is_empty() {
        // Ensure at least one author
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

    // Link genres
    for genre_code in &meta.genres {
        genres::link_book_by_code(pool, book_id, genre_code).await?;
    }

    // Link series
    if let Some(ref ser_title) = meta.series_title {
        if !ser_title.is_empty() {
            let series_id = ensure_series(pool, ser_title).await?;
            series::link_book(pool, book_id, series_id, meta.series_index).await?;
        }
    }

    Ok(book_id)
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

/// Save cover image bytes to disk as `{covers_dir}/{book_id}.{ext}`.
pub fn save_cover(
    covers_dir: &Path,
    book_id: i64,
    data: &[u8],
    mime: &str,
) -> Result<(), std::io::Error> {
    let ext = mime_to_ext(mime);
    let path = covers_dir.join(format!("{book_id}.{ext}"));
    fs::write(&path, data)
}

fn mime_to_ext(mime: &str) -> &str {
    match mime {
        "image/png" => "png",
        "image/gif" => "gif",
        _ => "jpg",
    }
}

/// Remove cover file for a book (tries all known extensions).
fn delete_cover(covers_dir: &Path, book_id: i64) {
    for ext in &["jpg", "png", "gif"] {
        let path = covers_dir.join(format!("{book_id}.{ext}"));
        if path.exists() {
            if let Err(e) = fs::remove_file(&path) {
                warn!("Failed to remove cover {}: {e}", path.display());
            }
        }
    }
}

fn rel_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .to_string()
}

#[derive(Debug, thiserror::Error)]
pub enum ScanError {
    #[error("scan already running")]
    AlreadyRunning,
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("ZIP error: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("parse error: {0}")]
    Parse(String),
    #[error("internal error: {0}")]
    Internal(String),
}
