use serde::Serialize;
use std::path::PathBuf;

#[derive(thiserror::Error, Debug)]
pub enum AppError {
    #[error("Interception driver is not installed")]
    DriverNotInstalled,

    #[error("Interception driver is installed but not running")]
    DriverNotRunning,

    #[error("Driver I/O failed: {0}")]
    DriverIo(String),

    #[error("Macro not found: {0}")]
    MacroNotFound(String),

    #[error("A recording is already in progress")]
    RecordingActive,

    #[error("I/O error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("Serialization error: {0}")]
    Serde(String),

    #[error("{0}")]
    Other(String),
}

impl From<serde_json::Error> for AppError {
    fn from(e: serde_json::Error) -> Self {
        AppError::Serde(e.to_string())
    }
}

/// Wire-friendly serialization for Tauri (Plan 3).
#[derive(Serialize)]
pub struct WireError {
    pub kind: &'static str,
    pub message: String,
}

impl AppError {
    pub fn kind(&self) -> &'static str {
        match self {
            AppError::DriverNotInstalled => "DriverNotInstalled",
            AppError::DriverNotRunning => "DriverNotRunning",
            AppError::DriverIo(_) => "DriverIo",
            AppError::MacroNotFound(_) => "MacroNotFound",
            AppError::RecordingActive => "RecordingActive",
            AppError::Io { .. } => "Io",
            AppError::Serde(_) => "Serde",
            AppError::Other(_) => "Other",
        }
    }

    pub fn to_wire(&self) -> WireError {
        WireError {
            kind: self.kind(),
            message: self.to_string(),
        }
    }
}

pub type Result<T> = std::result::Result<T, AppError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn driver_not_installed_kind_is_stable() {
        assert_eq!(AppError::DriverNotInstalled.kind(), "DriverNotInstalled");
    }

    #[test]
    fn macro_not_found_renders_name() {
        let e = AppError::MacroNotFound("foo".into());
        assert_eq!(e.to_string(), "Macro not found: foo");
        assert_eq!(e.kind(), "MacroNotFound");
    }

    #[test]
    fn serde_error_converts() {
        let bad: serde_json::Error = serde_json::from_str::<i32>("not json").unwrap_err();
        let app: AppError = bad.into();
        assert_eq!(app.kind(), "Serde");
    }

    #[test]
    fn wire_form_roundtrips_through_json() {
        let e = AppError::DriverIo("device closed".into());
        let wire = e.to_wire();
        let json = serde_json::to_string(&wire).unwrap();
        assert!(json.contains("\"kind\":\"DriverIo\""));
        assert!(json.contains("device closed"));
    }
}
