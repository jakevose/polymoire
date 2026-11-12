//! The music catalog: every track under the library folder, its tags, beat map and play
//! history, plus playlists. One serializable value, cached locally and backed up to the
//! library folder (`store`, `dropbox`). Views slice it for the library browser.

use crate::beatmap::BeatMap;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const VERSION: u32 = 1;
pub const EXTENSIONS: [&str; 2] = ["mp3", "m4a"];

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Tags {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub album_artist: String,
    pub genre: String,
    pub year: u16,
    pub track_no: u16,
    pub disc: u16,
}

/// A file as the cloud lists it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Listed {
    pub id: String,
    pub path: String,
    /// Changes whenever the content does (Dropbox's content hash).
    pub hash: String,
    pub size: u64,
    /// ISO 8601, so it sorts as text.
    pub modified: String,
}

/// A beat map in its compact form (see `BeatMap::encode`). `v` is the mapper version:
/// maps from an older mapper are dropped and made again when the track next plays.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StoredBeats {
    pub v: u32,
    pub bpm: f32,
    pub beats: String,
}

/// Bump when the beat mapper changes enough that old maps should be redone.
pub const MAP_VERSION: u32 = 2;

impl From<&BeatMap> for StoredBeats {
    fn from(m: &BeatMap) -> Self {
        StoredBeats {
            v: MAP_VERSION,
            bpm: m.bpm,
            beats: m.encode(),
        }
    }
}

