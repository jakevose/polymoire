//! Tempo and beat-phase tracking by autocorrelation.
//!
//! Instead of timing the gaps between kicks (which falls apart on syncopated patterns), this
//! keeps the last few seconds of "onset strength" from all the drums and asks: at which lag
//! does the pattern repeat? Lags are scored together with their double and quadruple, so a
//! tresillo or other cross-rhythm doesn't beat the true pulse, with a mild preference for
//! tempos near 120 bpm to settle octave ties. The beat phase is then the offset whose grid
//! collects the most low-end onsets. Both are updated gently, and a new tempo must persist
//! before it replaces the current one.

/// Seconds of onset history used.
pub const WINDOW: f64 = 6.0;
/// Tempo range searched, bpm.
pub const MIN_BPM: f64 = 70.0;
pub const MAX_BPM: f64 = 180.0;
const EVAL_EVERY: f64 = 0.5;
const MIN_CONFIDENCE: f32 = 0.15;

#[derive(Clone, Debug)]
pub struct Tempo {
    hop: f64,
    flux: Vec<f32>,
    phase: Vec<f32>,
    head: usize,
    filled: usize,
    lin: Vec<f32>,
    lin_p: Vec<f32>,
    ac: Vec<f32>,
    since_eval: usize,
    eval_every: usize,
    lmin: usize,
    lmax: usize,
    /// Beat period in seconds.
    pub period: f64,
    pub locked: bool,
    /// Time of a beat; beats fall at `anchor + k * period`.
    pub anchor: f64,
    /// How periodic the onsets are at the chosen tempo, 0..1.
    pub confidence: f32,
    last_confident: f64,
    pending_period: f64,
    pending_count: u8,
    phase_miss: u8,
}

impl Tempo {
    /// `hop` is the time between pushes, in seconds.
    pub fn new(hop: f64) -> Self {
        let n = (WINDOW / hop).ceil() as usize;
        let lmin = (60.0 / MAX_BPM / hop).floor() as usize;
        let lmax = (60.0 / MIN_BPM / hop).ceil() as usize;
        Tempo {
            hop,
            flux: vec![0.0; n],
            phase: vec![0.0; n],
            head: 0,
            filled: 0,
            lin: vec![0.0; n],
            lin_p: vec![0.0; n],
            ac: vec![0.0; 4 * lmax + 2],
            since_eval: 0,
            eval_every: (EVAL_EVERY / hop).round().max(1.0) as usize,
            lmin,
            lmax,
            period: 0.5,
            locked: false,
            anchor: 0.0,
            confidence: 0.0,
            last_confident: -99.0,
            pending_period: 0.0,
            pending_count: 0,
            phase_miss: 0,
        }
    }

    pub fn bpm(&self) -> f32 {
        if self.locked {
            (60.0 / self.period) as f32
        } else {
            0.0
        }
    }

    /// Feed one hop: onset strength for tempo (all drums) and for phase (low end), and the
    /// time at the middle of the hop.
    pub fn push(&mut self, tempo_flux: f32, phase_flux: f32, t: f64) {
        self.flux[self.head] = tempo_flux;
        self.phase[self.head] = phase_flux;
        self.head = (self.head + 1) % self.flux.len();
        self.filled = (self.filled + 1).min(self.flux.len());
        self.since_eval += 1;
        if self.since_eval >= self.eval_every {
            self.since_eval = 0;
            self.evaluate(t);
        }
    }

    /// Copies the ring into `out` oldest-first with a light 3-tap smooth, so onsets that
    /// land a hop early or late still line up.
    fn linearize(src: &[f32], head: usize, len: usize, out: &mut [f32]) {
        let n = src.len();
        let at = |i: usize| src[(head + n - len + i) % n];
        for (i, o) in out.iter_mut().enumerate().take(len) {
            let a = if i > 0 { at(i - 1) } else { at(i) };
            let c = if i + 1 < len { at(i + 1) } else { at(i) };
            *o = 0.25 * a + 0.5 * at(i) + 0.25 * c;
        }
    }

    fn lose_confidence(&mut self, t: f64) {
        self.confidence = 0.0;
        if self.locked && t - self.last_confident > 10.0 * self.period {
            self.locked = false;
            self.pending_count = 0;
        }
    }

