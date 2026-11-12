//! Offline beat mapping for whole tracks (the media player analyzes a track before it
//! plays). With the whole track in hand it can do better than the live tracker:
//!
//! 1. an onset envelope from eight log-spaced bands (energy rises, summed);
//! 2. one tempo for the track from the envelope's autocorrelation, with a mild preference
//!    for ~120 bpm to settle half/double-tempo ties;
//! 3. dynamic-programming beat placement (Ellis 2007): each beat sits on strong onsets
//!    while keeping intervals close to the tempo, so syncopation and breakdowns don't
//!    knock it off the grid, and slow tempo drift is followed.
//!
//! The tracker is streaming (`push` blocks, then `finish`), so a track never has to be
//! held in memory as decoded audio.

use crate::dsp::{Biquad, FilterKind};
use serde::{Deserialize, Serialize};

const HOP: usize = 256;
const BANDS: [f32; 8] = [60.0, 120.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0];
const MIN_BPM: f64 = 70.0;
const MAX_BPM: f64 = 180.0;
/// How strongly the beat placement holds to the tempo (Ellis's "tightness").
const TIGHTNESS: f32 = 100.0;
/// The tempo is measured over windows this long (seconds)...
const TEMPO_WINDOW: f64 = 8.0;
/// ...every this often.
const TEMPO_STEP: f64 = 1.0;
/// Cost of changing tempo between steps, per squared log ratio: small drifts are
/// nearly free, a jump needs a few windows of evidence.
const TEMPO_CHANGE: f32 = 40.0;
/// Weight of the whole-track tempo in each window, so windows with little rhythm
/// (breakdowns) don't wander or flip octave.
const GLOBAL_PULL: f32 = 0.3;

/// Beat times for one track, in seconds from the start of the track.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct BeatMap {
    pub bpm: f32,
    pub beats: Vec<f64>,
}

impl BeatMap {
    /// True if a beat falls in `(from, to]`.
    pub fn beat_in(&self, from: f64, to: f64) -> bool {
        let i = self.beats.partition_point(|&b| b <= from);
        self.beats.get(i).is_some_and(|&b| b <= to)
    }

    /// The local beat period around `t`.
    pub fn period_at(&self, t: f64) -> f64 {
        let i = self
            .beats
            .partition_point(|&b| b <= t)
            .clamp(1, self.beats.len().max(2) - 1);
        match (self.beats.get(i - 1), self.beats.get(i)) {
            (Some(a), Some(b)) if b > a => b - a,
            _ => 60.0 / self.bpm.max(1.0) as f64,
        }
    }
}

/// Compact storage. Beat times are rounded to milliseconds and stored as the change in
/// beat interval from one beat to the next (zero while the tempo holds), with runs of
/// zeros collapsed, as varint bytes in base64url text. A steady track costs a few bytes;
/// a live drummer about one byte per beat.
impl BeatMap {
    pub fn encode(&self) -> String {
        let ms: Vec<i64> = self
            .beats
            .iter()
            .map(|&b| (b * 1000.0).round().max(0.0) as i64)
            .collect();
        let mut values = Vec::with_capacity(ms.len());
        let (mut prev, mut gap) = (0i64, 0i64);
        for (i, &t) in ms.iter().enumerate() {
            let g = t - prev;
            // First: the time itself. Second: the first interval. Then interval changes.
            values.push(if i < 2 { g } else { g - gap });
            gap = g;
            prev = t;
        }
        let mut bytes = Vec::new();
        let mut i = 0;
        while i < values.len() {
            // Never collapse the first two: they're absolute, not changes.
            let run = if i >= 2 {
                values[i..].iter().take_while(|&&v| v == 0).count()
            } else {
                0
            };
            if run > 0 {
                varint(&mut bytes, (run as u64) << 1 | 1);
                i += run;
            } else {
                let v = values[i];
                varint(&mut bytes, ((v << 1) ^ (v >> 63)) as u64 * 2);
                i += 1;
            }
        }
        b64_encode(&bytes)
    }

