//! The library controller: choosing the library folder, scanning it into the catalog, a
//! background crawl for tags, the local cache and the backup in the library folder.
//! File work runs on threads and reports back through one channel.

use super::player::{Fetched, Player};
use polymoire::library::catalog::{Catalog, Group, Tags, View, VERSION};
use polymoire::library::folder::{self, Folder};
use polymoire::library::media;
use polymoire::library::scan::{self, Progress, Step};
use polymoire::library::store::{self, Settings};
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

/// Tags are read from the start of each file; most tag blocks fit in this.
const TAG_BYTES: u64 = 256 * 1024;
/// Tag reads at once.
const TAG_READS: usize = 4;
/// Largest tag block worth a second request (big embedded cover art).
const MAX_TAG_BYTES: u64 = 4 << 20;
/// Save the local cache this long after a change.
const SAVE_AFTER: Duration = Duration::from_secs(5);
/// Write the backup in the library folder at most this often (and when the app closes,
/// and after a scan that changed something).
const BACKUP_EVERY: Duration = Duration::from_secs(600);

enum Event {
    Folders(String, Result<Vec<String>, String>),
    Restored(u64, Option<Catalog>),
    /// A sync run's findings, tagged with the run so a cancelled one is ignored.
    Scan(u64, Step),
    Synced(u64, Result<(), String>),
    Tagged(String, Result<(Tags, f32), String>),
    BackedUp(Result<(), String>),
}

type GroupsKey = (u64, View, String);
type ListKey = (u64, View, Option<String>, String);

/// The folder picker: where it is, and what's in it once listed.
pub struct Browse {
    pub path: String,
    pub folders: Option<Result<Vec<String>, String>>,
    /// The path box, for typing or pasting a folder.
    pub typed: String,
}

/// What the library window shows.
pub struct UiState {
    pub open: bool,
    pub view: View,
    pub group: Option<String>,
    pub query: String,
    pub new_playlist: String,
    pub renaming: Option<(usize, String)>,
}

pub struct Library {
    pub settings: Settings,
    pub catalog: Catalog,
    pub folder: Option<Arc<Folder>>,
    pub player: Player,
    pub status: String,
    pub browse: Option<Browse>,
    pub ui: UiState,
    pub syncing: bool,
    /// Bumped on every catalog change; the browse views are cached against it (they
    /// walk every track, too slow to redo each frame in a big library).
    rev: u64,
    groups_cache: Option<(GroupsKey, Arc<Vec<Group>>)>,
    list_cache: Option<(ListKey, Arc<Vec<String>>)>,
    /// How far the current sync has got.
    pub scan: Option<Progress>,
    /// Numbers sync runs; events from an older one are ignored.
    run: u64,
    cancel: Arc<AtomicBool>,
    restore_tried: bool,
    /// Tag reads in flight.
    tagging: usize,
    tried: HashSet<String>,
    changed_at: Option<Instant>,
    backup_due: bool,
    last_backup: Instant,
    tx: Sender<Event>,
    rx: Receiver<Event>,
}

fn spawn<F: FnOnce() -> Event + Send + 'static>(tx: &Sender<Event>, f: F) {
    let tx = tx.clone();
    std::thread::spawn(move || {
        let _ = tx.send(f());
    });
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

