//! Reading the library folder: a full scan the first time, then only what changed.
//!
//! Dropbox can't say how big a folder is, so the full scan first walks the top levels
//! (one cheap request per folder) until it has enough folders to count, then lists each
//! of those whole on a few threads. "Folders done of total" is the progress.
//!
//! Everything the scan learns goes out as a `Step` in order, and the catalog records it
//! with the folders' cursors, so the saved catalog is always a checkpoint: an interrupted
//! scan resumes from it (here, or from the Dropbox backup on another computer).

use super::catalog::{Listed, ScanState};
use super::dropbox::{Dropbox, Entry, Page};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Walk down until there are at least this many folders to count...
const ENOUGH_FOLDERS: usize = 24;
/// ...but no more than this many levels.
const MAX_DEPTH: usize = 3;
/// Folders listed at once. Dropbox rate-limits heavier use.
const THREADS: usize = 4;
/// Tries per request before the sync gives up (rate limits and dropped connections).
const TRIES: u32 = 4;

/// What the scan found, for the catalog to record, in order.
#[derive(Debug)]
pub enum Step {
    /// The changes since a sync's cursor ran out (Dropbox forgets old cursors): start a
    /// new full scan.
    Reset,
    /// A new full scan begins; changes after this cursor are fetched by the next sync.
    Started(String),
    /// The top levels: the files in them and the folders to list whole.
    Walked {
        depth: usize,
        files: Vec<Listed>,
        folders: Vec<String>,
    },
    /// A page of one folder's listing, and where to continue it from.
    Page {
        folder: String,
        files: Vec<Listed>,
        cursor: String,
        done: bool,
    },
    /// A page of changes since the last sync, in the order they happened.
    Changes {
        entries: Vec<Entry>,
        cursor: String,
    },
    Progress(Progress),
}

#[derive(Clone, Debug, Default)]
pub struct Progress {
    /// Fetching changes since the last sync, not a full scan.
    pub changes: bool,
    /// Folders fully listed, of `total` (0 while the top levels are being walked), and
    /// how many were already done when this run started (a resumed scan).
    pub done: usize,
    pub total: usize,
    pub resumed_from: usize,
    /// Entries (files and folders) received in this run.
    pub entries: usize,
    pub elapsed: Duration,
}

impl Progress {
    /// Time left, once enough folders are done in this run for a guess to mean something.
    pub fn remaining(&self) -> Option<Duration> {
        let done_here = self.done.saturating_sub(self.resumed_from);
        if done_here < 3 || self.total <= self.done || self.elapsed < Duration::from_secs(2) {
            return None;
        }
        let per = self.elapsed.as_secs_f64() / done_here as f64;
        Some(Duration::from_secs_f64(
            per * (self.total - self.done) as f64,
        ))
    }

    /// The bar's position, if there's a total to measure against.
    pub fn fraction(&self) -> Option<f32> {
        (self.total > 0).then(|| self.done as f32 / self.total as f32)
    }

    /// "Reading folders: 37 of 412 · 5,210 tracks · about 2 min left".
    pub fn describe(&self, tracks: usize) -> String {
        if self.changes {
            return format!(
                "Checking Dropbox for changes · {} entries so far",
                group_digits(self.entries)
            );
        }
        let mut s = if self.total == 0 {
            "Looking through the library folder".to_string()
        } else if self.resumed_from > 0 {
            format!("Resuming: folder {} of {}", self.done, self.total)
        } else {
            format!("Reading folders: {} of {}", self.done, self.total)
        };
        s += &format!(" · {} tracks", group_digits(tracks));
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

/// Rate limits, server trouble and dropped connections pass; a refusal won't change.
fn worth_retrying(e: &str) -> bool {
    let refused = e.starts_with("Dropbox sign-in") || e.starts_with("Not found");
    let client = e.starts_with("Dropbox error 4") && !e.starts_with("Dropbox error 429");
    !(refused || client)
}

/// Dropbox no longer knows this cursor; list again from the start.
fn is_reset(e: &str) -> bool {
    e.starts_with("Dropbox error 409") && e.contains("reset")
}

const CANCELLED: &str = "Sync cancelled";

fn files(entries: &[Entry]) -> Vec<Listed> {
    entries
        .iter()
        .filter_map(|e| match e {
            Entry::File(f) => Some(f.clone()),
            _ => None,
        })
        .collect()
}

/// What every part of a sync shares.
struct Ctx<'a, S> {
    dbx: &'a Dropbox,
    cancel: &'a AtomicBool,
    send: S,
    progress: Mutex<Progress>,
    start: Instant,
}

impl<S: Fn(Step) + Sync> Ctx<'_, S> {
    fn report(&self, f: impl FnOnce(&mut Progress)) {
        let mut p = lock(&self.progress);
        f(&mut p);
        p.elapsed = self.start.elapsed();
        (self.send)(Step::Progress(p.clone()));
    }

    /// One request, retried through rate limits and dropped connections.
    fn page(&self, f: impl Fn(&Dropbox) -> Result<Page, String>) -> Result<Page, String> {
        let mut tries = 0;
        loop {
            if self.cancel.load(Ordering::Relaxed) {
                return Err(CANCELLED.to_string());
            }
            match f(self.dbx) {
                Ok(page) => {
                    self.report(|p| p.entries += page.entries.len());
                    return Ok(page);
                }
                Err(e) if tries + 1 >= TRIES || !worth_retrying(&e) => return Err(e),
                Err(_) => {
                    tries += 1;
                    std::thread::sleep(Duration::from_secs(1 << tries));
                }
            }
        }
    }