impl StoredBeats {
    /// The map, if it's from the current mapper and reads back.
    pub fn to_map(&self) -> Option<BeatMap> {
        (self.v == MAP_VERSION)
            .then(|| BeatMap::decode(self.bpm, &self.beats))
            .flatten()
            .filter(|m| !m.beats.is_empty())
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Track {
    pub id: String,
    pub path: String,
    pub hash: String,
    pub size: u64,
    pub modified: String,
    pub tags: Tags,
    /// Tags have been read from the file (otherwise they're guessed from the path).
    pub tagged: bool,
    pub duration: f32,
    pub beats: Option<StoredBeats>,
    pub plays: u32,
    /// Unix seconds.
    pub last_played: u64,
    /// The full scan that last listed this file (`ScanState::pass`).
    pub seen: u32,
}

impl Track {
    pub fn ext(&self) -> &str {
        self.path.rsplit_once('.').map_or("", |(_, e)| e)
    }
    pub fn file_stem(&self) -> &str {
        let name = self.path.rsplit('/').next().unwrap_or(&self.path);
        name.rsplit_once('.').map_or(name, |(s, _)| s)
    }
    pub fn title(&self) -> &str {
        nonempty(&self.tags.title).unwrap_or_else(|| self.file_stem())
    }
    pub fn artist(&self) -> &str {
        nonempty(&self.tags.artist)
            .or(nonempty(&self.tags.album_artist))
            .unwrap_or("Unknown artist")
    }
    pub fn album_artist(&self) -> &str {
        nonempty(&self.tags.album_artist).unwrap_or_else(|| self.artist())
    }
    pub fn album(&self) -> &str {
        nonempty(&self.tags.album).unwrap_or("Unknown album")
    }
    pub fn bpm(&self) -> Option<f32> {
        self.beats.as_ref().map(|b| b.bpm)
    }
    fn folder(&self) -> &str {
        self.path.rsplit_once('/').map_or("", |(d, _)| d)
    }
    fn matches(&self, q: &str) -> bool {
        q.is_empty()
            || [
                self.title(),
                self.artist(),
                self.album(),
                &self.tags.genre,
                &self.path,
            ]
            .iter()
            .any(|f| f.to_lowercase().contains(q))
    }
}

fn nonempty(s: &str) -> Option<&str> {
    let s = s.trim();
    (!s.is_empty()).then_some(s)
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Playlist {
    pub name: String,
    pub tracks: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Catalog {
    pub version: u32,
    /// The library folder this catalog describes.
    pub root: String,
    pub tracks: BTreeMap<String, Track>,
    pub playlists: Vec<Playlist>,
    pub scan: ScanState,
}

/// How far reading the library folder has got. Saved with the catalog (and so in the
/// Dropbox backup), so an interrupted scan picks up where it stopped.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ScanState {
    /// Changes after this cursor are what the next sync fetches. Taken just before the
    /// full scan starts, so nothing that changes during the scan is missed.
    pub cursor: Option<String>,
    /// The full scan has finished; syncs now fetch only what changed.
    pub complete: bool,
    /// Levels walked before folders are listed whole. Kept so a resumed scan splits
    /// the library into the same folders.
    pub depth: Option<usize>,
    /// The folders the full scan lists whole, and how far each has got.
    pub folders: BTreeMap<String, FolderScan>,
    /// Numbers full scans. Files listed in this one carry it; at the end, tracks that
    /// don't are gone.
    pub pass: u32,
    /// Tracks deleted by the changes fetched so far, kept until the sync finishes: a
    /// moved file arrives as a delete and an add (same id), maybe pages apart, and should
    /// keep its tags, beat map and plays.
    pub gone: BTreeMap<String, Track>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FolderScan {
    /// Where to continue the listing from (`None`: not started).
    pub cursor: Option<String>,
    pub done: bool,
}

impl ScanState {
    /// Start over with a new full scan (keeping the pass count).
    pub fn restart(&mut self) {
        *self = ScanState {
            pass: self.pass,
            ..Default::default()
        };
    }

    /// Match the saved folders to what the walk found now: folders that went away are
    /// dropped, new ones start fresh. Returns the folders still to list, with where to
    /// continue each from.
    pub fn plan(&mut self, depth: usize, folders: &[String]) -> Vec<(String, Option<String>)> {
        self.depth = Some(depth);
        self.folders.retain(|f, _| folders.contains(f));
        let mut todo = Vec::new();
        for f in folders {
            let s = self.folders.entry(f.clone()).or_default();
            if !s.done {
                todo.push((f.clone(), s.cursor.clone()));
            }
        }
        todo
    }
}

/// Ways to browse. Grouped views list groups, then the tracks in the chosen group.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    Artists,
    Albums,
    Genres,
    Years,
    Tempo,
    Folders,
    Playlists,
    Recent,
    MostPlayed,
    Songs,
}

impl View {
    pub const ALL: [View; 10] = [
        View::Artists,
        View::Albums,
        View::Genres,
        View::Years,
        View::Tempo,
        View::Folders,
        View::Playlists,
        View::Recent,
        View::MostPlayed,
        View::Songs,
    ];
    pub fn label(self) -> &'static str {
        match self {
            View::Artists => "Artists",
            View::Albums => "Albums",
            View::Genres => "Genres",
            View::Years => "Years",
            View::Tempo => "Tempo",
            View::Folders => "Folders",
            View::Playlists => "Playlists",
            View::Recent => "Recently added",
            View::MostPlayed => "Most played",
            View::Songs => "Songs",
        }
    }
    pub fn grouped(self) -> bool {
        !matches!(self, View::Recent | View::MostPlayed | View::Songs)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    pub key: String,
    pub label: String,
    pub count: usize,
}

const TEMPO_BANDS: [(f32, &str); 6] = [
    (90.0, "Slow · under 90"),
    (110.0, "Easy · 90–110"),
    (125.0, "Groove · 110–125"),
    (140.0, "Dance · 125–140"),
    (165.0, "Fast · 140–165"),
    (f32::MAX, "Very fast · 165+"),
];

impl Catalog {
    pub fn new(root: &str) -> Self {
        Catalog {
            version: VERSION,
            root: root.to_string(),
            ..Default::default()
        }
    }

    pub fn is_media(path: &str) -> bool {
        let lower = path.to_lowercase();
        EXTENSIONS.iter().any(|e| lower.ends_with(&format!(".{e}")))
    }

    /// Bring the catalog in line with a full listing of the library folder. Changed files
    /// lose their tags and beat map; play counts and playlists survive. Returns true if
    /// anything changed.
    pub fn merge_listing(&mut self, files: &[Listed]) -> bool {
        self.scan.pass += 1;
        let changed = self.add_listing(files);
        changed | self.finish_full_scan()
    }

    /// Add or update files from a listing, removing nothing (tracks show up while a big
    /// library is still being read). They're marked as seen in the current full scan.
    /// Returns true if anything changed.
    pub fn add_listing(&mut self, files: &[Listed]) -> bool {
        let mut changed = false;
        let pass = self.scan.pass;
        for f in files.iter().filter(|f| Self::is_media(&f.path)) {
            if let Some(t) = self.scan.gone.remove(&f.id) {
                self.tracks.entry(f.id.clone()).or_insert(t);
            }
            let t = self.tracks.entry(f.id.clone()).or_default();
            t.seen = pass;
            if t.hash != f.hash || t.path != f.path {
                changed = true;
                if t.hash != f.hash {
                    t.tags = Tags::default();
                    t.tagged = false;
                    t.beats = None;
                    t.duration = 0.0;
                }
                t.id = f.id.clone();
                t.path = f.path.clone();
                t.hash = f.hash.clone();
                t.size = f.size;
                t.modified = f.modified.clone();
                if !t.tagged {
                    t.tags = guess_tags(&self.root, &f.path);
                }
            }
        }
        changed
    }

    /// The full scan listed everything: tracks it didn't see are gone. From now on syncs
    /// fetch only changes. Returns true if anything was removed.
    pub fn finish_full_scan(&mut self) -> bool {
        let pass = self.scan.pass;
        self.scan.complete = true;
        self.scan.folders.clear();
        self.remove(|t| t.seen != pass)
    }

    /// Something at `path` was deleted (a file, or a folder and all inside it).
    pub fn delete_path(&mut self, path: &str) {
        let p = path.to_lowercase();
        let ids: Vec<String> = self
            .tracks
            .values()
            .filter(|t| {
                let tp = t.path.to_lowercase();
                tp == p
                    || tp
                        .strip_prefix(p.as_str())
                        .is_some_and(|r| r.starts_with('/'))
            })
            .map(|t| t.id.clone())
            .collect();
        for id in ids {
            if let Some(t) = self.tracks.remove(&id) {
                self.scan.gone.insert(id, t);
            }
        }
    }

    /// A sync's changes are all in: deleted tracks that didn't reappear are gone for
    /// good. Returns true if any were.
    pub fn finish_changes(&mut self) -> bool {
        let any = !std::mem::take(&mut self.scan.gone).is_empty();
        self.remove(|_| false);
        any
    }

    fn remove(&mut self, gone: impl Fn(&Track) -> bool) -> bool {
        let kept = self.tracks.len();
        self.tracks.retain(|_, t| !gone(t));
        let tracks = &self.tracks;
        for p in &mut self.playlists {
            p.tracks.retain(|id| tracks.contains_key(id));
        }
        self.tracks.len() != kept
    }

    pub fn set_tags(&mut self, id: &str, tags: Tags, duration: f32) {
        if let Some(t) = self.tracks.get_mut(id) {
            let guessed = std::mem::take(&mut t.tags);
            t.tags = fill(tags, guessed);
            t.tagged = true;
            if duration > 0.0 {
                t.duration = duration;
            }
        }
    }

    pub fn set_beats(&mut self, id: &str, map: &BeatMap) {
        if let Some(t) = self.tracks.get_mut(id) {
            t.beats = Some(map.into());
        }
    }

    pub fn played(&mut self, id: &str, now: u64) {
        if let Some(t) = self.tracks.get_mut(id) {
            t.plays += 1;
            t.last_played = now;
        }
    }

    /// Tracks still waiting for their tags, in browsing order.
    pub fn untagged(&self) -> impl Iterator<Item = &Track> {
        self.tracks.values().filter(|t| !t.tagged)
    }

    fn group_key(&self, view: View, t: &Track) -> Option<(String, String)> {
        Some(match view {
            View::Artists => {
                let a = t.album_artist();
                (a.to_lowercase(), a.to_string())
            }
            View::Albums => {
                let label = format!("{} — {}", t.album(), t.album_artist());
                (label.to_lowercase(), label)
            }
            View::Genres => {
                let g = nonempty(&t.tags.genre).unwrap_or("No genre");
                (g.to_lowercase(), g.to_string())
            }
            View::Years => match t.tags.year {
                0 => ("~".into(), "No year".into()),
                y => {
                    let d = y / 10 * 10;
                    // Newest first: invert the key.
                    (format!("{:04}", 9999 - d), format!("{d}s"))
                }
            },
            View::Tempo => match t.bpm() {
                None => ("~".into(), "Not analyzed yet".into()),
                Some(bpm) => {
                    let i = TEMPO_BANDS.iter().position(|(hi, _)| bpm < *hi)?;
                    (i.to_string(), TEMPO_BANDS[i].1.to_string())
                }
            },
            View::Folders => {
                let f = t.folder();
                let rel = f.strip_prefix(self.root.as_str()).unwrap_or(f);
                let rel = rel.trim_start_matches('/');
                let label = if rel.is_empty() { "(top)" } else { rel };
                (rel.to_lowercase(), label.to_string())
            }
            View::Playlists | View::Recent | View::MostPlayed | View::Songs => return None,
        })
    }

    /// Groups for a view, filtered by a search. Playlists list in their own order.
    pub fn groups(&self, view: View, query: &str) -> Vec<Group> {
        let q = query.to_lowercase();
        if view == View::Playlists {
            return self
                .playlists
                .iter()
                .enumerate()
                .map(|(i, p)| Group {
                    key: i.to_string(),
                    label: p.name.clone(),
                    count: p.tracks.len(),
                })
                .filter(|g| q.is_empty() || g.label.to_lowercase().contains(&q))
                .collect();
        }
        let mut map: BTreeMap<String, Group> = BTreeMap::new();
        for t in self.tracks.values().filter(|t| t.matches(&q)) {
            if let Some((key, label)) = self.group_key(view, t) {
                map.entry(key.clone())
                    .or_insert(Group {
                        key,
                        label,
                        count: 0,
                    })
                    .count += 1;
            }
        }
        map.into_values().collect()
    }

    /// Track ids for a view (and group, for grouped views), in display order.
    pub fn list(&self, view: View, group: Option<&str>, query: &str) -> Vec<String> {
        let q = query.to_lowercase();
        if view == View::Playlists {
            let Some(p) = group
                .and_then(|g| g.parse::<usize>().ok())
                .and_then(|i| self.playlists.get(i))
            else {
                return Vec::new();
            };
            return p
                .tracks
                .iter()
                .filter(|id| self.tracks.get(*id).is_some_and(|t| t.matches(&q)))
                .cloned()
                .collect();
        }
        let mut v: Vec<&Track> = self
            .tracks
            .values()
            .filter(|t| t.matches(&q))
            .filter(|t| match group {
                Some(g) => self.group_key(view, t).is_some_and(|(k, _)| k == g),
                None => !view.grouped(),
            })
            .collect();
        let album_order = |t: &&Track| {
            (
                t.album().to_lowercase(),
                t.tags.disc,
                t.tags.track_no,
                t.title().to_lowercase(),
            )
        };
        match view {
            View::Recent => v.sort_by(|a, b| b.modified.cmp(&a.modified)),
            View::MostPlayed => {
                v.retain(|t| t.plays > 0);
                v.sort_by_key(|t| std::cmp::Reverse((t.plays, t.last_played)));
            }
            View::Tempo => {
                v.sort_by(|a, b| a.bpm().unwrap_or(0.0).total_cmp(&b.bpm().unwrap_or(0.0)))
            }
            View::Folders => v.sort_by_key(|t| t.path.to_lowercase()),
            View::Songs | View::Genres => v.sort_by_key(|t| t.title().to_lowercase()),
            _ => v.sort_by_key(album_order),
        }
        v.into_iter().map(|t| t.id.clone()).collect()
    }

    pub fn new_playlist(&mut self, name: &str) -> usize {
        self.playlists.push(Playlist {
            name: name.to_string(),
            tracks: Vec::new(),
        });
        self.playlists.len() - 1
    }
}

/// Tags read from a file win; the path's guesses fill what the file leaves empty.
fn fill(mut tags: Tags, guess: Tags) -> Tags {
    for (t, g) in [
        (&mut tags.title, guess.title),
        (&mut tags.artist, guess.artist),
        (&mut tags.album, guess.album),
    ] {
        if t.trim().is_empty() {
            *t = g;
        }
    }
    if tags.track_no == 0 {
        tags.track_no = guess.track_no;
    }
    tags
}

/// Guess tags from a conventional `Artist/Album/NN Title.ext` layout under the root.
pub fn guess_tags(root: &str, path: &str) -> Tags {
    let rel = path
        .strip_prefix(root)
        .unwrap_or(path)
        .trim_start_matches('/');
    let parts: Vec<&str> = rel.split('/').collect();
    let name = parts.last().copied().unwrap_or("");
    let stem = name.rsplit_once('.').map_or(name, |(s, _)| s);
    let digits: String = stem.chars().take_while(|c| c.is_ascii_digit()).collect();
    let title = stem[digits.len()..]
        .trim_start_matches([' ', '-', '.', '_'])
        .trim();
    let mut tags = Tags {
        title: if title.is_empty() { stem } else { title }.to_string(),
        track_no: if digits.len() <= 3 {
            digits.parse().unwrap_or(0)
        } else {
            0
        },
        ..Default::default()
    };
    // "Artist - Title" file names.
    if let Some((a, t)) = tags.title.clone().split_once(" - ") {
        tags.artist = a.trim().to_string();
        tags.title = t.trim().to_string();
    }
    let n = parts.len();
    if n >= 3 {
        tags.artist = parts[n - 3].to_string();
        tags.album = parts[n - 2].to_string();
    } else if n == 2 {
        tags.album = parts[0].to_string();
    }
    tags
}

#[cfg(test)]
mod tests {
    use super::*;

    fn listed(id: &str, path: &str, hash: &str) -> Listed {
        Listed {
            id: id.into(),
            path: path.into(),
            hash: hash.into(),
            size: 1,
            modified: "2026-01-01T00:00:00Z".into(),
        }
    }

    fn sample() -> Catalog {
        let mut c = Catalog::new("/Music");
        c.merge_listing(&[
            listed("a", "/Music/Bonobo/Migration/01 Migration.mp3", "1"),
            listed("b", "/Music/Bonobo/Migration/02 Break Apart.m4a", "2"),
            listed("c", "/Music/Loose/Floating Points - Silhouettes.mp3", "3"),
            listed("d", "/Music/cover.jpg", "4"),
        ]);
        c
    }

    #[test]
    fn listing_guesses_tags_and_skips_non_media() {
        let c = sample();
        assert_eq!(c.tracks.len(), 3);
        let a = &c.tracks["a"];
        assert_eq!(
            (a.artist(), a.album(), a.title()),
            ("Bonobo", "Migration", "Migration")
        );
        assert_eq!(a.tags.track_no, 1);
        let loose = &c.tracks["c"];
        assert_eq!(
            (loose.artist(), loose.title()),
            ("Floating Points", "Silhouettes")
        );
    }

    #[test]
    fn changed_files_lose_analysis_but_keep_history() {
        let mut c = sample();
        c.set_beats(
            "a",
            &BeatMap {
                bpm: 120.0,
                beats: vec![0.5, 1.0],
            },
        );
        c.played("a", 10);
        c.new_playlist("Mix");
        c.playlists[0].tracks = vec!["a".into(), "b".into()];
        let mut files = vec![
            listed("a", "/Music/Bonobo/Migration/01 Migration.mp3", "1-new"),
            listed("c", "/Music/Loose/Floating Points - Silhouettes.mp3", "3"),
        ];
        assert!(c.merge_listing(&files));
        assert!(c.tracks["a"].beats.is_none());
        assert_eq!(c.tracks["a"].plays, 1);
        assert_eq!(c.playlists[0].tracks, vec!["a".to_string()]);
        files.truncate(2);
        assert!(
            !c.merge_listing(&files),
            "same listing again changes nothing"
        );
    }

    #[test]
    fn file_tags_win_over_guesses() {
        let mut c = sample();
        let tags = Tags {
            title: "Migration (Remaster)".into(),
            genre: "Electronic".into(),
            year: 2017,
            ..Default::default()
        };
        c.set_tags("a", tags, 300.0);
        let a = &c.tracks["a"];
        assert_eq!(a.title(), "Migration (Remaster)");
        assert_eq!(a.artist(), "Bonobo");
        assert!(a.tagged);
    }

    #[test]
    fn views_group_and_order() {
        let mut c = sample();
        c.set_beats(
            "b",
            &BeatMap {
                bpm: 128.0,
                beats: vec![],
            },
        );
        let artists = c.groups(View::Artists, "");
        assert_eq!(
            artists
                .iter()
                .map(|g| (g.label.as_str(), g.count))
                .collect::<Vec<_>>(),
            [("Bonobo", 2), ("Floating Points", 1)]
        );
        assert_eq!(c.list(View::Artists, Some("bonobo"), ""), ["a", "b"]);
        let tempo = c.groups(View::Tempo, "");
        assert_eq!(tempo[0].label, "Dance · 125–140");
        assert_eq!(c.list(View::Songs, None, "silhou"), ["c"]);
        assert!(c.list(View::MostPlayed, None, "").is_empty());
        let folders = c.groups(View::Folders, "");
        assert_eq!(folders[0].label, "Bonobo/Migration");
    }

    #[test]
    fn beats_round_trip_through_storage() {
        let m = BeatMap {
            bpm: 123.4,
            beats: vec![0.25, 0.737, 1.224],
        };
        let stored = StoredBeats::from(&m);
        let back = stored.to_map().unwrap();
        assert_eq!(back.bpm, m.bpm);
        // A map from an older mapper is dropped (made again on next play).
        assert!(StoredBeats { v: 1, ..stored }.to_map().is_none());
        for (a, b) in back.beats.iter().zip(&m.beats) {
            assert!((a - b).abs() < 0.001);
        }
    }

    #[test]
    fn resumed_scan_lists_only_unfinished_folders() {
        let mut st = ScanState::default();
        let all = ["/M/A".to_string(), "/M/B".into(), "/M/C".into()];
        assert_eq!(st.plan(1, &all).len(), 3);
        st.folders.get_mut("/M/A").unwrap().done = true;
        st.folders.get_mut("/M/B").unwrap().cursor = Some("half".into());
        // Interrupted. Since then /M/C was deleted and /M/D added.
        let now = ["/M/A".to_string(), "/M/B".into(), "/M/D".into()];
        let todo = st.plan(1, &now);
        assert_eq!(
            todo,
            vec![
                ("/M/B".to_string(), Some("half".to_string())),
                ("/M/D".into(), None)
            ]
        );
        assert!(!st.folders.contains_key("/M/C"));
    }

    #[test]
    fn full_scan_removes_unseen_tracks_only_at_the_end() {
        let mut c = sample();
        c.new_playlist("p");
        c.playlists[0].tracks = vec!["a".into(), "c".into()];
        c.scan.restart();
        c.scan.pass += 1;
        // A resumed scan sees the files in two batches; "c" is gone.
        c.add_listing(&[listed("a", "/Music/Bonobo/Migration/01 Migration.mp3", "1")]);
        assert_eq!(c.tracks.len(), 3, "nothing removed mid-scan");
        c.add_listing(&[listed(
            "b",
            "/Music/Bonobo/Migration/02 Break Apart.m4a",
            "2",
        )]);
        assert!(c.finish_full_scan());
        assert!(c.scan.complete);
        assert_eq!(c.tracks.keys().collect::<Vec<_>>(), ["a", "b"]);
        assert_eq!(c.playlists[0].tracks, ["a"]);
    }

    #[test]
    fn changes_apply_in_order_and_moves_keep_history() {
        let mut c = sample();
        c.played("a", 5);
        // "a" moved; the Loose folder was deleted, then a new file appeared there.
        c.delete_path("/music/bonobo/migration/01 migration.mp3");
        c.delete_path("/Music/Loose");
        c.add_listing(&[
            listed("a", "/Music/Best/01 Migration.mp3", "1"),
            listed("e", "/Music/Loose/New.mp3", "5"),
        ]);
        assert!(c.finish_changes());
        assert_eq!(c.tracks.keys().collect::<Vec<_>>(), ["a", "b", "e"]);
        assert_eq!(c.tracks["a"].plays, 1);
        assert!(c.scan.gone.is_empty());
    }
}
