use sqlx::FromRow;

#[derive(Debug, Clone, FromRow)]
pub struct Catalog {
    pub id: i64,
    pub parent_id: Option<i64>,
    pub path: String,
    pub cat_name: String,
    pub cat_type: i32,
}

#[derive(Debug, Clone, FromRow)]
pub struct Book {
    pub id: i64,
    pub catalog_id: i64,
    pub filename: String,
    pub path: String,
    pub format: String,
    pub title: String,
    pub search_title: String,
    pub annotation: String,
    pub docdate: String,
    pub lang: String,
    pub lang_code: i32,
    pub size: i64,
    pub avail: i32,
    pub cat_type: i32,
    pub cover: i32,
    pub cover_type: String,
    pub reg_date: String,
}

#[derive(Debug, Clone, FromRow)]
pub struct Author {
    pub id: i64,
    pub full_name: String,
    pub search_full_name: String,
    pub lang_code: i32,
}

#[derive(Debug, Clone, FromRow)]
pub struct Genre {
    pub id: i64,
    pub code: String,
    pub section: String,
    pub subsection: String,
}

#[derive(Debug, Clone, FromRow)]
pub struct Series {
    pub id: i64,
    pub ser_name: String,
    pub search_ser: String,
    pub lang_code: i32,
}

#[derive(Debug, Clone, FromRow)]
pub struct BookAuthor {
    pub id: i64,
    pub book_id: i64,
    pub author_id: i64,
}

#[derive(Debug, Clone, FromRow)]
pub struct BookGenre {
    pub id: i64,
    pub book_id: i64,
    pub genre_id: i64,
}

#[derive(Debug, Clone, FromRow)]
pub struct BookSeries {
    pub id: i64,
    pub book_id: i64,
    pub series_id: i64,
    pub ser_no: i32,
}

#[derive(Debug, Clone, FromRow)]
pub struct User {
    pub id: i64,
    pub username: String,
    pub password_hash: String,
    pub is_superuser: i32,
    pub created_at: String,
}

#[derive(Debug, Clone, FromRow)]
pub struct Bookshelf {
    pub id: i64,
    pub user_id: i64,
    pub book_id: i64,
    pub read_time: String,
}

#[derive(Debug, Clone, FromRow)]
pub struct Counter {
    pub name: String,
    pub value: i64,
    pub updated_at: String,
}

// Constants for cat_type values
pub const CAT_NORMAL: i32 = 0;
pub const CAT_ZIP: i32 = 1;
pub const CAT_INPX: i32 = 2;
pub const CAT_INP: i32 = 3;

// Constants for avail values
pub const AVAIL_DELETED: i32 = 0;
pub const AVAIL_UNVERIFIED: i32 = 1;
pub const AVAIL_CONFIRMED: i32 = 2;

// Constants for lang_code values
pub const LANG_CYRILLIC: i32 = 1;
pub const LANG_LATIN: i32 = 2;
pub const LANG_DIGIT: i32 = 3;
pub const LANG_OTHER: i32 = 9;

// Counter name constants
pub const COUNTER_ALL_BOOKS: &str = "allbooks";
pub const COUNTER_ALL_CATALOGS: &str = "allcatalogs";
pub const COUNTER_ALL_AUTHORS: &str = "allauthors";
pub const COUNTER_ALL_GENRES: &str = "allgenres";
pub const COUNTER_ALL_SERIES: &str = "allseries";
