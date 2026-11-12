//! Dropbox, read-mostly. Sign-in is OAuth with PKCE (no client secret; the app key is
//! public), so the user pastes a one-time code back into the app.
//!
//! **The one write.** Dropbox scopes can't limit writes to a single file, so the app asks
//! for `files.content.write` and enforces the limit here: `write_backup` is the only
//! request that changes anything, and it can only write `BACKUP_NAME` in the library
//! folder. Keep it that way; the website makes this promise.

use super::catalog::Listed;
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub type Result<T> = std::result::Result<T, String>;

pub const BACKUP_NAME: &str = "polymoire-library.json";
const SCOPES: &str = "account_info.read files.metadata.read files.content.read files.content.write";
const API: &str = "https://api.dropboxapi.com";
const CONTENT: &str = "https://content.dropboxapi.com";
/// Largest download accepted (a long DJ mix at 320 kbps is about 150 MB).
const MAX_FILE: u64 = 1 << 30;

/// One shared connection pool. Reads time out if Dropbox stalls; long downloads don't.
fn agent() -> &'static ureq::Agent {
    static AGENT: std::sync::OnceLock<ureq::Agent> = std::sync::OnceLock::new();
    AGENT.get_or_init(|| {
        ureq::Agent::config_builder()
            .timeout_connect(Some(Duration::from_secs(15)))
            .timeout_per_read(Some(Duration::from_secs(30)))
            // Error statuses come back as responses, so Dropbox's explanation can be read.
            .http_status_as_error(false)
            .build()
            .new_agent()
    })
}

fn err(e: impl std::fmt::Display) -> String {
    format!("Dropbox: {e}")
}

/// Turns an error status into a readable message, using Dropbox's own explanation
/// (`error_summary` from the API, `error_description` from sign-in, else the body text).
trait Checked {
    fn checked(self) -> Result<ureq::http::Response<ureq::Body>>;
}

impl Checked for std::result::Result<ureq::http::Response<ureq::Body>, ureq::Error> {
    fn checked(self) -> Result<ureq::http::Response<ureq::Body>> {
        let mut r = self.map_err(err)?;
        let status = r.status().as_u16();
        if status < 400 {
            return Ok(r);
        }
        let text = r.body_mut().read_to_string().unwrap_or_default();
        let why = serde_json::from_str::<Value>(&text)
            .ok()
            .and_then(|v| {
                v["error_summary"]
                    .as_str()
                    .or(v["error_description"].as_str())
                    .map(str::to_string)
            })
            .unwrap_or(text);
        Err(match status {
            401 => format!("Dropbox sign-in expired or lacks permission ({why}). Connect again."),
            409 if why.contains("not_found") => "Not found in Dropbox".into(),
            _ => format!("Dropbox error {status}: {}", why.trim()),
        })
    }
}

/// A PKCE pair: the verifier stays here, the challenge goes in the sign-in link.
pub struct Pkce {
    pub verifier: String,
    pub challenge: String,
}

impl Pkce {
    pub fn new() -> Result<Self> {
        let mut bytes = [0u8; 48];
        getrandom::fill(&mut bytes).map_err(|e| e.to_string())?;
        let verifier = b64url(&bytes);
        let challenge = b64url(&Sha256::digest(verifier.as_bytes()));
        Ok(Pkce {
            verifier,
            challenge,
        })
    }

    pub fn auth_url(&self, app_key: &str) -> String {
        format!(
            "https://www.dropbox.com/oauth2/authorize?client_id={app_key}&response_type=code\
             &code_challenge={}&code_challenge_method=S256&token_access_type=offline&scope={}",
            self.challenge,
            SCOPES.replace(' ', "%20"),
        )
    }

    /// Trade the pasted code for a long-lived refresh token.
    pub fn finish(&self, app_key: &str, code: &str) -> Result<String> {
        let r: Token = agent()
            .post(format!("{API}/oauth2/token"))
            .send_form([
                ("code", code.trim()),
                ("grant_type", "authorization_code"),
                ("code_verifier", &self.verifier),
                ("client_id", app_key),
            ])
            .checked()?
            .body_mut()
            .read_json()
            .map_err(err)?;
        r.refresh_token
            .ok_or("Dropbox didn't return a refresh token".into())
    }
}

