//! The media player: a play queue, an output stream that decodes and resamples as it
//! plays, and background fetches that download and beat-map the current and next track.
//!
//! What it plays is pushed into its own analyzer `Feed`, exactly like the demo beat, so
//! the visuals react to the real output. Beats come from the track's beat map when it has
//! one (`heard`, `map`), otherwise from the live analyzer.

use super::audio::{Feed, SharedFeed};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use polymoire::beatmap::BeatMap;
use polymoire::library::catalog::{Catalog, Tags};
use polymoire::library::dropbox::Dropbox;
use polymoire::library::media::{self, Decoder};
use polymoire::rng::Rng;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// Downloaded tracks kept in memory: the current one, the next, and the last.
const CACHE: usize = 3;

/// Recently downloaded files (track id, bytes), oldest first.
type Cache = Arc<Mutex<Vec<(String, Arc<Vec<u8>>)>>>;

/// What background fetches report.
pub enum Fetched {
    Bytes(String, Arc<Vec<u8>>),
    /// Beat map, duration and (from the whole file) tags.
    Analysis(String, Option<BeatMap>, f32, Option<Tags>),
    Failed(String, String),
}

/// State shared with the audio callback.
struct Deck {
    dec: Option<Decoder>,
    /// Decoded, not yet played source frames (interleaved) and the read position in them.
    src: Vec<f32>,
    src_ch: usize,
    frac: f64,
    /// Track time of the next frame to be output, in seconds.
    pos: f64,
    playing: bool,
    ended: bool,
    volume: f32,
    seek: Option<f64>,
    /// (track time at the end of the last block, when it was handed over, output latency).
    anchor: Option<(f64, Instant, f64)>,
}

impl Deck {
    /// Fill `out` (interleaved, `out_ch` channels at `out_rate`); `mono` gets the
    /// pre-volume mix for analysis. Runs on the audio thread.
    fn fill(&mut self, out: &mut [f32], out_ch: usize, out_rate: f64, mono: &mut Vec<f32>) {
        mono.clear();
        out.fill(0.0);
        let Some(dec) = self.dec.as_mut() else { return };
        if let Some(t) = self.seek.take() {
            self.pos = dec.seek(t);
            self.src.clear();
            self.frac = 0.0;
        }
        if !self.playing {
            return;
        }
        let ratio = dec.rate as f64 / out_rate;
        for frame in out.chunks_mut(out_ch) {
            let i = self.frac as usize;
            while (i + 2) * self.src_ch > self.src.len() {
                match dec.next_block() {
                    Some((block, ch)) => {
                        self.src_ch = ch;
                        self.src.extend_from_slice(block);
                    }
                    None => {
                        self.playing = false;
                        self.ended = true;
                        return;
                    }
                }
            }
            let (ch, a) = (self.src_ch, (self.frac - i as f64) as f32);
            let at = |c: usize| {
                let (x, y) = (self.src[i * ch + c], self.src[(i + 1) * ch + c]);
                x + (y - x) * a
            };
            for (c, o) in frame.iter_mut().enumerate() {
                *o = at(c.min(ch - 1)) * self.volume;
            }
            mono.push((0..ch).map(at).sum::<f32>() / ch as f32);
            self.frac += ratio;
        }
        let used = self.frac as usize;
        self.src.drain(..(used * self.src_ch).min(self.src.len()));
        self.frac -= used as f64;
        self.pos += mono.len() as f64 / out_rate;
    }
}

pub struct Player {
    deck: Arc<Mutex<Deck>>,
    pub feed: SharedFeed,
    stream: Option<cpal::Stream>,
    cache: Cache,
    /// Downloads and analyses under way (track ids).
    downloading: Vec<String>,
    analyzing: Vec<String>,
    tx: Sender<Fetched>,
    rx: Receiver<Fetched>,
    pub queue: Vec<String>,
    pub index: usize,
    /// The track playing or loading.
    pub current: Option<String>,
    pub map: Option<BeatMap>,
    pub status: String,
    pub shuffle: bool,
    rng: Rng,
    /// The current track has started making sound (counted as a play).
    counted: bool,
}

impl Player {
    pub fn new(volume: f32, shuffle: bool) -> Self {
        let (tx, rx) = channel();
        Player {
            deck: Arc::new(Mutex::new(Deck {
                dec: None,
                src: Vec::new(),
                src_ch: 2,
                frac: 0.0,
                pos: 0.0,
                playing: false,
                ended: false,
                volume,
                seek: None,
                anchor: None,
            })),
            feed: Arc::new(Mutex::new(None)),
            stream: None,
            cache: Arc::new(Mutex::new(Vec::new())),
            downloading: Vec::new(),
            analyzing: Vec::new(),
            tx,
            rx,
            queue: Vec::new(),
            index: 0,
            current: None,
            map: None,
            status: "Pick something from the library (L)".into(),
            shuffle,
            rng: Rng::from_time(),
            counted: false,
        }
    }

