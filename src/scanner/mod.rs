pub mod parsers;

use std::collections::HashSet;
use std::fs;
use std::io::{BufReader, Cursor};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use tracing::{debug, info, warn};
use walkdir::WalkDir;

use crate::config::Config;
use crate::db::DbPool;
use crate::db::models;
use crate::db::queries::{authors, books, catalogs, counters, genres, series};

use parsers::{detect_lang_code, normalise_author_name, BookMeta};

/// Global scan lock — prevents overlapping scans.
static SCAN_LOCK: AtomicBool = AtomicBool::new(false);

/// Statistics collected during a scan run.
#[derive(Debug, Default)]
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
            ScanEntry::File { path, rel_path, filename, extension, size } => {
                match process_file(
                    pool, root, &path, &rel_path, &filename, &extension, size, &mut stats, covers_dir,
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
                match process_zip(pool, root, &path, &rel_path, &extensions, &mut stats, covers_dir).await {
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

    // Step 3: Delete books still at avail <= 1
    let deleted = books::delete_unavailable(pool).await?;
    stats.books_deleted = deleted;
    info!("Deleted {deleted} unavailable books");

    // Step 4: Update counters
    counters::update_all(pool).await?;

    info!(
        "Scan complete: added={}, skipped={}, deleted={}, errors={}",
        stats.books_added, stats.books_skipped, stats.books_deleted, stats.errors
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
        books::set_avail(
            pool,
            _existing.id,
            models::AVAIL_CONFIRMED,
        )
        .await?;
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

    // Read ZIP contents in a blocking task
    let zip_path_buf = zip_path.to_path_buf();
    let extensions_clone = extensions.clone();

    let zip_entries = tokio::task::spawn_blocking(move || {
        read_zip_entries(&zip_path_buf, &extensions_clone)
    })
    .await
    .map_err(|e| ScanError::Internal(e.to_string()))??;

    let catalog_id = ensure_catalog(pool, &rel_zip, models::CAT_ZIP).await?;

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
            tokio::task::spawn_blocking(move || parse_book_bytes(&data, &ext))
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
        let book_path = if rel_path.is_empty() {
            record.folder.clone()
        } else {
            // rel_path is the path to the .inpx file's directory
            let inpx_dir = Path::new(rel_path)
                .parent()
                .unwrap_or(Path::new(""))
                .to_string_lossy()
                .to_string();
            if inpx_dir.is_empty() {
                record.folder.clone()
            } else {
                format!("{inpx_dir}/{}", record.folder)
            }
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
fn parse_book_file(path: &Path, ext: &str) -> Result<BookMeta, ScanError> {
    let file = fs::File::open(path)?;
    let reader = BufReader::new(file);
    match ext {
        "fb2" => parsers::fb2::parse(reader).map_err(|e| ScanError::Parse(e.to_string())),
        "epub" => {
            // EPUB needs Read + Seek, reopen as file
            let file = fs::File::open(path)?;
            parsers::epub::parse(file).map_err(|e| ScanError::Parse(e.to_string()))
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
fn parse_book_bytes(data: &[u8], ext: &str) -> Result<BookMeta, ScanError> {
    match ext {
        "fb2" => {
            let reader = BufReader::new(Cursor::new(data));
            parsers::fb2::parse(reader).map_err(|e| ScanError::Parse(e.to_string()))
        }
        "epub" => {
            let cursor = Cursor::new(data);
            parsers::epub::parse(cursor).map_err(|e| ScanError::Parse(e.to_string()))
        }
        _ => {
            Ok(BookMeta::default())
        }
    }
}

struct ZipBookEntry {
    filename: String,
    extension: String,
    size: i64,
    data: Vec<u8>,
}

/// Read all matching book files from a ZIP archive.
fn read_zip_entries(path: &Path, extensions: &HashSet<String>) -> Result<Vec<ZipBookEntry>, ScanError> {
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
async fn ensure_catalog(pool: &DbPool, path: &str, cat_type: i32) -> Result<i64, ScanError> {
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

    let id = catalogs::insert(pool, parent_id, path, &cat_name, cat_type).await?;
    Ok(id)
}

/// Insert a book record and link authors, genres, series.
/// Saves cover image to `covers_dir` if present.
async fn insert_book_with_meta(
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
async fn ensure_author(pool: &DbPool, full_name: &str) -> Result<i64, ScanError> {
    if let Some(a) = authors::find_by_name(pool, full_name).await? {
        return Ok(a.id);
    }
    let search = full_name.to_uppercase();
    let lang_code = detect_lang_code(full_name);
    let id = authors::insert(pool, full_name, &search, lang_code).await?;
    Ok(id)
}

/// Find or create a series by name.
async fn ensure_series(pool: &DbPool, ser_name: &str) -> Result<i64, ScanError> {
    if let Some(s) = series::find_by_name(pool, ser_name).await? {
        return Ok(s.id);
    }
    let search = ser_name.to_uppercase();
    let lang_code = detect_lang_code(ser_name);
    let id = series::insert(pool, ser_name, &search, lang_code).await?;
    Ok(id)
}

/// Save cover image bytes to disk as `{covers_dir}/{book_id}.{ext}`.
fn save_cover(covers_dir: &Path, book_id: i64, data: &[u8], mime: &str) -> Result<(), std::io::Error> {
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