#[derive(Deserialize)]
struct Token {
    access_token: String,
    expires_in: Option<u64>,
    refresh_token: Option<String>,
}

pub struct Dropbox {
    app_key: String,
    refresh: String,
    access: Mutex<Option<(String, Instant)>>,
}

impl Dropbox {
    pub fn new(app_key: &str, refresh_token: &str) -> Self {
        Dropbox {
            app_key: app_key.into(),
            refresh: refresh_token.into(),
            access: Mutex::new(None),
        }
    }

    fn token(&self) -> Result<String> {
        let mut g = self.access.lock().map_err(|e| e.to_string())?;
        if let Some((t, until)) = g.as_ref() {
            if Instant::now() < *until {
                return Ok(t.clone());
            }
        }
        let r: Token = agent()
            .post(format!("{API}/oauth2/token"))
            .send_form([
                ("grant_type", "refresh_token"),
                ("refresh_token", &self.refresh),
                ("client_id", &self.app_key),
            ])
            .checked()?
            .body_mut()
            .read_json()
            .map_err(err)?;
        let life = Duration::from_secs(r.expires_in.unwrap_or(3600).saturating_sub(300));
        *g = Some((r.access_token.clone(), Instant::now() + life));
        Ok(r.access_token)
    }

    fn rpc(&self, endpoint: &str, body: Value) -> Result<Value> {
        agent()
            .post(format!("{API}/2/{endpoint}"))
            .header("Authorization", format!("Bearer {}", self.token()?))
            .content_type("application/json")
            .send(body.to_string())
            .checked()?
            .body_mut()
            .read_json()
            .map_err(err)
    }

    /// The signed-in account's display name.
    pub fn account_name(&self) -> Result<String> {
        let v = agent()
            .post(format!("{API}/2/users/get_current_account"))
            .header("Authorization", format!("Bearer {}", self.token()?))
            .send_empty()
            .checked()?
            .body_mut()
            .read_json::<Value>()
            .map_err(err)?;
        Ok(v["name"]["display_name"]
            .as_str()
            .unwrap_or("Dropbox")
            .into())
    }

    /// Subfolders of a folder ("" is the top of the Dropbox), sorted by name.
    pub fn folders(&self, path: &str) -> Result<Vec<String>> {
        let mut out: Vec<String> = self
            .list(path, false)?
            .into_iter()
            .filter(|(folder, _)| *folder)
            .map(|(_, e)| e.path)
            .collect();
        out.sort_by_key(|p| p.to_lowercase());
        Ok(out)
    }

    /// Every file under `root`, recursively.
    pub fn files(&self, root: &str) -> Result<Vec<Listed>> {
        Ok(self
            .list(root, true)?
            .into_iter()
            .filter(|(folder, _)| !folder)
            .map(|(_, e)| e)
            .collect())
    }

    fn list(&self, path: &str, recursive: bool) -> Result<Vec<(bool, Listed)>> {
        let mut out = Vec::new();
        let mut page = self.rpc(
            "files/list_folder",
            json!({ "path": path, "recursive": recursive, "limit": 2000 }),
        )?;
        loop {
            for e in page["entries"].as_array().into_iter().flatten() {
                let s = |k: &str| e[k].as_str().unwrap_or_default().to_string();
                let folder = e[".tag"] == "folder";
                if folder || e[".tag"] == "file" {
                    out.push((
                        folder,
                        Listed {
                            id: s("id"),
                            path: s("path_display"),
                            hash: s("content_hash"),
                            size: e["size"].as_u64().unwrap_or(0),
                            modified: s("server_modified"),
                        },
                    ));
                }
            }
            if page["has_more"] != true {
                return Ok(out);
            }
            let cursor = page["cursor"].clone();
            page = self.rpc("files/list_folder/continue", json!({ "cursor": cursor }))?;
        }
    }

