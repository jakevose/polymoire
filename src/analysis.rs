//! Audio analysis: smooth band levels, kick onsets, tempo, and a phase-locked beat clock.
//!
//! The analyzer is fed every sample (mono) from the audio thread, so a slow video frame can
//! never make it miss a kick. All times are in "stream seconds": seconds of audio processed
//! since the analyzer was created. The app maps wall-clock time onto stream time to ask
//! "has a beat landed yet?" each frame.

use crate::dsp::{smooth_k, Biquad, FilterKind};
use std::collections::VecDeque;

/// Samples per analysis hop (~5 ms at 48 kHz).
pub const HOP: usize = 256;
/// RMS level below which we treat the input as silence (about -54 dBFS).
pub const PRESENCE_RMS: f32 = 0.002;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Levels {
    /// Auto-gained, smoothed band energies in 0..=1.
    pub low: f32,
    pub mid: f32,
    pub high: f32,
    /// Raw RMS of the whole signal, fast (~0.25 s) and slow (~4 s) averages.
    pub level_fast: f32,
    pub level_slow: f32,
    /// Estimated tempo, 0 when unknown.
    pub bpm: f32,
    /// Whether there is audible sound.
    pub present: bool,
}

#[derive(Clone, Debug)]
pub struct Analyzer {
    sample_rate: f32,
    lp_low: Biquad,
    lp_mid: Biquad,
    acc: [f32; 4],
    n: usize,
    t: f64,
    // levels
    peak: [f32; 3],
    smooth: [f32; 3],
    level_fast: f32,
    level_slow: f32,
    // onset detection on the low band
    e_mean: f32,
    e_peak: f32,
    e_hist: [f32; 4],
    eh: usize,
    last_onset: f64,
    onsets: u64,
    // tempo + beat clock
    ibi: Vec<f64>,
    period: f64,
    bpm: f32,
    locked: bool,
    anchor: f64,
    last_hit: f64,
    last_fired: f64,
    pending: VecDeque<f64>,
}

impl Analyzer {
    pub fn new(sample_rate: f32) -> Self {
        Analyzer {
            sample_rate,
            lp_low: Biquad::new(FilterKind::LowPass, sample_rate, 150.0, 0.707),
            lp_mid: Biquad::new(FilterKind::LowPass, sample_rate, 2_000.0, 0.707),
            acc: [0.0; 4],
            n: 0,
            t: 0.0,
            peak: [0.08, 0.06, 0.04],
            smooth: [0.0; 3],
            level_fast: 0.0,
            level_slow: 0.0,
            e_mean: 0.0,
            e_peak: 1e-3,
            e_hist: [0.0; 4],
            eh: 0,
            last_onset: -9.0,
            onsets: 0,
            ibi: Vec::with_capacity(16),
            period: 0.5,
            bpm: 0.0,
            locked: false,
            anchor: 0.0,
            last_hit: -99.0,
            last_fired: -99.0,
            pending: VecDeque::with_capacity(8),
        }
    }

    pub fn sample_rate(&self) -> f32 {
        self.sample_rate
    }

    /// Stream time of the most recent sample pushed, in seconds.
    pub fn time(&self) -> f64 {
        self.t + self.n as f64 / self.sample_rate as f64
    }

    /// Beat period in seconds (meaningful once `levels().bpm > 0`).
    pub fn period(&self) -> f64 {
        self.period
    }

    pub fn is_locked(&self) -> bool {
        self.locked
    }

    /// Total onsets detected so far (for diagnostics and tests).
    pub fn onset_count(&self) -> u64 {
        self.onsets
    }

    /// Feed mono samples.
    pub fn push(&mut self, samples: &[f32]) {
        for &x in samples {
            let low = self.lp_low.process(x);
            let below_mid = self.lp_mid.process(x);
            let mid = below_mid - low;
            let high = x - below_mid;
            self.acc[0] += low * low;
            self.acc[1] += mid * mid;
            self.acc[2] += high * high;
            self.acc[3] += x * x;
            self.n += 1;
            if self.n == HOP {
                self.hop();
            }
        }
    }