    /// Every page of changes after `cursor`.
    fn changes(&self, mut cursor: String) -> Result<(), String> {
        loop {
            let page = self.page(|d| d.list_continue(&cursor))?;
            cursor = page.cursor.clone();
            (self.send)(Step::Changes {
                entries: page.entries,
                cursor: page.cursor,
            });
            if !page.has_more {
                return Ok(());
            }
        }
    }

    /// The top levels, one folder at a time: `depth` levels if given, otherwise until
    /// there are enough folders to count. Returns the depth, the files found on the way
    /// and the folders below.
    fn walk(
        &self,
        root: &str,
        depth: Option<usize>,
    ) -> Result<(usize, Vec<Listed>, Vec<String>), String> {
        let mut found = Vec::new();
        let mut frontier = vec![root.to_string()];
        let mut level = 0;
        while !frontier.is_empty()
            && match depth {
                Some(d) => level < d,
                None => level < MAX_DEPTH && frontier.len() < ENOUGH_FOLDERS,
            }
        {
            let mut next = Vec::new();
            for dir in &frontier {
                let mut page = self.page(|d| d.list_folder(dir, false))?;
                loop {
                    found.extend(files(&page.entries));
                    next.extend(page.entries.iter().filter_map(|e| match e {
                        Entry::Folder(p) => Some(p.clone()),
                        _ => None,
                    }));
                    if !page.has_more {
                        break;
                    }
                    page = self.page(|d| d.list_continue(&page.cursor))?;
                }
            }
            frontier = next;
            level += 1;
        }
        Ok((level, found, frontier))
    }

    /// List one folder whole, continuing from `cursor` if a previous run got partway.
    fn list_whole(&self, folder: &str, mut cursor: Option<String>) -> Result<(), String> {
        loop {
            let page = match &cursor {
                Some(c) => match self.page(|d| d.list_continue(c)) {
                    Err(e) if is_reset(&e) => {
                        cursor = None;
                        continue;
                    }
                    other => other?,
                },
                None => self.page(|d| d.list_folder(folder, true))?,
            };
            cursor = Some(page.cursor.clone());
            (self.send)(Step::Page {
                folder: folder.to_string(),
                files: files(&page.entries),
                cursor: page.cursor,
                done: !page.has_more,
            });
            if !page.has_more {
                return Ok(());
            }
        }
    }
}

/// Bring the catalog up to date, starting from its saved `state`. Every finding goes to
/// `send`, in order. Returns once everything is listed (the catalog then finishes the
/// full scan or applies the deletes) or on failure (the catalog keeps its checkpoint).
/// Stops early if `cancel` is set.
pub fn sync(
    dbx: &Dropbox,
    root: &str,
    mut state: ScanState,
    cancel: &AtomicBool,
    send: impl Fn(Step) + Sync,
) -> Result<(), String> {
    let cx = Ctx {
        dbx,
        cancel,
        send,
        progress: Mutex::new(Progress::default()),
        start: Instant::now(),
    };
    if state.complete {
        cx.report(|p| p.changes = true);
        match cx.changes(state.cursor.clone().unwrap_or_default()) {
            Err(e) if is_reset(&e) => {
                (cx.send)(Step::Reset);
                state.restart();
                cx.report(|p| *p = Progress::default());
            }
            other => return other,
        }
    }
    if state.cursor.is_none() {
        // Taken before listing anything, so changes made during the scan aren't missed.
        let c = dbx.latest_cursor(root)?;
        (cx.send)(Step::Started(c));
    }

    let (depth, top, folders) = cx.walk(root, state.depth)?;
    let todo = state.plan(depth, &folders);
    let done = folders.len() - todo.len();
    (cx.send)(Step::Walked {
        depth,
        files: top,
        folders: folders.clone(),
    });
    cx.report(|p| {
        p.total = folders.len();
        p.done = done;
        p.resumed_from = done;
    });

    let queue = Mutex::new(todo);
    let failed = Mutex::new(None::<String>);
    std::thread::scope(|s| {
        for _ in 0..THREADS {
            s.spawn(|| {
                while lock(&failed).is_none() {
                    let Some((folder, cursor)) = lock(&queue).pop() else {
                        return;
                    };
                    if let Err(e) = cx.list_whole(&folder, cursor) {
                        lock(&failed).get_or_insert(e);
                        return;
                    }
                    cx.report(|p| p.done += 1);
                }
            });
        }
    });
    match failed.into_inner().unwrap_or_else(|e| e.into_inner()) {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estimate_counts_only_this_runs_folders() {
        let mut p = Progress {
            done: 2,
            total: 100,
            elapsed: Duration::from_secs(10),
            ..Default::default()
        };
        assert!(p.remaining().is_none());
        p.done = 10;
        assert_eq!(p.remaining(), Some(Duration::from_secs(90)));
        assert_eq!(
            p.describe(5210),
            "Reading folders: 10 of 100 · 5,210 tracks · about 2 min left"
        );
        // Resumed at 50 of 100: 5 more in 10 s means 50 s for the last 45.
        p.resumed_from = 50;
        p.done = 55;
        assert_eq!(p.remaining(), Some(Duration::from_secs(90)));
        assert!(p.describe(0).starts_with("Resuming: folder 55 of 100"));
    }

    #[test]
    fn refusals_are_not_retried() {
        assert!(worth_retrying("Dropbox error 429: too_many_requests"));
        assert!(worth_retrying("Dropbox error 503: unavailable"));
        assert!(worth_retrying("io: connection reset by peer"));
        assert!(!worth_retrying("Dropbox error 400: bad scope"));
        assert!(!worth_retrying("Not found in Dropbox"));
        assert!(is_reset("Dropbox error 409: reset/.."));
        assert!(!is_reset("io: connection reset by peer"));
    }
}