    pub fn decode(bpm: f32, text: &str) -> Option<BeatMap> {
        let bytes = b64_decode(text)?;
        let mut values = Vec::new();
        let mut at = 0;
        while at < bytes.len() {
            let token = read_varint(&bytes, &mut at)?;
            if token & 1 == 1 {
                values.extend(std::iter::repeat_n(0i64, (token >> 1) as usize));
            } else {
                let z = token >> 1;
                values.push((z >> 1) as i64 ^ -((z & 1) as i64));
            }
        }
        let (mut t, mut gap) = (0i64, 0i64);
        let beats = values
            .iter()
            .enumerate()
            .map(|(i, &v)| {
                gap = if i < 2 { v } else { gap + v };
                t += gap;
                t as f64 / 1000.0
            })
            .collect();
        Some(BeatMap { bpm, beats })
    }
}

fn varint(out: &mut Vec<u8>, mut v: u64) {
    while v >= 0x80 {
        out.push(v as u8 | 0x80);
        v >>= 7;
    }
    out.push(v as u8);
}

fn read_varint(bytes: &[u8], at: &mut usize) -> Option<u64> {
    let mut v = 0u64;
    for shift in (0..64).step_by(7) {
        let b = *bytes.get(*at)?;
        *at += 1;
        v |= ((b & 0x7f) as u64) << shift;
        if b < 0x80 {
            return Some(v);
        }
    }
    None
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

fn b64_encode(bytes: &[u8]) -> String {
    let mut out = String::new();
    for c in bytes.chunks(3) {
        let n = c
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, &b)| n | (b as u32) << (16 - 8 * i));
        for i in 0..=c.len() {
            out.push(B64[(n >> (18 - 6 * i) & 63) as usize] as char);
        }
    }
    out
}

