use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum_extra::extract::cookie::{Cookie, CookieJar};
use serde::{Deserialize, Serialize};

use crate::db::models::{Author, Genre};
use crate::db::queries::{authors, books, bookshelf, catalogs, genres, reading_positions, series};
use crate::state::AppState;
use crate::web::context::build_context;
use crate::web::i18n;
use crate::web::pagination::Pagination;

mod bookshelf_handlers;
mod browse_handlers;
mod reader_handlers;
mod shared;

pub use bookshelf_handlers::*;
pub use browse_handlers::*;
pub use reader_handlers::*;
pub use shared::*;

use shared::{build_breadcrumbs, enrich_book, render, sanitize_internal_redirect, session_user_id};

#[cfg(test)]
use bookshelf_handlers::parse_bookshelf_sort;
#[cfg(test)]
use shared::default_m;

include!("views/tests.rs");
