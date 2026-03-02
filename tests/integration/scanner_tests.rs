use ropds::db;
use ropds::db::models::AvailStatus;
use ropds::db::queries::{authors, books, counters, genres, series};
use ropds::scanner;
use std::io::Write;

use super::*;

/// Scan a temp library with one file of each format and verify books are added.
#[tokio::test]
async fn scan_adds_books_from_files() {
    let _lock = SCAN_MUTEX.lock().await;

    let pool = db::create_test_pool().await;
    let lib_dir = tempfile::tempdir().unwrap();
    let covers_dir = tempfile::tempdir().unwrap();
    let config = test_config(lib_dir.path(), covers_dir.path());

    copy_test_files(
        lib_dir.path(),
        &[
            "test_book.fb2",
            "test_book.epub",
            "test_book.pdf",
            "test_book.djvu",
            "test_book.mobi",
        ],
    );

    let stats = scanner::run_scan(&pool, &config).await.unwrap();
    assert_eq!(stats.books_added, 5, "should add 5 books");
    assert_eq!(stats.books_skipped, 0, "nothing to skip on first scan");

    // Verify counters
    counters::update_all(&pool).await.unwrap();
    let all = counters::get_all(&pool).await.unwrap();
    let allbooks = all.iter().find(|c| c.name == "allbooks").unwrap().value;
    assert_eq!(allbooks, 5);
}

/// Books inside ZIP archives are scanned.
#[tokio::test]
async fn scan_adds_books_from_zip() {
    let _lock = SCAN_MUTEX.lock().await;

    let pool = db::create_test_pool().await;
    let lib_dir = tempfile::tempdir().unwrap();
    let covers_dir = tempfile::tempdir().unwrap();
    let config = test_config(lib_dir.path(), covers_dir.path());

    copy_test_files(lib_dir.path(), &["test_book.fb2.zip", "test_book.pdf.zip"]);

    let stats = scanner::run_scan(&pool, &config).await.unwrap();
    assert_eq!(stats.books_added, 2, "should extract books from 2 ZIPs");
    assert_eq!(stats.archives_scanned, 2);

    // Verify the extracted books have cat_type = Zip (1)
    let all_books: Vec<_> = sqlx::query_as::<_, (i32,)>("SELECT cat_type FROM books")
        .fetch_all(pool.inner())
        .await
        .unwrap();
    assert!(
        all_books.iter().all(|(ct,)| *ct == 1),
        "all should be cat_type=Zip"
    );
}

