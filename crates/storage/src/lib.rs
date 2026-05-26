use std::fs;
use std::path::{Path, PathBuf};

use rm_error::{AppError, Result};
use rm_macro_model::Macro;
use tracing::warn;
use uuid::Uuid;

/// Returns the directory holding macro files, given a storage root.
pub fn macros_dir(root: &Path) -> PathBuf {
    root.join("macros")
}

/// Save (or overwrite) a macro to `<root>/macros/<id>.json` via atomic
/// write-then-rename. Creates the directory if missing.
pub fn save_macro(root: &Path, m: &Macro) -> Result<()> {
    let dir = macros_dir(root);
    fs::create_dir_all(&dir).map_err(|source| AppError::Io {
        path: dir.clone(),
        source,
    })?;
    let path = dir.join(format!("{}.json", m.id));
    let tmp = path.with_extension("json.tmp");
    let json = serde_json::to_string_pretty(m)?;
    fs::write(&tmp, json).map_err(|source| AppError::Io {
        path: tmp.clone(),
        source,
    })?;
    fs::rename(&tmp, &path).map_err(|source| AppError::Io {
        path: path.clone(),
        source,
    })?;
    Ok(())
}

/// Load a single macro by id. Returns `MacroNotFound` if no file exists.
pub fn load_macro(root: &Path, id: Uuid) -> Result<Macro> {
    let path = macros_dir(root).join(format!("{id}.json"));
    if !path.exists() {
        return Err(AppError::MacroNotFound(id.to_string()));
    }
    let s = fs::read_to_string(&path).map_err(|source| AppError::Io {
        path: path.clone(),
        source,
    })?;
    Ok(serde_json::from_str(&s)?)
}

/// Load every readable macro from `<root>/macros/`. Malformed files are logged
/// and skipped — never aborted on. Returns an empty vec if the directory is
/// missing.
pub fn load_all(root: &Path) -> Result<Vec<Macro>> {
    let dir = macros_dir(root);
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in fs::read_dir(&dir).map_err(|source| AppError::Io {
        path: dir.clone(),
        source,
    })? {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                warn!(error = %e, "skipping unreadable dir entry");
                continue;
            }
        };
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        match fs::read_to_string(&path) {
            Ok(text) => match serde_json::from_str::<Macro>(&text) {
                Ok(m) => out.push(m),
                Err(e) => {
                    warn!(path = %path.display(), error = %e, "skipping malformed macro file")
                }
            },
            Err(e) => warn!(path = %path.display(), error = %e, "skipping unreadable macro file"),
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

/// Delete a macro by id. No-op if it does not exist.
pub fn delete_macro(root: &Path, id: Uuid) -> Result<()> {
    let path = macros_dir(root).join(format!("{id}.json"));
    if path.exists() {
        fs::remove_file(&path).map_err(|source| AppError::Io { path, source })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rm_macro_model::{KeyCode, Modifier, PlaybackMode, Step, Trigger};
    use tempfile::TempDir;

    fn sample_macro(name: &str) -> Macro {
        let mut m = Macro::new(
            name,
            Trigger::Hotkey {
                key: KeyCode::F1,
                modifiers: vec![Modifier::Ctrl],
            },
            PlaybackMode::Once,
        );
        m.steps.push(Step::KeyPress {
            key: KeyCode::A,
            hold_ms: 100,
        });
        m
    }

    #[test]
    fn save_then_load_macro_roundtrips() {
        let tmp = TempDir::new().unwrap();
        let m = sample_macro("hello");
        save_macro(tmp.path(), &m).unwrap();
        let back = load_macro(tmp.path(), m.id).unwrap();
        assert_eq!(m, back);
    }

    #[test]
    fn load_missing_returns_not_found() {
        let tmp = TempDir::new().unwrap();
        let err = load_macro(tmp.path(), Uuid::new_v4()).unwrap_err();
        assert_eq!(err.kind(), "MacroNotFound");
    }

    #[test]
    fn load_all_empty_when_dir_missing() {
        let tmp = TempDir::new().unwrap();
        assert!(load_all(tmp.path()).unwrap().is_empty());
    }

    #[test]
    fn load_all_skips_malformed() {
        let tmp = TempDir::new().unwrap();
        let m1 = sample_macro("a");
        let m2 = sample_macro("b");
        save_macro(tmp.path(), &m1).unwrap();
        save_macro(tmp.path(), &m2).unwrap();
        // Write a junk file.
        fs::write(macros_dir(tmp.path()).join("garbage.json"), "not json").unwrap();
        // Write a non-json file (should be ignored by extension filter).
        fs::write(macros_dir(tmp.path()).join("readme.txt"), "ignored").unwrap();

        let all = load_all(tmp.path()).unwrap();
        let names: Vec<_> = all.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, vec!["a", "b"]);
    }

    #[test]
    fn delete_removes_file() {
        let tmp = TempDir::new().unwrap();
        let m = sample_macro("toremove");
        save_macro(tmp.path(), &m).unwrap();
        delete_macro(tmp.path(), m.id).unwrap();
        assert!(load_macro(tmp.path(), m.id).is_err());
        // Deleting again is no-op.
        delete_macro(tmp.path(), m.id).unwrap();
    }

    #[test]
    fn save_overwrites_existing() {
        let tmp = TempDir::new().unwrap();
        let mut m = sample_macro("over");
        save_macro(tmp.path(), &m).unwrap();
        m.name = "renamed".into();
        save_macro(tmp.path(), &m).unwrap();
        let back = load_macro(tmp.path(), m.id).unwrap();
        assert_eq!(back.name, "renamed");
    }
}
