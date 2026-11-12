//! The library controller: Dropbox sign-in, choosing the library folder, syncing the
//! catalog, a slow background crawl for tags, the local cache and the Dropbox backup.
//! All network work runs on threads and reports back through one channel.

use super::player::{Fetched, Player};
use polymoire::library::catalog::{Catalog, Group, Tags, View};
use polymoire::library::dropbox::{Dropbox, Entry, Pkce};
use polymoire::library::media;
use polymoire::library::scan::{self, Progress, Step};
use polymoire::library::store::{self, Settings};
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

/// Tags are read from the start of each file; most tag blocks fit in this.
const TAG_BYTES: u64 = 256 * 1024;
/// Tag reads at once.
const TAG_READS: usize = 4;
/// Largest tag block worth a second request (big embedded cover art).
const MAX_TAG_BYTES: u64 = 4 << 20;
/// Save the local cache this long after a change.
const SAVE_AFTER: Duration = Duration::from_secs(5);
/// Back up to Dropbox at most this often (and when the app closes)...
const BACKUP_EVERY: Duration = Duration::from_secs(600);
/// ...or this often while a scan runs, so an interrupted scan loses little.
const CHECKPOINT_EVERY: Duration = Duration::from_secs(120);

enum Event {
    Connected(Result<String, String>),
    Account(Result<String, String>),
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

pub struct Browse {
    pub path: String,
    pub folders: Option<Result<Vec<String>, String>>,
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
    pub dbx: Option<Arc<Dropbox>>,
    pub player: Player,
    pub account: Option<String>,
    pub status: String,
    pub auth: Option<Pkce>,
    pub code_input: String,
    pub key_input: String,
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
            dbx: None,
            player: Player::new(settings.volume, settings.shuffle),
            account: None,
            status: String::new(),
            auth: None,
            code_input: String::new(),
            key_input: String::new(),
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
        if let Some(root) = lib.settings.root.clone() {
            let cached: Catalog = store::load(store::CATALOG);
            lib.catalog = if cached.root == root {
                cached
            } else {
                Catalog::new(&root)
            };
            lib.catalog.refresh_guesses();
        }
        if let (Some(key), Some(token)) =
            (lib.settings.app_key(), lib.settings.refresh_token.clone())
        {
            lib.connect(Arc::new(Dropbox::new(key, &token)));
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

    fn connect(&mut self, dbx: Arc<Dropbox>) {
        let d = dbx.clone();
        spawn(&self.tx, move || Event::Account(d.account_name()));
        self.dbx = Some(dbx);
        match self.settings.root.clone() {
            Some(_) => self.sync(),
            None => self.browse_to(""),
        }
    }

    /// Step 1 of sign-in: open Dropbox's consent page.
    pub fn begin_auth(&mut self) {
        if self.settings.app_key().is_none() {
            self.settings.app_key = self.key_input.trim().to_string();
        }
        let Some(key) = self.settings.app_key().map(str::to_string) else {
            self.status = "Enter your Dropbox app key first".into();
            return;
        };
        match Pkce::new() {
            Ok(p) => {
                open_browser(&p.auth_url(&key));
                self.auth = Some(p);
                self.status = "Approve in the browser, then paste the code here".into();
            }
            Err(e) => self.status = e,
        }
    }

    /// Step 2: trade the pasted code for a token.
    pub fn finish_auth(&mut self) {
        let (Some(p), Some(key)) = (
            self.auth.take(),
            self.settings.app_key().map(str::to_string),
        ) else {
            return;
        };
        let code = std::mem::take(&mut self.code_input);
        self.status = "Connecting…".into();
        spawn(&self.tx, move || Event::Connected(p.finish(&key, &code)));
    }

    pub fn disconnect(&mut self) {
        self.stop_sync();
        self.player.stop();
        self.dbx = None;
        self.account = None;
        self.settings.refresh_token = None;
        self.save_settings();
        self.status = "Disconnected. The catalog stays cached on this computer.".into();
    }

    pub fn auth_url(&self) -> Option<String> {
        Some(self.auth.as_ref()?.auth_url(self.settings.app_key()?))
    }

    pub fn browse_to(&mut self, path: &str) {
        let Some(dbx) = self.dbx.clone() else { return };
        self.browse = Some(Browse {
            path: path.to_string(),
            folders: None,
        });
        let p = path.to_string();
        spawn(&self.tx, move || Event::Folders(p.clone(), dbx.folders(&p)));
    }

    /// Make `path` the library folder.
    pub fn choose_root(&mut self, path: &str) {
        self.browse = None;
        if self.settings.root.as_deref() != Some(path) {
            self.stop_sync();
            self.restore_tried = false;
            self.player.stop();
            self.catalog = Catalog::new(path);
            self.rev += 1;
            self.tried.clear();
            self.settings.root = Some(path.to_string());
            self.save_settings();
        }
        self.sync();
    }

    /// Bring the catalog up to date: restore the Dropbox backup first if there's no
    /// local cache, then resume or start a full scan, or fetch the changes since the last.
    pub fn sync(&mut self) {
        let (Some(dbx), Some(root)) = (self.dbx.clone(), self.settings.root.clone()) else {
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
                let restored = dbx
                    .read_backup(&root)
                    .ok()
                    .flatten()
                    .and_then(|b| serde_json::from_slice::<Catalog>(&b).ok())
                    .filter(|c| c.root == root);
                let _ = tx.send(Event::Restored(run, restored));
            });
            return;
        }
        let (state, cancel) = (self.catalog.scan.clone(), self.cancel.clone());
        std::thread::spawn(move || {
            let tx = Mutex::new(tx);
            let send = |e| {
                let _ = tx.lock().unwrap_or_else(|p| p.into_inner()).send(e);
            };
            let res = scan::sync(&dbx, &root, state, &cancel, |step| {
                send(Event::Scan(run, step))
            });
            send(Event::Synced(run, res));
        });
    }