    fn hop(&mut self) {
        let inv = 1.0 / HOP as f32;
        let rms = [
            (self.acc[0] * inv).sqrt(),
            (self.acc[1] * inv).sqrt(),
            (self.acc[2] * inv).sqrt(),
        ];
        let all = (self.acc[3] * inv).sqrt();
        let h = HOP as f32 / self.sample_rate;
        let t = self.t;
        self.acc = [0.0; 4];
        self.n = 0;
        self.t += h as f64;

        // Auto-gain: each band is measured against its own slowly decaying peak, so quiet
        // and loud tracks both move the picture.
        let floors = [0.004, 0.003, 0.002];
        let rates = [10.0, 10.0, 14.0];
        for b in 0..3 {
            self.peak[b] = rms[b].max(self.peak[b] * (-h / 4.0).exp()).max(floors[b]);
            let norm = (rms[b] / self.peak[b]).clamp(0.0, 1.0);
            self.smooth[b] += (norm - self.smooth[b]) * smooth_k(rates[b], h);
        }
        self.level_fast += (all - self.level_fast) * smooth_k(4.0, h);
        self.level_slow += (all - self.level_slow) * smooth_k(0.25, h);

        // Onset: low-band energy rising sharply above its recent mean and peak.
        let en = rms[0];
        self.e_mean += (en - self.e_mean) * (h / 0.8);
        self.e_peak = en.max(self.e_peak * (-h / 3.0).exp()).max(1e-4);
        let lag = self.e_hist[self.eh];
        self.e_hist[self.eh] = en;
        self.eh = (self.eh + 1) & 3;
        if en > self.e_mean * 1.3
            && en > self.e_peak * 0.45
            && en - lag > self.e_peak * 0.1
            && en > PRESENCE_RMS
            && t - self.last_onset > 0.25
        {
            self.on_onset(t);
        }
    }

    fn on_onset(&mut self, t: f64) {
        self.onsets += 1;
        let dtb = t - self.last_onset;
        self.last_onset = t;
        if dtb < 1.6 {
            // Fold intervals into one octave (75–150 bpm) so off-beats and half-time agree.
            let mut x = dtb;
            while x < 0.4 {
                x *= 2.0;
            }
            while x > 0.8 {
                x /= 2.0;
            }
            self.ibi.push(x);
            if self.ibi.len() > 16 {
                self.ibi.remove(0);
            }
        }
        if self.ibi.len() >= 4 {
            let mut s = self.ibi.clone();
            s.sort_by(|a, b| a.total_cmp(b));
            self.period = s[s.len() / 2];
            let q1 = s[s.len() / 4];
            let q3 = s[(s.len() * 3) / 4];
            self.bpm = (60.0 / self.period) as f32;
            if !self.locked && q3 - q1 < 0.05 {
                self.locked = true;
            }
        }
        if self.locked {
            let k = ((t - self.anchor) / self.period).round();
            let err = t - (self.anchor + k * self.period);
            if err.abs() < 0.3 * self.period {
                self.anchor += k * self.period + err * 0.35;
                self.last_hit = t;
            } else if t - self.last_hit > 3.0 * self.period {
                self.anchor = t;
                self.last_hit = t;
            }
        } else {
            self.anchor = t;
            self.last_hit = t;
        }
        self.pending.push_back(t);
        if self.pending.len() > 8 {
            self.pending.pop_front();
        }
    }

    /// Call once per video frame with the stream time the listener is hearing right now.
    /// Returns true when a beat lands. Once the tempo is locked, beats are predicted so they
    /// land on time (not after detection), and keep going through short gaps like breakdowns.
    pub fn poll_beat(&mut self, heard: f64) -> bool {
        if self.locked && heard - self.last_hit > 10.0 * self.period {
            self.locked = false;
            self.ibi.clear();
        }
        if self.locked {
            self.pending.clear();
            let k = ((heard - self.anchor) / self.period).floor();
            let bt = self.anchor + k * self.period;
            if bt > self.last_fired + self.period * 0.5 && heard - bt < 0.12 {
                self.last_fired = bt;
                return true;
            }
            false
        } else {
            let mut fired = false;
            while let Some(&t) = self.pending.front() {
                if t > heard {
                    break;
                }
                self.pending.pop_front();
                if t > self.last_fired + 0.2 {
                    self.last_fired = t;
                    fired = true;
                }
            }
            fired
        }
    }

