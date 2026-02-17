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
    pub converter: ConverterConfig,
    #[serde(default)]
    pub web: WebConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "default_log_level")]
    pub log_level: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LibraryConfig {
    pub root_path: PathBuf,
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
    #[serde(default = "default_cache_time")]
    pub cache_time: u64,
    #[serde(default = "default_covers_dir")]
    pub covers_dir: PathBuf,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ScannerConfig {
    #[serde(default = "default_schedule_zero")]
    pub schedule_minutes: String,
    #[serde(default = "default_schedule_hours")]
    pub schedule_hours: String,
    #[serde(default = "default_schedule_star")]
    pub schedule_day: String,
    #[serde(default = "default_schedule_star")]
    pub schedule_day_of_week: String,
    #[serde(default = "default_true")]
    pub delete_logical: bool,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ConverterConfig {
    #[serde(default)]
    pub fb2_to_epub: String,
    #[serde(default)]
    pub fb2_to_mobi: String,
    #[serde(default = "default_temp_dir")]
    pub temp_dir: PathBuf,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WebConfig {
    #[serde(default = "default_language")]
    pub language: String,
    #[serde(default = "default_theme")]
    pub theme: String,
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
    "sqlite://sopds.db".to_string()
}

fn default_opds_title() -> String {
    "SimpleOPDS".to_string()
}

fn default_max_items() -> u32 {
    30
}

fn default_split_items() -> u32 {
    300
}

fn default_cache_time() -> u64 {
    600
}

fn default_covers_dir() -> PathBuf {
    PathBuf::from("covers")
}

fn default_schedule_zero() -> String {
    "0".to_string()
}

fn default_schedule_hours() -> String {
    "0,12".to_string()
}

fn default_schedule_star() -> String {
    "*".to_string()
}

fn default_temp_dir() -> PathBuf {
    PathBuf::from("/tmp")
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
        assert_eq!(config.library.root_path, PathBuf::from("/books"));
        assert_eq!(config.database.url, "sqlite://sopds.db");
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
cache_time = 300

[scanner]
schedule_minutes = "30"
schedule_hours = "6"
schedule_day = "1"
schedule_day_of_week = "mon"
delete_logical = false

[converter]
fb2_to_epub = "/usr/bin/fb2epub"
fb2_to_mobi = "/usr/bin/fb2mobi"
temp_dir = "/var/tmp"

[web]
language = "ru"
theme = "dark"
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.server.host, "127.0.0.1");
        assert_eq!(config.server.port, 9090);
        assert_eq!(config.library.root_path, PathBuf::from("/media/books"));
        assert!(!config.library.scan_zip);
        assert!(config.library.inpx_enable);
        assert_eq!(config.opds.title, "My Library");
        assert_eq!(config.opds.max_items, 50);
        assert!(!config.opds.auth_required);
        assert_eq!(config.scanner.schedule_hours, "6");
        assert_eq!(config.converter.fb2_to_epub, "/usr/bin/fb2epub");
        assert_eq!(config.web.language, "ru");
        assert_eq!(config.web.theme, "dark");
    }
}
