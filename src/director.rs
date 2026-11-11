//! When the look changes. With music, trait shifts land on the beat every four or eight
//! beats, and a drop or a change of mood brings a bigger one. Without music the picture
//! settles: shifts come slowly, one trait at a time, with long crossfades.
//!
//! The pacing without music is a comfort rule. When system audio stopped, the old timer
//! kept changing traits every 4-8 s with no beat to anchor them, on top of a mood change
//! and the last beat shifts. Users read it as the app frantically "searching", which was
//! uncomfortable to watch. Tests here pin the calmer pacing down.

use crate::rng::Rng;

/// Shortest gap between two automatic shifts while music plays, seconds. A drop or mood
/// change landing just after a beat shift waits for it, instead of stacking a second change.
pub const MIN_GAP: f64 = 2.0;
/// With music but no beat (ambient music, or the tempo lost), shifts come on a timer:
/// this long after the last beat, then every [`DRIFT_HOLD`] seconds.
pub const BEATLESS_AFTER: f64 = 6.0;
pub const DRIFT_HOLD: (f32, f32) = (7.0, 11.0);
/// Without music: seconds between shifts, and their crossfade length.
pub const IDLE_HOLD: (f32, f32) = (12.0, 20.0);
pub const IDLE_FADE: (f32, f32) = (6.0, 9.0);
/// After the music stops, the picture holds still at least this long before its first
/// idle shift.
pub const SETTLE: f64 = 12.0;

/// A shift to start now: how many traits (`None` lets the trait engine pick one or two) and
/// the crossfade length in seconds (`None` for the engine's default).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cue {
    pub n: Option<usize>,
    pub dur: Option<f64>,
}

/// What the director needs to know each frame.
#[derive(Clone, Copy, Debug, Default)]
pub struct Moment {
    /// Simulation time, seconds (stops while paused).
    pub sim: f64,
    /// Music is driving the picture (`Pulse::active`).
    pub music: bool,
    /// A beat lands this frame.
    pub beat: bool,
    /// Beat period, seconds.
    pub period: f64,
    /// A drop or a change of mood this frame.
    pub big_change: bool,
    pub reduced_motion: bool,
}

pub struct Director {
    rng: Rng,
    /// Next timed shift (beatless music, or no music).
    next_at: f64,
    beats_to_shift: u32,
    last_big: f64,
    last_shift: f64,
    last_beat: f64,
    was_music: bool,
}

impl Director {
    pub fn new(rng: Rng) -> Self {
        Director {
            rng,
            next_at: 3.0,
            beats_to_shift: 8,
            last_big: -99.0,
            last_shift: -99.0,
            last_beat: -99.0,
            was_music: false,
        }
    }

    /// Call once per frame (not while paused). Returns a shift to start now, if any.
    pub fn step(&mut self, m: Moment) -> Option<Cue> {
        let slow = if m.reduced_motion { 2.0 } else { 1.0 };
        if m.beat {
            self.last_beat = m.sim;
        }
        let cue = if m.music {
            self.with_music(m, slow)
        } else {
            if self.was_music {
                // The music just stopped: hold still, then drift slowly.
                self.next_at = self.next_at.max(m.sim + SETTLE * slow);
            }
            (m.sim >= self.next_at).then(|| {
                self.next_at = m.sim + self.range(IDLE_HOLD) * slow;
                Cue {
                    n: Some(1),
                    dur: Some(self.range(IDLE_FADE)),
                }
            })
        };
        self.was_music = m.music;
        if cue.is_some() {
            self.last_shift = m.sim;
        }
        cue
    }

    fn with_music(&mut self, m: Moment, slow: f64) -> Option<Cue> {
        let since_shift = m.sim - self.last_shift;
        // A drop or a change of mood brings a bigger scene change, then the beat count
        // starts over so a beat shift doesn't follow straight after.
        if m.big_change && m.sim - self.last_big > 8.0 && since_shift >= MIN_GAP {
            self.last_big = m.sim;
            self.beats_to_shift = self.beats_until_next();
            return Some(Cue {
                n: Some(2),
                dur: Some(m.period * 4.0),
            });
        }
        if m.sim - self.last_beat < BEATLESS_AFTER {
            // On the beat.
            self.next_at = self.next_at.max(m.sim + BEATLESS_AFTER);
            if m.beat {
                self.beats_to_shift = self.beats_to_shift.saturating_sub(1);
                if self.beats_to_shift == 0 && since_shift >= MIN_GAP {
                    self.beats_to_shift = self.beats_until_next();
                    let beats = self.beat_fade();
                    return Some(Cue {
                        n: None,
                        dur: Some(m.period * beats),
                    });
                }
            }
            return None;
        }
        // Music without a beat: drift on a timer.
        (m.sim >= self.next_at).then(|| {
            self.next_at = m.sim + self.range(DRIFT_HOLD) * slow;
            Cue { n: None, dur: None }
        })
    }

    /// What the HUD shows: the beats or seconds until the next shift.
    pub fn describe(&self, sim: f64, music: bool) -> String {
        if music && sim - self.last_beat < BEATLESS_AFTER {
            format!("on the beat · {}", self.beats_to_shift)
        } else {
            format!("next shift {:.1}s", (self.next_at - sim).max(0.0))
        }
    }

    /// The user asked for a shift (the N key): on the next frame without music, or the
    /// caller shifts on the beat grid itself.
    pub fn shift_now(&mut self, sim: f64) {
        self.next_at = sim;
    }

