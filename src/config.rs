use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub server: ServerConfig,
    pub library: LibraryConfig,
    pub database: DatabaseConfig,
    pub opds: OpdsConfig,
    pub scanner: ScannerConfig,
    #[serde(default)]
    pub web: WebConfig,
    #[serde(default)]
    pub upload: UploadConfig,
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
    #[serde(default = "default_covers_path")]
    pub covers_path: PathBuf,
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
    #[serde(default = "default_true")]
    pub show_covers: bool,
    #[serde(default = "default_true")]
    pub alphabet_menu: bool,
    #[serde(default)]
    pub hide_doubles: bool,
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
        let config: Config = toml::from_str(&content).map_err(|e| ConfigError::Parse {
            path: path.to_path_buf(),
            source: e,
        })?;
        Ok(config)
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
        assert_eq!(config.library.covers_path, PathBuf::from("covers"));
        assert_eq!(config.library.root_path, PathBuf::from("/books"));
        assert_eq!(config.database.url, "sqlite://ropds.db");
        assert_eq!(config.opds.max_items, 30);
        assert!(config.opds.auth_required);
        assert_eq!(config.web.language, "en");
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
covers_path = "/tmp/covers"
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
show_covers = false
alphabet_menu = false
hide_doubles = true

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
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.server.host, "127.0.0.1");
        assert_eq!(config.server.port, 9090);
        assert_eq!(config.library.covers_path, PathBuf::from("/tmp/covers"));
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
    }
}