    pub fn levels(&self) -> Levels {
        Levels {
            low: self.smooth[0],
            mid: self.smooth[1],
            high: self.smooth[2],
            level_fast: self.level_fast,
            level_slow: self.level_slow,
            bpm: if self.ibi.len() >= 4 { self.bpm } else { 0.0 },
            present: self.level_fast > PRESENCE_RMS,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::PI;

    const SR: f32 = 48_000.0;

    /// A synthetic track: kicks on every beat, an off-beat bass line and hats.
    fn track(bpm: f32, seconds: f32, kick_gap: Option<(f32, f32)>) -> Vec<f32> {
        let n = (SR * seconds) as usize;
        let beat = 60.0 / bpm;
        let mut out = vec![0.0; n];
        let mut noise = crate::rng::Rng::new(3);
        for (i, o) in out.iter_mut().enumerate() {
            let t = i as f32 / SR;
            let in_gap = kick_gap.is_some_and(|(a, b)| t >= a && t < b);
            let tb = t % beat;
            let mut x = 0.0;
            if !in_gap {
                let f = 150.0 * (42.0f32 / 150.0).powf((tb / 0.12).min(1.0));
                x += (2.0 * PI * f * tb).sin() * (-tb / 0.08).exp() * 0.9;
                let tb2 = (t + beat / 2.0) % beat; // off-beat bass
                x += (2.0 * PI * 55.0 * t).sin() * (-tb2 / 0.1).exp() * 0.25;
            }
            let th = t % (beat / 2.0);
            x += (noise.f32() * 2.0 - 1.0) * (-th / 0.02).exp() * 0.08;
            *o = x;
        }
        out
    }

    fn feed(a: &mut Analyzer, s: &[f32]) {
        for chunk in s.chunks(512) {
            a.push(chunk);
        }
    }

    #[test]
    fn silence_has_no_presence_and_no_beats() {
        let mut a = Analyzer::new(SR);
        feed(&mut a, &vec![0.0; SR as usize * 3]);
        let l = a.levels();
        assert!(!l.present);
        assert_eq!(a.onset_count(), 0);
        assert_eq!(l.bpm, 0.0);
        assert!(!a.poll_beat(a.time()));
    }

    #[test]
    fn finds_tempo_of_a_steady_kick() {
        for bpm in [96.0, 120.0, 128.0, 140.0] {
            let mut a = Analyzer::new(SR);
            feed(&mut a, &track(bpm, 8.0, None));
            let got = a.levels().bpm;
            assert!((got - bpm).abs() < 2.5, "expected {bpm}, got {got}");
            assert!(a.is_locked(), "should lock at {bpm} bpm");
        }
    }

    #[test]
    fn time_advances_with_samples() {
        let mut a = Analyzer::new(SR);
        a.push(&vec![0.0; 48_000]);
        assert!((a.time() - 1.0).abs() < 1e-6);
    }

    #[test]
    fn beat_clock_fires_once_per_beat_and_rides_through_a_gap() {
        let bpm = 120.0;
        let audio = track(bpm, 12.0, Some((6.0, 8.0)));
        let mut a = Analyzer::new(SR);
        let frame = (SR / 60.0) as usize; // 60 fps
        let mut beats = Vec::new();
        for (i, chunk) in audio.chunks(frame).enumerate() {
            a.push(chunk);
            if a.poll_beat(a.time()) {
                beats.push((i * frame) as f64 / SR as f64);
            }
        }
        // After lock-in (~3 s), beats should be ~0.5 s apart, including inside the 2 s gap.
        let late: Vec<f64> = beats.iter().copied().filter(|&t| t > 3.0).collect();
        assert!(late.len() >= 16, "too few beats: {beats:?}");
        for w in late.windows(2) {
            let d = w[1] - w[0];
            assert!((d - 0.5).abs() < 0.06, "uneven beats {d}: {late:?}");
        }
        assert!(
            late.iter().any(|&t| (6.2..7.8).contains(&t)),
            "no beats in gap"
        );
    }

    #[test]
    fn bands_respond_to_their_own_frequencies() {
        for (freq, band) in [(60.0, 0usize), (900.0, 1), (7_000.0, 2)] {
            let mut a = Analyzer::new(SR);
            // Quiet first so the auto-gain peaks settle low, then the tone.
            let tone: Vec<f32> = (0..SR as usize * 2)
                .map(|i| (2.0 * PI * freq * i as f32 / SR).sin() * 0.5)
                .collect();
            feed(&mut a, &tone);
            let l = a.levels();
            let v = [l.low, l.mid, l.high];
            let winner = (0..3).max_by(|&x, &y| v[x].total_cmp(&v[y])).unwrap();
            assert_eq!(winner, band, "{freq} Hz gave {v:?}");
        }
    }
}
