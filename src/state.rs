use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::config::Config;
use crate::db::DbPool;
use crate::web::i18n::Translations;
use dashmap::DashMap;
use serde::Serialize;
use serde::de::DeserializeOwned;

#[derive(Debug, Clone)]
struct CachedValue {
    value: serde_json::Value,
    expires_at: Instant,
}

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub db: DbPool,
    pub tera: Arc<tera::Tera>,
    pub translations: Arc<Translations>,
    pub started_at: Instant,
    pub pdf_preview_tool_available: bool,
    pub djvu_preview_tool_available: bool,
    query_cache: Arc<DashMap<String, CachedValue>>,
}

impl AppState {
    pub fn new(
        config: Config,
        db: DbPool,
        tera: tera::Tera,
        translations: Translations,
        pdf_preview_tool_available: bool,
        djvu_preview_tool_available: bool,
    ) -> Self {
        Self {
            config: Arc::new(config),
            db,
            tera: Arc::new(tera),
            translations: Arc::new(translations),
            started_at: Instant::now(),
            pdf_preview_tool_available,
            djvu_preview_tool_available,
            query_cache: Arc::new(DashMap::new()),
        }
    }

    pub fn get_cached<T: DeserializeOwned>(&self, key: &str) -> Option<T> {
        let cached_value = {
            let entry = self.query_cache.get(key)?;
            if Instant::now() > entry.expires_at {
                None
            } else {
                Some(entry.value.clone())
            }
        };

        match cached_value {
            Some(value) => serde_json::from_value(value).ok(),
            None => {
                self.query_cache.remove(key);
                None
            }
        }
    }

    pub fn set_cached<T: Serialize>(&self, key: impl Into<String>, ttl: Duration, value: &T) {
        let Ok(serialized) = serde_json::to_value(value) else {
            return;
        };

        self.query_cache.insert(
            key.into(),
            CachedValue {
                value: serialized,
                expires_at: Instant::now() + ttl,
            },
        );
    }
}
