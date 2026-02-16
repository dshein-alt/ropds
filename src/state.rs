use std::sync::Arc;

use sqlx::SqlitePool;

use crate::config::Config;

/// Shared application state accessible from all Axum handlers.
#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub db: SqlitePool,
}

impl AppState {
    pub fn new(config: Config, db: SqlitePool) -> Self {
        Self {
            config: Arc::new(config),
            db,
        }
    }
}
