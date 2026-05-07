use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::str::FromStr;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub server: ServerConfig,
    pub library: LibraryConfig,
    #[serde(default)]
    pub covers: CoversConfig,
    pub database: DatabaseConfig,
    pub opds: OpdsConfig,
    pub scanner: ScannerConfig,
    #[serde(default)]
    pub web: WebConfig,
    #[serde(default)]
    pub upload: UploadConfig,
    #[serde(default)]
    pub reader: ReaderConfig,
    #[serde(default)]
    pub oauth: OauthConfig,
    #[serde(default)]
    pub smtp: SmtpConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "default_log_level")]
    pub log_level: String,
    /// HMAC secret for signing session cookies. If empty, a random key is generated at startup.
    #[serde(default)]
    pub session_secret: String,
    /// Session TTL in hours (default 24).
    #[serde(default = "default_session_ttl_hours")]
    pub session_ttl_hours: u64,
    /// Public base URL used for absolute links and OAuth redirect URIs.
    pub base_url: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LibraryConfig {
    pub root_path: PathBuf,
    // Legacy keys kept for backward compatibility with pre-[covers] configs.
    #[serde(default, alias = "covers_dir")]
    pub covers_path: Option<PathBuf>,
    #[serde(default)]
    pub cover_max_dimension_px: Option<u32>,
    #[serde(default)]
    pub cover_jpeg_quality: Option<u8>,
    #[serde(default = "default_book_extensions")]
    pub book_extensions: Vec<String>,
    #[serde(default = "default_true")]
    pub scan_zip: bool,
    #[serde(default = "default_zip_codepage")]
    pub zip_codepage: String,
    #[serde(default)]
    pub inpx_enable: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DatabaseConfig {
    #[serde(default = "default_db_url")]
    pub url: String,
    #[serde(default = "default_db_max_connections")]
    pub max_connections: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OpdsConfig {
    #[serde(default = "default_opds_title")]
    pub title: String,
    #[serde(default)]
    pub subtitle: String,
    #[serde(default = "default_max_items")]
    pub max_items: u32,
    #[serde(default = "default_split_items")]
    pub split_items: u32,
    #[serde(default = "default_true")]
    pub auth_required: bool,
    // Legacy key kept for backward compatibility with pre-[covers] configs.
    #[serde(default)]
    pub show_covers: Option<bool>,
    #[serde(default = "default_true")]
    pub alphabet_menu: bool,
    #[serde(default)]
    pub hide_doubles: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CoversConfig {
    #[serde(default = "default_covers_path", alias = "covers_dir")]
    pub covers_path: PathBuf,
    #[serde(default = "default_cover_max_dimension_px")]
    pub cover_max_dimension_px: u32,
    #[serde(default = "default_cover_jpeg_quality")]
    pub cover_jpeg_quality: u8,
    #[serde(default = "default_true")]
    pub show_covers: bool,
}

const DEFAULT_COVER_SCALE_TO: u32 = 600;
const DEFAULT_COVER_QUALITY: u8 = 85;

/// Validated cover image settings (max dimension and JPEG quality).
#[derive(Debug, Clone, Copy)]
pub struct CoverImageConfig {
    scale_to: u32,
    jpeg_quality: u8,
}

impl CoverImageConfig {
    pub fn new(max_size: u32, quality: u8) -> Self {
        let scale_to = if max_size == 0 {
            DEFAULT_COVER_SCALE_TO
        } else {
            max_size
        };
        let jpeg_quality = if (1..=100).contains(&quality) {
            quality
        } else {
            DEFAULT_COVER_QUALITY
        };
        Self {
            scale_to,
            jpeg_quality,
        }
    }

    pub fn scale_to(&self) -> u32 {
        self.scale_to
    }

    pub fn jpeg_quality(&self) -> u8 {
        self.jpeg_quality
    }
}

impl From<&CoversConfig> for CoverImageConfig {
    fn from(cfg: &CoversConfig) -> Self {
        Self::new(cfg.cover_max_dimension_px, cfg.cover_jpeg_quality)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ScannerConfig {
    /// Minutes to fire at (0..=59). Empty = every minute.
    #[serde(default = "default_schedule_minutes")]
    pub schedule_minutes: Vec<u32>,
    /// Hours to fire at (0..=23). Empty = every hour.
    #[serde(default = "default_schedule_hours")]
    pub schedule_hours: Vec<u32>,
    /// Days of week to fire on (1=Mon..7=Sun, ISO). Empty = every day.
    #[serde(default)]
    pub schedule_day_of_week: Vec<u32>,
    #[serde(default = "default_true")]
    pub delete_logical: bool,
    /// Compare mtime+size to skip unchanged archives (default: false — size-only check).
    #[serde(default)]
    pub skip_unchanged: bool,
    /// Validate ZIP CRC integrity before processing (default: false).
    #[serde(default)]
    pub test_zip: bool,
    /// Verify each file extracts cleanly from archives (default: false).
    #[serde(default)]
    pub test_files: bool,
    /// Parallel scan threads (default: 1 = sequential).
    #[serde(default = "default_workers_num")]
    pub workers_num: usize,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WebConfig {
    #[serde(default = "default_language")]
    pub language: String,
    #[serde(default = "default_theme")]
    pub theme: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UploadConfig {
    /// Master switch for upload functionality.
    #[serde(default)]
    pub allow_upload: bool,
    /// Directory where uploaded books are stored before being moved to root_path.
    #[serde(default)]
    pub upload_path: PathBuf,
    /// Maximum upload file size in megabytes (default 100).
    #[serde(default = "default_max_upload_size_mb")]
    pub max_upload_size_mb: u64,
}

impl Default for UploadConfig {
    fn default() -> Self {
        Self {
            allow_upload: false,
            upload_path: PathBuf::new(),
            max_upload_size_mb: default_max_upload_size_mb(),
        }
    }
}

fn default_cached_books_max() -> i64 {
    5
}

#[derive(Debug, Clone, Deserialize)]
pub struct OfflineReaderConfig {
    /// Maximum number of books cached in the browser for offline reading.
    /// 0 disables offline.
    #[serde(default = "default_cached_books_max")]
    pub cached_books_max: i64,
}

impl Default for OfflineReaderConfig {
    fn default() -> Self {
        Self {
            cached_books_max: default_cached_books_max(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ReaderConfig {
    /// Master switch for the embedded reader feature.
    #[serde(default = "default_true")]
    pub enable: bool,
    /// Maximum number of reading positions stored per user (default 100).
    #[serde(default = "default_read_history_max")]
    pub read_history_max: i64,
    #[serde(default)]
    pub offline: OfflineReaderConfig,
}

impl Default for ReaderConfig {
    fn default() -> Self {
        Self {
            enable: true,
            read_history_max: default_read_history_max(),
            offline: OfflineReaderConfig::default(),
        }
    }
}

impl Default for WebConfig {
    fn default() -> Self {
        Self {
            language: default_language(),
            theme: default_theme(),
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
#[serde(default)]
pub struct OauthConfig {
    pub google_client_id: String,
    pub google_client_secret: String,
    pub yandex_client_id: String,
    pub yandex_client_secret: String,
    pub keycloak_url: String,
    pub keycloak_realm: String,
    pub keycloak_client_id: String,
    pub keycloak_client_secret: String,
    #[serde(default = "default_false")]
    pub keycloak_auto_approve: bool,
    #[serde(default = "default_role_upload")]
    pub keycloak_role_upload: String,
    #[serde(default = "default_role_admin")]
    pub keycloak_role_admin: String,
    #[serde(default = "default_keycloak_button_label")]
    pub keycloak_button_label: String,
    #[serde(default = "default_cooldown")]
    pub rejection_cooldown_hours: u64,
    #[serde(default = "default_false")]
    pub notify_admin_email: bool,
}

impl Default for OauthConfig {
    fn default() -> Self {
        Self {
            google_client_id: String::new(),
            google_client_secret: String::new(),
            yandex_client_id: String::new(),
            yandex_client_secret: String::new(),
            keycloak_url: String::new(),
            keycloak_realm: String::new(),
            keycloak_client_id: String::new(),
            keycloak_client_secret: String::new(),
            keycloak_auto_approve: default_false(),
            keycloak_role_upload: default_role_upload(),
            keycloak_role_admin: default_role_admin(),
            keycloak_button_label: default_keycloak_button_label(),
            rejection_cooldown_hours: default_cooldown(),
            notify_admin_email: default_false(),
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
#[serde(default)]
pub struct SmtpConfig {
    pub host: String,
    #[serde(default = "default_smtp_port")]
    pub port: u16,
    pub username: String,
    pub password: String,
    pub from: String,
    #[serde(default)]
    pub send_to: Vec<String>,
    #[serde(default = "default_true")]
    pub starttls: bool,
}

impl Default for SmtpConfig {
    fn default() -> Self {
        Self {
            host: String::new(),
            port: default_smtp_port(),
            username: String::new(),
            password: String::new(),
            from: String::new(),
            send_to: Vec::new(),
            starttls: default_true(),
        }
    }
}

impl Config {
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let content = std::fs::read_to_string(path).map_err(|e| ConfigError::ReadFile {
            path: path.to_path_buf(),
            source: e,
        })?;
        let mut config: Config = toml::from_str(&content).map_err(|e| ConfigError::Parse {
            path: path.to_path_buf(),
            source: e,
        })?;
        config.apply_legacy_cover_fallbacks();
        config.server.base_url = config.server.base_url.trim().to_string();
        config.validate()?;
        Ok(config)
    }

    fn apply_legacy_cover_fallbacks(&mut self) {
        if self.covers.covers_path == default_covers_path()
            && let Some(path) = self.library.covers_path.clone()
        {
            self.covers.covers_path = path;
        }
        if self.covers.cover_max_dimension_px == default_cover_max_dimension_px()
            && let Some(max_px) = self.library.cover_max_dimension_px
        {
            self.covers.cover_max_dimension_px = max_px;
        }
        if self.covers.cover_jpeg_quality == default_cover_jpeg_quality()
            && let Some(quality) = self.library.cover_jpeg_quality
        {
            self.covers.cover_jpeg_quality = quality;
        }
        if self.covers.show_covers == default_true()
            && let Some(show) = self.opds.show_covers
        {
            self.covers.show_covers = show;
        }
    }

    fn validate(&self) -> Result<(), ConfigError> {
        let base_url = self.server.base_url.trim();
        if base_url.is_empty() {
            return Err(ConfigError::Validation(
                "server.base_url is required and must be non-empty".to_string(),
            ));
        }
        let parsed = reqwest::Url::parse(base_url).map_err(|e| {
            ConfigError::Validation(format!(
                "invalid server.base_url (must be a valid URL): {e}"
            ))
        })?;
        if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
            return Err(ConfigError::Validation(
                "invalid server.base_url (must include http/https scheme and host)".to_string(),
            ));
        }

        if self.database.max_connections == 0 {
            return Err(ConfigError::Validation(
                "database.max_connections must be greater than 0".to_string(),
            ));
        }

        if self.oauth.notify_admin_email {
            if self.smtp.host.trim().is_empty() {
                return Err(ConfigError::Validation(
                    "oauth.notify_admin_email=true requires [smtp].host".to_string(),
                ));
            }
            if self.smtp.from.trim().is_empty() {
                return Err(ConfigError::Validation(
                    "oauth.notify_admin_email=true requires [smtp].from".to_string(),
                ));
            }
            if self.smtp.send_to.is_empty() {
                return Err(ConfigError::Validation(
                    "oauth.notify_admin_email=true requires non-empty [smtp].send_to".to_string(),
                ));
            }

            if lettre::message::Mailbox::from_str(self.smtp.from.trim()).is_err() {
                return Err(ConfigError::Validation(
                    "invalid [smtp].from email address".to_string(),
                ));
            }
            for recipient in &self.smtp.send_to {
                if recipient.trim().is_empty() {
                    return Err(ConfigError::Validation(
                        "empty recipient in [smtp].send_to".to_string(),
                    ));
                }
                if lettre::message::Mailbox::from_str(recipient.trim()).is_err() {
                    return Err(ConfigError::Validation(format!(
                        "invalid recipient in [smtp].send_to: {recipient}"
                    )));
                }
            }
        }

        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("failed to read config file {path}: {source}")]
    ReadFile {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("failed to parse config file {path}: {source}")]
    Parse {
        path: PathBuf,
        source: toml::de::Error,
    },
    #[error("invalid config: {0}")]
    Validation(String),
}

// Default value functions

fn default_host() -> String {
    "0.0.0.0".to_string()
}

fn default_port() -> u16 {
    8081
}

fn default_log_level() -> String {
    "info".to_string()
}

fn default_book_extensions() -> Vec<String> {
    vec!["fb2", "epub", "mobi", "pdf", "djvu", "doc", "docx", "zip"]
        .into_iter()
        .map(String::from)
        .collect()
}

fn default_true() -> bool {
    true
}

fn default_zip_codepage() -> String {
    "cp866".to_string()
}

fn default_db_url() -> String {
    "sqlite://ropds.db".to_string()
}

fn default_db_max_connections() -> u32 {
    5
}

fn default_opds_title() -> String {
    "ROPDS".to_string()
}

fn default_max_items() -> u32 {
    30
}

fn default_split_items() -> u32 {
    300
}

fn default_covers_path() -> PathBuf {
    PathBuf::from("covers")
}

fn default_cover_max_dimension_px() -> u32 {
    600
}

fn default_cover_jpeg_quality() -> u8 {
    85
}

impl Default for CoversConfig {
    fn default() -> Self {
        Self {
            covers_path: default_covers_path(),
            cover_max_dimension_px: default_cover_max_dimension_px(),
            cover_jpeg_quality: default_cover_jpeg_quality(),
            show_covers: default_true(),
        }
    }
}

fn default_schedule_minutes() -> Vec<u32> {
    vec![0]
}

fn default_schedule_hours() -> Vec<u32> {
    vec![0, 12]
}

fn default_session_ttl_hours() -> u64 {
    24
}

fn default_max_upload_size_mb() -> u64 {
    100
}

fn default_workers_num() -> usize {
    1
}

fn default_read_history_max() -> i64 {
    100
}

fn default_language() -> String {
    "en".to_string()
}

fn default_theme() -> String {
    "light".to_string()
}

fn default_false() -> bool {
    false
}

fn default_cooldown() -> u64 {
    24
}

fn default_smtp_port() -> u16 {
    587
}

fn default_role_upload() -> String {
    "ropds_can_upload".to_string()
}

fn default_role_admin() -> String {
    "ropds_admin".to_string()
}

fn default_keycloak_button_label() -> String {
    "Company SSO".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_minimal_config() {
        let toml_str = r#"
[server]
base_url = "http://127.0.0.1:8081"
[library]
root_path = "/books"
[database]
[opds]
[scanner]
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.server.host, "0.0.0.0");
        assert_eq!(config.server.port, 8081);
        assert_eq!(config.covers.covers_path, PathBuf::from("covers"));
        assert_eq!(config.covers.cover_max_dimension_px, 600);
        assert_eq!(config.covers.cover_jpeg_quality, 85);
        assert!(config.covers.show_covers);
        assert_eq!(config.library.root_path, PathBuf::from("/books"));
        assert_eq!(config.database.url, "sqlite://ropds.db");
        assert_eq!(config.database.max_connections, 5);
        assert_eq!(config.opds.max_items, 30);
        assert!(config.opds.auth_required);
        assert_eq!(config.web.language, "en");
        assert!(config.reader.enable);
        assert_eq!(config.reader.read_history_max, 100);
        assert_eq!(config.oauth.keycloak_button_label, "Company SSO");
    }

    #[test]
    fn test_parse_full_config() {
        let toml_str = r#"
[server]
host = "127.0.0.1"
port = 9090
log_level = "debug"
base_url = "http://127.0.0.1:9090"

[library]
root_path = "/media/books"
book_extensions = ["fb2", "epub"]
scan_zip = false
zip_codepage = "utf-8"
inpx_enable = true

[database]
url = "sqlite://my.db"
max_connections = 8

[opds]
title = "My Library"
subtitle = "Home books"
max_items = 50
split_items = 200
auth_required = false
alphabet_menu = false
hide_doubles = true

[covers]
covers_path = "/tmp/covers"
cover_max_dimension_px = 512
cover_jpeg_quality = 80
show_covers = false

[scanner]
schedule_minutes = [30]
schedule_hours = [6]
schedule_day_of_week = [1, 4]
delete_logical = false
skip_unchanged = true
test_zip = true
test_files = true
workers_num = 4

[web]
language = "ru"
theme = "dark"

[reader]
enable = false
read_history_max = 50
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.server.host, "127.0.0.1");
        assert_eq!(config.server.port, 9090);
        assert_eq!(config.server.base_url, "http://127.0.0.1:9090");
        assert_eq!(config.covers.covers_path, PathBuf::from("/tmp/covers"));
        assert_eq!(config.covers.cover_max_dimension_px, 512);
        assert_eq!(config.covers.cover_jpeg_quality, 80);
        assert!(!config.covers.show_covers);
        assert_eq!(config.library.root_path, PathBuf::from("/media/books"));
        assert_eq!(config.database.max_connections, 8);
        assert!(!config.library.scan_zip);
        assert!(config.library.inpx_enable);
        assert_eq!(config.opds.title, "My Library");
        assert_eq!(config.opds.max_items, 50);
        assert!(!config.opds.auth_required);
        assert_eq!(config.scanner.schedule_hours, vec![6]);
        assert!(config.scanner.skip_unchanged);
        assert!(config.scanner.test_zip);
        assert!(config.scanner.test_files);
        assert_eq!(config.scanner.workers_num, 4);
        assert_eq!(config.web.language, "ru");
        assert_eq!(config.web.theme, "dark");
        assert!(!config.reader.enable);
        assert_eq!(config.reader.read_history_max, 50);
    }

    #[test]
    fn test_oauth_config_defaults() {
        let cfg: OauthConfig = toml::from_str("").unwrap();
        assert_eq!(cfg.rejection_cooldown_hours, 24);
        assert!(!cfg.keycloak_auto_approve);
        assert!(!cfg.notify_admin_email);
    }

    #[test]
    fn test_smtp_config_defaults() {
        let cfg: SmtpConfig = toml::from_str("").unwrap();
        assert_eq!(cfg.port, 587);
        assert!(cfg.send_to.is_empty());
        assert!(cfg.starttls);
    }

    #[test]
    fn test_validate_notify_email_requires_smtp_fields() {
        let toml_str = r#"
[server]
base_url = "http://127.0.0.1:8081"
[library]
root_path = "/books"
[database]
[opds]
[scanner]
[oauth]
notify_admin_email = true
[smtp]
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert!(matches!(config.validate(), Err(ConfigError::Validation(_))));
    }

    #[test]
    fn test_validate_notify_email_accepts_valid_smtp() {
        let toml_str = r#"
[server]
base_url = "http://127.0.0.1:8081"
[library]
root_path = "/books"
[database]
[opds]
[scanner]
[oauth]
notify_admin_email = true
[smtp]
host = "smtp.example.com"
from = "ropds@example.com"
send_to = ["admin1@example.com", "Admin Team <admin2@example.com>"]
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_parse_legacy_cover_options_in_library_and_opds() {
        let toml_str = r#"
[server]
base_url = "http://127.0.0.1:8081"
[library]
root_path = "/books"
covers_dir = "/books/covers"
cover_max_dimension_px = 500
cover_jpeg_quality = 70
[database]
[opds]
show_covers = false
[scanner]
"#;
        let mut config: Config = toml::from_str(toml_str).unwrap();
        config.apply_legacy_cover_fallbacks();
        assert_eq!(config.covers.covers_path, PathBuf::from("/books/covers"));
        assert_eq!(config.covers.cover_max_dimension_px, 500);
        assert_eq!(config.covers.cover_jpeg_quality, 70);
        assert!(!config.covers.show_covers);
    }

    #[test]
    fn test_validate_base_url_rejects_empty() {
        let toml_str = r#"
[server]
base_url = "   "
[library]
root_path = "/books"
[database]
[opds]
[scanner]
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert!(matches!(config.validate(), Err(ConfigError::Validation(_))));
    }

    #[test]
    fn test_parse_requires_base_url() {
        let toml_str = r#"
[server]
[library]
root_path = "/books"
[database]
[opds]
[scanner]
"#;
        assert!(toml::from_str::<Config>(toml_str).is_err());
    }

    #[test]
    fn test_validate_base_url_rejects_invalid_url() {
        let toml_str = r#"
[server]
base_url = "not a url"
[library]
root_path = "/books"
[database]
[opds]
[scanner]
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert!(matches!(config.validate(), Err(ConfigError::Validation(_))));
    }

    #[test]
    fn test_validate_rejects_zero_db_max_connections() {
        let toml_str = r#"
[server]
base_url = "http://127.0.0.1:8081"
[library]
root_path = "/books"
[database]
max_connections = 0
[opds]
[scanner]
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert!(matches!(config.validate(), Err(ConfigError::Validation(_))));
    }

    #[test]
    fn test_reader_offline_default_when_section_missing() {
        let toml_src = r#"
[server]
base_url = "http://localhost:8081"
[library]
root_path = "/tmp"
[database]
[opds]
[scanner]
"#;
        let cfg: Config = toml::from_str(toml_src).unwrap();
        assert_eq!(cfg.reader.offline.cached_books_max, 5);
    }

    #[test]
    fn test_reader_offline_section_override() {
        let toml_src = r#"
[server]
base_url = "http://localhost:8081"
[library]
root_path = "/tmp"
[database]
[opds]
[scanner]
[reader.offline]
cached_books_max = 0
"#;
        let cfg: Config = toml::from_str(toml_src).unwrap();
        assert_eq!(cfg.reader.offline.cached_books_max, 0);
    }
}