    fn range(&mut self, (lo, hi): (f32, f32)) -> f64 {
        self.rng.range(lo, hi) as f64
    }

    /// Beats between shifts while music drives the timing.
    fn beats_until_next(&mut self) -> u32 {
        if self.rng.chance(0.6) {
            8
        } else {
            4
        }
    }

    /// Crossfade length, in beats, for shifts that land on the beat.
    fn beat_fade(&mut self) -> f64 {
        if self.rng.chance(0.5) {
            2.0
        } else {
            4.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FPS: f64 = 60.0;

    /// Runs the director for `secs` seconds from `start` and returns the times of its cues.
    fn run(
        d: &mut Director,
        start: f64,
        secs: f64,
        music: bool,
        beats: Option<f64>,
        reduced_motion: bool,
    ) -> Vec<(f64, Cue)> {
        let mut out = Vec::new();
        let frames = (secs * FPS) as usize;
        for f in 0..frames {
            let sim = start + f as f64 / FPS;
            let beat = beats.is_some_and(|p| f > 0 && f % (p * FPS).round() as usize == 0);
            let m = Moment {
                sim,
                music,
                beat,
                period: beats.unwrap_or(0.5),
                big_change: false,
                reduced_motion,
            };
            if let Some(c) = d.step(m) {
                out.push((sim, c));
            }
        }
        out
    }

    fn gaps(cues: &[(f64, Cue)]) -> Vec<f64> {
        cues.windows(2).map(|w| w[1].0 - w[0].0).collect()
    }

    #[test]
    fn music_shifts_on_the_beat_every_four_or_eight() {
        let mut d = Director::new(Rng::new(1));
        let cues = run(&mut d, 0.0, 120.0, true, Some(0.5), false);
        assert!(cues.len() > 20, "{}", cues.len());
        for g in gaps(&cues) {
            let beats = (g / 0.5).round();
            assert!(beats == 4.0 || beats == 8.0, "gap of {g:.2} s");
        }
    }

    #[test]
    fn when_the_music_stops_the_picture_settles() {
        for seed in 0..20 {
            let mut d = Director::new(Rng::new(seed));
            let music = run(&mut d, 0.0, 60.0, true, Some(0.5), false);
            let last = music.last().map_or(0.0, |c| c.0);
            let quiet = run(&mut d, 60.0, 600.0, false, None, false);
            let first = quiet[0].0;
            assert!(
                first >= 60.0 + SETTLE,
                "seed {seed}: first idle shift at {first:.1}"
            );
            assert!(first - last >= SETTLE);
            for g in gaps(&quiet) {
                assert!(
                    g >= IDLE_HOLD.0 as f64 - 0.1,
                    "seed {seed}: idle gap {g:.1}"
                );
            }
            for (_, c) in &quiet {
                assert_eq!(c.n, Some(1), "one trait at a time");
                assert!(c.dur.unwrap() >= IDLE_FADE.0 as f64);
            }
            // At most one change every twelve seconds, over ten minutes of silence.
            assert!(quiet.len() <= 50, "seed {seed}: {}", quiet.len());
        }
    }

    #[test]
    fn music_that_comes_and_goes_never_speeds_things_up() {
        // Audio flickering on and off (a paused video, notification sounds) must not turn
        // into a burst of changes.
        let mut d = Director::new(Rng::new(5));
        let mut cues = Vec::new();
        let mut t = 0.0;
        for k in 0..40 {
            let on = k % 2 == 0;
            let secs = if on { 1.5 } else { 2.5 };
            cues.extend(run(&mut d, t, secs, on, on.then_some(0.5), false));
            t += secs;
        }
        for g in gaps(&cues) {
            assert!(g >= MIN_GAP - 1e-9, "gap {g:.2} in {cues:?}");
        }
        assert!(cues.len() <= 160 / 8, "{} shifts in 160 s", cues.len());
    }

    #[test]
    fn beatless_music_still_drifts() {
        // Music playing but no beat (ambient, or the tempo lost): the look must not freeze.
        let mut d = Director::new(Rng::new(2));
        let cues = run(&mut d, 0.0, 120.0, true, None, false);
        assert!(cues.len() >= 8, "{}", cues.len());
        for g in gaps(&cues) {
            assert!(g >= DRIFT_HOLD.0 as f64 - 0.1);
        }
    }

    #[test]
    fn a_drop_does_not_stack_on_a_beat_shift() {
        let mut d = Director::new(Rng::new(3));
        let p = 0.5;
        let mut cues = Vec::new();
        for f in 0..(60.0 * FPS) as usize {
            let sim = f as f64 / FPS;
            let beat = f > 0 && f % (p * FPS) as usize == 0;
            // A drop or mood change right after every beat.
            let m = Moment {
                sim,
                music: true,
                beat,
                period: p,
                big_change: (sim % p) > 0.05 && (sim % p) < 0.07,
                reduced_motion: false,
            };
            if let Some(c) = d.step(m) {
                cues.push((sim, c));
            }
        }
        for g in gaps(&cues) {
            assert!(g >= MIN_GAP - 1e-9, "gap {g:.2}");
        }
    }

    #[test]
    fn reduce_motion_slows_the_idle_drift() {
        let mut a = Director::new(Rng::new(4));
        let mut b = Director::new(Rng::new(4));
        let na = run(&mut a, 0.0, 600.0, false, None, false).len();
        let nb = run(&mut b, 0.0, 600.0, false, None, true).len();
        assert!(nb * 2 <= na + 1, "{na} vs {nb}");
    }
}
