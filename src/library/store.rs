//! Local files: settings and the catalog cache, in the platform's per-user app data
//! folder.

use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// The library folder on this computer. (Versions before 0.3 kept a Dropbox folder
    /// in `root`, and a Dropbox app key; both are ignored and dropped on the next save.)
    pub folder: Option<String>,
    /// Media player mode (otherwise the app listens to system audio).
    pub player: bool,
    pub volume: f32,
    pub shuffle: bool,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dropbox_era_settings_keep_only_what_still_applies() {
        let old =
            r#"{"app_key":"k","refresh_token":"t","root":"/Music","player":true,"volume":0.5}"#;
        let s: Settings = serde_json::from_str(old).unwrap();
        assert!(s.player);
        assert_eq!(s.folder, None, "a Dropbox path isn't a local folder");
        let json = serde_json::to_string(&s).unwrap();
        assert!(
            !json.contains("app_key") && !json.contains("root"),
            "{json}"
        );
    }
}
