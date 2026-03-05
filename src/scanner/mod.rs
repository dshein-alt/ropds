mod book;
mod cover;
mod db;
mod inpx;
pub mod parsers;
mod zip;

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use chrono::Utc;
use dashmap::{DashMap, DashSet};
use tokio::sync::{Semaphore, mpsc};
use tracing::{debug, info, warn};
use walkdir::WalkDir;

use crate::config::{Config, CoverImageConfig};
use crate::db::DbPool;
use crate::db::models::{AvailStatus, CatType};
use crate::db::queries::{authors, books, catalogs, counters, genres, series};

use book::process_file;
pub use book::{insert_book_with_meta, parse_book_bytes, parse_book_file};
use cover::delete_cover;
pub(crate) use cover::normalize_cover_for_storage_with_options;
pub use cover::{
    cover_storage_path, legacy_cover_storage_path, save_cover, two_level_cover_storage_path,
};
use db::{
    build_pending_book_insert, enqueue_pending_book, ensure_archive_catalog,
    run_pending_book_writer,
};
pub use db::{ensure_author, ensure_catalog, ensure_series};
use inpx::process_inpx;
use parsers::{BookMeta, detect_lang_code, normalise_author_name};
use zip::process_zip;

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
    genre_cache: DashMap<String, Option<i64>>,
    series_cache: DashMap<String, i64>,
    existing_books_by_path: HashMap<String, HashMap<String, i64>>,
    confirmed_existing_ids: DashSet<i64>,
    pending_new_books: DashSet<String>,
    pending_book_tx: mpsc::Sender<PendingBookMsg>,
}

impl ScanContext {
    fn existing_book_id(&self, path: &str, filename: &str) -> Option<i64> {
        self.existing_books_by_path
            .get(path)
            .and_then(|by_name| by_name.get(filename))
            .copied()
    }

    fn mark_existing_book_confirmed(&self, book_id: i64) {
        self.confirmed_existing_ids.insert(book_id);
    }

    fn pending_book_key(path: &str, filename: &str) -> String {
        // NUL cannot appear in filesystem path components, so this separator
        // avoids accidental key collisions across (path, filename) pairs.
        format!("{path}\0{filename}")
    }

    fn try_mark_pending_new_book(&self, path: &str, filename: &str) -> bool {
        self.pending_new_books
            .insert(Self::pending_book_key(path, filename))
    }
}

struct PendingBookInsert {
    catalog_id: i64,
    filename: String,
    path: String,
    format: String,
    size: i64,
    cat_type: CatType,
    title: String,
    search_title: String,
    annotation: String,
    docdate: String,
    lang: String,
    lang_code: i32,
    cover_type: String,
    cover_data: Option<Vec<u8>>,
    author_ids: Vec<i64>,
    genre_ids: Vec<i64>,
    series_link: Option<(i64, i32)>,
    author_key: String,
}

