pub mod parsers;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::io::{BufReader, Cursor};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use chrono::Utc;
use dashmap::DashMap;
use image::DynamicImage;
use image::GenericImageView;
use image::codecs::jpeg::JpegEncoder;
use tokio::sync::Semaphore;
use tracing::{debug, info, warn};
use walkdir::WalkDir;

use crate::config::{Config, CoverImageConfig};
use crate::db::DbPool;
use crate::db::models::{AvailStatus, CatType};
use crate::db::queries::{authors, books, catalogs, counters, genres, series};

use parsers::{BookMeta, detect_lang_code, normalise_author_name};

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Result / stats types
// ---------------------------------------------------------------------------

/// Outcome of a completed scan (stats or error message).
#[derive(Debug, Clone, serde::Serialize)]
pub struct ScanResult {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stats: Option<ScanStatsSnapshot>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Thread-safe statistics collected during a scan run.
#[derive(Debug, Default)]
pub struct ScanStats {
    pub books_added: AtomicU64,
    pub books_skipped: AtomicU64,
    pub books_deleted: AtomicU64,
    pub archives_scanned: AtomicU64,
    pub archives_skipped: AtomicU64,
    pub errors: AtomicU64,
}

impl ScanStats {
    pub fn snapshot(&self) -> ScanStatsSnapshot {
        ScanStatsSnapshot {
            books_added: self.books_added.load(Ordering::Relaxed),
            books_skipped: self.books_skipped.load(Ordering::Relaxed),
            books_deleted: self.books_deleted.load(Ordering::Relaxed),
            archives_scanned: self.archives_scanned.load(Ordering::Relaxed),
            archives_skipped: self.archives_skipped.load(Ordering::Relaxed),
            errors: self.errors.load(Ordering::Relaxed),
        }
    }
}

/// Snapshot of scan statistics (plain `u64` fields for serialization / cloning).
#[derive(Debug, Default, Clone, serde::Serialize)]
pub struct ScanStatsSnapshot {
    pub books_added: u64,
    pub books_skipped: u64,
    pub books_deleted: u64,
    pub archives_scanned: u64,
    pub archives_skipped: u64,
    pub errors: u64,
}

// ---------------------------------------------------------------------------
// run_scan — public entry point
// ---------------------------------------------------------------------------

/// Run a full scan of the library directory.
pub async fn run_scan(pool: &DbPool, config: &Config) -> Result<ScanStatsSnapshot, ScanError> {
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

// ---------------------------------------------------------------------------
// ScanContext — shared state for (parallel) scan workers
// ---------------------------------------------------------------------------

struct ScanContext {
    pool: DbPool,
    root: PathBuf,
    covers_path: PathBuf,
    cover_image_cfg: CoverImageConfig,
    workers_num: usize,
    concurrency_semaphore: Arc<Semaphore>,
    extensions: HashSet<String>,
    stats: Arc<ScanStats>,
    // Config flags
    skip_unchanged: bool,
    test_zip: bool,
    test_files: bool,
    // Caches (reduces DB round-trips under parallelism)
    catalog_cache: DashMap<String, i64>,
    author_cache: DashMap<String, i64>,
    series_cache: DashMap<String, i64>,
}

// ---------------------------------------------------------------------------
// do_scan — internal scan logic
// ---------------------------------------------------------------------------

async fn do_scan(pool: &DbPool, config: &Config) -> Result<ScanStatsSnapshot, ScanError> {
    let root = &config.library.root_path;
    let covers_path = &config.covers.covers_path;
    let extensions: HashSet<String> = config
        .library
        .book_extensions
        .iter()
        .map(|e| e.to_lowercase())
        .collect();
    let scan_zip = config.library.scan_zip;
    let inpx_enable = config.library.inpx_enable;
    let workers_num = config.scanner.workers_num;

    info!("Starting library scan: {}", root.display());

    let stats = Arc::new(ScanStats::default());

    // Step 1: Mark all available books as unverified (avail=1)
    let marked = books::set_avail_all(pool, AvailStatus::Unverified).await?;
    info!("Marked {marked} books as unverified");

    // Step 2: Walk filesystem
    let root_path = root.clone();
    let extensions_clone = extensions.clone();
    let walk_result = tokio::task::spawn_blocking(move || {
        collect_entries(&root_path, &extensions_clone, scan_zip, inpx_enable)
    })
    .await
    .map_err(|e| ScanError::Internal(e.to_string()))?;

    let entries = walk_result?;
    info!("Found {} entries to process", entries.len());

    let ctx = ScanContext {
        pool: pool.clone(),
        root: root.clone(),
        covers_path: covers_path.clone(),
        cover_image_cfg: CoverImageConfig::from(&config.covers),
        workers_num,
        concurrency_semaphore: Arc::new(Semaphore::new(workers_num.max(1))),
        extensions,
        stats: Arc::clone(&stats),
        skip_unchanged: config.scanner.skip_unchanged,
        test_zip: config.scanner.test_zip,
        test_files: config.scanner.test_files,
        catalog_cache: DashMap::new(),
        author_cache: DashMap::new(),
        series_cache: DashMap::new(),
    };

    let ctx = Arc::new(ctx);

    if workers_num <= 1 {
        // Sequential processing (default)
        for entry in entries {
            process_entry(Arc::clone(&ctx), entry).await;
        }
    } else {
        // Dynamic parallel processing with bounded in-flight tasks.
        let limit = workers_num;
        let mut iter = entries.into_iter();
        let mut tasks = tokio::task::JoinSet::new();

        info!("Parallel scan: dynamic queue with {} workers", limit);

        for _ in 0..limit {
            if let Some(entry) = iter.next() {
                let ctx = Arc::clone(&ctx);
                tasks.spawn(async move {
                    process_entry(ctx, entry).await;
                });
            }
        }

        while let Some(join_result) = tasks.join_next().await {
            join_result.map_err(|e| ScanError::Internal(e.to_string()))?;
            if let Some(entry) = iter.next() {
                let ctx = Arc::clone(&ctx);
                tasks.spawn(async move {
                    process_entry(ctx, entry).await;
                });
            }
        }
    }

    // Step 3: Handle books not found during scan (avail <= 1)
    if config.scanner.delete_logical {
        let deleted = books::logical_delete_unavailable(pool).await?;
        stats.books_deleted.store(deleted, Ordering::Relaxed);
        info!("Logically deleted {deleted} unavailable books");
    } else {
        // Get IDs before deletion so we can remove cover files
        let ids = books::get_unavailable_ids(pool).await?;
        let deleted = books::physical_delete_unavailable(pool).await?;
        stats.books_deleted.store(deleted, Ordering::Relaxed);
        // Remove cover files from disk
        for id in &ids {
            delete_cover(covers_path, *id);
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

    let snap = stats.snapshot();
    info!(
        "Scan complete: added={}, skipped={}, deleted={}, archives_scanned={}, archives_skipped={}, errors={}",
        snap.books_added,
        snap.books_skipped,
        snap.books_deleted,
        snap.archives_scanned,
        snap.archives_skipped,
        snap.errors
    );

    Ok(snap)
}

// ---------------------------------------------------------------------------
// ScanEntry — entries discovered during filesystem walk
// ---------------------------------------------------------------------------

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
        mtime: String,
    },
    Inpx {
        path: PathBuf,
        rel_path: String,
        mtime: String,
    },
}

/// Get file modification time as RFC 3339 string.
fn file_mtime(path: &Path) -> String {
    fs::metadata(path)
        .ok()
        .and_then(|m| m.modified().ok())
        .map(|t| {
            let dt: chrono::DateTime<Utc> = t.into();
            dt.to_rfc3339()
        })
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Filesystem walk
// ---------------------------------------------------------------------------

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
            if entry.file_type().is_file()
                && let Some(ext) = entry.path().extension()
                && ext.to_string_lossy().eq_ignore_ascii_case("inpx")
            {
                if let Some(parent) = entry.path().parent() {
                    inpx_dirs.insert(parent.to_path_buf());
                }
                let rel = rel_path(root, entry.path());
                let mtime = file_mtime(entry.path());
                entries.push(ScanEntry::Inpx {
                    path: entry.path().to_path_buf(),
                    rel_path: rel,
                    mtime,
                });
            }
        }
    }

