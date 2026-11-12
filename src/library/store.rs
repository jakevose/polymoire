//! Local files: settings (including the Dropbox sign-in) and the catalog cache, in the
//! platform's per-user app data folder.

use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::path::PathBuf;

/// A Dropbox app key built in at compile time (`POLYMOIRE_DROPBOX_KEY`). PKCE app keys are
/// public. Without one, the sign-in screen asks for a key.
pub const BUILT_IN_KEY: Option<&str> = option_env!("POLYMOIRE_DROPBOX_KEY");

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub app_key: String,
    /// Dropbox refresh token. Stored in plain text in the user's app data folder.
    pub refresh_token: Option<String>,
    /// The library folder in Dropbox ("" is the whole Dropbox).
    pub root: Option<String>,
    /// Media player mode (otherwise the app listens to system audio).
    pub player: bool,
    pub volume: f32,
    pub shuffle: bool,
}

impl Settings {
    pub fn app_key(&self) -> Option<&str> {
        BUILT_IN_KEY
            .filter(|k| !k.is_empty())
            .or(Some(self.app_key.trim()).filter(|k| !k.is_empty()))
    }
}

/// The app data folder: `~/Library/Application Support/Polymoire`, `%APPDATA%\Polymoire`
/// or `$XDG_CONFIG_HOME/polymoire` (`~/.config/polymoire`).
pub fn dir() -> Option<PathBuf> {
    let env = |k: &str| {
        std::env::var_os(k)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    if cfg!(target_os = "macos") {
        env("HOME").map(|h| h.join("Library/Application Support/Polymoire"))
    } else if cfg!(windows) {
        env("APPDATA").map(|a| a.join("Polymoire"))
    } else {
        env("XDG_CONFIG_HOME")
            .or_else(|| env("HOME").map(|h| h.join(".config")))
            .map(|c| c.join("polymoire"))
    }
}

pub fn load<T: DeserializeOwned + Default>(name: &str) -> T {
    dir()
        .and_then(|d| std::fs::read(d.join(name)).ok())
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

/// Write atomically (temp file, then rename) so a crash never leaves half a file.
pub fn save<T: Serialize>(name: &str, value: &T) -> Result<(), String> {
    let d = dir().ok_or("no app data folder")?;
    std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
    let tmp = d.join(format!("{name}.tmp"));
    let bytes = serde_json::to_vec(value).map_err(|e| e.to_string())?;
    std::fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, d.join(name)).map_err(|e| e.to_string())
}

pub const SETTINGS: &str = "settings.json";
pub const CATALOG: &str = "library.json";
