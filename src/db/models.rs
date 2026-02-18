use sqlx::FromRow;

#[derive(Debug, Clone, FromRow, serde::Serialize)]
pub struct Catalog {
    pub id: i64,
    pub parent_id: Option<i64>,
    pub path: String,
    pub cat_name: String,
    pub cat_type: i32,
    pub cat_size: i64,
    pub cat_mtime: String,
}

#[derive(Debug, Clone, FromRow, serde::Serialize)]
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

#[derive(Debug, Clone, FromRow, serde::Serialize)]
pub struct Author {
    pub id: i64,
    pub full_name: String,
    pub search_full_name: String,
    pub lang_code: i32,
}

#[derive(Debug, Clone, FromRow, serde::Serialize)]
pub struct Genre {
    pub id: i64,
    pub code: String,
    pub section: String,
    pub subsection: String,
}

#[derive(Debug, Clone, FromRow, serde::Serialize)]
pub struct Series {
    pub id: i64,
    pub ser_name: String,
    pub search_ser: String,
    pub lang_code: i32,
}

#[derive(Debug, Clone, FromRow, serde::Serialize)]
pub struct User {
    pub id: i64,
    pub username: String,
    pub password_hash: String,
    pub is_superuser: i32,
    pub created_at: String,
    pub last_login: String,
    pub password_change_required: i32,
    pub display_name: String,
    pub allow_upload: i32,
}

#[derive(Debug, Clone, FromRow, serde::Serialize)]
pub struct Counter {
    pub name: String,
    pub value: i64,
    pub updated_at: String,
}

/// Catalog type stored in `catalogs.cat_type` and `books.cat_type`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum CatType {
    Normal = 0,
    Zip = 1,
    Inpx = 2,
    Inp = 3,
}

impl From<CatType> for i32 {
    fn from(value: CatType) -> Self {
        value as i32
    }
}

impl TryFrom<i32> for CatType {
    type Error = ();

    fn try_from(value: i32) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Normal),
            1 => Ok(Self::Zip),
            2 => Ok(Self::Inpx),
            3 => Ok(Self::Inp),
            _ => Err(()),
        }
    }
}

/// Availability status stored in `books.avail`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum AvailStatus {
    Deleted = 0,
    Unverified = 1,
    Confirmed = 2,
}

impl From<AvailStatus> for i32 {
    fn from(value: AvailStatus) -> Self {
        value as i32
    }
}

impl TryFrom<i32> for AvailStatus {
    type Error = ();

    fn try_from(value: i32) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Deleted),
            1 => Ok(Self::Unverified),
            2 => Ok(Self::Confirmed),
            _ => Err(()),
        }
    }
}
