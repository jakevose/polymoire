//! The library folder on this computer: reading its files, the folder picker's listings
//! and the catalog backup.
//!
//! **The one write.** Polymoire only reads your music. The single file it writes in the
//! library folder is `BACKUP_NAME`, the catalog backup, and only `write_backup` writes
//! it. Keep it that way; the website makes this promise, and a test below checks it.

use std::io::Read;
use std::path::{Path, PathBuf};

pub type Result<T> = std::result::Result<T, String>;

pub const BACKUP_NAME: &str = "polymoire-library.json";
/// Largest file read whole (a long DJ mix at 320 kbps is about 150 MB).
const MAX_FILE: u64 = 1 << 30;

/// A path as the app stores and shows it: forward slashes on every OS.
pub fn display(path: &Path) -> String {
    let s = path.to_string_lossy();
    if cfg!(windows) {
        s.replace('\\', "/")
    } else {
        s.into_owned()
    }
}

/// The user's home folder, where the folder picker starts.
pub fn home() -> Option<PathBuf> {
    let var = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    std::env::var_os(var)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

/// A typed path, with a leading `~` meaning the home folder.
pub fn expand(typed: &str) -> PathBuf {
    let t = typed.trim();
    match (t.strip_prefix('~'), home()) {
        (Some(rest), Some(h)) if rest.is_empty() || rest.starts_with(['/', '\\']) => {
            h.join(rest.trim_start_matches(['/', '\\']))
        }
        _ => PathBuf::from(t),
    }
}

/// Hidden files and folders (and macOS's `._` resource-fork files) are skipped.
pub fn hidden(name: &str) -> bool {
    name.starts_with('.')
}

/// Subfolders of a folder, for the picker, sorted by name.
pub fn subfolders(path: &Path) -> Result<Vec<String>> {
    let rd = std::fs::read_dir(path).map_err(|e| format!("Can't open {}: {e}", display(path)))?;
    let mut out: Vec<String> = rd
        .flatten()
        .filter(|e| {
            let name = e.file_name();
            let visible = name.to_str().is_some_and(|n| !hidden(n));
            // Follows links, so a linked music folder can be chosen.
            visible && e.path().is_dir()
        })
        .map(|e| display(&e.path()))
        .collect();
    out.sort_by_key(|p| p.to_lowercase());
    Ok(out)
}

/// The chosen library folder. Tracks are named by their path inside it, with forward
/// slashes, so the catalog (and its backup) still fits if the folder moves or is
/// synced to another computer under a different path.
pub struct Folder {
    root: PathBuf,
}

impl Folder {
    pub fn new(root: &str) -> Self {
        Folder {
            root: PathBuf::from(root),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Where a track (a path inside the folder) is on disk.
    pub fn file(&self, rel: &str) -> PathBuf {
        rel.split('/')
            .filter(|p| !p.is_empty())
            .fold(self.root.clone(), |p, part| p.join(part))
    }

    /// A track's bytes. `first` limits it to the first that many bytes (enough for the
    /// tags at the front of most files).
    pub fn read(&self, rel: &str, first: Option<u64>) -> Result<Vec<u8>> {
        let path = self.file(rel);
        let why = |e: std::io::Error| format!("Can't read {rel}: {e}");
        let f = std::fs::File::open(&path).map_err(why)?;
        let mut out = Vec::new();
        f.take(first.unwrap_or(MAX_FILE))
            .read_to_end(&mut out)
            .map_err(why)?;
        Ok(out)
    }

    /// The catalog backup in the library folder, if there is one.
    pub fn read_backup(&self) -> Result<Option<Vec<u8>>> {
        match std::fs::read(self.root.join(BACKUP_NAME)) {
            Ok(b) => Ok(Some(b)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }

    /// The app's only write to the library folder: the catalog backup. Written to a
    /// temporary name first, then renamed, so a crash never leaves half a backup.
    pub fn write_backup(&self, bytes: &[u8]) -> Result<()> {
        let tmp = self.root.join(format!("{BACKUP_NAME}.tmp"));
        std::fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, self.root.join(BACKUP_NAME)).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("polymoire-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("Artist/Album")).unwrap();
        d
    }

    #[test]
    fn reads_tracks_by_their_path_inside_the_folder() {
        let d = scratch("read");
        std::fs::write(d.join("Artist/Album/01 Song.mp3"), b"0123456789").unwrap();
        std::fs::create_dir_all(d.join(".hidden")).unwrap();
        let f = Folder::new(&display(&d));
        assert_eq!(f.read("Artist/Album/01 Song.mp3", None).unwrap().len(), 10);
        assert_eq!(
            f.read("Artist/Album/01 Song.mp3", Some(4)).unwrap(),
            b"0123"
        );
        assert!(f.read("Artist/missing.mp3", None).is_err());
        assert_eq!(subfolders(&d).unwrap(), [display(&d.join("Artist"))]);
        assert_eq!(f.read_backup().unwrap(), None);
        f.write_backup(b"{}").unwrap();
        assert_eq!(f.read_backup().unwrap().as_deref(), Some(&b"{}"[..]));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn tilde_means_home() {
        if let Some(h) = home() {
            assert_eq!(expand("~"), h);
            assert_eq!(expand("~/Music"), h.join("Music"));
        }
        assert_eq!(expand(" /srv/music "), PathBuf::from("/srv/music"));
    }

    #[test]
    fn the_only_write_is_the_backup() {
        // Every call that could change a file, in the library code. Only `write_backup`
        // here writes (a temp file, renamed over the backup); the scan only reads.
        let writes = [
            "fs::write",
            "fs::rename",
            "fs::remove",
            "fs::create",
            "fs::copy",
            "File::create",
            "OpenOptions",
            "set_permissions",
        ];
        let count = |src: &str| -> usize {
            // Skip this test's own list.
            let src = src.split("#[cfg(test)]").next().unwrap();
            writes.iter().map(|w| src.matches(w).count()).sum()
        };
        assert_eq!(count(include_str!("folder.rs")), 2);
        assert_eq!(count(include_str!("scan.rs")), 0);
        assert_eq!(count(include_str!("media.rs")), 0);
    }
}
