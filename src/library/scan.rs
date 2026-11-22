//! Reading the library folder: a walk of every folder under it, listing the mp3 and m4a
//! files with their size and modification time. It only reads.
//!
//! Each folder's files go out as a `Step` as soon as it's read, so tracks appear while a
//! big library is still being walked. The catalog removes tracks that weren't seen only
//! once the walk finishes cleanly (`Catalog::finish_full_scan`).

use super::catalog::{Catalog, Listed};
use super::folder::{display, hidden};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime};

/// What the scan found, for the catalog to record, in order.
#[derive(Debug)]
pub enum Step {
    /// The media files in one folder.
    Files(Vec<Listed>),
    Progress(Progress),
}

#[derive(Clone, Debug, Default)]
pub struct Progress {
    /// Folders read so far, and how many more are known to be waiting.
    pub done: usize,
    pub waiting: usize,
    pub elapsed: Duration,
}

impl Progress {
    /// The bar's position. A walk can't know the total ahead, so this is the share of
    /// the folders found so far that have been read: rough, but it moves.
    pub fn fraction(&self) -> Option<f32> {
        let seen = self.done + self.waiting;
        (seen > 0 && self.done >= 8).then(|| self.done as f32 / seen as f32)
    }

    /// "Reading the library folder · 37 folders · 5,210 tracks".
    pub fn describe(&self, tracks: usize) -> String {
        format!(
            "Reading the library folder · {} folders · {} tracks",
            group_digits(self.done),
            group_digits(tracks)
        )
    }
}

fn group_digits(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

const CANCELLED: &str = "Scan cancelled";
/// Progress reports at most this often.
const REPORT_EVERY: Duration = Duration::from_millis(100);

/// Walk the whole library folder, sending every finding to `send` in order. Returns
/// `Ok` once everything was read (the catalog then drops tracks it didn't see), or an
/// error if the folder, or any folder under it, couldn't be read (the catalog keeps
/// everything, so a folder that's briefly unreadable, like an unmounted drive, doesn't
/// wipe its tracks). Stops early if `cancel` is set.
pub fn sync(root: &Path, cancel: &AtomicBool, send: impl Fn(Step)) -> Result<(), String> {
    std::fs::read_dir(root).map_err(|e| format!("Can't open {}: {e}", display(root)))?;
    let start = Instant::now();
    let mut last_report = start;
    let mut todo: Vec<PathBuf> = vec![root.to_path_buf()];
    let mut progress = Progress::default();
    let mut failed: Vec<String> = Vec::new();
    while let Some(dir) = todo.pop() {
        if cancel.load(Ordering::Relaxed) {
            return Err(CANCELLED.into());
        }
        let rd = match std::fs::read_dir(&dir) {
            Ok(rd) => rd,
            Err(e) => {
                failed.push(format!("{}: {e}", display(&dir)));
                continue;
            }
        };
        let mut files = Vec::new();
        for entry in rd.flatten() {
            let Some(name) = entry.file_name().to_str().map(str::to_string) else {
                continue;
            };
            if hidden(&name) {
                continue;
            }
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            let path = entry.path();
            // Linked folders aren't followed (they can loop); linked files are.
            if kind.is_dir() {
                todo.push(path);
            } else if Catalog::is_media(&name) {
                if let (Ok(meta), Some(rel)) = (std::fs::metadata(&path), relative(root, &path)) {
                    if meta.is_file() {
                        files.push(listed(rel, &meta));
                    }
                }
            }
        }
        progress.done += 1;
        progress.waiting = todo.len();
        if !files.is_empty() {
            send(Step::Files(files));
        }
        if last_report.elapsed() >= REPORT_EVERY {
            last_report = Instant::now();
            progress.elapsed = start.elapsed();
            send(Step::Progress(progress.clone()));
        }
    }
    match failed.len() {
        0 => Ok(()),
        1 => Err(format!("Couldn't read {}", failed[0])),
        n => Err(format!("Couldn't read {n} folders, e.g. {}", failed[0])),
    }
}

/// A file's path inside the library folder, with forward slashes.
fn relative(root: &Path, path: &Path) -> Option<String> {
    let parts: Option<Vec<&str>> = path
        .strip_prefix(root)
        .ok()?
        .components()
        .map(|c| c.as_os_str().to_str())
        .collect();
    Some(parts?.join("/"))
}

fn listed(rel: String, meta: &std::fs::Metadata) -> Listed {
    let since = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
        .unwrap_or_default();
    Listed {
        id: rel.clone(),
        path: rel,
        // Size and modification time stand in for a content hash: reading every file
        // to hash it would take as long as analyzing it.
        hash: format!("{}-{}", meta.len(), since.as_nanos()),
        size: meta.len(),
        modified: iso8601(since.as_secs()),
    }
}

/// Unix seconds as `YYYY-MM-DDTHH:MM:SSZ`, which sorts as text.
fn iso8601(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + (month <= 2) as i64;
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem / 60 % 60,
        rem % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[test]
    fn dates_sort_as_text() {
        assert_eq!(iso8601(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso8601(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(iso8601(1_798_761_599), "2026-12-31T23:59:59Z");
    }

    #[test]
    fn walks_every_folder_for_media_files() {
        let d = std::env::temp_dir().join(format!("polymoire-scan-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        for dir in ["A/Album", "B", ".Trash"] {
            std::fs::create_dir_all(d.join(dir)).unwrap();
        }
        for f in [
            "A/Album/01 One.mp3",
            "A/Album/02 Two.M4A",
            "A/Album/cover.jpg",
            "A/Album/._01 One.mp3",
            "B/Loose.mp3",
            ".Trash/Old.mp3",
        ] {
            std::fs::write(d.join(f), b"x").unwrap();
        }
        let found = Mutex::new(Vec::new());
        let res = sync(&d, &AtomicBool::new(false), |s| {
            if let Step::Files(f) = s {
                found.lock().unwrap().extend(f.into_iter().map(|l| l.id));
            }
        });
        assert!(res.is_ok(), "{res:?}");
        let mut found = found.into_inner().unwrap();
        found.sort();
        assert_eq!(
            found,
            ["A/Album/01 One.mp3", "A/Album/02 Two.M4A", "B/Loose.mp3"]
        );
        assert!(sync(&d.join("nope"), &AtomicBool::new(false), |_| {}).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }
}