/// INPX processing enriches records from referenced ZIP entries (annotation + cover).
#[tokio::test]
async fn scan_inpx_enriches_annotation_and_cover_from_zip() {
    let _lock = SCAN_MUTEX.lock().await;

    let pool = db::create_test_pool().await;
    let lib_dir = tempfile::tempdir().unwrap();
    let covers_dir = tempfile::tempdir().unwrap();
    let mut config = test_config(lib_dir.path(), covers_dir.path());
    config.library.inpx_enable = true;

    let fb2_bytes = std::fs::read(test_data_dir().join("test_book.fb2")).unwrap();

    // Referenced ZIP archive containing the FB2 file.
    let zip_path = lib_dir.path().join("pack-0001.zip");
    {
        let file = std::fs::File::create(&zip_path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        zip.start_file("test_book.fb2", opts).unwrap();
        zip.write_all(&fb2_bytes).unwrap();
        zip.finish().unwrap();
    }

    // INPX index with one record pointing to pack-0001.zip/test_book.fb2.
    let sep = '\u{0004}';
    let inpx_line = format!(
        "Doe,John{sep}sf_fantasy{sep}INPX Title{sep}INPX Series{sep}1{sep}test_book{sep}{}{sep}lib{sep}0{sep}fb2{sep}2025-01-01{sep}en",
        fb2_bytes.len()
    );
    let inpx_path = lib_dir.path().join("library.inpx");
    {
        let file = std::fs::File::create(&inpx_path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        zip.start_file("pack-0001.inp", opts).unwrap();
        zip.write_all(inpx_line.as_bytes()).unwrap();
        zip.write_all(b"\n").unwrap();
        zip.finish().unwrap();
    }

    let stats = scanner::run_scan(&pool, &config).await.unwrap();
    assert_eq!(stats.books_added, 1);
    assert_eq!(stats.archives_scanned, 1);

    let book = books::find_by_path_and_filename(&pool, "pack-0001.zip", "test_book.fb2")
        .await
        .unwrap()
        .expect("book referenced by INPX should be inserted");
    assert_eq!(book.cover, 1, "cover should be extracted from FB2 in ZIP");
    assert_eq!(
        book.annotation, "This is a test annotation for the book.",
        "annotation should be extracted from FB2 in ZIP"
    );

    let cover_path = scanner::cover_storage_path(covers_dir.path(), book.id, "jpg");
    assert!(
        cover_path.exists(),
        "cover file should be saved to covers dir"
    );
}

/// Missing referenced ZIP during INPX scan should not fail the book insert.
#[tokio::test]
async fn scan_inpx_missing_referenced_zip_keeps_inpx_metadata_only() {
    let _lock = SCAN_MUTEX.lock().await;

    let pool = db::create_test_pool().await;
    let lib_dir = tempfile::tempdir().unwrap();
    let covers_dir = tempfile::tempdir().unwrap();
    let mut config = test_config(lib_dir.path(), covers_dir.path());
    config.library.inpx_enable = true;

    let sep = '\u{0004}';
    let inpx_line = format!(
        "Doe,John{sep}sf_fantasy{sep}INPX Missing ZIP{sep}INPX Series{sep}1{sep}test_book{sep}123{sep}lib{sep}0{sep}fb2{sep}2025-01-01{sep}en"
    );
    let inpx_path = lib_dir.path().join("library.inpx");
    {
        let file = std::fs::File::create(&inpx_path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        zip.start_file("missing-pack.inp", opts).unwrap();
        zip.write_all(inpx_line.as_bytes()).unwrap();
        zip.write_all(b"\n").unwrap();
        zip.finish().unwrap();
    }

    let stats = scanner::run_scan(&pool, &config).await.unwrap();
    assert_eq!(stats.books_added, 1);
    assert_eq!(stats.archives_scanned, 1);

    let book = books::find_by_path_and_filename(&pool, "missing-pack.zip", "test_book.fb2")
        .await
        .unwrap()
        .expect("book should still be inserted from INPX even if ZIP is missing");
    assert_eq!(book.title, "INPX Missing ZIP");
    assert_eq!(book.cover, 0, "missing ZIP means no extracted cover");
    assert_eq!(
        book.annotation, "",
        "missing ZIP means no extracted annotation"
    );
}

/// INPX enrichment should process multiple referenced ZIP archives correctly
/// when scanner workers run in parallel mode.
#[tokio::test]
async fn scan_inpx_enriches_multiple_archives_with_parallel_workers() {
    let _lock = SCAN_MUTEX.lock().await;

    let pool = db::create_test_pool().await;
    let lib_dir = tempfile::tempdir().unwrap();
    let covers_dir = tempfile::tempdir().unwrap();
    let mut config = test_config(lib_dir.path(), covers_dir.path());
    config.library.inpx_enable = true;
    config.scanner.workers_num = 4;

    let fb2_bytes = std::fs::read(test_data_dir().join("test_book.fb2")).unwrap();

    for (zip_name, book_name) in [
        ("pack-0001.zip", "test_book.fb2"),
        ("pack-0002.zip", "second_book.fb2"),
    ] {
        let zip_path = lib_dir.path().join(zip_name);
        let file = std::fs::File::create(&zip_path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        zip.start_file(book_name, opts).unwrap();
        zip.write_all(&fb2_bytes).unwrap();
        zip.finish().unwrap();
    }

    let sep = '\u{0004}';
    let inpx_line_1 = format!(
        "Doe,John{sep}sf_fantasy{sep}INPX Title A{sep}INPX Series{sep}1{sep}test_book{sep}{}{sep}lib{sep}0{sep}fb2{sep}2025-01-01{sep}en",
        fb2_bytes.len()
    );
    let inpx_line_2 = format!(
        "Doe,John{sep}sf_fantasy{sep}INPX Title B{sep}INPX Series{sep}1{sep}second_book{sep}{}{sep}lib{sep}0{sep}fb2{sep}2025-01-01{sep}en",
        fb2_bytes.len()
    );
    let inpx_path = lib_dir.path().join("library.inpx");
    {
        let file = std::fs::File::create(&inpx_path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        zip.start_file("pack-0001.inp", opts).unwrap();
        zip.write_all(inpx_line_1.as_bytes()).unwrap();
        zip.write_all(b"\n").unwrap();
        zip.start_file("pack-0002.inp", opts).unwrap();
        zip.write_all(inpx_line_2.as_bytes()).unwrap();
        zip.write_all(b"\n").unwrap();
        zip.finish().unwrap();
    }

    let stats = scanner::run_scan(&pool, &config).await.unwrap();
    assert_eq!(stats.books_added, 2);
    assert_eq!(stats.archives_scanned, 1);

    for (book_path, filename) in [
        ("pack-0001.zip", "test_book.fb2"),
        ("pack-0002.zip", "second_book.fb2"),
    ] {
        let book = books::find_by_path_and_filename(&pool, book_path, filename)
            .await
            .unwrap()
            .expect("book referenced by INPX should be inserted");
        assert_eq!(book.cover, 1, "cover should be extracted from FB2 in ZIP");
        assert_eq!(
            book.annotation, "This is a test annotation for the book.",
            "annotation should be extracted from FB2 in ZIP"
        );

        let cover_path = scanner::cover_storage_path(covers_dir.path(), book.id, "jpg");
        assert!(
            cover_path.exists(),
            "cover file should be saved to covers dir"
        );
    }
}

/// Second scan of an unchanged library should skip all books.
#[tokio::test]
async fn scan_skips_existing_books() {
    let _lock = SCAN_MUTEX.lock().await;

    let pool = db::create_test_pool().await;
    let lib_dir = tempfile::tempdir().unwrap();
    let covers_dir = tempfile::tempdir().unwrap();
    let config = test_config(lib_dir.path(), covers_dir.path());

    copy_test_files(lib_dir.path(), &["test_book.fb2", "test_book.epub"]);

    let stats1 = scanner::run_scan(&pool, &config).await.unwrap();
    assert_eq!(stats1.books_added, 2);

    let stats2 = scanner::run_scan(&pool, &config).await.unwrap();
    assert_eq!(stats2.books_added, 0, "no new books on second scan");
    assert_eq!(stats2.books_skipped, 2, "both books should be skipped");
}

/// Removing a file from disk causes the book to be (logically) deleted on rescan.
#[tokio::test]
async fn scan_deletes_removed_books() {
    let _lock = SCAN_MUTEX.lock().await;

    let pool = db::create_test_pool().await;
    let lib_dir = tempfile::tempdir().unwrap();
    let covers_dir = tempfile::tempdir().unwrap();
    let config = test_config(lib_dir.path(), covers_dir.path());

    copy_test_files(lib_dir.path(), &["test_book.fb2", "test_book.epub"]);
    scanner::run_scan(&pool, &config).await.unwrap();

    // Remove one file
    std::fs::remove_file(lib_dir.path().join("test_book.fb2")).unwrap();

    let stats = scanner::run_scan(&pool, &config).await.unwrap();
    assert_eq!(stats.books_deleted, 1, "one book should be deleted");
    assert_eq!(stats.books_skipped, 1, "one book should remain");

    // The deleted book should have avail=0
    let deleted: Vec<(i32,)> = sqlx::query_as("SELECT avail FROM books WHERE avail = 0")
        .fetch_all(pool.inner())
        .await
        .unwrap();
    assert_eq!(deleted.len(), 1);
}

/// Various FB2 metadata combinations are parsed correctly.
#[tokio::test]
async fn scan_handles_metadata_variants() {
    let _lock = SCAN_MUTEX.lock().await;

    let pool = db::create_test_pool().await;
    let lib_dir = tempfile::tempdir().unwrap();
    let covers_dir = tempfile::tempdir().unwrap();
    let config = test_config(lib_dir.path(), covers_dir.path());

    copy_test_files(
        lib_dir.path(),
        &[
            "test_book.fb2",       // full: 2 authors, 2 genres, series, cover
            "title_only.fb2",      // title only
            "author_no_genre.fb2", // author + title, no genre
            "no_cover.fb2",        // genre + series, no cover
            "series_no_genre.fb2", // series + cover, no genre
        ],
    );

    let stats = scanner::run_scan(&pool, &config).await.unwrap();
    assert_eq!(stats.books_added, 5);

    // test_book.fb2 — full metadata
    let full = books::find_by_path_and_filename(&pool, "", "test_book.fb2")
        .await
        .unwrap()
        .expect("test_book.fb2 should exist");
    assert_eq!(full.title, "Test Book Title");
    assert_eq!(full.cover, 1, "should have a cover");
    let full_authors = authors::get_for_book(&pool, full.id).await.unwrap();
    assert_eq!(full_authors.len(), 2);
    let full_genres = genres::get_for_book(&pool, full.id, "en").await.unwrap();
    assert_eq!(full_genres.len(), 2);
    let full_series = series::get_for_book(&pool, full.id).await.unwrap();
    assert_eq!(full_series.len(), 1);
    assert_eq!(full_series[0].0.ser_name, "Test Series");

    // title_only.fb2 — no author, no genre, no series, no cover
    let title_only = books::find_by_path_and_filename(&pool, "", "title_only.fb2")
        .await
        .unwrap()
        .expect("title_only.fb2 should exist");
    assert_eq!(title_only.title, "Lonely Title Book");
    assert_eq!(title_only.cover, 0);
    let to_authors = authors::get_for_book(&pool, title_only.id).await.unwrap();
    // Books without authors get "Unknown" as author
    assert_eq!(to_authors.len(), 1);
    assert_eq!(to_authors[0].full_name, "Unknown");

    // no_cover.fb2 — genre + series, no cover
    let no_cover = books::find_by_path_and_filename(&pool, "", "no_cover.fb2")
        .await
        .unwrap()
        .expect("no_cover.fb2 should exist");
    assert_eq!(no_cover.title, "No Cover Book");
    assert_eq!(no_cover.cover, 0);
    let nc_series = series::get_for_book(&pool, no_cover.id).await.unwrap();
    assert_eq!(nc_series.len(), 1);
    assert_eq!(nc_series[0].0.ser_name, "Coverless Series");

    // series_no_genre.fb2 — series + cover, no genre
    let sng = books::find_by_path_and_filename(&pool, "", "series_no_genre.fb2")
        .await
        .unwrap()
        .expect("series_no_genre.fb2 should exist");
    assert_eq!(sng.title, "Series Without Genre");
    assert_eq!(sng.cover, 1);
    let sng_genres = genres::get_for_book(&pool, sng.id, "en").await.unwrap();
    assert_eq!(sng_genres.len(), 0, "no genres expected");
}

/// Scanning the same file twice doesn't create duplicate DB rows.
#[tokio::test]
async fn scan_duplicate_path_detection() {
    let _lock = SCAN_MUTEX.lock().await;

    let pool = db::create_test_pool().await;
    let lib_dir = tempfile::tempdir().unwrap();
    let covers_dir = tempfile::tempdir().unwrap();
    let config = test_config(lib_dir.path(), covers_dir.path());

    copy_test_files(lib_dir.path(), &["test_book.fb2"]);

    scanner::run_scan(&pool, &config).await.unwrap();
    scanner::run_scan(&pool, &config).await.unwrap();

    let count: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM books WHERE filename = 'test_book.fb2'")
            .fetch_one(pool.inner())
            .await
            .unwrap();
    assert_eq!(count.0, 1, "should have exactly one row, not a duplicate");
}

/// EPUB metadata is parsed correctly.
#[tokio::test]
async fn scan_epub_metadata() {
    let _lock = SCAN_MUTEX.lock().await;

    let pool = db::create_test_pool().await;
    let lib_dir = tempfile::tempdir().unwrap();
    let covers_dir = tempfile::tempdir().unwrap();
    let config = test_config(lib_dir.path(), covers_dir.path());

    copy_test_files(lib_dir.path(), &["test_book.epub"]);

    scanner::run_scan(&pool, &config).await.unwrap();

    let book = books::find_by_path_and_filename(&pool, "", "test_book.epub")
        .await
        .unwrap()
        .expect("test_book.epub should exist");
    assert_eq!(book.title, "EPUB Test Book");
    assert_eq!(book.cover, 1, "should have a cover");

    let book_authors = authors::get_for_book(&pool, book.id).await.unwrap();
    assert_eq!(book_authors.len(), 2);

    let book_series = series::get_for_book(&pool, book.id).await.unwrap();
    assert_eq!(book_series.len(), 1);
    assert_eq!(book_series[0].0.ser_name, "EPUB Test Series");
    assert_eq!(book_series[0].1, 2); // series index
}

/// MOBI metadata is parsed correctly.
#[tokio::test]
async fn scan_mobi_metadata() {
    let _lock = SCAN_MUTEX.lock().await;

    let pool = db::create_test_pool().await;
    let lib_dir = tempfile::tempdir().unwrap();
    let covers_dir = tempfile::tempdir().unwrap();
    let config = test_config(lib_dir.path(), covers_dir.path());

    copy_test_files(lib_dir.path(), &["test_book.mobi"]);

    scanner::run_scan(&pool, &config).await.unwrap();

    let book = books::find_by_path_and_filename(&pool, "", "test_book.mobi")
        .await
        .unwrap()
        .expect("test_book.mobi should exist");
    assert_eq!(book.title, "Test MOBI Book");
    assert_eq!(book.cover, 1, "should have a cover");

    let book_authors = authors::get_for_book(&pool, book.id).await.unwrap();
    assert_eq!(book_authors.len(), 1);
    assert_eq!(book_authors[0].full_name, "J. R. R. Tolkien");

    // MOBI has no genres or series
    let book_genres = genres::get_for_book(&pool, book.id, "en").await.unwrap();
    assert_eq!(book_genres.len(), 0, "MOBI has no genre metadata");
    let book_series = series::get_for_book(&pool, book.id).await.unwrap();
    assert_eq!(book_series.len(), 0, "MOBI has no series metadata");
}

/// Cyrillic, digit-prefixed, and symbol-prefixed titles get correct lang_code.
#[tokio::test]
async fn scan_lang_code_variants() {
    let _lock = SCAN_MUTEX.lock().await;

    let pool = db::create_test_pool().await;
    let lib_dir = tempfile::tempdir().unwrap();
    let covers_dir = tempfile::tempdir().unwrap();
    let config = test_config(lib_dir.path(), covers_dir.path());

    copy_test_files(
        lib_dir.path(),
        &[
            "test_book.fb2",     // Latin title → lang_code=2
            "cyrillic_book.fb2", // Cyrillic title → lang_code=1
            "digit_title.fb2",   // Digit-prefixed title → lang_code=3
            "quoted_title.fb2",  // Symbol-prefixed title → lang_code=9
        ],
    );

    let stats = scanner::run_scan(&pool, &config).await.unwrap();
    assert_eq!(stats.books_added, 4);

    // Latin title → lang_code=2
    let latin = books::find_by_path_and_filename(&pool, "", "test_book.fb2")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(latin.lang_code, 2, "Latin title should have lang_code=2");

    // Cyrillic title → lang_code=1
    let cyrillic = books::find_by_path_and_filename(&pool, "", "cyrillic_book.fb2")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(cyrillic.title, "Тайна старого дома");
    assert_eq!(
        cyrillic.lang_code, 1,
        "Cyrillic title should have lang_code=1"
    );
    let cyr_authors = authors::get_for_book(&pool, cyrillic.id).await.unwrap();
    assert_eq!(cyr_authors.len(), 1);
    assert_eq!(cyr_authors[0].full_name, "Иванов Пётр");
    let cyr_series = series::get_for_book(&pool, cyrillic.id).await.unwrap();
    assert_eq!(cyr_series.len(), 1);
    assert_eq!(cyr_series[0].0.ser_name, "Серия расследований");

    // Digit-prefixed title → lang_code=3
    let digit = books::find_by_path_and_filename(&pool, "", "digit_title.fb2")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(digit.title, "451 Degree");
    assert_eq!(digit.lang_code, 3, "digit title should have lang_code=3");

    // Symbol-prefixed title → lang_code=9
    let quoted = books::find_by_path_and_filename(&pool, "", "quoted_title.fb2")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(quoted.title, "\"Цитата\" и другие рассказы");
    assert_eq!(
        quoted.lang_code, 9,
        "symbol-prefixed title should have lang_code=9"
    );
}

/// Books added by scan have avail = Confirmed (2).
#[tokio::test]
async fn scan_books_have_confirmed_status() {
    let _lock = SCAN_MUTEX.lock().await;

    let pool = db::create_test_pool().await;
    let lib_dir = tempfile::tempdir().unwrap();
    let covers_dir = tempfile::tempdir().unwrap();
    let config = test_config(lib_dir.path(), covers_dir.path());

    copy_test_files(lib_dir.path(), &["test_book.fb2"]);
    scanner::run_scan(&pool, &config).await.unwrap();

    let book = books::find_by_path_and_filename(&pool, "", "test_book.fb2")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(book.avail, AvailStatus::Confirmed as i32);
}
