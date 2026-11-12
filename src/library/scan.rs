//! Reading a whole library folder with progress. Dropbox can't say how big a folder is
//! up front, so the scan first walks the top levels (cheap, one request per folder) until
//! it has enough folders to count, then lists each of those recursively on a few threads.
//! "Folders done of total" is the progress; the time left is estimated from it.

use super::catalog::{Catalog, Listed};
use super::dropbox::Dropbox;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Walk down until there are at least this many folders to count...
const ENOUGH_FOLDERS: usize = 24;
/// ...but no more than this many levels.
const MAX_DEPTH: usize = 3;
/// Folders listed at once. Dropbox rate-limits heavier use.
const THREADS: usize = 4;
/// Tries per folder before the scan gives up (rate limits and dropped connections).
const TRIES: u32 = 3;

#[derive(Clone, Debug, Default)]
pub struct Progress {
    /// Folders fully listed, of `total` (0 while the top levels are being walked).
    pub done: usize,
    pub total: usize,
    /// Entries (files and folders) seen so far, and mp3/m4a files among them.
    pub entries: usize,
    pub media: usize,
    pub elapsed: Duration,
}

impl Progress {
    /// Time left, once enough folders are done for a guess to mean something.
    pub fn remaining(&self) -> Option<Duration> {
        if self.done < 3 || self.total <= self.done || self.elapsed < Duration::from_secs(2) {
            return None;
        }
        let per = self.elapsed.as_secs_f64() / self.done as f64;
        Some(Duration::from_secs_f64(
            per * (self.total - self.done) as f64,
        ))
    }

    /// "Reading folder 37 of 412 · 5,210 tracks · about 2 min left".
    pub fn describe(&self) -> String {
        let mut s = if self.total == 0 {
            "Looking through the library folder".to_string()
        } else {
            format!("Reading folders: {} of {}", self.done, self.total)
        };
        s += &format!(" · {} tracks found", group_digits(self.media));
        if let Some(r) = self.remaining() {
            let secs = r.as_secs();
            s += &if secs < 60 {
                " · under a minute left".to_string()
            } else {
                format!(" · about {} min left", secs.div_ceil(60))
            };
        }
        s
    }
}

/// Rate limits, server trouble and dropped connections pass; a refusal won't change.
fn worth_retrying(e: &str) -> bool {
    let refused = e.starts_with("Dropbox sign-in") || e.starts_with("Not found");
    let client = e.starts_with("Dropbox error 4") && !e.starts_with("Dropbox error 429");
    !(refused || client)
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
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

/// Every file under `root`. `progress` hears about each page; `found` gets each batch of
/// files as soon as its folder is done, so the catalog can fill in while the scan runs.
pub fn scan(
    dbx: &Dropbox,
    root: &str,
    progress: impl Fn(&Progress) + Sync,
    found: impl Fn(Vec<Listed>) + Sync,
) -> Result<Vec<Listed>, String> {
    let start = Instant::now();
    let state = Mutex::new(Progress::default());
    let bump = |entries: usize, media: usize, done: usize| {
        let mut p = lock(&state);
        p.entries += entries;
        p.media += media;
        p.done += done;
        p.elapsed = start.elapsed();
        progress(&p);
    };
    let count_media =
        |files: &[Listed]| files.iter().filter(|f| Catalog::is_media(&f.path)).count();
    let retry = |path: &str, recursive: bool, page: &dyn Fn(usize)| {
        let mut tries = 0;
        loop {
            match dbx.entries(path, recursive, page) {
                Ok(v) => return Ok(v),
                Err(e) if tries + 1 >= TRIES || !worth_retrying(&e) => return Err(e),
                Err(_) => {
                    tries += 1;
                    std::thread::sleep(Duration::from_secs(2 << tries));
                }
            }
        }
    };

    // Walk the top levels, keeping the files found along the way.
    let mut all = Vec::new();
    let mut frontier = vec![root.to_string()];
    for _ in 0..MAX_DEPTH {
        if frontier.len() >= ENOUGH_FOLDERS {
            break;
        }
        let mut next = Vec::new();
        for dir in &frontier {
            let (folders, files): (Vec<_>, Vec<_>) = retry(dir, false, &|_| {})?
                .into_iter()
                .partition(|(f, _)| *f);
            let files: Vec<Listed> = files.into_iter().map(|(_, e)| e).collect();
            bump(folders.len() + files.len(), count_media(&files), 0);
            found(files.clone());
            all.extend(files);
            next.extend(folders.into_iter().map(|(_, e)| e.path));
        }
        frontier = next;
        if frontier.is_empty() {
            return Ok(all);
        }
    }

    // List each remaining folder recursively, a few at a time.
    lock(&state).total = frontier.len();
    bump(0, 0, 0);
    let queue = Mutex::new(frontier);
    let results = Mutex::new((all, None::<String>));
    std::thread::scope(|s| {
        for _ in 0..THREADS {
            s.spawn(|| loop {
                if lock(&results).1.is_some() {
                    return;
                }
                let Some(dir) = lock(&queue).pop() else {
                    return;
                };
                match retry(&dir, true, &|n| bump(n, 0, 0)) {
                    Ok(entries) => {
                        let files: Vec<Listed> = entries
                            .into_iter()
                            .filter(|(f, _)| !f)
                            .map(|(_, e)| e)
                            .collect();
                        bump(0, count_media(&files), 1);
                        found(files.clone());
                        lock(&results).0.extend(files);
                    }
                    Err(e) => {
                        lock(&results).1.get_or_insert(e);
                        return;
                    }
                }
            });
        }
    });
    let (all, err) = results.into_inner().unwrap_or_else(|e| e.into_inner());
    match err {
        Some(e) => Err(e),
        None => Ok(all),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estimate_waits_for_data_then_extrapolates() {
        let mut p = Progress {
            done: 2,
            total: 100,
            elapsed: Duration::from_secs(10),
            ..Default::default()
        };
        assert!(p.remaining().is_none());
        p.done = 10;
        assert_eq!(p.remaining(), Some(Duration::from_secs(90)));
        p.media = 5210;
        assert_eq!(
            p.describe(),
            "Reading folders: 10 of 100 · 5,210 tracks found · about 2 min left"
        );
    }
}
