use ropds::db;
use ropds::db::models::AvailStatus;
use ropds::db::queries::{authors, books, counters, genres, series};
use ropds::scanner;

use super::*;

/// Scan a temp library with one file of each format and verify books are added.
#[tokio::test]
async fn scan_adds_books_from_files() {
    let _lock = SCAN_MUTEX.lock().await;

    let (pool, _) = db::create_test_pool().await;
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
        ],
    );

    let stats = scanner::run_scan(&pool, &config).await.unwrap();
    assert_eq!(stats.books_added, 4, "should add 4 books");
    assert_eq!(stats.books_skipped, 0, "nothing to skip on first scan");

    // Verify counters
    counters::update_all(&pool).await.unwrap();
    let all = counters::get_all(&pool).await.unwrap();
    let allbooks = all.iter().find(|c| c.name == "allbooks").unwrap().value;
    assert_eq!(allbooks, 4);
}

/// Books inside ZIP archives are scanned.
#[tokio::test]
async fn scan_adds_books_from_zip() {
    let _lock = SCAN_MUTEX.lock().await;

    let (pool, _) = db::create_test_pool().await;
    let lib_dir = tempfile::tempdir().unwrap();
    let covers_dir = tempfile::tempdir().unwrap();
    let config = test_config(lib_dir.path(), covers_dir.path());

    copy_test_files(lib_dir.path(), &["test_book.fb2.zip", "test_book.pdf.zip"]);

    let stats = scanner::run_scan(&pool, &config).await.unwrap();
    assert_eq!(stats.books_added, 2, "should extract books from 2 ZIPs");
    assert_eq!(stats.archives_scanned, 2);

    // Verify the extracted books have cat_type = Zip (1)
    let all_books: Vec<_> = sqlx::query_as::<_, (i32,)>("SELECT cat_type FROM books")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert!(
        all_books.iter().all(|(ct,)| *ct == 1),
        "all should be cat_type=Zip"
    );
}

/// Second scan of an unchanged library should skip all books.
#[tokio::test]
async fn scan_skips_existing_books() {
    let _lock = SCAN_MUTEX.lock().await;

    let (pool, _) = db::create_test_pool().await;
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

    let (pool, _) = db::create_test_pool().await;
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
        .fetch_all(&pool)
        .await
        .unwrap();
    assert_eq!(deleted.len(), 1);
}

/// Various FB2 metadata combinations are parsed correctly.
#[tokio::test]
async fn scan_handles_metadata_variants() {
    let _lock = SCAN_MUTEX.lock().await;

    let (pool, _) = db::create_test_pool().await;
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

    let (pool, _) = db::create_test_pool().await;
    let lib_dir = tempfile::tempdir().unwrap();
    let covers_dir = tempfile::tempdir().unwrap();
    let config = test_config(lib_dir.path(), covers_dir.path());

    copy_test_files(lib_dir.path(), &["test_book.fb2"]);

    scanner::run_scan(&pool, &config).await.unwrap();
    scanner::run_scan(&pool, &config).await.unwrap();

    let count: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM books WHERE filename = 'test_book.fb2'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(count.0, 1, "should have exactly one row, not a duplicate");
}

/// EPUB metadata is parsed correctly.
#[tokio::test]
async fn scan_epub_metadata() {
    let _lock = SCAN_MUTEX.lock().await;

    let (pool, _) = db::create_test_pool().await;
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

/// Cyrillic, digit-prefixed, and symbol-prefixed titles get correct lang_code.
#[tokio::test]
async fn scan_lang_code_variants() {
    let _lock = SCAN_MUTEX.lock().await;

    let (pool, _) = db::create_test_pool().await;
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

    let (pool, _) = db::create_test_pool().await;
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
