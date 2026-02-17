use std::sync::Arc;
use std::time::Instant;

use crate::config::Config;
use crate::db::DbPool;
use crate::web::i18n::Translations;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub db: DbPool,
    pub tera: Arc<tera::Tera>,
    pub translations: Arc<Translations>,
    pub started_at: Instant,
    pub pdf_preview_tool_available: bool,
    pub djvu_preview_tool_available: bool,
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
        }
    }
}