fn b64_decode(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let (mut acc, mut bits) = (0u32, 0);
    for c in text.bytes() {
        acc = acc << 6 | B64.iter().position(|&b| b == c)? as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Some(out)
}

pub struct Tracker {
    sr: f32,
    filters: Vec<Biquad>,
    acc: [f32; BANDS.len()],
    n: usize,
    prev: Option<[f32; BANDS.len()]>,
    env: Vec<f32>,
}

impl Tracker {
    pub fn new(sample_rate: f32) -> Self {
        let filters = BANDS
            .iter()
            .map(|&f| {
                let f = f.min(sample_rate * 0.45);
                Biquad::new(FilterKind::BandPass, sample_rate, f, 1.2)
            })
            .collect();
        Tracker {
            sr: sample_rate,
            filters,
            acc: [0.0; BANDS.len()],
            n: 0,
            prev: None,
            env: Vec::new(),
        }
    }

    /// Feed mono samples.
    pub fn push(&mut self, mono: &[f32]) {
        for &x in mono {
            for (a, f) in self.acc.iter_mut().zip(&mut self.filters) {
                let y = f.process(x);
                *a += y * y;
            }
            self.n += 1;
            if self.n == HOP {
                let e = self.acc.map(|a| (1e-7 + a / HOP as f32).ln());
                let flux = match self.prev {
                    Some(p) => e.iter().zip(p).map(|(e, p)| (e - p).max(0.0)).sum(),
                    None => 0.0,
                };
                self.env.push(flux);
                self.prev = Some(e);
                self.acc = [0.0; BANDS.len()];
                self.n = 0;
            }
        }
    }

    /// Seconds per envelope frame.
    fn hop_secs(&self) -> f64 {
        HOP as f64 / self.sr as f64
    }

    /// The beat map, or None if the track is too short or has no rhythm to speak of.
    pub fn finish(self) -> Option<BeatMap> {
        let hop = self.hop_secs();
        let o = normalize(&self.env, (1.0 / hop) as usize);
        let period = tempo_curve(&o, hop)?;
        let beats = place_beats(&o, &period, TIGHTNESS);
        if beats.len() < 8 {
            return None;
        }
        // The flux at frame k measures the rise from frame k-1 to k; its onset sits about
        // half a frame before the frame's end.
        let beats: Vec<f64> = refine(&o, &beats, &period)
            .iter()
            .map(|&k| (k + 0.5) * hop)
            .collect();
        // The typical tempo: the median beat interval.
        let mut gaps: Vec<f64> = beats.windows(2).map(|w| w[1] - w[0]).collect();
        gaps.sort_by(f64::total_cmp);
        Some(BeatMap {
            bpm: (60.0 / gaps[gaps.len() / 2]) as f32,
            beats,
        })
    }
}

/// Remove the local mean (about one second), clip at zero, scale to unit deviation.
fn normalize(env: &[f32], win: usize) -> Vec<f32> {
    let win = win.max(1);
    let mut prefix = vec![0.0f64; env.len() + 1];
    for (i, &v) in env.iter().enumerate() {
        prefix[i + 1] = prefix[i] + v as f64;
    }
    let mut out: Vec<f32> = (0..env.len())
        .map(|i| {
            let (a, b) = (i.saturating_sub(win / 2), (i + win / 2 + 1).min(env.len()));
            let mean = (prefix[b] - prefix[a]) / (b - a) as f64;
            (env[i] - mean as f32).max(0.0)
        })
        .collect();
    let sd = (out.iter().map(|v| v * v).sum::<f32>() / out.len().max(1) as f32).sqrt();
    if sd > 0.0 {
        out.iter_mut().for_each(|v| *v /= sd);
    }
    out
}

/// Score for a beat period of `l` frames, from the autocorrelation `raw` (a period also
/// correlates at twice its lag, which settles octave ties), with a mild 120 bpm prior.
fn lag_score(raw: &[f32], l: usize, hop: f64) -> f32 {
    let bpm = 60.0 / (l as f64 * hop);
    let prior = (-0.5 * ((bpm / 120.0).log2() / 0.9).powi(2)).exp() as f32;
    (raw[l] + 0.5 * raw[2 * l]) * prior
}

/// Parabolic refinement of a peak at `l` to a fractional lag.
fn refine_lag(raw: &[f32], l: usize) -> f64 {
    let (a, b, c) = (raw[l - 1], raw[l], raw[l + 1]);
    let d = a - 2.0 * b + c;
    let shift = if d < 0.0 {
        (0.5 * (a - c) / d).clamp(-0.5, 0.5)
    } else {
        0.0
    };
    l as f64 + shift as f64
}

/// The beat period in frames (fractional) at every frame of the envelope.
///
/// The tempo is measured in overlapping windows (autocorrelation over each), then a
/// Viterbi pass picks the path through the windows' candidate tempos that best fits
/// the evidence while charging for each change. Steady tracks come out flat; a real
/// tempo change shows up as a step or a ramp. Windows are blended linearly into
/// a per-frame curve.
fn tempo_curve(o: &[f32], hop: f64) -> Option<Vec<f64>> {
    let n = o.len();
    let lag_of = |bpm: f64| 60.0 / (bpm * hop);
    let (lo, hi) = (
        lag_of(MAX_BPM).floor() as usize,
        lag_of(MIN_BPM).ceil() as usize,
    );
    if n < hi * 8 {
        return None;
    }
    let win = ((TEMPO_WINDOW / hop) as usize).min(n);
    let step = ((TEMPO_STEP / hop) as usize).max(1);
    let mut starts: Vec<usize> = (0..=n - win).step_by(step).collect();
    if starts.last() != Some(&(n - win)) {
        starts.push(n - win);
    }
    let max_lag = 2 * hi + 1;
    // raw[w][l]: mean of o[i]*o[i+l] over window w (the lagged partner may run past the
    // window's end). One prefix sum per lag keeps this linear in the track length.
    let mut raw = vec![vec![0.0f32; max_lag + 1]; starts.len()];
    let mut global = vec![0.0f32; max_lag + 1];
    let mut prefix = vec![0.0f64; n + 1];
    for l in 1..=max_lag.min(n - 1) {
        for i in 0..n - l {
            prefix[i + 1] = prefix[i] + (o[i] * o[i + l]) as f64;
        }
        let m = n - l;
        global[l] = (prefix[m] / m as f64) as f32;
        for (w, &s) in starts.iter().enumerate() {
            let (a, b) = (s.min(m), (s + win).min(m));
            if b > a {
                raw[w][l] = ((prefix[b] - prefix[a]) / (b - a) as f64) as f32;
            }
        }
    }
    let best_global = (lo..=hi)
        .max_by(|&a, &b| lag_score(&global, a, hop).total_cmp(&lag_score(&global, b, hop)))?;
    if global[best_global] <= 0.0 {
        return None;
    }

    // Viterbi over lags lo..=hi, one step per window.
    let states = hi - lo + 1;
    let evidence =
        |w: usize, l: usize| lag_score(&raw[w], l, hop) + GLOBAL_PULL * lag_score(&global, l, hop);
    let change: Vec<Vec<f32>> = (0..states)
        .map(|a| {
            (0..states)
                .map(|b| {
                    let r = ((lo + a) as f32 / (lo + b) as f32).ln();
                    TEMPO_CHANGE * r * r
                })
                .collect()
        })
        .collect();
    let mut score: Vec<f32> = (0..states).map(|s| evidence(0, lo + s)).collect();
    let mut back = vec![vec![0u16; states]; starts.len()];
    for (w, back_w) in back.iter_mut().enumerate().skip(1) {
        let next: Vec<f32> = (0..states)
            .map(|s| {
                let (from, best) = (0..states)
                    .map(|p| (p, score[p] - change[p][s]))
                    .max_by(|a, b| a.1.total_cmp(&b.1))
                    .unwrap_or((s, 0.0));
                back_w[s] = from as u16;
                best + evidence(w, lo + s)
            })
            .collect();
        score = next;
    }
    let mut s = (0..states).max_by(|&a, &b| score[a].total_cmp(&score[b]))?;
    let mut lags = vec![0.0f64; starts.len()];
    for w in (0..starts.len()).rev() {
        let l = lo + s;
        lags[w] = if l > 1 && l < max_lag {
            refine_lag(&raw[w], l)
        } else {
            l as f64
        };
        s = back[w][s] as usize;
    }

    // Blend window centres into a per-frame curve.
    let centres: Vec<f64> = starts.iter().map(|&s| (s + win / 2) as f64).collect();
    Some(
        (0..n)
            .map(|t| {
                let t = t as f64;
                let j = centres.partition_point(|&c| c <= t);
                match (j.checked_sub(1), centres.get(j)) {
                    (Some(i), Some(&c1)) => {
                        let f = (t - centres[i]) / (c1 - centres[i]);
                        lags[i] + (lags[j] - lags[i]) * f
                    }
                    (None, _) => lags[0],
                    (Some(i), None) => lags[i],
                }
            })
            .collect(),
    )
}

/// Dynamic programming: the best chain of beats through the envelope at this period.
fn place_beats(o: &[f32], periods: &[f64], tightness: f32) -> Vec<usize> {
    let n = o.len();
    let mut score = vec![0.0f32; n];
    let mut back = vec![usize::MAX; n];
    for t in 0..n {
        let period = periods[t];
        let (min_gap, max_gap) = (
            (period * 0.5).round() as usize,
            (period * 2.0).round() as usize,
        );
        let best = (min_gap.max(1)..=max_gap.min(t))
            .map(|gap| {
                let penalty = -tightness * (gap as f64 / period).ln().powi(2) as f32;
                (score[t - gap] + penalty, t - gap)
            })
            .max_by(|a, b| a.0.total_cmp(&b.0));
        if let Some((s, p)) = best.filter(|b| b.0 > 0.0) {
            score[t] = o[t] + s;
            back[t] = p;
        } else {
            score[t] = o[t];
        }
    }
    // Start from the best-scoring frame within the last period.
    let tail = n.saturating_sub(periods[n - 1].ceil() as usize);
    let Some(mut t) = (tail..n).max_by(|&a, &b| score[a].total_cmp(&score[b])) else {
        return Vec::new();
    };
    let mut beats = vec![t];
    while back[t] != usize::MAX {
        t = back[t];
        beats.push(t);
    }
    beats.reverse();
    beats
}

/// Onset strength, against the track's median beat, that a beat needs to count as heard
/// rather than implied.
const STRONG: f32 = 0.5;

/// Sub-frame beat times. A beat on a clear onset moves to the onset's interpolated peak.
/// Beats without one (a breakdown with no drums) are spread evenly between the heard beats
/// either side, instead of snapping to whatever hat or stab is nearest.
fn refine(o: &[f32], beats: &[usize], periods: &[f64]) -> Vec<f64> {
    let at = |i: usize| o.get(i).copied().unwrap_or(0.0);
    let mut out: Vec<f64> = beats.iter().map(|&k| k as f64).collect();
    let peak = |k: usize| at(k).max(at(k + 1)).max(at(k.saturating_sub(1)));
    let mut typical: Vec<f32> = beats.iter().map(|&k| peak(k)).collect();
    typical.sort_by(f32::total_cmp);
    let floor = STRONG * typical.get(typical.len() / 2).copied().unwrap_or(0.0);
    let strong: Vec<bool> = beats.iter().map(|&k| peak(k) >= floor.max(1e-3)).collect();
    for (t, (&k, &s)) in out.iter_mut().zip(beats.iter().zip(&strong)) {
        if !s {
            continue;
        }
        let k = (k.saturating_sub(1)..=k + 1)
            .max_by(|&a, &b| at(a).total_cmp(&at(b)))
            .unwrap_or(k);
        let (a, b, c) = (at(k.wrapping_sub(1)), at(k), at(k + 1));
        let d = a - 2.0 * b + c;
        let shift = if k > 0 && d < 0.0 {
            (0.5 * (a - c) / d).clamp(-0.5, 0.5)
        } else {
            0.0
        };
        *t = k as f64 + shift as f64;
    }
    let anchors: Vec<usize> = (0..beats.len()).filter(|&i| strong[i]).collect();
    for w in anchors.windows(2) {
        let (i, j) = (w[0], w[1]);
        let step = (out[j] - out[i]) / (j - i) as f64;
        if j - i > 1 && (step / periods[beats[i]] - 1.0).abs() < 0.15 {
            for m in i + 1..j {
                out[m] = out[i] + step * (m - i) as f64;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::demo::{DemoSynth, BPM};

    fn map_demo(sr: f32, secs: f32) -> BeatMap {
        let mut synth = DemoSynth::new(sr);
        let mut tr = Tracker::new(sr);
        let mut buf = vec![0.0; 1024];
        for _ in 0..(sr * secs) as usize / buf.len() {
            synth.render(&mut buf);
            tr.push(&buf);
        }
        tr.finish().expect("demo has a beat")
    }

    #[test]
    fn demo_maps_to_its_true_grid() {
        for sr in [44_100.0, 48_000.0] {
            let m = map_demo(sr, 60.0);
            assert!((m.bpm - BPM).abs() < 1.0, "{sr}: {} bpm", m.bpm);
            let period = 60.0 / BPM as f64;
            let true_beats = (60.0 / period) as usize;
            // Every true beat (kicks, and the breakdown's kickless beats) has a mapped beat
            // within 10 ms, and there are no extras.
            let found = (1..true_beats - 1)
                .filter(|&k| {
                    let t = k as f64 * period;
                    m.beats.iter().any(|b| (b - t).abs() < 0.010)
                })
                .count();
            assert!(
                found >= true_beats - 3,
                "{sr}: {found}/{true_beats} beats on the grid"
            );
            assert!(
                m.beats.len() <= true_beats + 1,
                "{sr}: {} beats",
                m.beats.len()
            );
        }
    }

    /// Kicks on `beats` (seconds), with a hat halfway between each pair.
    fn kick_track(sr: f32, beats: &[f64], secs: f64) -> Vec<f32> {
        let mut out = vec![0.0f32; (secs * sr as f64) as usize];
        let mut hit = |at: f64, f0: f32, f1: f32, decay: f32, amp: f32| {
            let start = (at * sr as f64) as usize;
            let mut phase = 0.0f32;
            for i in 0..(0.25 * sr) as usize {
                let Some(o) = out.get_mut(start + i) else {
                    break;
                };
                let t = i as f32 / sr;
                let f = f1 + (f0 - f1) * (-t * 30.0).exp();
                phase += std::f32::consts::TAU * f / sr;
                *o += amp * phase.sin() * (-t * decay).exp();
            }
        };
        for (i, &b) in beats.iter().enumerate() {
            hit(b, 150.0, 50.0, 12.0, 0.8);
            if let Some(&n) = beats.get(i + 1) {
                hit((b + n) / 2.0, 7000.0, 7000.0, 60.0, 0.15);
            }
        }
        out
    }

    /// Beats from `bpm(t)`, integrated.
    fn beats_from(bpm: impl Fn(f64) -> f64, secs: f64) -> Vec<f64> {
        let mut t = 0.5;
        let mut out = Vec::new();
        while t < secs - 0.5 {
            out.push(t);
            t += 60.0 / bpm(t);
        }
        out
    }

    /// Fraction of true beats with a mapped beat within 15 ms, and the mapped count.
    fn score(sr: f32, truth: &[f64], secs: f64) -> (f64, usize) {
        let mut tr = Tracker::new(sr);
        for c in kick_track(sr, truth, secs).chunks(1024) {
            tr.push(c);
        }
        let m = tr.finish().expect("a beat");
        let hit = truth[1..truth.len() - 1]
            .iter()
            .filter(|&&t| m.beats.iter().any(|b| (b - t).abs() < 0.015))
            .count();
        (hit as f64 / (truth.len() - 2) as f64, m.beats.len())
    }

    #[test]
    fn follows_a_tempo_change() {
        let sr = 44_100.0;
        type Bpm = Box<dyn Fn(f64) -> f64>;
        let cases: [(&str, Bpm); 4] = [
            (
                "step 112 -> 136",
                Box::new(|t| if t < 30.0 { 112.0 } else { 136.0 }),
            ),
            (
                "ramp 118 -> 134",
                Box::new(|t| 118.0 + 16.0 * (t / 60.0).min(1.0)),
            ),
            (
                "step 128 -> 96",
                Box::new(|t| if t < 30.0 { 128.0 } else { 96.0 }),
            ),
            (
                "step 100 -> 140",
                Box::new(|t| if t < 30.0 { 100.0 } else { 140.0 }),
            ),
        ];
        for (name, bpm) in cases {
            let truth = beats_from(bpm, 60.0);
            let (frac, n) = score(sr, &truth, 60.0);
            eprintln!(
                "{name}: {:.1}% on the grid, {n} beats for {}",
                frac * 100.0,
                truth.len()
            );
            assert!(frac > 0.95, "{name}: {:.1}% on the grid", frac * 100.0);
            assert!(
                n <= truth.len() + 2,
                "{name}: {n} beats for {}",
                truth.len()
            );
        }
    }

    #[test]
    fn silence_has_no_map() {
        let mut tr = Tracker::new(44_100.0);
        tr.push(&vec![0.0; 44_100 * 30]);
        assert!(tr.finish().is_none());
    }

    #[test]
    fn storage_round_trips_and_is_small() {
        let steady = BeatMap {
            bpm: 120.0,
            beats: (0..400).map(|i| 0.25 + i as f64 * 0.5).collect(),
        };
        let code = steady.encode();
        assert!(code.len() <= 8, "steady: {} chars", code.len());
        assert_eq!(BeatMap::decode(120.0, &code).unwrap(), steady);

        // A drifting, jittery, tempo-changing track survives to the millisecond.
        let mut t = 0.0123;
        let wobbly = BeatMap {
            bpm: 100.0,
            beats: (0..600)
                .map(|i| {
                    t += 60.0 / if i < 300 { 100.0 } else { 137.0 }
                        + 0.004 * ((i * 7919) % 11) as f64 / 11.0;
                    t
                })
                .collect(),
        };
        let code = wobbly.encode();
        let back = BeatMap::decode(100.0, &code).unwrap();
        assert_eq!(back.beats.len(), 600);
        for (a, b) in back.beats.iter().zip(&wobbly.beats) {
            assert!((a - b).abs() <= 0.0005 + 1e-9, "{a} vs {b}");
        }
        assert!(
            code.len() <= 600 * 4 / 3 + 8,
            "wobbly: {} chars",
            code.len()
        );
        assert!(BeatMap::decode(1.0, "!").is_none());
    }

    #[test]
    fn lookups() {
        let m = BeatMap {
            bpm: 120.0,
            beats: vec![0.5, 1.0, 1.5, 2.1],
        };
        assert!(m.beat_in(0.9, 1.0));
        assert!(!m.beat_in(1.0, 1.4));
        assert!((m.period_at(1.2) - 0.5).abs() < 1e-9);
        assert!((m.period_at(1.9) - 0.6).abs() < 1e-9);
        assert!((m.period_at(9.0) - 0.6).abs() < 1e-9);
    }
}