    /// Stop a running sync; what it found so far stays in the catalog.
    fn stop_sync(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        self.run += 1;
        self.syncing = false;
        self.scan = None;
    }

    /// Record one finding. The scan state moves with the files, so every save of the
    /// catalog is a checkpoint a later sync can resume from.
    fn scan_step(&mut self, step: Step) {
        let scan = &mut self.catalog.scan;
        match step {
            Step::Progress(p) => {
                self.scan = Some(p);
                return;
            }
            Step::Reset => scan.restart(),
            Step::Started(cursor) => {
                scan.pass += 1;
                scan.cursor = Some(cursor);
            }
            Step::Walked {
                depth,
                files,
                folders,
            } => {
                scan.plan(depth, &folders);
                self.catalog.add_listing(&files);
            }
            Step::Page {
                folder,
                files,
                cursor,
                done,
            } => {
                let f = scan.folders.entry(folder).or_default();
                f.cursor = Some(cursor);
                f.done = done;
                self.catalog.add_listing(&files);
            }
            Step::Changes { entries, cursor } => {
                scan.cursor = Some(cursor);
                for e in entries {
                    match e {
                        Entry::File(f) => {
                            self.catalog.add_listing(&[f]);
                        }
                        Entry::Deleted(p) => self.catalog.delete_path(&p),
                        Entry::Folder(_) => {}
                    }
                }
            }
        }
        self.changed();
    }

    fn changed(&mut self) {
        self.rev += 1;
        self.changed_at.get_or_insert_with(Instant::now);
        self.backup_due = true;
    }

    /// Start reading the next untagged file's tags, a few at a time (each is one small
    /// ranged download; Dropbox rate-limits well above this). One start per frame.
    fn crawl(&mut self) {
        let Some(dbx) = self.dbx.clone() else { return };
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
            let res = dbx.download(&id, Some(TAG_BYTES)).and_then(|mut b| {
                // An ID3v2 block bigger than the first read (cover art): fetch all of it.
                if let Some(n) = id3_size(&b).filter(|&n| n > b.len() as u64 && n <= MAX_TAG_BYTES)
                {
                    b = dbx.download(&id, Some(n + 4096))?;
                }
                media::read_tags(Arc::new(b), &ext)
            });
            Event::Tagged(id, res)
        });
    }

    /// Call every frame.
    pub fn frame(&mut self) {
        while let Ok(e) = self.rx.try_recv() {
            self.handle(e);
        }
        let (fetched, started) = self.player.update(&self.catalog, self.dbx.as_ref());
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
        let every = if self.syncing {
            CHECKPOINT_EVERY
        } else {
            BACKUP_EVERY
        };
        if self.backup_due && self.last_backup.elapsed() > every {
            self.backup(false);
        }
    }

    /// Upload the catalog backup (the app's only Dropbox write).
    pub fn backup(&mut self, wait: bool) {
        let (Some(dbx), Some(root)) = (self.dbx.clone(), self.settings.root.clone()) else {
            return;
        };
        let Ok(bytes) = serde_json::to_vec(&self.catalog) else {
            return;
        };
        self.backup_due = false;
        self.last_backup = Instant::now();
        if wait {
            let _ = dbx.write_backup(&root, &bytes);
        } else {
            spawn(&self.tx, move || {
                Event::BackedUp(dbx.write_backup(&root, &bytes))
            });
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
            Event::Connected(Ok(token)) => {
                self.settings.refresh_token = Some(token.clone());
                self.save_settings();
                let key = self.settings.app_key().unwrap_or_default().to_string();
                self.status = "Connected".into();
                self.connect(Arc::new(Dropbox::new(&key, &token)));
            }
            Event::Connected(Err(e)) => self.status = format!("Sign-in failed: {e}"),
            Event::Account(Ok(name)) => self.account = Some(name),
            Event::Account(Err(e)) => self.status = e,
            Event::Folders(path, res) => {
                if let Some(b) = self.browse.as_mut().filter(|b| b.path == path) {
                    b.folders = Some(res);
                }
            }
            Event::Restored(run, c) if run == self.run => {
                if let Some(c) = c.filter(|_| self.catalog.tracks.is_empty()) {
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
                        let full = !self.catalog.scan.complete;
                        if full {
                            self.catalog.finish_full_scan();
                        } else {
                            self.catalog.finish_changes();
                        }
                        self.changed();
                        self.status = format!("{} tracks", self.catalog.tracks.len());
                        if full {
                            self.backup(false);
                        }
                    }
                    Err(e) => self.status = format!("{e}. Sync again to pick up where it stopped."),
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
        if let Some(dbx) = self.dbx.clone() {
            self.player.play_list(ids, start, &self.catalog, &dbx);
        }
    }

    pub fn skip(&mut self, dir: i32) {
        if let Some(dbx) = self.dbx.clone() {
            self.player.skip(dir, &self.catalog, &dbx);
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

/// Open a link in the default browser (egui's own link opening is off in this build).
fn open_browser(url: &str) {
    let mut cmd = if cfg!(target_os = "macos") {
        std::process::Command::new("open")
    } else if cfg!(windows) {
        let mut c = std::process::Command::new("rundll32");
        c.arg("url.dll,FileProtocolHandler");
        c
    } else {
        std::process::Command::new("xdg-open")
    };
    let _ = cmd.arg(url).spawn();
}