    fn evaluate(&mut self, t: f64) {
        let len = self.filled;
        let (lmin, lmax) = (self.lmin, self.lmax);
        if len < 2 * lmax + lmin {
            return;
        }
        Self::linearize(&self.flux, self.head, len, &mut self.lin);
        Self::linearize(&self.phase, self.head, len, &mut self.lin_p);
        let x = &mut self.lin[..len];
        let peak = x.iter().fold(0.0f32, |m, v| m.max(*v));
        if peak < 0.1 {
            return self.lose_confidence(t);
        }
        let mean = x.iter().sum::<f32>() / len as f32;
        x.iter_mut().for_each(|v| *v -= mean);
        let ac0 = x.iter().map(|v| v * v).sum::<f32>() / len as f32;
        if ac0 < 1e-6 {
            return self.lose_confidence(t);
        }

        // Autocorrelation for every lag we need, including doubles and quadruples.
        let quad = len >= 4 * lmax + lmin;
        let top = if quad { 4 * lmax + 1 } else { 2 * lmax + 1 };
        for lag in (lmin - 1)..=top {
            let mut s = 0.0;
            for i in lag..len {
                s += x[i] * x[i - lag];
            }
            self.ac[lag] = s / (len - lag) as f32;
        }
        let ac = &self.ac;
        let score = |lag: usize| {
            let mut s = ac[lag] + 0.5 * ac[2 * lag];
            if quad {
                s += 0.25 * ac[4 * lag];
            }
            let bpm = 60.0 / (lag as f64 * self.hop);
            let octaves = (bpm / 120.0).log2() / 0.7;
            let mut w = (-0.5 * octaves * octaves).exp() as f32;
            if self.locked && ((lag as f64 * self.hop) / self.period - 1.0).abs() < 0.04 {
                w *= 1.2;
            }
            s * w
        };
        let mut best = lmin;
        let mut best_s = f32::MIN;
        for lag in lmin..=lmax {
            let s = score(lag);
            if s > best_s {
                best_s = s;
                best = lag;
            }
        }
        let confidence = (ac[best] / ac0).clamp(0.0, 1.0);
        if confidence < MIN_CONFIDENCE {
            return self.lose_confidence(t);
        }
        // Parabolic refinement for sub-hop precision.
        let (a, b, c) = (ac[best - 1], ac[best], ac[best + 1]);
        let denom = a - 2.0 * b + c;
        let delta = if denom.abs() > 1e-12 {
            (0.5 * (a - c) / denom).clamp(-0.5, 0.5)
        } else {
            0.0
        };
        let new_period = (best as f64 + delta as f64) * self.hop;
        self.confidence = confidence;
        self.last_confident = t;

        let mut jumped = false;
        if !self.locked {
            self.period = new_period;
            self.locked = true;
            jumped = true;
        } else if (new_period / self.period - 1.0).abs() < 0.04 {
            self.period += (new_period - self.period) * 0.3;
            self.pending_count = 0;
        } else {
            // A different tempo must persist before it takes over.
            if self.pending_count > 0 && (new_period / self.pending_period - 1.0).abs() < 0.04 {
                self.pending_count += 1;
            } else {
                self.pending_period = new_period;
                self.pending_count = 1;
            }
            if self.pending_count >= 3 {
                self.period = new_period;
                self.pending_count = 0;
                jumped = true;
            }
        }
        self.update_phase(t, len, jumped);
    }

