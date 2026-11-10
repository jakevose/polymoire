//! Audio analysis: band levels, sustained bass, per-instrument hits (kick, snare, hats),
//! tempo with a phase-locked beat clock, and rhythm character (syncopation, triplet feel,
//! dotted / polyrhythmic patterns).
//!
//! The analyzer is fed every sample (mono) from the audio thread, so a slow video frame can
//! never make it miss a hit. All times are in "stream seconds": seconds of audio processed
//! since the analyzer was created. The app maps wall-clock time onto stream time to ask
//! "has a beat or hit landed yet?" each frame.

use crate::dsp::{smooth_k, Biquad, FilterKind};
use std::collections::VecDeque;

/// Samples per analysis hop (~5 ms at 48 kHz).
pub const HOP: usize = 256;
/// RMS level below which we treat the input as silence (about -54 dBFS).
pub const PRESENCE_RMS: f32 = 0.002;
/// Time constant for rhythm statistics, seconds: how far back "the feel" looks.
pub const RHYTHM_TAU: f32 = 4.0;

const KICK: usize = 0;
const SNARE: usize = 1;
const HAT: usize = 2;

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
    /// Steady low end (bass lines, sub), 0..1, as opposed to kick transients.
    pub bass_sustain: f32,
    /// Share of energy in low / mid / high, summing to 1 (all zero in silence).
    pub share: [f32; 3],
}

/// Hits that landed this frame, as strengths in 0..1 (0 = no hit).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Hits {
    pub kick: f32,
    pub snare: f32,
    pub hat: f32,
}

impl Hits {
    pub fn any(&self) -> bool {
        self.kick > 0.0 || self.snare > 0.0 || self.hat > 0.0
    }
}

/// The rhythmic character of the last few seconds.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rhythm {
    /// Hits per second for each instrument, and in total.
    pub kick_rate: f32,
    pub snare_rate: f32,
    pub hat_rate: f32,
    pub density: f32,
    /// 0 = everything on the beat, 1 = everything between beats.
    pub syncopation: f32,
    /// Share of hit spacing that falls on triplets.
    pub triplet: f32,
    /// Share of hit spacing in dotted (3-against-4) groupings.
    pub dotted: f32,
    /// 0..1: instruments moving in different subdivisions (e.g. hats in triplets over a
    /// straight kick) or strongly dotted cross-rhythms.
    pub poly: f32,
}

/// Transient detector on one band's energy envelope.
#[derive(Clone, Debug)]
struct Onset {
    mean: f32,
    peak: f32,
    hist: [f32; 4],
    h: usize,
    last: f64,
    refractory: f64,
    ratio: f32,
    peak_frac: f32,
    rise_frac: f32,
    /// A hit must be this many times louder than ~20 ms before it: a real attack, not the
    /// ripple of a held note.
    attack: f32,
}

impl Onset {
    fn new(refractory: f64, ratio: f32, peak_frac: f32, rise_frac: f32, attack: f32) -> Self {
        Onset {
            mean: 0.0,
            peak: 1e-3,
            hist: [0.0; 4],
            h: 0,
            last: -9.0,
            refractory,
            ratio,
            peak_frac,
            rise_frac,
            attack,
        }
    }

    /// Feeds one hop's RMS; returns the hit strength when an onset starts.
    fn feed(&mut self, en: f32, t: f64, hop_secs: f32) -> Option<f32> {
        self.mean += (en - self.mean) * (hop_secs / 0.8);
        self.peak = en.max(self.peak * (-hop_secs / 3.0).exp()).max(1e-4);
        let lag = self.hist[self.h];
        self.hist[self.h] = en;
        self.h = (self.h + 1) & 3;
        if en > self.mean * self.ratio
            && en > self.peak * self.peak_frac
            && en - lag > self.peak * self.rise_frac
            && en > lag * self.attack
            && en > PRESENCE_RMS * 0.5
            && t - self.last > self.refractory
        {
            self.last = t;
            Some((en / self.peak).clamp(0.05, 1.0))
        } else {
            None
        }
    }
}

/// Decaying statistics of one instrument's timing.
#[derive(Clone, Debug)]
struct BandStats {
    count: f32,
    straight: f32,
    triplet: f32,
    dotted: f32,
    last: f64,
    grid: [f32; 12],
}

