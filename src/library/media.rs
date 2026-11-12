//! Decoding mp3 and m4a (Symphonia): tags, streaming playback and whole-track analysis.

use super::catalog::{Tags, TAGS_VERSION};
use crate::beatmap::{BeatMap, Tracker};
use crate::loudness::{Loudness, Meter};
use std::io::Cursor;
use std::sync::Arc;
use symphonia::core::codecs::audio::{AudioDecoder, AudioDecoderOptions};
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo, TrackType};
use symphonia::core::io::{MediaSourceStream, MediaSourceStreamOptions};
use symphonia::core::meta::{MetadataOptions, StandardTag};
use symphonia::core::units::Time;

pub type Result<T> = std::result::Result<T, String>;

fn open(bytes: Arc<Vec<u8>>, ext: &str) -> Result<Box<dyn FormatReader>> {
    let mss = MediaSourceStream::new(
        Box::new(Cursor::new(ArcBytes(bytes))),
        MediaSourceStreamOptions::default(),
    );
    let mut hint = Hint::new();
    hint.with_extension(ext);
    symphonia::default::get_probe()
        .probe(
            &hint,
            mss,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .map_err(|e| format!("can't read this file: {e}"))
}

/// Shared bytes that Symphonia can read without copying.
struct ArcBytes(Arc<Vec<u8>>);
impl AsRef<[u8]> for ArcBytes {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

/// Tags and duration (seconds, 0 if unknown). Works on the first few hundred KB of most
/// files, since tags usually sit at the front.
pub fn read_tags(bytes: Arc<Vec<u8>>, ext: &str) -> Result<(Tags, f32)> {
    let mut format = open(bytes, ext)?;
    let mut tags = Tags::default();
    let mut meta = format.metadata();
    if let Some(rev) = meta.skip_to_latest() {
        let all = rev
            .media
            .tags
            .iter()
            .chain(rev.per_track.iter().flat_map(|t| t.metadata.tags.iter()));
        for tag in all {
            let Some(std) = &tag.std else { continue };
            match std {
                StandardTag::TrackTitle(s) => tags.title = s.to_string(),
                StandardTag::Artist(s) => tags.artist = s.to_string(),
                StandardTag::Album(s) => tags.album = s.to_string(),
                StandardTag::AlbumArtist(s) => tags.album_artist = s.to_string(),
                StandardTag::Genre(s) => tags.genre = s.to_string(),
                StandardTag::TrackNumber(n) => tags.track_no = *n as u16,
                StandardTag::DiscNumber(n) => tags.disc = *n as u16,
                StandardTag::CompilationFlag(c) => tags.compilation = *c,
                StandardTag::ReplayGainTrackGain(g) => tags.gain = parse_gain(g),
                StandardTag::ReleaseYear(y) | StandardTag::RecordingYear(y) => tags.year = *y,
                StandardTag::ReleaseDate(s) | StandardTag::RecordingDate(s) if tags.year == 0 => {
                    tags.year = s.get(..4).and_then(|y| y.parse().ok()).unwrap_or(0)
                }
                _ => {}
            }
        }
    }
    tags.v = TAGS_VERSION;
    Ok((tags, duration(format.as_ref())))
}

/// "-7.32 dB" → -7.32.
fn parse_gain(s: &str) -> Option<f32> {
    let n = s
        .trim()
        .trim_end_matches(|c: char| c.is_alphabetic())
        .trim();
    n.parse()
        .ok()
        .filter(|g: &f32| g.is_finite() && g.abs() < 60.0)
}

fn duration(format: &dyn FormatReader) -> f32 {
    let Some(t) = format.default_track(TrackType::Audio) else {
        return 0.0;
    };
    match (t.time_base, t.duration) {
        (Some(tb), Some(d)) => tb.calc_duration(d).map_or(0.0, |t| t.as_secs_f64() as f32),
        _ => 0.0,
    }
}

/// A streaming decoder: interleaved f32 blocks at the file's own rate.
pub struct Decoder {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn AudioDecoder>,
    track: u32,
    pub rate: u32,
    pub channels: usize,
    pub duration: f64,
    buf: Vec<f32>,
}

impl Decoder {
    pub fn new(bytes: Arc<Vec<u8>>, ext: &str) -> Result<Self> {
        let format = open(bytes, ext)?;
        let t = format
            .default_track(TrackType::Audio)
            .ok_or("no audio in this file")?;
        let params = t
            .codec_params
            .as_ref()
            .and_then(|p| p.audio())
            .ok_or("no audio in this file")?;
        let decoder = symphonia::default::get_codecs()
            .make_audio_decoder(params, &AudioDecoderOptions::default())
            .map_err(|e| format!("can't decode this file: {e}"))?;
        let rate = params.sample_rate.unwrap_or(44_100);
        let channels = params.channels.as_ref().map_or(2, |c| c.count()).max(1);
        let track = t.id;
        let duration = duration(format.as_ref()) as f64;
        Ok(Decoder {
            format,
            decoder,
            track,
            rate,
            channels,
            duration,
            buf: Vec::new(),
        })
    }

    /// The next block of interleaved samples and its channel count, or None at the end.
    pub fn next_block(&mut self) -> Option<(&[f32], usize)> {
        loop {
            let packet = self.format.next_packet().ok()??;
            if packet.track_id != self.track {
                continue;
            }
            match self.decoder.decode(&packet) {
                Ok(buf) => {
                    // The stream's channel count is what the decoder actually produces.
                    self.channels = buf.spec().channels().count().max(1);
                    self.rate = buf.spec().rate();
                    buf.copy_to_vec_interleaved(&mut self.buf);
                    return Some((&self.buf, self.channels));
                }
                Err(symphonia::core::errors::Error::DecodeError(_)) => continue,
                Err(_) => return None,
            }
        }
    }

    /// Seek to `secs`; returns where playback actually resumes.
    pub fn seek(&mut self, secs: f64) -> f64 {
        let time = Time::try_from_secs_f64(secs.max(0.0)).unwrap_or_default();
        match self.format.seek(
            SeekMode::Coarse,
            SeekTo::Time {
                time,
                track_id: Some(self.track),
            },
        ) {
            Ok(_) => {
                self.decoder.reset();
                secs
            }
            Err(_) => 0.0,
        }
    }
}

/// Decode a whole track once: its beat map (None if it has no steady beat) and duration.
/// What one decode of a whole track learns.
pub struct Analysis {
    pub map: Option<BeatMap>,
    /// Seconds.
    pub duration: f32,
    pub loudness: Option<Loudness>,
}

/// Decode the whole track once: beat map, duration and loudness.
pub fn analyze(bytes: Arc<Vec<u8>>, ext: &str) -> Result<Analysis> {
    let mut d = Decoder::new(bytes, ext)?;
    let rate = d.rate;
    let mut tracker = Tracker::new(rate as f32);
    let mut meter = Meter::new(rate as f32);
    let mut mono = Vec::new();
    let mut frames = 0u64;
    while let Some((block, ch)) = d.next_block() {
        mono.clear();
        mono.extend(block.chunks(ch).map(|f| f.iter().sum::<f32>() / ch as f32));
        frames += mono.len() as u64;
        tracker.push(&mono);
        meter.push(block, ch);
    }
    Ok(Analysis {
        map: tracker.finish(),
        duration: frames as f32 / rate as f32,
        loudness: meter.finish(),
    })
}