impl Library {
    pub fn new() -> Self {
        let mut settings: Settings = store::load(store::SETTINGS);
        if settings.volume <= 0.0 {
            settings.volume = 0.8;
        }
        let (tx, rx) = channel();
        let mut lib = Library {
            catalog: Catalog::default(),
            folder: None,
            player: Player::new(settings.volume, settings.shuffle),
            status: String::new(),
            browse: None,
            ui: UiState {
                // In player mode the library is the first thing you need.
                open: settings.player,
                view: View::Artists,
                group: None,
                query: String::new(),
                new_playlist: String::new(),
                renaming: None,
            },
            syncing: false,
            rev: 0,
            groups_cache: None,
            list_cache: None,
            scan: None,
            run: 0,
            cancel: Arc::new(AtomicBool::new(false)),
            restore_tried: false,
            tagging: 0,
            tried: HashSet::new(),
            changed_at: None,
            backup_due: false,
            last_backup: Instant::now(),
            tx,
            rx,
            settings,
        };
        match lib.settings.folder.clone() {
            Some(root) => {
                let cached: Catalog = store::load(store::CATALOG);
                lib.catalog = if cached.version == VERSION && cached.root == root {
                    cached
                } else {
                    Catalog::new(&root)
                };
                lib.catalog.refresh_guesses();
                lib.folder = Some(Arc::new(Folder::new(&root)));
                lib.sync();
            }
            None => {
                let home = folder::home().map(|h| folder::display(&h));
                lib.browse_to(&home.unwrap_or_else(|| "/".into()));
            }
        }
        lib
    }

    pub fn save_settings(&mut self) {
        self.settings.volume = self.player.volume();
        self.settings.shuffle = self.player.shuffle;
        if let Err(e) = store::save(store::SETTINGS, &self.settings) {
            self.status = format!("Couldn't save settings: {e}");
        }
    }

    /// Show a folder in the picker (listed on a thread: a network drive can be slow).
    pub fn browse_to(&mut self, path: &str) {
        self.browse = Some(Browse {
            path: path.to_string(),
            folders: None,
            typed: path.to_string(),
        });
        let p = path.to_string();
        spawn(&self.tx, move || {
            Event::Folders(p.clone(), folder::subfolders(std::path::Path::new(&p)))
        });
    }

    /// The picker's path box: go to what was typed (`~` is the home folder).
    pub fn browse_typed(&mut self) {
        if let Some(b) = &self.browse {
            let p = folder::display(&folder::expand(&b.typed));
            self.browse_to(&p);
        }
    }

    /// Make `path` the library folder.
    pub fn choose_root(&mut self, path: &str) {
        self.browse = None;
        if self.settings.folder.as_deref() != Some(path) {
            self.stop_sync();
            self.restore_tried = false;
            self.player.stop();
            self.catalog = Catalog::new(path);
            self.rev += 1;
            self.tried.clear();
            self.folder = Some(Arc::new(Folder::new(path)));
            self.settings.folder = Some(path.to_string());
            self.save_settings();
        }
        self.sync();
    }

    /// Bring the catalog up to date: restore the backup in the library folder first if
    /// there's no local cache, then walk the folder.
    pub fn sync(&mut self) {
        let Some(dir) = self.folder.clone() else {
            return;
        };
        if self.syncing {
            return;
        }
        self.syncing = true;
        self.scan = Some(Progress::default());
        self.status.clear();
        self.run += 1;
        self.cancel = Arc::new(AtomicBool::new(false));
        let (run, tx) = (self.run, self.tx.clone());
        if self.catalog.tracks.is_empty() && !self.restore_tried {
            self.restore_tried = true;
            std::thread::spawn(move || {
                let restored = dir
                    .read_backup()
                    .ok()
                    .flatten()
                    .and_then(|b| serde_json::from_slice::<Catalog>(&b).ok())
                    .filter(|c| c.version == VERSION);
                let _ = tx.send(Event::Restored(run, restored));
            });
            return;
        }
        self.catalog.start_full_scan();
        let cancel = self.cancel.clone();
        std::thread::spawn(move || {
            let send = |step| drop(tx.send(Event::Scan(run, step)));
            let res = scan::sync(dir.root(), &cancel, send);
            let _ = tx.send(Event::Synced(run, res));
        });
    }

    /// Stop a running sync; what it found so far stays in the catalog.
    fn stop_sync(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        self.run += 1;
        self.syncing = false;
        self.scan = None;
    }

