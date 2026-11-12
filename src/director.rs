//! When the look changes. With a song's map (the media player), the look follows the
//! song: a new scene where a new section starts, small variations on phrase lines in
//! between. With live music, trait shifts land on the beat every four or eight beats,
//! and a drop or a change of mood brings a bigger one. Without music the picture
//! settles: shifts come slowly, one trait at a time, with long crossfades.
//!
//! The pacing without music is a comfort rule. When system audio stopped, the old timer
//! kept changing traits every 4-8 s with no beat to anchor them, on top of a mood change
//! and the last beat shifts. Users read it as the app frantically "searching", which was
//! uncomfortable to watch. Tests here pin the calmer pacing down.

use crate::rng::Rng;
use crate::structure::Place;
use crate::traits::Scope;

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

/// Live music: shortest time between two drop-or-mood scene changes, seconds. The live
/// mood can flip back and forth across a boundary.
pub const BIG_GAP: f64 = 16.0;
/// With a song map: bars between variations in a loud section, and in a quieter one.
pub const PHRASE_BARS: usize = 4;
pub const QUIET_PHRASE_BARS: usize = 8;
/// A section this many dB under the loudest counts as quieter.
pub const QUIET_SECTION_DB: f32 = 3.0;
/// A long section gets a new scene this often, in bars, so a steady track still moves on.
pub const SCENE_BARS: usize = 16;
/// A new section this much louder (or quieter) than the last, dB, is a drop (or a
/// breakdown).
pub const SECTION_JUMP_DB: f32 = 3.0;