    fn deck(&self) -> std::sync::MutexGuard<'_, Deck> {
        self.deck.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn playing(&self) -> bool {
        self.deck().playing
    }
    pub fn position(&self) -> f64 {
        self.deck().pos
    }
    pub fn duration(&self) -> f64 {
        self.deck().dec.as_ref().map_or(0.0, |d| d.duration)
    }
    pub fn volume(&self) -> f32 {
        self.deck().volume
    }
    pub fn set_volume(&self, v: f32) {
        self.deck().volume = v.clamp(0.0, 1.0);
    }
    pub fn seek(&self, secs: f64) {
        self.deck().seek = Some(secs);
    }

    /// Track time the listener is hearing now, if sound is coming out.
    pub fn heard(&self) -> Option<f64> {
        let d = self.deck();
        let (pos, at, latency) = d.anchor?;
        let age = at.elapsed().as_secs_f64();
        (d.playing && age < 0.5).then(|| pos + age.min(0.25) - latency)
    }

    pub fn toggle(&mut self) {
        let mut d = self.deck();
        if d.dec.is_some() {
            d.playing = !d.playing;
        }
    }

    pub fn stop(&mut self) {
        let mut d = self.deck();
        d.playing = false;
        d.dec = None;
        drop(d);
        self.stream = None;
        *self.feed.lock().unwrap() = None;
        self.current = None;
    }

    /// Play `ids` from `start` (shuffled after it, if shuffle is on).
    pub fn play_list(&mut self, ids: Vec<String>, start: usize, cat: &Catalog, dbx: &Arc<Dropbox>) {
        self.queue = ids;
        self.index = start.min(self.queue.len().saturating_sub(1));
        if self.shuffle && self.queue.len() > 1 {
            self.queue.swap(0, self.index);
            self.rng.shuffle(&mut self.queue[1..]);
            self.index = 0;
        }
        self.start(cat, dbx);
    }

    /// Insert after the current track.
    pub fn play_next(&mut self, id: String) {
        let at = (self.index + 1).min(self.queue.len());
        self.queue.insert(at, id);
    }

    pub fn enqueue(&mut self, id: String) {
        self.queue.push(id);
    }

    pub fn skip(&mut self, dir: i32, cat: &Catalog, dbx: &Arc<Dropbox>) {
        // "Previous" restarts the track unless it's just begun.
        if dir < 0 && self.position() > 3.0 {
            self.seek(0.0);
            return;
        }
        let next = self.index as i64 + dir as i64;
        if next < 0 || next as usize >= self.queue.len() {
            return;
        }
        self.index = next as usize;
        self.start(cat, dbx);
    }

    fn start(&mut self, cat: &Catalog, dbx: &Arc<Dropbox>) {
        let Some(id) = self.queue.get(self.index).cloned() else {
            return;
        };
        {
            let mut d = self.deck();
            d.dec = None;
            d.playing = false;
            d.ended = false;
        }
        self.map = cat
            .tracks
            .get(&id)
            .and_then(|t| t.beats.as_ref())
            .map(|b| b.to_map());
        self.current = Some(id.clone());
        self.counted = false;
        match self.cached(&id) {
            Some(bytes) => self.open(&id, bytes, cat),
            None => self.status = "Downloading…".into(),
        }
        self.fetch(&id, cat, dbx);
        if let Some(next) = self.queue.get(self.index + 1).cloned() {
            self.fetch(&next, cat, dbx);
        }
    }

    fn cached(&self, id: &str) -> Option<Arc<Vec<u8>>> {
        let c = self.cache.lock().unwrap();
        c.iter().find(|(k, _)| k == id).map(|(_, b)| b.clone())
    }

    /// Download (unless cached) and, if it has no beat map yet, analyze, on a thread.
    fn fetch(&mut self, id: &str, cat: &Catalog, dbx: &Arc<Dropbox>) {
        let Some(t) = cat.tracks.get(id) else { return };
        let busy = |v: &Vec<String>| v.iter().any(|i| i == id);
        let needs_analysis = (t.beats.is_none() || t.duration <= 0.0) && !busy(&self.analyzing);
        let hit = self.cached(id);
        if busy(&self.downloading) || (hit.is_some() && !needs_analysis) {
            return;
        }
        if hit.is_none() {
            self.downloading.push(id.to_string());
        }
        if needs_analysis {
            self.analyzing.push(id.to_string());
        }
        let (id, ext, tx, dbx, cache) = (
            id.to_string(),
            t.ext().to_lowercase(),
            self.tx.clone(),
            dbx.clone(),
            self.cache.clone(),
        );
        std::thread::spawn(move || {
            let bytes = match hit {
                Some(b) => b,
                None => match dbx.download(&id, None) {
                    Ok(b) => {
                        let b = Arc::new(b);
                        let mut c = cache.lock().unwrap();
                        c.push((id.clone(), b.clone()));
                        let excess = c.len().saturating_sub(CACHE);
                        c.drain(..excess);
                        b
                    }
                    Err(e) => return drop(tx.send(Fetched::Failed(id, e))),
                },
            };
            let _ = tx.send(Fetched::Bytes(id.clone(), bytes.clone()));
            if needs_analysis {
                let tags = media::read_tags(bytes.clone(), &ext).ok().map(|(t, _)| t);
                let msg = match media::analyze(bytes, &ext) {
                    Ok((map, secs)) => Fetched::Analysis(id, map, secs, tags),
                    Err(e) => Fetched::Failed(id, e),
                };
                let _ = tx.send(msg);
            }
        });
    }

