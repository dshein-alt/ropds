use std::collections::HashMap;
use std::path::Path;

/// Translations loaded from TOML locale files.
/// Key: locale code ("en", "ru"), Value: parsed TOML as JSON value.
pub type Translations = HashMap<String, serde_json::Value>;

/// Load all `.toml` files from the given directory.
/// Each file stem becomes the locale key (e.g., `en.toml` → "en").
pub fn load_translations(dir: &Path) -> Result<Translations, TranslationError> {
    let mut map = Translations::new();

    let entries = std::fs::read_dir(dir).map_err(|e| TranslationError::Io {
        path: dir.to_path_buf(),
        source: e,
    })?;

    for entry in entries {
        let entry = entry.map_err(|e| TranslationError::Io {
            path: dir.to_path_buf(),
            source: e,
        })?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("toml") {
            continue;
        }
        let locale = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();

        let content = std::fs::read_to_string(&path).map_err(|e| TranslationError::Io {
            path: path.clone(),
            source: e,
        })?;
        let toml_value: toml::Value =
            toml::from_str(&content).map_err(|e| TranslationError::Parse {
                path: path.clone(),
                source: e,
            })?;
        let json_value = serde_json::to_value(&toml_value)
            .map_err(|e| TranslationError::Convert { source: e })?;

        map.insert(locale, json_value);
    }

    if map.is_empty() {
        return Err(TranslationError::Empty {
            path: dir.to_path_buf(),
        });
    }

    Ok(map)
}

/// Get the translation object for a locale, falling back to "en".
pub fn get_locale<'a>(translations: &'a Translations, locale: &str) -> &'a serde_json::Value {
    translations
        .get(locale)
        .or_else(|| translations.get("en"))
        .expect("english locale must exist")
}

#[derive(Debug, thiserror::Error)]
pub enum TranslationError {
    #[error("failed to read locale directory {path}: {source}")]
    Io {
        path: std::path::PathBuf,
        source: std::io::Error,
    },
    #[error("failed to parse locale file {path}: {source}")]
    Parse {
        path: std::path::PathBuf,
        source: toml::de::Error,
    },
    #[error("failed to convert TOML to JSON: {source}")]
    Convert { source: serde_json::Error },
    #[error("no locale files found in {path}")]
    Empty { path: std::path::PathBuf },
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_load_translations_success_and_get_locale_fallback() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("en.toml"),
            "[admin]\nhello = \"Hello\"\n[web]\nlang = \"en\"\n",
        )
        .unwrap();
        fs::write(
            dir.path().join("ru.toml"),
            "[admin]\nhello = \"Привет\"\n[web]\nlang = \"ru\"\n",
        )
        .unwrap();
        fs::write(dir.path().join("README.txt"), "ignored").unwrap();

        let translations = load_translations(dir.path()).unwrap();
        assert!(translations.contains_key("en"));
        assert!(translations.contains_key("ru"));
        assert_eq!(translations.len(), 2);

        let ru = get_locale(&translations, "ru");
        assert_eq!(ru["admin"]["hello"], "Привет");

        let fallback = get_locale(&translations, "de");
        assert_eq!(fallback["admin"]["hello"], "Hello");
    }

    #[test]
    fn test_load_translations_empty_dir_error() {
        let dir = tempdir().unwrap();
        let err = load_translations(dir.path()).unwrap_err();
        match err {
            TranslationError::Empty { path } => assert_eq!(path, dir.path()),
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn test_load_translations_invalid_toml_error() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("en.toml"), "not = [valid toml").unwrap();

        let err = load_translations(dir.path()).unwrap_err();
        match err {
            TranslationError::Parse { path, .. } => assert_eq!(path, dir.path().join("en.toml")),
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn test_load_translations_missing_dir_io_error() {
        let missing = std::path::PathBuf::from("/definitely-missing-locale-dir-for-tests");
        let err = load_translations(&missing).unwrap_err();
        match err {
            TranslationError::Io { path, .. } => assert_eq!(path, missing),
            other => panic!("unexpected error: {other:?}"),
        }
    }
}
