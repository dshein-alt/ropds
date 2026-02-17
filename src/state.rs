use std::sync::Arc;

use crate::config::Config;
use crate::db::DbPool;
use crate::web::i18n::Translations;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub db: DbPool,
    pub tera: Arc<tera::Tera>,
    pub translations: Arc<Translations>,
}

impl AppState {
    pub fn new(
        config: Config,
        db: DbPool,
        tera: tera::Tera,
        translations: Translations,
    ) -> Self {
        Self {
            config: Arc::new(config),
            db,
            tera: Arc::new(tera),
            translations: Arc::new(translations),
        }
    }
}