impl Default for BandStats {
    fn default() -> Self {
        BandStats {
            count: 0.0,
            straight: 0.0,
            triplet: 0.0,
            dotted: 0.0,
            last: -9.0,
            grid: [0.0; 12],
        }
    }
}

impl BandStats {
    fn decay(&mut self, k: f32) {
        self.count *= k;
        self.straight *= k;
        self.triplet *= k;
        self.dotted *= k;
        for g in self.grid.iter_mut() {
            *g *= k;
        }
    }

    fn spacing_total(&self) -> f32 {
        self.straight + self.triplet + self.dotted
    }

    fn active(&self) -> bool {
        self.spacing_total() > 2.0
    }

    fn triplet_share(&self) -> f32 {
        self.triplet / self.spacing_total().max(1e-6)
    }

    fn dotted_share(&self) -> f32 {
        self.dotted / self.spacing_total().max(1e-6)
    }
}

fn near(r: f64, targets: &[f64], tol: f64) -> bool {
    targets.iter().any(|&x| (r - x).abs() < tol)
}

#[derive(Clone, Debug)]
pub struct Analyzer {
    sample_rate: f32,
    lp_low: Biquad,
    lp_mid: Biquad,
    bp_snare: Biquad,
    hp_hat: Biquad,
    acc: [f32; 6],
    /// Low-band energy of the last 4 hops (~20 ms), so a held bass note's ripple between
    /// hops doesn't look like a kick.
    low_win: [f32; 4],
    lw: usize,
    n: usize,
    t: f64,
    // levels
    peak: [f32; 3],
    smooth: [f32; 3],
    level_fast: f32,
    level_slow: f32,
    low_slow: f32,
    band_slow: [f32; 3],
    // onsets
    det: [Onset; 3],
    onsets: u64,
    band_onsets: [u64; 3],
    last_kick: f64,
    stats: [BandStats; 3],
    events: VecDeque<(f64, usize, f32)>,
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
            bp_snare: Biquad::new(FilterKind::BandPass, sample_rate, 3_000.0, 1.0),
            hp_hat: Biquad::new(FilterKind::HighPass, sample_rate, 6_500.0, 0.707),
            acc: [0.0; 6],
            low_win: [0.0; 4],
            lw: 0,
            n: 0,
            t: 0.0,
            peak: [0.08, 0.06, 0.04],
            smooth: [0.0; 3],
            level_fast: 0.0,
            level_slow: 0.0,
            low_slow: 0.0,
            band_slow: [0.0; 3],
            det: [
                Onset::new(0.25, 1.3, 0.45, 0.1, 1.6),  // kick
                Onset::new(0.10, 1.4, 0.35, 0.12, 1.5), // snare
                Onset::new(0.05, 1.4, 0.3, 0.12, 1.5),  // hats
            ],
            onsets: 0,
            band_onsets: [0; 3],
            last_kick: -9.0,
            stats: Default::default(),
            events: VecDeque::with_capacity(32),
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

    /// Total kick onsets detected so far (for diagnostics and tests).
    pub fn onset_count(&self) -> u64 {
        self.onsets
    }

    /// Hits detected so far per instrument: [kick, snare, hats].
    pub fn hit_counts(&self) -> [u64; 3] {
        self.band_onsets
    }

    /// Feed mono samples.
    pub fn push(&mut self, samples: &[f32]) {
        for &x in samples {
            let low = self.lp_low.process(x);
            let below_mid = self.lp_mid.process(x);
            let mid = below_mid - low;
            let high = x - below_mid;
            let sn = self.bp_snare.process(x);
            let hh = self.hp_hat.process(x);
            self.acc[0] += low * low;
            self.acc[1] += mid * mid;
            self.acc[2] += high * high;
            self.acc[3] += x * x;
            self.acc[4] += sn * sn;
            self.acc[5] += hh * hh;
            self.n += 1;
            if self.n == HOP {
                self.hop();
            }
        }
    }

    fn hop(&mut self) {
        let inv = 1.0 / HOP as f32;
        let rms: [f32; 6] = std::array::from_fn(|i| (self.acc[i] * inv).sqrt());
        let h = HOP as f32 / self.sample_rate;
        let t = self.t;
        self.acc = [0.0; 6];
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
            self.band_slow[b] += (rms[b] - self.band_slow[b]) * smooth_k(0.5, h);
        }
        self.level_fast += (rms[3] - self.level_fast) * smooth_k(4.0, h);
        self.level_slow += (rms[3] - self.level_slow) * smooth_k(0.25, h);
        self.low_slow += (rms[0] - self.low_slow) * smooth_k(1.5, h);