/// A shift to start now: how many traits (`None` lets the trait engine pick one or two) and
/// the crossfade length in seconds (`None` for the engine's default).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cue {
    pub n: Option<usize>,
    pub dur: Option<f64>,
    pub scope: Scope,
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
    /// Where the last beat sits in the song, when the song is mapped.
    pub place: Option<Place>,
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
    /// Where the song is, while following a map.
    place: Option<Place>,
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
            place: None,
        }
    }

    /// Call once per frame (not while paused). Returns a shift to start now, if any.
    pub fn step(&mut self, m: Moment) -> Option<Cue> {
        let slow = if m.reduced_motion { 2.0 } else { 1.0 };
        if m.beat {
            self.last_beat = m.sim;
        }
        self.place = m.place;
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
                    scope: Scope::Any,
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
        if let Some(p) = m.place {
            return self.with_map(m, p);
        }
        let since_shift = m.sim - self.last_shift;
        // A drop or a change of mood brings a bigger scene change, then the beat count
        // starts over so a beat shift doesn't follow straight after.
        if m.big_change && m.sim - self.last_big > BIG_GAP && since_shift >= MIN_GAP {
            self.last_big = m.sim;
            self.beats_to_shift = self.beats_until_next();
            return Some(Cue {
                n: Some(2),
                dur: Some(m.period * 4.0),
                scope: Scope::Any,
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
                        scope: Scope::Any,
                    });
                }
            }
            return None;
        }
        // Music without a beat: drift on a timer.
        (m.sim >= self.next_at).then(|| {
            self.next_at = m.sim + self.range(DRIFT_HOLD) * slow;
            Cue {
                n: None,
                dur: None,
                scope: Scope::Any,
            }
        })
    }

    /// Following a mapped song, on its beats only. A section start brings a new scene
    /// (quickly into a drop, slowly into a breakdown); a long section gets a new scene
    /// every [`SCENE_BARS`]; in between, one detail varies every phrase. Drops and moods
    /// aren't guessed from the live sound: the map already knows the sections.
    fn with_map(&mut self, m: Moment, p: Place) -> Option<Cue> {
        if !m.beat || p.beat_in_bar != 0 || m.sim - self.last_shift < MIN_GAP {
            return None;
        }
        self.next_at = self.next_at.max(m.sim + BEATLESS_AFTER);
        let beats = |n: f64| Some(m.period * n);
        if p.section_start {
            let rise = p.level - p.prev_level;
            let (n, dur) = if rise >= SECTION_JUMP_DB {
                (2, 2.0)
            } else if rise <= -SECTION_JUMP_DB {
                (2, 8.0)
            } else {
                (1, 4.0)
            };
            return Some(Cue {
                n: Some(n),
                dur: beats(dur),
                scope: Scope::Scene,
            });
        }
        let bar = p.bar_in_section;
        if bar == 0 {
            return None;
        }
        if bar.is_multiple_of(SCENE_BARS) {
            return Some(Cue {
                n: Some(1),
                dur: beats(4.0),
                scope: Scope::Scene,
            });
        }
        let phrase = if p.level < -QUIET_SECTION_DB {
            QUIET_PHRASE_BARS
        } else {
            PHRASE_BARS
        };
        bar.is_multiple_of(phrase).then(|| Cue {
            n: Some(1),
            dur: beats(4.0),
            scope: Scope::Detail,
        })
    }

    /// What the HUD shows: the beats or seconds until the next shift.
    pub fn describe(&self, sim: f64, music: bool) -> String {
        if let (true, Some(p)) = (music, self.place) {
            format!("following the song · bar {}", p.bar_in_section + 1)
        } else if music && sim - self.last_beat < BEATLESS_AFTER {
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

    /// Beats between shifts while live music drives the timing: two or four bars' worth
    /// (every four or eight beats kept a trait mid-change nearly half the time).
    fn beats_until_next(&mut self) -> u32 {
        if self.rng.chance(0.6) {
            8
        } else {
            16
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

/// Silence this long (seconds) brings the picture to a stop...
pub const STOP_AFTER: f32 = 1.0;
/// ...easing down over this long...
pub const STOP_FADE: f32 = 2.0;
/// ...and back up this fast when sound returns...
pub const START_FADE: f32 = 0.5;
/// ...once it has been back this long. A tail hovering at the presence threshold
/// flickers in and out; that counts as silence, not as sound returning.
pub const SOUND_FOR: f32 = 0.25;

/// Brings everything to a gentle stop when the sound goes away, and starts it again
/// when it comes back (the user's request: near-silent tails made the picture twitch,
/// and a still picture is calmer than a drift).
#[derive(Debug)]
pub struct Halt {
    silent_for: f32,
    heard_for: f32,
    /// 1 = full speed, 0 = stopped (linear; `speed` eases it).
    run: f32,
}

impl Default for Halt {
    fn default() -> Self {
        Halt {
            silent_for: 0.0,
            heard_for: SOUND_FOR,
            run: 1.0,
        }
    }
}

impl Halt {
    /// Advance one frame. `sound`: audio is arriving and above the presence threshold.
    /// Returns the speed to run the picture at, 0..=1.
    pub fn step(&mut self, dt: f32, sound: bool) -> f32 {
        self.heard_for = if sound { self.heard_for + dt } else { 0.0 };
        let back = self.heard_for >= SOUND_FOR;
        self.silent_for = if back { 0.0 } else { self.silent_for + dt };
        if self.silent_for > STOP_AFTER {
            self.run = (self.run - dt / STOP_FADE).max(0.0);
        } else if back {
            self.run = (self.run + dt / START_FADE).min(1.0);
        }
        self.speed()
    }

    /// Smoothstep of the ramp, so motion eases out and in rather than braking.
    pub fn speed(&self) -> f32 {
        let x = self.run;
        x * x * (3.0 - 2.0 * x)
    }

    pub fn stopped(&self) -> bool {
        self.run <= 0.0
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
                place: None,
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
    fn music_shifts_on_the_beat_every_eight_or_sixteen() {
        let mut d = Director::new(Rng::new(1));
        let cues = run(&mut d, 0.0, 120.0, true, Some(0.5), false);
        assert!(cues.len() > 12, "{}", cues.len());
        for g in gaps(&cues) {
            let beats = (g / 0.5).round();
            assert!(beats == 8.0 || beats == 16.0, "gap of {g:.2} s");
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
                place: None,
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

    #[test]
    fn silence_eases_to_a_stop_and_sound_starts_it_again() {
        let mut h = Halt::default();
        let dt = 1.0 / 60.0;
        let mut t = 0.0;
        let mut last = 1.0;
        // A second of silence changes nothing yet.
        while t < STOP_AFTER - 0.05 {
            assert_eq!(h.step(dt, false), 1.0);
            t += dt;
        }
        // Then a smooth, monotonic slowdown to a full stop.
        let mut biggest_step: f32 = 0.0;
        while !h.stopped() {
            let v = h.step(dt, false);
            assert!(v <= last);
            biggest_step = biggest_step.max(last - v);
            last = v;
            t += dt;
            assert!(t < STOP_AFTER + STOP_FADE + 0.5, "never stopped");
        }
        assert!(biggest_step < 0.02, "slowed too abruptly: {biggest_step}");
        // Flickering at the threshold doesn't start it.
        for i in 0..120 {
            assert_eq!(h.step(dt, i % 6 < 3), 0.0);
        }
        // Sound that stays starts it moving again, gently.
        let mut v = 0.0;
        for _ in 0..(SOUND_FOR * 60.0) as usize + 1 {
            v = h.step(dt, true);
        }
        assert!(v > 0.0 && v < 0.05, "{v}");
        for _ in 0..(START_FADE * 60.0) as usize + 2 {
            h.step(dt, true);
        }
        assert_eq!(h.speed(), 1.0);
    }

    #[test]
    fn a_mapped_song_changes_with_its_sections_and_phrases() {
        use crate::structure::{Section, Structure};
        // Intro (8 bars, -6 dB), drop (24 bars), breakdown (16 bars, -8 dB), drop again.
        let shape = Structure {
            bar: 1,
            sections: vec![
                Section {
                    beat: 1,
                    level: -6.0,
                },
                Section {
                    beat: 33,
                    level: 0.0,
                },
                Section {
                    beat: 129,
                    level: -8.0,
                },
                Section {
                    beat: 193,
                    level: 0.0,
                },
            ],
        };
        let p = 0.5;
        let mut d = Director::new(Rng::new(9));
        let mut cues = Vec::new();
        for i in 0..260 {
            for f in 0..(p * FPS) as usize {
                let sim = i as f64 * p + f as f64 / FPS;
                let m = Moment {
                    sim,
                    music: true,
                    beat: f == 0,
                    period: p,
                    big_change: f == 3,
                    reduced_motion: false,
                    place: Some(shape.place(i)),
                };
                if let Some(c) = d.step(m) {
                    cues.push((i, c));
                }
            }
        }
        let at = |beat: usize| cues.iter().find(|(i, _)| *i == beat).map(|(_, c)| *c);
        // Every change lands on a downbeat; drops and moods guessed live are ignored.
        assert!(cues.iter().all(|(i, _)| (i - 1) % 4 == 0), "{cues:?}");
        // Into the drop: a quick two-trait new scene. Into the breakdown: a slow one.
        let drop = at(33).expect("scene at the drop");
        assert_eq!(
            (drop.scope, drop.n, drop.dur),
            (Scope::Scene, Some(2), Some(1.0))
        );
        let calm = at(129).expect("scene at the breakdown");
        assert_eq!((calm.scope, calm.dur), (Scope::Scene, Some(4.0)));
        // In the drop, a detail every four bars, and a new scene after sixteen.
        assert_eq!(at(49).map(|c| c.scope), Some(Scope::Detail));
        assert_eq!(at(65).map(|c| c.scope), Some(Scope::Detail));
        assert_eq!(at(97).map(|c| c.scope), Some(Scope::Scene));
        assert!(at(37).is_none() && at(53).is_none());
        // In the quiet breakdown, every eight bars.
        assert!(at(145).is_none());
        assert_eq!(at(161).map(|c| c.scope), Some(Scope::Detail));
        // Far calmer than counting beats: 260 beats at 120 bpm, about 16 changes.
        assert!(cues.len() <= 18, "{} changes", cues.len());
    }
}
