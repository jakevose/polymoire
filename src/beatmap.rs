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
        let period = tempo(&o, hop)?;
        let beats = place_beats(&o, period, TIGHTNESS);
        if beats.len() < 8 {
            return None;
        }
        // The flux at frame k measures the rise from frame k-1 to k; its onset sits about
        // half a frame before the frame's end.
        let beats = refine(&o, &beats, period)
            .iter()
            .map(|&k| (k + 0.5) * hop)
            .collect();
        Some(BeatMap {
            bpm: (60.0 / (period * hop)) as f32,
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

/// Beat period in frames (fractional), from the autocorrelation of the whole envelope.
fn tempo(o: &[f32], hop: f64) -> Option<f64> {
    let lag_of = |bpm: f64| 60.0 / (bpm * hop);
    let (lo, hi) = (
        lag_of(MAX_BPM).floor() as usize,
        lag_of(MIN_BPM).ceil() as usize,
    );
    if o.len() < hi * 8 {
        return None;
    }
    let ac = |lag: usize| -> f32 {
        o.iter().zip(&o[lag..]).map(|(a, b)| a * b).sum::<f32>() / (o.len() - lag) as f32
    };
    let raw: Vec<f32> = (0..=hi * 2)
        .map(|l| if l == 0 { 0.0 } else { ac(l) })
        .collect();
    // A beat period also correlates at twice its lag; counting that settles octave ties.
    let score = |l: usize| -> f32 {
        let bpm = 60.0 / (l as f64 * hop);
        let prior = (-0.5 * ((bpm / 120.0).log2() / 0.9).powi(2)).exp() as f32;
        (raw[l] + 0.5 * raw[2 * l]) * prior
    };
    let best = (lo..=hi).max_by(|&a, &b| score(a).total_cmp(&score(b)))?;
    if raw[best] <= 0.0 {
        return None;
    }
    // Parabolic refinement to a fractional lag.
    let (a, b, c) = (raw[best - 1], raw[best], raw[best + 1]);
    let d = a - 2.0 * b + c;
    let shift = if d < 0.0 {
        (0.5 * (a - c) / d).clamp(-0.5, 0.5)
    } else {
        0.0
    };
    Some(best as f64 + shift as f64)
}

/// Dynamic programming: the best chain of beats through the envelope at this period.
fn place_beats(o: &[f32], period: f64, tightness: f32) -> Vec<usize> {
    let n = o.len();
    let mut score = vec![0.0f32; n];
    let mut back = vec![usize::MAX; n];
    let (min_gap, max_gap) = (
        (period * 0.5).round() as usize,
        (period * 2.0).round() as usize,
    );
    for t in 0..n {
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
    let tail = n.saturating_sub(period.ceil() as usize);
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
fn refine(o: &[f32], beats: &[usize], period: f64) -> Vec<f64> {
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
        if j - i > 1 && (step / period - 1.0).abs() < 0.15 {
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

    #[test]
    fn silence_has_no_map() {
        let mut tr = Tracker::new(44_100.0);
        tr.push(&vec![0.0; 44_100 * 30]);
        assert!(tr.finish().is_none());
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
