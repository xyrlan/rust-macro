//! Persistent app settings. Stored in `{storage_root}/settings.json`.
//! Loaded once at app startup; written on every `save_settings` Tauri call.

use std::path::{Path, PathBuf};

use rm_macro_model::KeyCode;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Settings {
    /// Key that stops an in-app recording. Defaults to F10.
    #[serde(default = "default_stop_key")]
    pub stop_key: KeyCode,

    /// Override for the storage root. When `None`, the app uses
    /// `dirs::data_dir().join("rust-macro")`.
    #[serde(default)]
    pub storage_root_override: Option<PathBuf>,
}

fn default_stop_key() -> KeyCode {
    KeyCode::F10
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            stop_key: default_stop_key(),
            storage_root_override: None,
        }
    }
}

pub fn settings_path(storage_root: &Path) -> PathBuf {
    storage_root.join("settings.json")
}

/// Load settings from `{storage_root}/settings.json`. Returns `Settings::default()`
/// if the file doesn't exist. Any parse error is returned to the caller (don't
/// silently overwrite a corrupt user file).
pub fn load(storage_root: &Path) -> Result<Settings, std::io::Error> {
    let path = settings_path(storage_root);
    if !path.exists() {
        return Ok(Settings::default());
    }
    let bytes = std::fs::read(&path)?;
    let s = serde_json::from_slice(&bytes)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    Ok(s)
}

/// Atomically save settings via write-then-rename.
pub fn save(storage_root: &Path, s: &Settings) -> Result<(), std::io::Error> {
    std::fs::create_dir_all(storage_root)?;
    let path = settings_path(storage_root);
    let tmp = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec_pretty(s)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::write(&tmp, &bytes)?;
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn default_has_f10_stop_key() {
        let s = Settings::default();
        assert_eq!(s.stop_key, KeyCode::F10);
        assert!(s.storage_root_override.is_none());
    }

    #[test]
    fn load_missing_returns_default() {
        let tmp = TempDir::new().unwrap();
        let s = load(tmp.path()).unwrap();
        assert_eq!(s, Settings::default());
    }

    #[test]
    fn save_then_load_roundtrips() {
        let tmp = TempDir::new().unwrap();
        let s = Settings {
            stop_key: KeyCode::Escape,
            storage_root_override: Some(PathBuf::from("/custom/path")),
        };
        save(tmp.path(), &s).unwrap();
        let back = load(tmp.path()).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn load_corrupt_file_returns_err() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("settings.json"), b"{ bogus").unwrap();
        assert!(load(tmp.path()).is_err());
    }
}