    /// A file's bytes, by id or path. `first` limits it to the first that many bytes
    /// (enough for the tags at the front of most files).
    pub fn download(&self, path: &str, first: Option<u64>) -> Result<Vec<u8>> {
        let mut req = agent()
            .post(format!("{CONTENT}/2/files/download"))
            .header("Authorization", format!("Bearer {}", self.token()?))
            .header("Dropbox-API-Arg", api_arg(&json!({ "path": path })));
        if let Some(n) = first {
            req = req.header("Range", format!("bytes=0-{}", n.saturating_sub(1)));
        }
        req.send_empty()
            .checked()?
            .body_mut()
            .with_config()
            .limit(MAX_FILE)
            .read_to_vec()
            .map_err(err)
    }

    /// The catalog backup in the library folder, if there is one.
    pub fn read_backup(&self, root: &str) -> Result<Option<Vec<u8>>> {
        match self.download(&backup_path(root), None) {
            Ok(b) => Ok(Some(b)),
            Err(e) if e.starts_with("Not found") => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// The app's only write to Dropbox: the catalog backup, in the library folder.
    pub fn write_backup(&self, root: &str, bytes: &[u8]) -> Result<()> {
        let arg = json!({ "path": backup_path(root), "mode": "overwrite", "mute": true });
        agent()
            .post(format!("{CONTENT}/2/files/upload"))
            .header("Authorization", format!("Bearer {}", self.token()?))
            .header("Dropbox-API-Arg", api_arg(&arg))
            .header("Content-Type", "application/octet-stream")
            .send(bytes)
            .checked()?;
        Ok(())
    }
}

pub fn backup_path(root: &str) -> String {
    format!("{}/{BACKUP_NAME}", root.trim_end_matches('/'))
}

/// JSON for the `Dropbox-API-Arg` header, which must be ASCII: everything else is escaped.
fn api_arg(v: &Value) -> String {
    let mut out = String::new();
    for c in v.to_string().chars() {
        if c.is_ascii() {
            out.push(c);
        } else {
            let mut buf = [0u16; 2];
            for u in c.encode_utf16(&mut buf) {
                out.push_str(&format!("\\u{u:04x}"));
            }
        }
    }
    out
}

fn b64url(bytes: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, &b)| n | (b as u32) << (16 - 8 * i));
        for i in 0..=chunk.len() {
            out.push(A[(n >> (18 - 6 * i) & 63) as usize] as char);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_json_is_ascii() {
        let s = api_arg(&json!({ "path": "/Música/Björk 🎵.mp3" }));
        assert!(s.is_ascii());
        let back: Value = serde_json::from_str(&s).unwrap();
        assert_eq!(back["path"], "/Música/Björk 🎵.mp3");
    }

    #[test]
    fn base64url_matches_rfc4648() {
        assert_eq!(b64url(b"f"), "Zg");
        assert_eq!(b64url(b"fo"), "Zm8");
        assert_eq!(b64url(b"foo"), "Zm9v");
        assert_eq!(b64url(&[0xfb, 0xff]), "-_8");
    }

    #[test]
    fn pkce_challenge_is_sha256_of_verifier() {
        // RFC 7636 appendix B.
        let v = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        assert_eq!(
            b64url(&Sha256::digest(v.as_bytes())),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
        let p = Pkce::new().unwrap();
        assert_eq!(p.verifier.len(), 64);
        assert!(p.auth_url("KEY").contains("code_challenge_method=S256"));
    }

    #[test]
    fn the_only_write_is_the_backup_in_the_library_folder() {
        assert_eq!(backup_path("/Music/"), "/Music/polymoire-library.json");
        assert_eq!(backup_path(""), "/polymoire-library.json");
        // Every Dropbox file endpoint this file calls. Uploading is the only write, and
        // only `write_backup` uses it.
        let src = include_str!("dropbox.rs");
        // ("" is this test's own search string.)
        let allowed = [
            "",
            "list_folder",
            "list_folder/continue",
            "download",
            "upload",
        ];
        let mut uploads = 0;
        for (at, _) in src.match_indices("files/") {
            let name: String = src[at + 6..]
                .chars()
                .take_while(|c| c.is_ascii_lowercase() || *c == '_' || *c == '/')
                .collect();
            assert!(
                allowed.contains(&name.as_str()),
                "unexpected endpoint files/{name}"
            );
            uploads += (name == "upload") as usize;
        }
        assert_eq!(uploads, 1);
    }
}
