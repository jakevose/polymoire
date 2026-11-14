//! Local files: settings and the catalog cache, in the platform's per-user app data
//! folder. The Dropbox sign-in lives in the system keychain instead (see `token`).

use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::path::PathBuf;

/// A Dropbox app key built in at compile time (`POLYMOIRE_DROPBOX_KEY`). PKCE app keys are
/// public. Without one, the sign-in screen asks for a key.
pub const BUILT_IN_KEY: Option<&str> = option_env!("POLYMOIRE_DROPBOX_KEY");

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub app_key: String,
    /// Dropbox refresh token, held in memory only. It's saved in the system keychain
    /// (`save_token`), never in this file. Versions before 0.2.1 wrote it here in plain
    /// text, so it is still read once, moved to the keychain and dropped from the file.
    #[serde(skip_serializing)]
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

/// Where the Dropbox refresh token lives: macOS Keychain, Windows Credential Manager, or
/// the Secret Service on Linux (GNOME Keyring, KWallet). Encrypted at rest by the OS and
/// out of the app data folder, so backups and file sync tools don't copy it.
mod keychain {
    pub const SERVICE: &str = "Polymoire";
    pub const ACCOUNT: &str = "dropbox-refresh-token";

    pub fn entry() -> Result<keyring::Entry, String> {
        keyring::Entry::new(SERVICE, ACCOUNT).map_err(|e| e.to_string())
    }
}

/// The saved Dropbox sign-in, if there is one.
pub fn load_token() -> Option<String> {
    keychain::entry()
        .ok()?
        .get_password()
        .ok()
        .filter(|t| !t.is_empty())
}

/// Save the Dropbox sign-in to the keychain. On failure (no keychain service, as on a
/// bare Linux window manager) the sign-in only lasts until the app quits.
pub fn save_token(token: &str) -> Result<(), String> {
    keychain::entry()?
        .set_password(token)
        .map_err(|e| e.to_string())
}

pub fn delete_token() {
    if let Ok(e) = keychain::entry() {
        let _ = e.delete_credential();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_never_write_the_token() {
        let s = Settings {
            refresh_token: Some("secret-token".into()),
            ..Settings::default()
        };
        let json = serde_json::to_string(&s).unwrap();
        assert!(!json.contains("secret-token"), "{json}");
        assert!(!json.contains("refresh_token"), "{json}");
    }

    #[test]
    fn old_settings_files_still_yield_the_token_for_migration() {
        let s: Settings = serde_json::from_str(r#"{"refresh_token":"old"}"#).unwrap();
        assert_eq!(s.refresh_token.as_deref(), Some("old"));
    }
}