        // Rhythm statistics forget slowly so the "feel" follows the music.
        let k = (-h / RHYTHM_TAU).exp();
        for s in self.stats.iter_mut() {
            s.decay(k);
        }

        // Hits per instrument.
        self.low_win[self.lw] = rms[0] * rms[0];
        self.lw = (self.lw + 1) & 3;
        let low_env = (self.low_win.iter().sum::<f32>() / 4.0).sqrt();
        let energies = [low_env, rms[4], rms[5]];
        for (band, &en) in energies.iter().enumerate() {
            if let Some(strength) = self.det[band].feed(en, t, h) {
                // A kick's click spills into the snare band. While a kick is sounding, only
                // a hit near the snare's usual strength counts as a snare.
                if band == SNARE && low_env > en && strength < 0.75 {
                    continue;
                }
                // Hats spill into the snare band too; a snare is broadband, so it is at least
                // half as loud in its own band as in the hat band.
                if band == SNARE && en < rms[5] * 0.5 {
                    continue;
                }
                self.on_hit(band, t, strength);
            }
        }
    }

    fn on_hit(&mut self, band: usize, t: f64, strength: f32) {
        // A snare also splashes into the hat band; keep a hat only if it isn't just that.
        if band == HAT && t - self.det[SNARE].last < 0.02 && strength < 0.8 {
            return;
        }
        self.band_onsets[band] += 1;
        if band == KICK {
            self.on_kick(t);
        }
        let s = &mut self.stats[band];
        s.count += 1.0;
        if self.locked {
            let r = (t - s.last) / self.period;
            if near(r, &[0.25, 0.5, 1.0, 2.0], 0.06) {
                s.straight += 1.0;
            } else if near(r, &[1.0 / 3.0, 2.0 / 3.0, 4.0 / 3.0], 0.05) {
                s.triplet += 1.0;
            } else if near(r, &[0.75, 1.5], 0.06) {
                s.dotted += 1.0;
            }
            let phase = ((t - self.anchor) / self.period).rem_euclid(1.0);
            let bin = ((phase * 12.0).round() as usize) % 12;
            s.grid[bin] += strength;
        }
        s.last = t;
        self.events.push_back((t, band, strength));
        if self.events.len() > 32 {
            self.events.pop_front();
        }
    }

    fn on_kick(&mut self, t: f64) {
        self.onsets += 1;
        let dtb = t - self.last_kick;
        self.last_kick = t;
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

    /// Hits the listener is hearing as of `heard`, strongest per instrument. Hits more than
    /// 0.3 s old are dropped rather than played late.
    pub fn poll_hits(&mut self, heard: f64) -> Hits {
        let mut hits = Hits::default();
        while let Some(&(t, band, s)) = self.events.front() {
            if t > heard {
                break;
            }
            self.events.pop_front();
            if heard - t > 0.3 {
                continue;
            }
            let slot = match band {
                KICK => &mut hits.kick,
                SNARE => &mut hits.snare,
                _ => &mut hits.hat,
            };
            *slot = slot.max(s);
        }
        hits
    }

    pub fn levels(&self) -> Levels {
        let e: [f32; 3] = std::array::from_fn(|b| self.band_slow[b] * self.band_slow[b]);
        let total: f32 = e.iter().sum();
        let share = if total > PRESENCE_RMS * PRESENCE_RMS {
            [e[0] / total, e[1] / total, e[2] / total]
        } else {
            [0.0; 3]
        };
        Levels {
            low: self.smooth[0],
            mid: self.smooth[1],
            high: self.smooth[2],
            level_fast: self.level_fast,
            level_slow: self.level_slow,
            bpm: if self.ibi.len() >= 4 { self.bpm } else { 0.0 },
            present: self.level_fast > PRESENCE_RMS,
            bass_sustain: (self.low_slow / self.peak[0]).clamp(0.0, 1.0),
            share,
        }
    }

    pub fn rhythm(&self) -> Rhythm {
        let s = &self.stats;
        let rate = |b: usize| s[b].count / RHYTHM_TAU;
        let active: Vec<usize> = (0..3).filter(|&b| s[b].active()).collect();

        let spacing: f32 = active.iter().map(|&b| s[b].spacing_total()).sum();
        let triplet = if spacing > 0.0 {
            active.iter().map(|&b| s[b].triplet).sum::<f32>() / spacing
        } else {
            0.0
        };
        let dotted = active
            .iter()
            .map(|&b| s[b].dotted_share())
            .fold(0.0, f32::max);
        let mut cross = 0.0f32;
        for (i, &a) in active.iter().enumerate() {
            for &b in &active[i + 1..] {
                cross = cross.max((s[a].triplet_share() - s[b].triplet_share()).abs());
            }
        }

        let mut grid = [0.0f32; 12];
        for st in s.iter() {
            for (g, v) in grid.iter_mut().zip(st.grid.iter()) {
                *g += v;
            }
        }
        let total: f32 = grid.iter().sum();
        let syncopation = if total > 1.0 {
            let on = grid[0] + 0.5 * (grid[1] + grid[11]);
            let and = grid[6] + 0.5 * (grid[5] + grid[7]);
            ((total - on - 0.5 * and) / total).clamp(0.0, 1.0)
        } else {
            0.0
        };

        Rhythm {
            kick_rate: rate(KICK),
            snare_rate: rate(SNARE),
            hat_rate: rate(HAT),
            density: rate(KICK) + rate(SNARE) + rate(HAT),
            syncopation,
            triplet,
            dotted,
            poly: (cross * 1.2).max(dotted * 0.9).clamp(0.0, 1.0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::Rng;
    use std::f32::consts::PI;

    const SR: f32 = 48_000.0;

    /// A synthetic track: kicks on every beat, an off-beat bass line and hats.
    fn track(bpm: f32, seconds: f32, kick_gap: Option<(f32, f32)>) -> Vec<f32> {
        let n = (SR * seconds) as usize;
        let beat = 60.0 / bpm;
        let mut out = vec![0.0; n];
        let mut noise = Rng::new(3);
        for (i, o) in out.iter_mut().enumerate() {
            let t = i as f32 / SR;
            let in_gap = kick_gap.is_some_and(|(a, b)| t >= a && t < b);
            let tb = t % beat;
            let mut x = 0.0;
            if !in_gap {
                x += kick_at(tb);
                let tb2 = (t + beat / 2.0) % beat; // off-beat bass
                x += (2.0 * PI * 55.0 * t).sin() * (-tb2 / 0.1).exp() * 0.25;
            }
            let th = t % (beat / 2.0);
            x += (noise.f32() * 2.0 - 1.0) * (-th / 0.02).exp() * 0.08;
            *o = x;
        }
        out
    }

    fn kick_at(tb: f32) -> f32 {
        let f = 150.0 * (42.0f32 / 150.0).powf((tb / 0.12).min(1.0));
        (2.0 * PI * f * tb).sin() * (-tb / 0.08).exp() * 0.9
    }

    /// Kick on every beat plus a hat voice on its own subdivision (`hat_div` per beat) and
    /// optional snare on beats 2 and 4. `hat_div` = 3 gives triplets over a straight kick.
    fn groove(bpm: f32, seconds: f32, hat_div: u32, snare: bool) -> Vec<f32> {
        let n = (SR * seconds) as usize;
        let beat = 60.0 / bpm;
        let mut noise = Rng::new(11);
        let mut hp = Biquad::new(FilterKind::HighPass, SR, 8_000.0, 0.7);
        let mut bp = Biquad::new(FilterKind::BandPass, SR, 2_000.0, 0.8);
        (0..n)
            .map(|i| {
                let t = i as f32 / SR;
                let tb = t % beat;
                let mut x = kick_at(tb);
                let sub = beat / hat_div as f32;
                let th = t % sub;
                let w = noise.f32() * 2.0 - 1.0;
                x += hp.process(w) * (-th / 0.015).exp() * 0.5;
                if snare {
                    let bar_beat = (t / beat).floor() as u32 % 4;
                    if bar_beat == 1 || bar_beat == 3 {
                        x += bp.process(w) * (-tb / 0.06).exp() * 0.9;
                    }
                }
                x
            })
            .collect()
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
        assert_eq!(a.hit_counts(), [0, 0, 0]);
        assert_eq!(l.bpm, 0.0);
        assert_eq!(l.share, [0.0; 3]);
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
            let tone: Vec<f32> = (0..SR as usize * 2)
                .map(|i| (2.0 * PI * freq * i as f32 / SR).sin() * 0.5)
                .collect();
            feed(&mut a, &tone);
            let l = a.levels();
            let v = [l.low, l.mid, l.high];
            let winner = (0..3).max_by(|&x, &y| v[x].total_cmp(&v[y])).unwrap();
            assert_eq!(winner, band, "{freq} Hz gave {v:?}");
            let s = l.share;
            let top = (0..3).max_by(|&x, &y| s[x].total_cmp(&s[y])).unwrap();
            assert_eq!(top, band, "{freq} Hz share {s:?}");
        }
    }

    #[test]
    fn sustained_bass_is_told_apart_from_kicks() {
        let mut a = Analyzer::new(SR);
        let bass: Vec<f32> = (0..SR as usize * 4)
            .map(|i| (2.0 * PI * 55.0 * i as f32 / SR).sin() * 0.5)
            .collect();
        feed(&mut a, &bass);
        assert!(a.levels().bass_sustain > 0.6, "{}", a.levels().bass_sustain);
        assert!(a.hit_counts()[KICK] <= 1, "a held note is not a kick");

        let mut k = Analyzer::new(SR);
        let kicks: Vec<f32> = (0..SR as usize * 4)
            .map(|i| kick_at((i as f32 / SR) % 0.5))
            .collect();
        feed(&mut k, &kicks);
        assert!(
            k.levels().bass_sustain < 0.45,
            "{}",
            k.levels().bass_sustain
        );
        assert!(k.hit_counts()[KICK] >= 7);
    }

    #[test]
    fn instruments_are_detected_separately() {
        let mut a = Analyzer::new(SR);
        feed(&mut a, &groove(120.0, 8.0, 2, true));
        let [kick, snare, hat] = a.hit_counts();
        assert!((14..=17).contains(&kick), "kicks {kick}");
        assert!((7..=10).contains(&snare), "snares {snare}");
        assert!((26..=34).contains(&hat), "hats {hat}");
        let r = a.rhythm();
        assert!(r.density > 4.0 && r.hat_rate > r.kick_rate, "{r:?}");
    }

    #[test]
    fn straight_hats_are_not_polyrhythmic() {
        let mut a = Analyzer::new(SR);
        feed(&mut a, &groove(120.0, 10.0, 4, false));
        let r = a.rhythm();
        assert!(r.triplet < 0.2, "{r:?}");
        assert!(r.poly < 0.3, "{r:?}");
    }

    #[test]
    fn triplet_hats_over_a_straight_kick_read_as_polyrhythm() {
        let mut a = Analyzer::new(SR);
        feed(&mut a, &groove(120.0, 10.0, 3, false));
        let r = a.rhythm();
        assert!(r.triplet > 0.35, "{r:?}");
        assert!(r.poly > 0.5, "{r:?}");
    }

    #[test]
    fn off_beat_hats_are_more_syncopated_than_on_beat_hats() {
        let on = {
            let mut a = Analyzer::new(SR);
            feed(&mut a, &groove(120.0, 10.0, 1, false));
            a.rhythm().syncopation
        };
        let off = {
            let mut a = Analyzer::new(SR);
            feed(&mut a, &groove(120.0, 10.0, 4, false));
            a.rhythm().syncopation
        };
        assert!(off > on + 0.2, "on {on}, sixteenths {off}");
    }

    #[test]
    fn hits_are_delivered_once_and_never_late() {
        let mut a = Analyzer::new(SR);
        let audio = groove(120.0, 4.0, 2, true);
        let frame = (SR / 60.0) as usize;
        let mut kicks = 0;
        for chunk in audio.chunks(frame) {
            a.push(chunk);
            let h = a.poll_hits(a.time());
            if h.kick > 0.0 {
                kicks += 1;
            }
            assert!(h.kick <= 1.0 && h.snare <= 1.0 && h.hat <= 1.0);
        }
        assert_eq!(kicks as u64, a.hit_counts()[KICK]);
        // Stale hits are dropped, not delivered late.
        let mut b = Analyzer::new(SR);
        feed(&mut b, &audio);
        assert!(!b.poll_hits(b.time() + 5.0).any());
    }
}
