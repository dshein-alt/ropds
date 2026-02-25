use serde::Deserialize;
use std::path::{Path, PathBuf};

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

#[derive(Debug, Clone, Deserialize, Default)]
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

#[derive(Debug, Clone, Deserialize)]
pub struct ReaderConfig {
    /// Master switch for the embedded reader feature.
    #[serde(default = "default_true")]
    pub enable: bool,
    /// Maximum number of reading positions stored per user (default 100).
    #[serde(default = "default_read_history_max")]
    pub read_history_max: i64,
}

impl Default for ReaderConfig {
    fn default() -> Self {
        Self {
            enable: true,
            read_history_max: default_read_history_max(),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_minimal_config() {
        let toml_str = r#"
[server]
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
        assert_eq!(config.opds.max_items, 30);
        assert!(config.opds.auth_required);
        assert_eq!(config.web.language, "en");
        assert!(config.reader.enable);
        assert_eq!(config.reader.read_history_max, 100);
    }

    #[test]
    fn test_parse_full_config() {
        let toml_str = r#"
[server]
host = "127.0.0.1"
port = 9090
log_level = "debug"

[library]
root_path = "/media/books"
book_extensions = ["fb2", "epub"]
scan_zip = false
zip_codepage = "utf-8"
inpx_enable = true

[database]
url = "sqlite://my.db"

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
        assert_eq!(config.covers.covers_path, PathBuf::from("/tmp/covers"));
        assert_eq!(config.covers.cover_max_dimension_px, 512);
        assert_eq!(config.covers.cover_jpeg_quality, 80);
        assert!(!config.covers.show_covers);
        assert_eq!(config.library.root_path, PathBuf::from("/media/books"));
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
    fn test_parse_legacy_cover_options_in_library_and_opds() {
        let toml_str = r#"
[server]
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
}
