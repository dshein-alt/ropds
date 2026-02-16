use std::sync::Arc;

use crate::config::Config;
use crate::db::DbPool;

/// Shared application state accessible from all Axum handlers.
#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub db: DbPool,
}

impl AppState {
    pub fn new(config: Config, db: DbPool) -> Self {
        Self {
            config: Arc::new(config),
            db,
        }
    }
}
