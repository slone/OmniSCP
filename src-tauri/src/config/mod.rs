pub mod credentials;
pub mod exchange;

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{OmniError, Result};

use crate::protocols::ProtocolType;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum AuthMethod {
    Password,
    KeyFile { path: String },
    KeyWithPassphrase { path: String },
    Agent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    /// Missing in hand-written import files; a fresh one is given on import anyway.
    #[serde(default)]
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub protocol: ProtocolType,
    pub username: String,
    pub auth_method: AuthMethod,
    pub remote_directory: Option<String>,
    pub description: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    /// Folder shown in the profile list; one level, `None` for ungrouped.
    #[serde(default)]
    pub group: Option<String>,
    /// The password or passphrase is kept in the OS credential store (see `credentials`).
    #[serde(default)]
    pub save_password: bool,
}

pub const MIN_CONCURRENT_TRANSFERS: u32 = 1;
pub const MAX_CONCURRENT_TRANSFERS: u32 = 4;

/// What to do when a manual transfer meets an existing name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DefaultConflict {
    Ask,
    Overwrite,
    Skip,
    Rename,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    System,
    Dark,
    Light,
}

/// Application settings, saved as `settings.json` in the config directory.
/// Missing fields take their default, so older files keep loading.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub language: String,
    pub theme: Theme,
    /// Transfers running at once, each on its own session. Applies from the next connection.
    pub concurrent_transfers: u32,
    pub show_hidden_files: bool,
    pub default_conflict: DefaultConflict,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            language: "fr".to_string(),
            theme: Theme::System,
            concurrent_transfers: 2,
            show_hidden_files: true,
            default_conflict: DefaultConflict::Ask,
        }
    }
}

impl AppConfig {
    /// Brings hand-edited or out-of-range values back within what the app supports.
    pub fn sanitized(mut self) -> Self {
        self.concurrent_transfers = self
            .concurrent_transfers
            .clamp(MIN_CONCURRENT_TRANSFERS, MAX_CONCURRENT_TRANSFERS);
        self
    }

    /// Defaults when the file is missing; an unreadable file is an error, so it is never overwritten silently.
    pub fn load(path: &Path) -> Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(data) => serde_json::from_str::<Self>(&data)
                .map(Self::sanitized)
                .map_err(|e| OmniError::Config(format!("{}: {e}", path.display()))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(OmniError::io_context(path.display(), e)),
        }
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| OmniError::io_context(dir.display(), e))?;
        }
        let data = serde_json::to_string_pretty(self).map_err(|e| OmniError::Config(e.to_string()))?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, data).map_err(|e| OmniError::io_context(tmp.display(), e))?;
        std::fs::rename(&tmp, path).map_err(|e| OmniError::io_context(path.display(), e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip_and_tolerate_old_or_odd_files() {
        let dir = std::env::temp_dir().join(format!("omniscp-settings-{}", std::process::id()));
        let path = dir.join("settings.json");
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(AppConfig::load(&path).unwrap(), AppConfig::default());

        let custom = AppConfig {
            theme: Theme::Light,
            concurrent_transfers: 3,
            show_hidden_files: false,
            default_conflict: DefaultConflict::Rename,
            ..AppConfig::default()
        };
        custom.save(&path).unwrap();
        assert_eq!(AppConfig::load(&path).unwrap(), custom);

        // A file from before a setting existed, with a value out of range.
        std::fs::write(&path, r#"{"language":"fr","theme":"dark","concurrent_transfers":50}"#).unwrap();
        let loaded = AppConfig::load(&path).unwrap();
        assert_eq!(loaded.theme, Theme::Dark);
        assert_eq!(loaded.concurrent_transfers, MAX_CONCURRENT_TRANSFERS);
        assert!(loaded.show_hidden_files);

        std::fs::write(&path, "not json").unwrap();
        assert!(matches!(AppConfig::load(&path), Err(OmniError::Config(_))));

        std::fs::remove_dir_all(&dir).unwrap();
    }
}