enum PendingBookMsg {
    Insert(Box<PendingBookInsert>),
    Finish,
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
    let existing_books = books::list_existing_for_scan(pool).await?;
    let mut existing_books_by_path: HashMap<String, HashMap<String, i64>> = HashMap::new();
    for row in existing_books {
        existing_books_by_path
            .entry(row.path)
            .or_default()
            .insert(row.filename, row.id);
    }

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
    let (pending_book_tx, pending_book_rx) =
        mpsc::channel::<PendingBookMsg>(workers_num.max(1) * 128);

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
        genre_cache: DashMap::new(),
        series_cache: DashMap::new(),
        existing_books_by_path,
        confirmed_existing_ids: DashSet::new(),
        pending_new_books: DashSet::new(),
        pending_book_tx,
    };

    let ctx = Arc::new(ctx);
    let writer_ctx = Arc::clone(&ctx);
    let writer_task =
        tokio::spawn(async move { run_pending_book_writer(writer_ctx, pending_book_rx).await });

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
            if let Err(e) = join_result {
                warn!("Top-level scan worker join failure: {e}");
                ctx.stats.errors.fetch_add(1, Ordering::Relaxed);
            }
            if let Some(entry) = iter.next() {
                let ctx = Arc::clone(&ctx);
                tasks.spawn(async move {
                    process_entry(ctx, entry).await;
                });
            }
        }
    }

    if let Err(e) = ctx.pending_book_tx.send(PendingBookMsg::Finish).await {
        warn!("Failed to finalize pending-book writer: {e}");
        ctx.stats.errors.fetch_add(1, Ordering::Relaxed);
    }
    match writer_task.await {
        Ok(Ok(())) => {}
        Ok(Err(e)) => {
            warn!("Pending-book writer failed: {e}");
            ctx.stats.errors.fetch_add(1, Ordering::Relaxed);
        }
        Err(e) => {
            warn!("Pending-book writer join failure: {e}");
            ctx.stats.errors.fetch_add(1, Ordering::Relaxed);
        }
    }

    let mut confirmed_existing_ids: Vec<i64> =
        ctx.confirmed_existing_ids.iter().map(|id| *id).collect();
    confirmed_existing_ids.sort_unstable();
    let confirmed_updated =
        books::set_avail_confirmed_for_ids(pool, &confirmed_existing_ids).await?;
    debug!(
        "Confirmed existing books by id: requested={}, updated={}",
        confirmed_existing_ids.len(),
        confirmed_updated
    );

    // Step 3: Handle books not found during scan (avail <= 1)
    let scan_errors = stats.errors.load(Ordering::Relaxed);
    if scan_errors > 0 {
        warn!(
            "Skipping deletion step: {scan_errors} error(s) occurred during scan, \
             some books may have been left unverified due to worker failures"
        );
    } else if config.scanner.delete_logical {
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

// ---------------------------------------------------------------------------
// Covers & utilities
// ---------------------------------------------------------------------------

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
    Zip(#[from] ::zip::result::ZipError),
    #[error("parse error: {0}")]
    Parse(String),
    #[error("internal error: {0}")]
    Internal(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::create_test_pool;
    use image::{DynamicImage, GenericImageView};
    use std::io::{Cursor, Write};
    use tempfile::tempdir;

    fn test_cover_cfg() -> CoverImageConfig {
        CoverImageConfig::new(0, 0)
    }

    fn make_zip(path: &Path, entries: &[(&str, &[u8])]) {
        let file = fs::File::create(path).unwrap();
        let mut zip = ::zip::ZipWriter::new(file);
        let opts = ::zip::write::SimpleFileOptions::default()
            .compression_method(::zip::CompressionMethod::Stored);
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

        let entries = zip::read_zip_entries(&zip_path, &exts, false).unwrap();
        assert_eq!(entries.len(), 2);
        assert!(entries.iter().any(|e| e.filename == "a.fb2"));
        assert!(entries.iter().any(|e| e.filename == "c.epub"));
        assert!(zip::validate_zip_integrity(&zip_path).unwrap());
    }

    #[test]
    fn test_zip_helpers_invalid_archive_errors() {
        let dir = tempdir().unwrap();
        let bad = dir.path().join("bad.zip");
        fs::write(&bad, b"not-a-zip").unwrap();

        let exts = HashSet::from(["fb2".to_string()]);
        assert!(matches!(
            zip::read_zip_entries(&bad, &exts, false),
            Err(ScanError::Zip(_))
        ));
        assert!(matches!(
            zip::validate_zip_integrity(&bad),
            Err(ScanError::Zip(_))
        ));
    }

    #[tokio::test]
    async fn test_ensure_catalog_author_series() {
        let pool = create_test_pool().await;
        let cat_id = ensure_catalog(&pool, "a/b", CatType::Normal).await.unwrap();
        assert!(cat_id > 0);

        let parent: Option<(i64,)> =
            sqlx::query_as(&pool.sql("SELECT id FROM catalogs WHERE path = ?"))
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
base_url = "http://127.0.0.1:8081"
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

        // Also create legacy and old two-level files to ensure backward-compatible cleanup.
        let legacy_jpg = legacy_cover_storage_path(dir.path(), 42, "jpg");
        fs::write(&legacy_jpg, b"x").unwrap();
        let two_level_jpg = two_level_cover_storage_path(dir.path(), 42, "jpg");
        fs::create_dir_all(two_level_jpg.parent().unwrap()).unwrap();
        fs::write(&two_level_jpg, b"x").unwrap();
        delete_cover(dir.path(), 42);
        assert!(!png.exists());
        assert!(!legacy_jpg.exists());
        assert!(!two_level_jpg.exists());
        // Bucket directories should be removed when empty.
        assert!(
            !png.parent().unwrap().exists(),
            "1-level bucket dir should be removed"
        );
        assert!(
            !two_level_jpg.parent().unwrap().exists(),
            "2-level inner bucket dir should be removed"
        );
        assert!(
            !two_level_jpg.parent().unwrap().parent().unwrap().exists(),
            "2-level outer bucket dir should be removed"
        );

        assert_eq!(cover::mime_to_ext("image/png"), "png");
        assert_eq!(cover::mime_to_ext("image/gif"), "gif");
        assert_eq!(cover::mime_to_ext("image/jpeg"), "jpg");

        assert_eq!(
            cover_storage_path(dir.path(), 1_500_123, "jpg"),
            dir.path().join("500").join("1500123.jpg")
        );
        assert_eq!(
            two_level_cover_storage_path(dir.path(), 1_500_123, "jpg"),
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