    // Second pass: collect regular files and ZIPs (skip INPX directories)
    for entry in WalkDir::new(root).follow_links(true).into_iter().flatten() {
        if !entry.file_type().is_file() {
            continue;
        }
        if let Some(parent) = entry.path().parent()
            && inpx_dirs.contains(parent)
        {
            continue; // Skip files in INPX directories
        }

        let ext = match entry.path().extension() {
            Some(e) => e.to_string_lossy().to_lowercase(),
            None => continue,
        };

        if ext == "zip" && scan_zip {
            let rel = rel_path(root, entry.path().parent().unwrap_or(entry.path()));
            let mtime = file_mtime(entry.path());
            entries.push(ScanEntry::Zip {
                path: entry.path().to_path_buf(),
                rel_path: rel,
                mtime,
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

// ---------------------------------------------------------------------------
// Entry processing
// ---------------------------------------------------------------------------

/// Dispatch a single scan entry to the appropriate handler.
async fn process_entry(ctx: Arc<ScanContext>, entry: ScanEntry) {
    match entry {
        ScanEntry::File {
            path,
            rel_path,
            filename,
            extension,
            size,
        } => {
            if let Err(e) = process_file(&ctx, &path, &rel_path, &filename, &extension, size).await
            {
                debug!("Error processing {}: {e}", path.display());
                ctx.stats.errors.fetch_add(1, Ordering::Relaxed);
            }
        }
        ScanEntry::Zip {
            path,
            rel_path,
            mtime,
        } => {
            if let Err(e) = process_zip(&ctx, &path, &rel_path, &mtime).await {
                debug!("Error processing ZIP {}: {e}", path.display());
                ctx.stats.errors.fetch_add(1, Ordering::Relaxed);
            }
        }
        ScanEntry::Inpx {
            path,
            rel_path,
            mtime,
        } => {
            if let Err(e) = process_inpx(Arc::clone(&ctx), &path, &rel_path, &mtime).await {
                debug!("Error processing INPX {}: {e}", path.display());
                ctx.stats.errors.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}

async fn acquire_scan_permit(
    ctx: &ScanContext,
) -> Result<tokio::sync::OwnedSemaphorePermit, ScanError> {
    ctx.concurrency_semaphore
        .clone()
        .acquire_owned()
        .await
        .map_err(|e| ScanError::Internal(format!("scan semaphore closed: {e}")))
}

/// Process a single book file on disk.
async fn process_file(
    ctx: &ScanContext,
    path: &Path,
    rel_path: &str,
    filename: &str,
    extension: &str,
    size: i64,
) -> Result<(), ScanError> {
    // Check if already in DB
    if let Some(existing) = books::find_by_path_and_filename(&ctx.pool, rel_path, filename).await? {
        books::set_avail(&ctx.pool, existing.id, AvailStatus::Confirmed).await?;
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

    // Ensure catalog exists
    let catalog_id = cached_ensure_catalog(ctx, rel_path, CatType::Normal).await?;

    // Insert book and link metadata
    ctx_insert_book_with_meta(
        ctx,
        catalog_id,
        filename,
        rel_path,
        extension,
        size,
        CatType::Normal,
        &meta,
    )
    .await?;

    ctx.stats.books_added.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

/// Process a ZIP archive containing book files.
async fn process_zip(
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

    let catalog_id =
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
        // Check if already in DB
        if let Some(existing) =
            books::find_by_path_and_filename(&ctx.pool, &rel_zip, &ze.filename).await?
        {
            books::set_avail(&ctx.pool, existing.id, AvailStatus::Confirmed).await?;
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

        ctx_insert_book_with_meta(
            ctx,
            catalog_id,
            &ze.filename,
            &rel_zip,
            &ze.extension,
            ze.size,
            CatType::Zip,
            &meta,
        )
        .await?;

        ctx.stats.books_added.fetch_add(1, Ordering::Relaxed);
    }

    ctx.stats.archives_scanned.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

/// Process an INPX index file.
async fn process_inpx(
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

    let inpx_path_buf = inpx_path.to_path_buf();
    let records = {
        let _permit = acquire_scan_permit(&ctx).await?;
        tokio::task::spawn_blocking(move || {
            let file = fs::File::open(&inpx_path_buf)?;
            let reader = BufReader::new(file);
            parsers::inpx::parse(reader)
        })
        .await
        .map_err(|e| ScanError::Internal(e.to_string()))?
        .map_err(|e| ScanError::Internal(e.to_string()))?
    };

    info!("INPX: parsed {} records from {}", records.len(), rel_path);

    // Group records by referenced ZIP archive to avoid reopening the same ZIP
    // for each book entry.
    let mut by_zip_path: BTreeMap<String, Vec<parsers::inpx::InpxRecord>> = BTreeMap::new();
    for record in records {
        let book_path = if inpx_dir.is_empty() {
            record.folder.clone()
        } else {
            format!("{inpx_dir}/{}", record.folder)
        };
        by_zip_path.entry(book_path).or_default().push(record);
    }

    let groups: Vec<(String, Vec<parsers::inpx::InpxRecord>)> = by_zip_path.into_iter().collect();
    if groups.is_empty() {
        ctx.stats.archives_scanned.fetch_add(1, Ordering::Relaxed);
        return Ok(());
    }

    if ctx.workers_num <= 1 || groups.len() == 1 {
        for (book_path, zip_records) in groups {
            if let Err(e) = process_inpx_zip_group(&ctx, &book_path, zip_records).await {
                warn!("INPX group processing failed for '{}': {}", book_path, e);
                ctx.stats.errors.fetch_add(1, Ordering::Relaxed);
            }
        }
    } else {
        let limit = ctx.workers_num.min(groups.len()).max(1);
        let mut iter = groups.into_iter();
        let mut tasks = tokio::task::JoinSet::new();

        for _ in 0..limit {
            if let Some((book_path, zip_records)) = iter.next() {
                let ctx = Arc::clone(&ctx);
                tasks.spawn(
                    async move { process_inpx_zip_group(&ctx, &book_path, zip_records).await },
                );
            }
        }

        while let Some(join_result) = tasks.join_next().await {
            match join_result {
                Ok(Ok(())) => {}
                Ok(Err(e)) => {
                    warn!("INPX worker task failed: {e}");
                    ctx.stats.errors.fetch_add(1, Ordering::Relaxed);
                }
                Err(e) => {
                    warn!("INPX worker join failure: {e}");
                    ctx.stats.errors.fetch_add(1, Ordering::Relaxed);
                }
            }
            if let Some((book_path, zip_records)) = iter.next() {
                let ctx = Arc::clone(&ctx);
                tasks.spawn(
                    async move { process_inpx_zip_group(&ctx, &book_path, zip_records).await },
                );
            }
        }
    }

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
        // Check if already in DB
        if let Some(existing) =
            books::find_by_path_and_filename(&ctx.pool, book_path, &record.filename).await?
        {
            books::set_avail(&ctx.pool, existing.id, AvailStatus::Confirmed).await?;
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
                read_selected_zip_entries_meta(
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

    let catalog_id = cached_ensure_catalog(ctx, book_path, CatType::Inpx).await?;
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

        match ctx_insert_book_with_meta(
            ctx,
            catalog_id,
            &record.filename,
            book_path,
            &record.format,
            record.size,
            CatType::Inpx,
            &meta,
        )
        .await
        {
            Ok(_) => {
                ctx.stats.books_added.fetch_add(1, Ordering::Relaxed);
            }
            Err(e) => {
                warn!(
                    "Failed to insert INPX book '{}::{}': {}",
                    book_path, record.filename, e
                );
                ctx.stats.errors.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Book parsing
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// ZIP reading / validation
// ---------------------------------------------------------------------------

struct ZipBookEntry {
    filename: String,
    extension: String,
    size: i64,
    data: Vec<u8>,
}

/// Iterate ZIP entries, read matching files into memory, validate size if
/// requested, and hand data to a callback.
fn for_each_matching_zip_entry<S, H>(
    path: &Path,
    extensions: &HashSet<String>,
    test_files: bool,
    mut select: S,
    mut on_entry: H,
) -> Result<(), ScanError>
where
    S: FnMut(&str, &str, &str) -> bool,
    H: FnMut(String, String, u64, Vec<u8>),
{
    let file = fs::File::open(path)?;
    let reader = BufReader::new(file);
    let mut archive = zip::ZipArchive::new(reader)?;

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

        let filename = Path::new(&name)
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        if !select(&name, &filename, &ext) {
            continue;
        }

        let declared_size = entry.size();
        let mut data = Vec::new();
        if let Err(e) = std::io::Read::read_to_end(&mut entry, &mut data) {
            warn!("Failed to read {name} from ZIP: {e}");
            continue;
        }

        if test_files && data.len() as u64 != declared_size {
            warn!(
                "Size mismatch for {} in {}: expected {}, got {}",
                name,
                path.display(),
                declared_size,
                data.len()
            );
            continue;
        }

        on_entry(filename, ext, declared_size, data);
    }

    Ok(())
}

/// Read all matching book files from a ZIP archive.
/// When `test_files` is enabled, entries whose extracted size does not match
/// the declared size are skipped.
fn read_zip_entries(
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
fn read_selected_zip_entries_meta(
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
fn validate_zip_integrity(path: &Path) -> Result<bool, ScanError> {
    let file = fs::File::open(path)?;
    let reader = BufReader::new(file);
    let mut archive = zip::ZipArchive::new(reader)?;
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

// ---------------------------------------------------------------------------
// Catalog / author / series helpers (public API — used by upload, admin)
// ---------------------------------------------------------------------------

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
async fn ensure_archive_catalog(
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

// ---------------------------------------------------------------------------
// Cache-aware helpers (scanner-internal, uses DashMap to reduce DB round-trips)
// ---------------------------------------------------------------------------

async fn cached_ensure_catalog(
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

// ---------------------------------------------------------------------------
// Book insertion (public + cache-aware scanner-internal)
// ---------------------------------------------------------------------------

/// Insert a book record and link authors, genres, series (scanner-internal,
/// uses DashMap-cached ensure functions).
#[allow(clippy::too_many_arguments)]
async fn ctx_insert_book_with_meta(
    ctx: &ScanContext,
    catalog_id: i64,
    filename: &str,
    path: &str,
    format: &str,
    size: i64,
    cat_type: CatType,
    meta: &BookMeta,
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
        &ctx.pool,
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
            &ctx.covers_path,
            book_id,
            cover_data,
            &meta.cover_type,
            ctx.cover_image_cfg,
        )
    {
        warn!("Failed to save cover for book {book_id}: {e}");
    }

    // Link authors
    if meta.authors.is_empty() {
        let author_id = cached_ensure_author(ctx, "Unknown").await?;
        authors::link_book(&ctx.pool, book_id, author_id).await?;
    } else {
        for author_name in &meta.authors {
            let name = normalise_author_name(author_name);
            if name.is_empty() {
                continue;
            }
            let author_id = cached_ensure_author(ctx, &name).await?;
            authors::link_book(&ctx.pool, book_id, author_id).await?;
        }
    }
    books::update_author_key(&ctx.pool, book_id).await?;

    // Link genres
    for genre_code in &meta.genres {
        genres::link_book_by_code(&ctx.pool, book_id, genre_code).await?;
    }

    // Link series
    if let Some(ref ser_title) = meta.series_title
        && !ser_title.is_empty()
    {
        let series_id = cached_ensure_series(ctx, ser_title).await?;
        series::link_book(&ctx.pool, book_id, series_id, meta.series_index).await?;
    }

    Ok(book_id)
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

// ---------------------------------------------------------------------------
// Skip-unchanged logic
// ---------------------------------------------------------------------------

/// Try to skip scanning an unchanged ZIP archive.
/// With `skip_unchanged` enabled, also checks mtime (backward-compatible with
/// empty mtime in old DB records).
async fn try_skip_zip_archive(
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
    Ok(updated > 0)
}

// ---------------------------------------------------------------------------
// Covers & utilities
// ---------------------------------------------------------------------------

pub(crate) fn normalize_cover_for_storage_with_options(
    data: &[u8],
    mime: &str,
    cover_cfg: CoverImageConfig,
) -> (Vec<u8>, String) {
    let max_dimension_px = cover_cfg.scale_to();
    let jpeg_quality = cover_cfg.jpeg_quality();

    let Ok(img) = image::load_from_memory(data) else {
        // Keep original bytes if decoder can't parse this format.
        return (data.to_vec(), normalize_mime(mime).to_string());
    };

    let (w, h) = img.dimensions();
    let is_jpeg = matches!(mime, "image/jpeg" | "image/jpg" | "image/pjpeg");
    let needs_resize = w.max(h) > max_dimension_px;
    let needs_format_conversion = !is_jpeg;

    if !needs_resize && !needs_format_conversion {
        return (data.to_vec(), normalize_mime(mime).to_string());
    }

    let processed = if needs_resize {
        img.resize(
            max_dimension_px,
            max_dimension_px,
            image::imageops::FilterType::Lanczos3,
        )
    } else {
        img
    };

    // Store decodable covers as JPEG to ensure uniform quality/size handling.
    if let Some(bytes) = encode_jpeg(&processed, jpeg_quality) {
        return (bytes, "image/jpeg".to_string());
    }

    // Fallback if encoding fails for any reason.
    (data.to_vec(), normalize_mime(mime).to_string())
}

/// Save cover image bytes to disk using hierarchical cover storage.
pub fn save_cover(
    covers_path: &Path,
    book_id: i64,
    data: &[u8],
    mime: &str,
    cover_cfg: CoverImageConfig,
) -> Result<(), std::io::Error> {
    let (normalized_data, normalized_mime) =
        normalize_cover_for_storage_with_options(data, mime, cover_cfg);
    let ext = mime_to_ext(&normalized_mime);
    let path = cover_storage_path(covers_path, book_id, ext);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, normalized_data)
}

/// Return hierarchical storage path for a cover file.
/// Layout: `{covers_dir}/{bucket_millions}/{bucket_thousands}/{book_id}.{ext}`.
pub fn cover_storage_path(covers_path: &Path, book_id: i64, ext: &str) -> PathBuf {
    let id = book_id.unsigned_abs();
    let bucket_millions = (id / 1_000_000) % 1_000;
    let bucket_thousands = (id / 1_000) % 1_000;
    covers_path
        .join(format!("{bucket_millions:03}"))
        .join(format!("{bucket_thousands:03}"))
        .join(format!("{book_id}.{ext}"))
}

/// Return legacy flat storage path for a cover file.
pub fn legacy_cover_storage_path(covers_path: &Path, book_id: i64, ext: &str) -> PathBuf {
    covers_path.join(format!("{book_id}.{ext}"))
}

fn mime_to_ext(mime: &str) -> &str {
    match mime {
        "image/png" => "png", // legacy/decode-fallback covers
        "image/gif" => "gif", // legacy/decode-fallback covers
        _ => "jpg",
    }
}

fn normalize_mime(mime: &str) -> &str {
    match mime {
        "image/png" => "image/png",
        "image/gif" => "image/gif",
        _ => "image/jpeg",
    }
}

fn encode_jpeg(img: &DynamicImage, quality: u8) -> Option<Vec<u8>> {
    let mut out = Cursor::new(Vec::new());
    let mut encoder = JpegEncoder::new_with_quality(&mut out, quality);
    encoder.encode_image(img).ok()?;
    Some(out.into_inner())
}

/// Remove cover file for a book (tries all known extensions).
fn delete_cover(covers_path: &Path, book_id: i64) {
    for ext in &["jpg", "png", "gif"] {
        for path in [
            cover_storage_path(covers_path, book_id, ext),
            legacy_cover_storage_path(covers_path, book_id, ext),
        ] {
            if path.exists() {
                match fs::remove_file(&path) {
                    Ok(()) => remove_empty_cover_dirs(covers_path, &path),
                    Err(e) => warn!("Failed to remove cover {}: {e}", path.display()),
                }
            }
        }
    }
}

fn remove_empty_cover_dirs(covers_path: &Path, file_path: &Path) {
    let Some(dir) = file_path.parent() else {
        return;
    };
    if dir == covers_path {
        return;
    }
    let _ = fs::remove_dir(dir);
    if let Some(parent) = dir.parent()
        && parent != covers_path
    {
        let _ = fs::remove_dir(parent);
    }
}

fn rel_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .to_string()
}

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::create_test_pool;
    use std::io::Write;
    use tempfile::tempdir;

    fn test_cover_cfg() -> CoverImageConfig {
        CoverImageConfig::new(0, 0)
    }

    fn make_zip(path: &Path, entries: &[(&str, &[u8])]) {
        let file = fs::File::create(path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        for (name, data) in entries {
            zip.start_file(*name, opts).unwrap();
            zip.write_all(data).unwrap();
        }
        zip.finish().unwrap();
    }

    #[test]
    fn test_scan_result_store_and_take() {
        store_scan_result(ScanResult {
            ok: true,
            stats: Some(ScanStatsSnapshot::default()),
            error: None,
        });
        assert!(take_last_scan_result().is_some());
        assert!(take_last_scan_result().is_none());
        assert!(!is_scanning());
    }

    #[test]
    fn test_parse_book_bytes_fallback_for_unknown_ext() {
        let meta = parse_book_bytes(b"ignored", "txt", "my-file.txt", test_cover_cfg()).unwrap();
        assert_eq!(meta.title, "my-file");
    }

    #[test]
    fn test_parse_book_file_fallback_for_unknown_ext() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("book.unknown");
        fs::write(&path, b"data").unwrap();
        let meta = parse_book_file(&path, "unknown", test_cover_cfg()).unwrap();
        assert_eq!(meta.title, "book");
    }

    #[test]
    fn test_read_zip_entries_and_validate_integrity() {
        let dir = tempdir().unwrap();
        let zip_path = dir.path().join("books.zip");
        make_zip(
            &zip_path,
            &[
                ("a.fb2", b"one"),
                ("b.txt", b"two"),
                ("nested/c.epub", b"three"),
            ],
        );

        let mut exts = HashSet::new();
        exts.insert("fb2".to_string());
        exts.insert("epub".to_string());

        let entries = read_zip_entries(&zip_path, &exts, false).unwrap();
        assert_eq!(entries.len(), 2);
        assert!(entries.iter().any(|e| e.filename == "a.fb2"));
        assert!(entries.iter().any(|e| e.filename == "c.epub"));
        assert!(validate_zip_integrity(&zip_path).unwrap());
    }

    #[test]
    fn test_zip_helpers_invalid_archive_errors() {
        let dir = tempdir().unwrap();
        let bad = dir.path().join("bad.zip");
        fs::write(&bad, b"not-a-zip").unwrap();

        let exts = HashSet::from(["fb2".to_string()]);
        assert!(matches!(
            read_zip_entries(&bad, &exts, false),
            Err(ScanError::Zip(_))
        ));
        assert!(matches!(
            validate_zip_integrity(&bad),
            Err(ScanError::Zip(_))
        ));
    }

    #[tokio::test]
    async fn test_ensure_catalog_author_series() {
        let pool = create_test_pool().await;
        let cat_id = ensure_catalog(&pool, "a/b", CatType::Normal).await.unwrap();
        assert!(cat_id > 0);

        let parent: Option<(i64,)> =
            sqlx::query_as(&*pool.sql("SELECT id FROM catalogs WHERE path = ?"))
                .bind("a")
                .fetch_optional(pool.inner())
                .await
                .unwrap();
        assert!(parent.is_some());

        let a1 = ensure_author(&pool, "Isaac Asimov").await.unwrap();
        let a2 = ensure_author(&pool, "Isaac Asimov").await.unwrap();
        assert_eq!(a1, a2);

        let s1 = ensure_series(&pool, "Foundation").await.unwrap();
        let s2 = ensure_series(&pool, "Foundation").await.unwrap();
        assert_eq!(s1, s2);
    }

    #[tokio::test]
    async fn test_run_scan_already_running() {
        let pool = create_test_pool().await;
        let cfg: crate::config::Config = toml::from_str(
            r#"
[server]
[library]
root_path = "/tmp"
[database]
[opds]
[scanner]
"#,
        )
        .unwrap();

        SCAN_LOCK.store(true, Ordering::SeqCst);
        let res = run_scan(&pool, &cfg).await;
        SCAN_LOCK.store(false, Ordering::SeqCst);
        assert!(matches!(res, Err(ScanError::AlreadyRunning)));
    }

    #[test]
    fn test_cover_helpers_and_rel_path() {
        let dir = tempdir().unwrap();
        save_cover(dir.path(), 42, b"cover", "image/png", test_cover_cfg()).unwrap();
        let png = cover_storage_path(dir.path(), 42, "png");
        assert!(png.exists());

        // Also create a legacy flat file to ensure backward-compatible cleanup.
        let legacy_jpg = legacy_cover_storage_path(dir.path(), 42, "jpg");
        fs::write(&legacy_jpg, b"x").unwrap();
        delete_cover(dir.path(), 42);
        assert!(!png.exists());
        assert!(!legacy_jpg.exists());

        assert_eq!(mime_to_ext("image/png"), "png");
        assert_eq!(mime_to_ext("image/gif"), "gif");
        assert_eq!(mime_to_ext("image/jpeg"), "jpg");

        assert_eq!(
            cover_storage_path(dir.path(), 1_500_123, "jpg"),
            dir.path().join("001").join("500").join("1500123.jpg")
        );

        let root = Path::new("/tmp/root");
        let file = Path::new("/tmp/root/sub/book.fb2");
        assert_eq!(rel_path(root, file), "sub/book.fb2");
    }

    #[test]
    fn test_normalize_cover_for_storage_converts_non_jpeg_and_resizes_when_needed() {
        let small = DynamicImage::new_rgb8(320, 480);
        let mut small_png = Cursor::new(Vec::new());
        small
            .write_to(&mut small_png, image::ImageFormat::Png)
            .unwrap();
        let small_bytes = small_png.into_inner();
        let cfg = test_cover_cfg();
        let (converted_data, converted_mime) =
            normalize_cover_for_storage_with_options(&small_bytes, "image/png", cfg);
        assert_eq!(converted_mime, "image/jpeg");
        assert_ne!(converted_data, small_bytes);
        assert!(matches!(
            image::guess_format(&converted_data),
            Ok(image::ImageFormat::Jpeg)
        ));

        let large = DynamicImage::new_rgb8(1800, 1200);
        let mut large_png = Cursor::new(Vec::new());
        large
            .write_to(&mut large_png, image::ImageFormat::Png)
            .unwrap();
        let (resized_data, resized_mime) =
            normalize_cover_for_storage_with_options(&large_png.into_inner(), "image/png", cfg);
        assert_eq!(resized_mime, "image/jpeg");
        let resized = image::load_from_memory(&resized_data).unwrap();
        let (w, h) = resized.dimensions();
        assert_eq!(w.max(h), cfg.scale_to());
    }

    #[test]
    fn test_normalize_cover_for_storage_converts_gif_to_jpeg() {
        let gif_1x1 = b"GIF89a\x01\x00\x01\x00\x80\x00\x00\
\x00\x00\x00\xff\xff\xff!\xf9\x04\x01\x00\x00\x00\x00,\
\x00\x00\x00\x00\x01\x00\x01\x00\x00\x02\x02D\x01\x00;";
        let cfg = test_cover_cfg();
        let (converted_data, converted_mime) =
            normalize_cover_for_storage_with_options(gif_1x1, "image/gif", cfg);
        assert_eq!(converted_mime, "image/jpeg");
        assert!(matches!(
            image::guess_format(&converted_data),
            Ok(image::ImageFormat::Jpeg)
        ));
    }

    #[test]
    fn test_parse_book_bytes_invalid_epub_returns_parse_error() {
        let err =
            parse_book_bytes(b"not-an-epub", "epub", "bad.epub", test_cover_cfg()).unwrap_err();
        assert!(matches!(err, ScanError::Parse(_)));
    }
}