    /// Record one finding.
    fn scan_step(&mut self, step: Step) {
        match step {
            Step::Progress(p) => self.scan = Some(p),
            Step::Files(files) => {
                if self.catalog.add_listing(&files) {
                    self.changed();
                }
            }
        }
    }

    fn changed(&mut self) {
        self.rev += 1;
        self.changed_at.get_or_insert_with(Instant::now);
        self.backup_due = true;
    }

    /// Start reading the next untagged file's tags, a few at a time (each reads only the
    /// start of the file). One start per frame.
    fn crawl(&mut self) {
        let Some(dir) = self.folder.clone() else {
            return;
        };
        if self.tagging >= TAG_READS || self.syncing {
            return;
        }
        let Some(t) = self
            .catalog
            .untagged()
            .find(|t| !self.tried.contains(&t.id))
        else {
            return;
        };
        self.tried.insert(t.id.clone());
        self.tagging += 1;
        let (id, ext) = (t.id.clone(), t.ext().to_lowercase());
        spawn(&self.tx, move || {
            let res = dir.read(&id, Some(TAG_BYTES)).and_then(|mut b| {
                // An ID3v2 block bigger than the first read (cover art): read all of it.
                if let Some(n) = id3_size(&b).filter(|&n| n > b.len() as u64 && n <= MAX_TAG_BYTES)
                {
                    b = dir.read(&id, Some(n + 4096))?;
                }
                media::read_tags(Arc::new(b), &ext)
            });
            // Some files can't be read from their start alone (m4a with the index at the
            // end; some mp3s Symphonia won't probe from a partial read). Locally, reading
            // the whole file is cheap enough for the few that need it.
            let res = res.or_else(|_| media::read_tags(Arc::new(dir.read(&id, None)?), &ext));
            Event::Tagged(id, res)
        });
    }

    /// Call every frame.
    pub fn frame(&mut self) {
        while let Ok(e) = self.rx.try_recv() {
            self.handle(e);
        }
        let (fetched, started) = self.player.update(&self.catalog, self.folder.as_ref());
        for f in fetched {
            match f {
                Fetched::Analysis(id, a, tags) => {
                    if let Some(map) = &a.map {
                        self.catalog.set_beats(&id, map);
                    }
                    if let Some(tags) = tags {
                        self.catalog.set_tags(&id, tags, a.duration);
                    } else if let Some(t) = self.catalog.tracks.get_mut(&id) {
                        t.duration = a.duration;
                    }
                    if let Some(t) = self.catalog.tracks.get_mut(&id) {
                        t.loudness = a.loudness;
                    }
                    self.changed();
                }
                Fetched::Failed(_, e) => self.status = e,
                Fetched::Bytes(..) => {}
            }
        }
        if let Some(id) = started {
            self.catalog.played(&id, now_secs());
            self.changed();
        }
        self.crawl();
        if self.changed_at.is_some_and(|t| t.elapsed() > SAVE_AFTER) {
            self.changed_at = None;
            if let Err(e) = store::save(store::CATALOG, &self.catalog) {
                self.status = format!("Couldn't save the catalog: {e}");
            }
        }
        if self.backup_due && !self.syncing && self.last_backup.elapsed() > BACKUP_EVERY {
            self.backup(false);
        }
    }

    /// Write the catalog backup (the app's only write to the library folder).
    pub fn backup(&mut self, wait: bool) {
        let Some(dir) = self.folder.clone() else {
            return;
        };
        let Ok(bytes) = serde_json::to_vec(&self.catalog) else {
            return;
        };
        self.backup_due = false;
        self.last_backup = Instant::now();
        if wait {
            let _ = dir.write_backup(&bytes);
        } else {
            spawn(&self.tx, move || Event::BackedUp(dir.write_backup(&bytes)));
        }
    }

    /// Save everything on the way out.
    pub fn shutdown(&mut self) {
        self.save_settings();
        let _ = store::save(store::CATALOG, &self.catalog);
        if self.backup_due {
            self.backup(true);
        }
    }