    fn open(&mut self, id: &str, bytes: Arc<Vec<u8>>, cat: &Catalog) {
        let ext = cat
            .tracks
            .get(id)
            .map(|t| t.ext().to_lowercase())
            .unwrap_or_default();
        match Decoder::new(bytes, &ext) {
            Ok(dec) => {
                if let Err(e) = self.ensure_stream() {
                    self.status = e;
                    return;
                }
                let mut d = self.deck();
                d.dec = Some(dec);
                d.src.clear();
                d.frac = 0.0;
                d.pos = 0.0;
                d.playing = true;
                drop(d);
                self.status = String::new();
            }
            Err(e) => self.status = e,
        }
    }

    /// Call every frame. Returns finished analyses for the catalog, and whether a track
    /// started playing (to count the play).
    pub fn update(
        &mut self,
        cat: &Catalog,
        dbx: Option<&Arc<Dropbox>>,
    ) -> (Vec<Fetched>, Option<String>) {
        let mut out = Vec::new();
        while let Ok(msg) = self.rx.try_recv() {
            match &msg {
                Fetched::Bytes(id, bytes) => {
                    self.downloading.retain(|i| i != id);
                    let opened = self.deck().dec.is_some();
                    if self.current.as_deref() == Some(id) && !opened {
                        self.open(&id.clone(), bytes.clone(), cat);
                    }
                    continue;
                }
                Fetched::Analysis(id, map, ..) => {
                    self.analyzing.retain(|i| i != id);
                    if self.current.as_deref() == Some(id) {
                        self.map = map.clone();
                    }
                }
                Fetched::Failed(id, e) => {
                    self.downloading.retain(|i| i != id);
                    self.analyzing.retain(|i| i != id);
                    if self.current.as_deref() == Some(id) {
                        self.status = e.clone();
                    }
                }
            }
            out.push(msg);
        }
        let ended = std::mem::take(&mut self.deck().ended);
        if ended {
            match dbx {
                Some(dbx) if self.index + 1 < self.queue.len() => self.skip(1, cat, dbx),
                _ => self.status = "End of the queue".into(),
            }
        }
        let mut started = None;
        if !self.counted && self.heard().is_some() {
            self.counted = true;
            started = self.current.clone();
        }
        (out, started)
    }

    fn ensure_stream(&mut self) -> Result<(), String> {
        if self.stream.is_some() {
            return Ok(());
        }
        let device = cpal::default_host()
            .default_output_device()
            .ok_or("No audio output device found")?;
        let supported = device.default_output_config().map_err(|e| e.to_string())?;
        let channels = supported.channels() as usize;
        let sr = supported.sample_rate() as f64;
        *self.feed.lock().unwrap() = Some(Feed::new(sr as f32));
        let (deck, feed) = (self.deck.clone(), self.feed.clone());
        let mut buf: Vec<f32> = Vec::new();
        let mut mono: Vec<f32> = Vec::new();

        macro_rules! build {
            ($t:ty) => {
                device.build_output_stream::<$t, _, _>(
                    supported.config(),
                    move |data: &mut [$t], info: &cpal::OutputCallbackInfo| {
                        buf.resize(data.len(), 0.0);
                        let Ok(mut d) = deck.lock() else { return };
                        d.fill(&mut buf, channels, sr, &mut mono);
                        for (o, &v) in data.iter_mut().zip(&buf) {
                            *o = <$t as cpal::Sample>::from_sample::<f32>(v);
                        }
                        if mono.is_empty() {
                            return;
                        }
                        let ts = info.timestamp();
                        let latency = ts.playback.duration_since(ts.callback).as_secs_f64()
                            + mono.len() as f64 / sr;
                        d.anchor = Some((d.pos, Instant::now(), latency));
                        drop(d);
                        if let Ok(mut g) = feed.lock() {
                            if let Some(f) = g.as_mut() {
                                f.push(&mono, latency);
                            }
                        }
                    },
                    |e| eprintln!("player output error: {e}"),
                    None,
                )
            };
        }

        let stream = match supported.sample_format() {
            cpal::SampleFormat::F32 => build!(f32),
            cpal::SampleFormat::I16 => build!(i16),
            cpal::SampleFormat::U16 => build!(u16),
            cpal::SampleFormat::I32 => build!(i32),
            other => return Err(format!("unsupported output format {other}")),
        }
        .map_err(|e| e.to_string())?;
        stream.play().map_err(|e| e.to_string())?;
        self.stream = Some(stream);
        Ok(())
    }
}