    fn update_phase(&mut self, t: f64, len: usize, jumped: bool) {
        let p = self.period / self.hop;
        let offsets = p.ceil() as usize;
        let ph = &self.lin_p[..len];
        let mut best = 0usize;
        let mut best_s = 0.0f32;
        let mut total = 0.0f32;
        for off in 0..offsets {
            let mut s = 0.0;
            let mut k = 0.0f64;
            loop {
                let back = off as f64 + k * p;
                if back > (len - 1) as f64 {
                    break;
                }
                s += ph[len - 1 - back.round() as usize];
                k += 1.0;
            }
            total += s;
            if s > best_s {
                best_s = s;
                best = off;
            }
        }
        let mean = total / offsets as f32;
        if best_s < 0.05 || best_s < 1.6 * mean {
            return; // nothing clear to align to (e.g. a breakdown): keep the grid running
        }
        let found = t - best as f64 * self.hop;
        if jumped {
            self.anchor = found;
            self.phase_miss = 0;
            return;
        }
        let k = ((found - self.anchor) / self.period).round();
        let err = found - (self.anchor + k * self.period);
        if err.abs() < 0.15 * self.period {
            self.anchor += k * self.period + err * 0.3;
            self.phase_miss = 0;
        } else {
            self.phase_miss += 1;
            if self.phase_miss >= 3 {
                self.anchor = found;
                self.phase_miss = 0;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOP: f64 = 256.0 / 48_000.0;

    /// Runs `secs` of a pattern. `events(t)` returns (tempo flux, phase flux) for a hop
    /// starting at `t`; helper `hit` makes that easy.
    fn run(tempo: &mut Tempo, from: f64, secs: f64, events: impl Fn(f64) -> (f32, f32)) {
        let n = (secs / HOP) as usize;
        let start = (from / HOP).round() as usize;
        for i in start..start + n {
            let t = i as f64 * HOP;
            let (a, b) = events(t);
            tempo.push(a, b, t + HOP / 2.0);
        }
    }

    /// 1.0 if an event at phase `at` (in seconds, repeating every `every`) starts in the hop
    /// beginning at `t`.
    fn hit(t: f64, every: f64, at: f64) -> f32 {
        let x = (t - at).rem_euclid(every);
        if x < HOP {
            1.0
        } else {
            0.0
        }
    }

    fn four_on_floor(period: f64) -> impl Fn(f64) -> (f32, f32) {
        move |t| {
            let k = hit(t, period, 0.0);
            let h = hit(t, period / 2.0, 0.0);
            (k + 0.3 * h, k)
        }
    }

    #[test]
    fn locks_to_a_steady_pulse_and_lands_on_the_kicks() {
        for bpm in [80.0, 100.0, 120.0, 128.0, 150.0, 160.0] {
            let period = 60.0 / bpm;
            let mut t = Tempo::new(HOP);
            run(&mut t, 0.0, 8.0, four_on_floor(period));
            assert!(t.locked, "{bpm} bpm should lock");
            assert!(
                (t.bpm() as f64 - bpm).abs() < 1.5,
                "{bpm} bpm read as {}",
                t.bpm()
            );
            let off = (t.anchor / period).rem_euclid(1.0);
            let off = off.min(1.0 - off) * period;
            assert!(off < 0.015, "{bpm} bpm: grid is {off:.3}s off the kicks");
        }
    }

    #[test]
    fn locks_quickly() {
        let mut t = Tempo::new(HOP);
        run(&mut t, 0.0, 3.0, four_on_floor(0.5));
        assert!(t.locked);
        assert!((t.bpm() - 120.0).abs() < 2.0);
    }

    #[test]
    fn a_tresillo_kick_does_not_fool_it() {
        // Kicks on a 3-3-2 pattern (the gaps suggest 160 bpm), snare on the backbeat and
        // straight eighth-note hats: the pulse is still 120.
        let mut t = Tempo::new(HOP);
        run(&mut t, 0.0, 10.0, |t| {
            let k = hit(t, 1.0, 0.0) + hit(t, 1.0, 0.375) + hit(t, 1.0, 0.75);
            let s = hit(t, 1.0, 0.5);
            let h = hit(t, 0.25, 0.0);
            (k + 0.7 * s + 0.3 * h, k + 0.3 * s)
        });
        assert!(t.locked);
        assert!((t.bpm() - 120.0).abs() < 2.0, "read as {}", t.bpm());
    }

    #[test]
    fn follows_a_tempo_change_but_not_instantly() {
        let mut t = Tempo::new(HOP);
        run(&mut t, 0.0, 8.0, four_on_floor(0.5));
        run(&mut t, 8.0, 1.0, four_on_floor(60.0 / 140.0));
        assert!((t.bpm() - 120.0).abs() < 3.0, "one second in: {}", t.bpm());
        run(&mut t, 9.0, 9.0, four_on_floor(60.0 / 140.0));
        assert!(
            (t.bpm() - 140.0).abs() < 2.5,
            "after ten seconds: {}",
            t.bpm()
        );
    }

    #[test]
    fn keeps_time_through_a_gap_then_lets_go() {
        let mut t = Tempo::new(HOP);
        run(&mut t, 0.0, 8.0, four_on_floor(0.5));
        let anchor = t.anchor;
        run(&mut t, 8.0, 3.0, |_| (0.0, 0.0));
        assert!(t.locked, "a 3 s gap keeps the grid");
        let drift = ((t.anchor - anchor) / 0.5).rem_euclid(1.0);
        assert!(drift.min(1.0 - drift) < 0.05);
        run(&mut t, 11.0, 12.0, |_| (0.0, 0.0));
        assert!(!t.locked, "long silence unlocks");
        assert_eq!(t.bpm(), 0.0);
    }

    #[test]
    fn noise_and_silence_do_not_lock() {
        let mut t = Tempo::new(HOP);
        run(&mut t, 0.0, 10.0, |_| (0.0, 0.0));
        assert!(!t.locked);
        let mut rng = crate::rng::Rng::new(5);
        let noise: Vec<f32> = (0..4000).map(|_| rng.f32() * 0.5).collect();
        let mut t = Tempo::new(HOP);
        run(&mut t, 0.0, 10.0, |x| {
            let v = noise[((x / HOP) as usize) % noise.len()];
            (v, v)
        });
        assert!(!t.locked, "confidence {}", t.confidence);
    }
}