    fn handle(&mut self, e: Event) {
        match e {
            Event::Folders(path, res) => {
                if let Some(b) = self.browse.as_mut().filter(|b| b.path == path) {
                    b.folders = Some(res);
                }
            }
            Event::Restored(run, c) if run == self.run => {
                if let Some(mut c) = c.filter(|_| self.catalog.tracks.is_empty()) {
                    // Tracks are named inside the folder, so a backup made where the
                    // folder had another path (another computer) still fits.
                    c.root = self.catalog.root.clone();
                    c.refresh_guesses();
                    self.catalog = c;
                    self.rev += 1;
                    self.status = "Restored the catalog from the library folder".into();
                }
                self.syncing = false;
                self.sync();
            }
            Event::Scan(run, step) if run == self.run => self.scan_step(step),
            Event::Synced(run, res) if run == self.run => {
                self.syncing = false;
                self.scan = None;
                match res {
                    Ok(()) => {
                        if self.catalog.finish_full_scan() {
                            self.changed();
                        }
                        self.status = format!("{} tracks", self.catalog.tracks.len());
                        if self.backup_due {
                            self.backup(false);
                        }
                    }
                    // Nothing is removed after a failed scan, so an unreadable folder
                    // (an unplugged drive) doesn't lose its tracks.
                    Err(e) => self.status = format!("{e}. Nothing was removed; rescan to retry."),
                }
            }
            Event::Restored(..) | Event::Scan(..) | Event::Synced(..) => {}
            Event::Tagged(id, res) => {
                self.tagging -= 1;
                if let Ok((tags, secs)) = res {
                    self.catalog.set_tags(&id, tags, secs);
                    self.changed();
                }
            }
            Event::BackedUp(Err(e)) => self.status = format!("Backup failed: {e}"),
            Event::BackedUp(Ok(())) => {}
        }
    }

    /// Tags still to read, for the status line.
    pub fn untagged_left(&self) -> usize {
        self.catalog
            .untagged()
            .filter(|t| !self.tried.contains(&t.id))
            .count()
    }

    pub fn play(&mut self, ids: Vec<String>, start: usize) {
        if let Some(f) = self.folder.clone() {
            self.player.play_list(ids, start, &self.catalog, &f);
        }
    }

    pub fn skip(&mut self, dir: i32) {
        if let Some(f) = self.folder.clone() {
            self.player.skip(dir, &self.catalog, &f);
        }
    }

    /// The current view's groups (cached).
    pub fn groups(&mut self) -> Arc<Vec<Group>> {
        let key = (self.rev, self.ui.view, self.ui.query.clone());
        match &self.groups_cache {
            Some((k, v)) if *k == key => v.clone(),
            _ => {
                let v = Arc::new(self.catalog.groups(key.1, &key.2));
                self.groups_cache = Some((key, v.clone()));
                v
            }
        }
    }

    /// The current view's tracks (cached).
    pub fn tracks(&mut self) -> Arc<Vec<String>> {
        let key = (
            self.rev,
            self.ui.view,
            self.ui.group.clone(),
            self.ui.query.clone(),
        );
        match &self.list_cache {
            Some((k, v)) if *k == key => v.clone(),
            _ => {
                let v = Arc::new(self.catalog.list(key.1, key.2.as_deref(), &key.3));
                self.list_cache = Some((key, v.clone()));
                v
            }
        }
    }

    pub fn edit_playlists(&mut self, f: impl FnOnce(&mut Catalog)) {
        f(&mut self.catalog);
        self.changed();
    }
}

/// Total size of an ID3v2 tag at the start of a file, header included.
fn id3_size(b: &[u8]) -> Option<u64> {
    if b.len() < 10 || &b[..3] != b"ID3" {
        return None;
    }
    let n = b[6..10]
        .iter()
        .fold(0u64, |n, &x| n << 7 | (x & 0x7f) as u64);
    Some(n + 10)
}
