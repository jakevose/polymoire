//! Turns audio (levels + beats) and the Energy trait into per-frame visual modifiers.
//!
//! Two comfort rules shape everything here, and tests enforce them:
//! - **Gentle camera.** Most movement is local and incoherent: the image wobbles in many
//!   directions at once, shapes re-form in eased steps on the beat, and motes orbit in
//!   place. Whole-view zooms and spins happen only at half the tempo (every other beat),
//!   are small ([`MAX_ZOOM`], [`MAX_SWAY`]) and eased, and Reduce motion turns them off.
//!   Whole-field motion is what triggers motion sickness, so it is rationed.
//! - **No flashing.** Brightness may rise at most [`MAX_EXPO`] above normal and changes no
//!   faster than [`EXPO_SLEW`] per second, well inside photosensitivity guidance.

use crate::analysis::{Hits, Levels};
use crate::dsp::smooth_k;
use crate::traits::ReactionWeights;

/// Highest brightness multiplier a beat may cause.
pub const MAX_EXPO: f32 = 1.10;
/// Fastest brightness change, per second.
pub const EXPO_SLEW: f32 = 0.6;
/// How long a beat-stepped re-form takes, in seconds.
pub const MORPH_EASE: f32 = 0.22;
/// Typical half-time zoom depth at full intensity.
pub const ZOOM_BASE: f32 = 0.05;
/// Deepest half-time zoom ever (heavy, intense music): the view scales in by at most this.
pub const MAX_ZOOM: f32 = 0.08;
/// Widest half-time sway, in radians (about 4°), alternating left and right.
pub const MAX_SWAY: f32 = 0.07;

/// Per-frame visual modifiers handed to the renderer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Visual {
    /// Half-time zoom: 1.0 at rest, down to `1 - MAX_ZOOM` on a hit.
    pub zoom: f32,
    /// Half-time sway added to the view angle, radians.
    pub ang_off: f32,
    pub warp_add: f32,
    pub chroma_add: f32,
    pub expo: f32,
    pub hue: f32,
    pub spark: f32,
    pub vig: f32,
    /// Local, incoherent displacement ("jelly") amplitude and its clock.
    pub jelly: f32,
    pub jelly_t: f32,
    /// Beat-stepped re-forming: displacement seeds A→B, eased by `morph_m`.
    pub morph_a: f32,
    pub morph_b: f32,
    pub morph_m: f32,
    pub morph_amp: f32,
    /// Motes: density (0..1), orbit clock, and size in logical pixels.
    pub motes: f32,
    pub mote_t: f32,
    pub mote_size: f32,
}

impl Visual {
    pub const NEUTRAL: Visual = Visual {
        zoom: 1.0,
        ang_off: 0.0,
        warp_add: 0.0,
        chroma_add: 0.0,
        expo: 1.0,
        hue: 0.0,
        spark: 0.0,
        vig: 0.6,
        jelly: 0.0,
        jelly_t: 0.0,
        morph_a: 0.0,
        morph_b: 0.0,
        morph_m: 1.0,
        morph_amp: 0.0,
        motes: 0.0,
        mote_t: 0.0,
        mote_size: 3.0,
    };
}

/// Envelopes and clocks that persist between frames.
#[derive(Clone, Debug)]
pub struct Pulse {
    /// 0..1: how much audio is currently driving the picture.
    pub gate: f32,
    pub kick: f32,
    kick_raw: f32,
    glow: f32,
    glow_raw: f32,
    hue_acc: f32,
    hue: f32,
    pub flow_boost: f32,
    expo: f32,
    jelly_t: f64,
    mote_t: f64,
    morph_seed: f32,
    morph_age: f32,
    ambient_step: f32,
    beats: u64,
    cam_raw: f32,
    cam: f32,
    sway: f32,
    sway_mid: f32,
    sway_sign: f32,
    pub snare_env: f32,
    pub hat_env: f32,
    /// Half-time camera hits so far (diagnostics and tests).
    pub cam_hits: u64,
}

impl Default for Pulse {
    fn default() -> Self {
        Pulse {
            gate: 0.0,
            kick: 0.0,
            kick_raw: 0.0,
            glow: 0.0,
            glow_raw: 0.0,
            hue_acc: 0.0,
            hue: 0.0,
            flow_boost: 0.0,
            expo: 1.0,
            jelly_t: 0.0,
            mote_t: 0.0,
            morph_seed: 0.0,
            morph_age: 10.0,
            ambient_step: 0.0,
            beats: 0,
            cam_raw: 0.0,
            cam: 0.0,
            sway: 0.0,
            sway_mid: 0.0,
            sway_sign: 1.0,
            snare_env: 0.0,
            hat_env: 0.0,
            cam_hits: 0,
        }
    }
}

/// What the music is doing beyond the beat (from the analyzer and the vibe).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Sound {
    /// Steady low end, 0..1.
    pub bass_sustain: f32,
    /// Overall intensity (loudness and busyness), 0..1.
    pub intensity: f32,
    /// How bass-weighted and heavy, 0..1.
    pub heaviness: f32,
    /// Polyrhythm and syncopation, 0..1.
    pub poly: f32,
    pub syncopation: f32,
    /// Snare hits per second.
    pub snare_rate: f32,
}

/// Everything a frame needs to compute the visual.
#[derive(Clone, Copy, Debug)]
pub struct FrameInput<'a> {
    pub dt: f32,
    pub levels: &'a Levels,
    /// Audio is arriving from the selected source.
    pub live: bool,
    /// A beat lands this frame.
    pub beat: bool,
    pub weights: &'a ReactionWeights,
    /// The Energy trait: ambient motion level without audio, 0..1.
    pub energy: f32,
    /// Beat period in seconds (used to pace the half-time camera).
    pub period: f32,
    /// Instrument hits landing this frame.
    pub hits: Hits,
    pub sound: Sound,
    pub reduced_motion: bool,
}

impl Pulse {
    /// Audio is considered "driving" once the gate is past halfway.
    pub fn active(&self) -> bool {
        self.gate > 0.5
    }

    /// How much to add to the flow speed (scaled by the gate).
    pub fn flow_speed_add(&self, reduced_motion: bool) -> f32 {
        let m = if reduced_motion { 0.3 } else { 1.0 };
        self.gate * m * self.flow_boost * 0.8
    }

    fn step_morph(&mut self) {
        self.morph_seed += 1.0;
        self.morph_age = 0.0;
    }

    /// Advance one frame and return the visual modifiers.
    pub fn frame(&mut self, f: FrameInput) -> Visual {
        let dt = f.dt;
        let l = f.levels;
        let calm = if f.reduced_motion { 0.35 } else { 1.0 };

        // Envelopes
        if f.beat {
            self.kick_raw = 1.0;
            self.glow_raw = 1.0;
            self.hue_acc += 0.012;
            // Camera moves at half tempo: every other beat.
            self.beats += 1;
            if self.beats % 2 == 1 {
                self.cam_raw = 1.0;
                self.cam_hits += 1;
                if f.sound.snare_rate < 0.3 {
                    self.sway_sign = -self.sway_sign;
                }
            }
        }
        // Each instrument has its own job. Kicks punch; snares shift color and, when there
        // is a backbeat, set the sway; hats make the motes twinkle.
        let hits = f.hits;
        let s = f.sound;
        if hits.kick > 0.0 {
            self.kick_raw = self.kick_raw.max(hits.kick);
        }
        if hits.snare > 0.0 {
            self.snare_env = self.snare_env.max(hits.snare);
            self.hue_acc += 0.02 * hits.snare * (1.0 + s.poly);
            if s.snare_rate >= 0.3 {
                self.sway_sign = -self.sway_sign;
            }
        }
        if hits.hat > 0.0 {
            self.hat_env = self.hat_env.max(hits.hat);
        }
        self.snare_env *= (-dt * 6.0).exp();
        self.hat_env *= (-dt * 10.0).exp();
        let period = f.period.clamp(0.25, 1.5);
        self.cam_raw *= (-dt * 2.0 / period).exp();
        self.cam += (self.cam_raw - self.cam) * smooth_k(12.0, dt);
        // Two smoothing stages give an ease-in-out rock rather than a jolt. Heavy music
        // pushes and pulls more than it rocks.
        let sway_amp = MAX_SWAY * (0.5 + 0.5 * s.intensity) * (1.0 - 0.5 * s.heaviness);
        let k = smooth_k(4.0 / period, dt);
        self.sway_mid += (self.sway_sign * sway_amp - self.sway_mid) * k;
        self.sway += (self.sway_mid - self.sway) * k;
        self.kick_raw *= (-dt * 5.0).exp();
        self.kick += (self.kick_raw - self.kick) * smooth_k(22.0, dt);
        self.glow_raw *= (-dt * 1.8).exp();
        self.glow += (self.glow_raw - self.glow) * smooth_k(6.0, dt);
        self.hue += (self.hue_acc - self.hue) * smooth_k(5.0, dt);
        let level = if f.live { l.level_fast } else { 0.0 };
        self.flow_boost += (level - self.flow_boost) * smooth_k(0.6, dt);
        let present = f.live && l.present;
        let target = if present { 1.0 } else { 0.0 };
        let rate = if present { 3.0 } else { 0.5 };
        self.gate += (target - self.gate) * smooth_k(rate, dt);

        let g = self.gate;
        let [w_br, w_pu, w_gl, w_sh, w_hu, w_je, w_mo, w_mt, w_zo, w_sp] = *f.weights;
        let cam = if f.reduced_motion { 0.0 } else { g };
        let (low, mid, high, kk) = (l.low * g, l.mid * g, l.high * g, self.kick * g);
        let e = f.energy.clamp(0.0, 1.0);

        // Beat-stepped re-forming. With music, steps land on beats the reaction asks for;
        // without music, Energy sets an ambient step every couple of seconds.
        self.morph_age += dt;
        let music_morph = g * w_mo;
        let syncopated_snare = hits.snare > 0.0 && s.syncopation > 0.35 && self.morph_age > 0.12;
        if (f.beat || syncopated_snare) && music_morph > 0.25 {
            self.step_morph();
        } else if g < 0.5 && e > 0.4 {
            self.ambient_step += dt;
            if self.ambient_step > 3.2 - e * 1.6 {
                self.ambient_step = 0.0;
                self.step_morph();
            }
        }
        let morph_m = crate::traits::ease((self.morph_age / MORPH_EASE).min(1.0));
        let morph_amp = calm * ((e - 0.3).max(0.0) * 0.12 + music_morph * 0.22);

        // Intensity scales how hard the music pushes; bass sustain adds weight.
        let react = 0.55 + 0.6 * s.intensity;
        let bass = s.bass_sustain * g;
        let heav = s.heaviness;

        // Local wobble and motes: always some with Energy, more with the music.
        let jelly = calm
            * (e * 0.12
                + react * (w_je * (kk * 0.22 + mid * 0.08) + w_pu * kk * 0.08)
                + bass * 0.18 * (0.5 + heav));
        self.jelly_t += (dt * (0.25 + e * 0.5 + g * (mid * 0.5 + w_je * kk * 0.8)) * calm) as f64;
        let motes = ((e - 0.3).max(0.0) * 0.55 + w_mt * g * 0.25 + g * s.poly * 0.15).min(0.6);
        self.mote_t += (dt * (0.5 + e * 0.7 + w_mt * high * 1.0) * calm) as f64;
        let mote_size = 2.5 + e * 1.5 + w_mt * high * 2.0 + self.hat_env * g * 2.5 * (0.5 + s.poly);

        // Brightness: small target, slew-limited so it can never flash.
        let expo_target =
            (1.0 + w_pu * kk * 0.07 + w_gl * self.glow * g * 0.08 + w_br * low * 0.04)
                .min(MAX_EXPO);
        let max_step = EXPO_SLEW * dt;
        self.expo += (expo_target - self.expo).clamp(-max_step, max_step);

        let depth = (ZOOM_BASE * (0.6 + 0.4 * s.intensity) * (1.0 + 0.6 * heav)).min(MAX_ZOOM);
        Visual {
            zoom: 1.0 - cam * w_zo.min(1.0) * self.cam * depth,
            ang_off: cam * w_sp.min(1.0) * self.sway,
            warp_add: calm * (w_br * mid * 0.1 + bass * 0.25 * heav),
            chroma_add: w_sh * high * 0.3 + g * self.snare_env * 0.2,
            expo: self.expo,
            hue: g * self.hue + w_hu * kk * 0.03 + w_sh * high * 0.015,
            spark: w_sh * high * 0.06 + g * self.hat_env * 0.03,
            vig: 0.6 - (w_br + 0.2) * low * 0.2 - bass * 0.12 * heav,
            jelly,
            jelly_t: (self.jelly_t % 10_000.0) as f32,
            morph_a: self.morph_seed - 1.0,
            morph_b: self.morph_seed,
            morph_m,
            morph_amp,
            motes,
            mote_t: (self.mote_t % 10_000.0) as f32,
            mote_size,
        }
    }
}

/// Detects a "drop": energy jumping well above its recent average.
pub fn is_drop(levels: &Levels) -> bool {
    levels.level_slow > 0.004 && levels.level_fast > levels.level_slow * 1.7
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 60.0;
    const ALL: ReactionWeights = [1.0; 10];

    fn loud() -> Levels {
        Levels {
            low: 1.0,
            mid: 1.0,
            high: 1.0,
            level_fast: 0.2,
            level_slow: 0.2,
            bpm: 120.0,
            present: true,
            ..Default::default()
        }
    }

    fn input<'a>(levels: &'a Levels, live: bool, beat: bool, energy: f32) -> FrameInput<'a> {
        FrameInput {
            dt: DT,
            levels,
            live,
            beat,
            weights: &ALL,
            energy,
            period: 0.5,
            hits: Hits::default(),
            sound: if live { MUSIC } else { Sound::default() },
            reduced_motion: false,
        }
    }

    /// A typical lively track.
    const MUSIC: Sound = Sound {
        bass_sustain: 0.4,
        intensity: 0.8,
        heaviness: 0.3,
        poly: 0.0,
        syncopation: 0.2,
        snare_rate: 0.0,
    };

    /// Runs `secs` of music where `hit(i)` gives the hits on frame `i`; returns visuals.
    fn run_with(
        p: &mut Pulse,
        secs: f32,
        sound: Sound,
        hit: impl Fn(usize) -> Hits,
    ) -> Vec<Visual> {
        let l = loud();
        (0..(secs / DT) as usize)
            .map(|i| {
                p.frame(FrameInput {
                    hits: hit(i),
                    sound,
                    ..input(&l, true, i.is_multiple_of(30), 0.5)
                })
            })
            .collect()
    }

    fn peak(vs: &[Visual], f: impl Fn(&Visual) -> f32) -> f32 {
        vs.iter().map(f).fold(f32::MIN, f32::max)
    }

    /// Runs `secs` of loud music with beats every `period` seconds; returns all visuals.
    fn run_music(p: &mut Pulse, secs: f32, period: f32, energy: f32) -> Vec<Visual> {
        let l = loud();
        let frames = (secs / DT) as usize;
        let per = (period / DT).round() as usize;
        (0..frames)
            .map(|i| p.frame(input(&l, true, i.is_multiple_of(per), energy)))
            .collect()
    }

    #[test]
    fn silence_is_calm_and_unlit() {
        let mut p = Pulse::default();
        let quiet = Levels::default();
        let mut v = Visual::NEUTRAL;
        for _ in 0..600 {
            v = p.frame(input(&quiet, false, false, 0.0));
        }
        assert_eq!(v.zoom, 1.0);
        assert_eq!(v.ang_off, 0.0);
        assert_eq!(v.expo, 1.0);
        assert_eq!(v.jelly, 0.0);
        assert_eq!(v.motes, 0.0);
        assert!(!p.active());
    }

    #[test]
    fn energy_moves_the_picture_without_music() {
        let mut p = Pulse::default();
        let quiet = Levels::default();
        let first = p.frame(input(&quiet, false, false, 0.9));
        let mut v = first;
        for _ in 0..600 {
            v = p.frame(input(&quiet, false, false, 0.9));
        }
        assert!(v.jelly > 0.05, "jelly {}", v.jelly);
        assert!(v.motes > 0.2, "motes {}", v.motes);
        assert!(v.jelly_t > first.jelly_t, "wobble clock should advance");
        assert!(v.morph_b > first.morph_b, "ambient re-forms should step");
        assert_eq!(v.expo, 1.0, "no brightness change without music");
    }

    #[test]
    fn beats_step_the_shapes_with_an_ease() {
        let mut p = Pulse::default();
        let vs = run_music(&mut p, 4.0, 0.5, 0.5);
        let steps = vs
            .windows(2)
            .filter(|w| w[1].morph_b > w[0].morph_b)
            .count();
        assert!(
            (6..=9).contains(&steps),
            "{steps} re-forms in 4 s at 120 bpm"
        );
        let after = vs
            .windows(2)
            .position(|w| w[1].morph_b > w[0].morph_b)
            .unwrap()
            + 1;
        assert!(vs[after].morph_m < 0.2, "re-form must ease, not cut");
    }

    #[test]
    fn camera_moves_at_half_tempo_and_gently() {
        let mut p = Pulse::default();
        let vs = run_music(&mut p, 4.0, 0.5, 1.0);
        assert_eq!(p.cam_hits, 4, "8 beats should give 4 half-time hits");
        let deepest = vs.iter().map(|v| v.zoom).fold(f32::MAX, f32::min);
        assert!(deepest < 0.99, "zoom should be visible: {deepest}");
        assert!(deepest >= 1.0 - MAX_ZOOM - 1e-6, "zoom too deep: {deepest}");
        for v in &vs {
            assert!(v.zoom <= 1.0 + 1e-6);
            assert!(v.ang_off.abs() <= MAX_SWAY + 1e-6, "sway {}", v.ang_off);
        }
        // The sway rocks both ways rather than spinning on.
        assert!(vs.iter().any(|v| v.ang_off > 0.03) && vs.iter().any(|v| v.ang_off < -0.03));
    }

    #[test]
    fn camera_eases_instead_of_jumping() {
        let mut p = Pulse::default();
        let vs = run_music(&mut p, 6.0, 0.5, 1.0);
        for w in vs.windows(2) {
            assert!((w[1].zoom - w[0].zoom).abs() < 0.02, "zoom jump");
            assert!((w[1].ang_off - w[0].ang_off).abs() < 0.01, "sway jump");
        }
    }

    #[test]
    fn reduced_motion_keeps_the_camera_still() {
        let l = loud();
        let mut p = Pulse::default();
        for i in 0..300 {
            let v = p.frame(FrameInput {
                reduced_motion: true,
                ..input(&l, true, i % 30 == 0, 1.0)
            });
            assert_eq!(v.zoom, 1.0);
            assert_eq!(v.ang_off, 0.0);
        }
    }

    #[test]
    fn brightness_never_flashes() {
        // Fast, loud music (180 bpm) with every reaction at full strength.
        let mut p = Pulse::default();
        let vs = run_music(&mut p, 10.0, 1.0 / 3.0, 1.0);
        let mut prev = 1.0;
        for v in &vs {
            assert!(v.expo <= MAX_EXPO + 1e-6, "expo {}", v.expo);
            assert!(v.expo >= 1.0 - 1e-6);
            assert!((v.expo - prev).abs() <= EXPO_SLEW * DT + 1e-6, "too fast");
            prev = v.expo;
        }
        // Any one second spans less than a 10% brightness swing, the threshold for a flash.
        for w in vs.windows(60) {
            let hi = w.iter().map(|v| v.expo).fold(f32::MIN, f32::max);
            let lo = w.iter().map(|v| v.expo).fold(f32::MAX, f32::min);
            assert!(hi - lo < 0.1, "swing {} in one second", hi - lo);
        }
    }

    #[test]
    fn music_adds_kinetics() {
        let mut calm = Pulse::default();
        let quiet = Levels::default();
        let mut base = Visual::NEUTRAL;
        for _ in 0..300 {
            base = calm.frame(input(&quiet, false, false, 0.5));
        }
        let mut p = Pulse::default();
        let vs = run_music(&mut p, 5.0, 0.5, 0.5);
        let peak_jelly = vs.iter().map(|v| v.jelly).fold(0.0, f32::max);
        assert!(
            peak_jelly > base.jelly * 2.0,
            "{peak_jelly} vs {}",
            base.jelly
        );
        assert!(vs.last().unwrap().motes > base.motes);
    }

    #[test]
    fn reduced_motion_damps_kinetics() {
        let l = loud();
        let mut a = Pulse::default();
        let mut b = Pulse::default();
        let mut va = Visual::NEUTRAL;
        let mut vb = Visual::NEUTRAL;
        for i in 0..300 {
            let beat = i % 30 == 0;
            va = a.frame(input(&l, true, beat, 1.0));
            vb = b.frame(FrameInput {
                reduced_motion: true,
                ..input(&l, true, beat, 1.0)
            });
        }
        assert!(vb.jelly < va.jelly * 0.5);
        assert!(vb.morph_amp < va.morph_amp * 0.5);
    }

    #[test]
    fn gate_opens_fast_and_closes_slowly() {
        let mut p = Pulse::default();
        let l = loud();
        for _ in 0..60 {
            p.frame(input(&l, true, false, 0.5));
        }
        assert!(p.active(), "gate after 1 s: {}", p.gate);
        let quiet = Levels::default();
        for _ in 0..60 {
            p.frame(input(&quiet, true, false, 0.5));
        }
        assert!(p.gate > 0.3, "should fade, not snap: {}", p.gate);
    }

    #[test]
    fn snares_shift_color_without_moving_the_camera() {
        let none = |_| Hits::default();
        let snares = |i: usize| Hits {
            snare: if i % 60 == 30 { 1.0 } else { 0.0 },
            ..Default::default()
        };
        let quiet = run_with(&mut Pulse::default(), 6.0, MUSIC, none);
        let hit = run_with(&mut Pulse::default(), 6.0, MUSIC, snares);
        assert!(peak(&hit, |v| v.chroma_add) > peak(&quiet, |v| v.chroma_add) + 0.1);
        assert!(hit.last().unwrap().hue > quiet.last().unwrap().hue + 0.05);
        let zq: Vec<f32> = quiet.iter().map(|v| v.zoom).collect();
        let zh: Vec<f32> = hit.iter().map(|v| v.zoom).collect();
        assert_eq!(zq, zh, "snares must not drive the zoom");
    }

    #[test]
    fn hats_make_the_motes_twinkle() {
        let none = |_| Hits::default();
        let hats = |i: usize| Hits {
            hat: if i.is_multiple_of(8) { 1.0 } else { 0.0 },
            ..Default::default()
        };
        let quiet = run_with(&mut Pulse::default(), 4.0, MUSIC, none);
        let hit = run_with(&mut Pulse::default(), 4.0, MUSIC, hats);
        assert!(peak(&hit, |v| v.mote_size) > peak(&quiet, |v| v.mote_size) + 1.0);
        assert!(peak(&hit, |v| v.spark) > peak(&quiet, |v| v.spark));
    }

    #[test]
    fn sustained_bass_adds_weight() {
        let light = Sound {
            bass_sustain: 0.0,
            ..MUSIC
        };
        let heavy = Sound {
            bass_sustain: 0.9,
            heaviness: 0.9,
            ..MUSIC
        };
        let a = run_with(&mut Pulse::default(), 4.0, light, |_| Hits::default());
        let b = run_with(&mut Pulse::default(), 4.0, heavy, |_| Hits::default());
        let (la, lb) = (a.last().unwrap(), b.last().unwrap());
        assert!(lb.jelly > la.jelly + 0.1, "{} vs {}", lb.jelly, la.jelly);
        assert!(lb.warp_add > la.warp_add + 0.1);
        assert!(lb.vig < la.vig);
    }

    #[test]
    fn heavy_music_zooms_deeper_but_within_the_limit() {
        let light = Sound {
            heaviness: 0.0,
            ..MUSIC
        };
        let heavy = Sound {
            heaviness: 1.0,
            intensity: 1.0,
            ..MUSIC
        };
        let a = run_with(&mut Pulse::default(), 4.0, light, |_| Hits::default());
        let b = run_with(&mut Pulse::default(), 4.0, heavy, |_| Hits::default());
        let deep_a = a.iter().map(|v| v.zoom).fold(f32::MAX, f32::min);
        let deep_b = b.iter().map(|v| v.zoom).fold(f32::MAX, f32::min);
        assert!(deep_b < deep_a - 0.01, "{deep_b} vs {deep_a}");
        assert!(deep_b >= 1.0 - MAX_ZOOM - 1e-6);
        // Heavy music rocks less.
        assert!(peak(&b, |v| v.ang_off.abs()) < peak(&a, |v| v.ang_off.abs()));
    }

    #[test]
    fn intensity_scales_the_push() {
        let soft = Sound {
            intensity: 0.1,
            ..MUSIC
        };
        let hard = Sound {
            intensity: 1.0,
            ..MUSIC
        };
        let a = run_with(&mut Pulse::default(), 4.0, soft, |_| Hits::default());
        let b = run_with(&mut Pulse::default(), 4.0, hard, |_| Hits::default());
        assert!(peak(&b, |v| v.jelly) > peak(&a, |v| v.jelly) * 1.3);
    }

    #[test]
    fn syncopated_snares_also_re_form_the_shapes() {
        let straight = Sound {
            syncopation: 0.0,
            ..MUSIC
        };
        let synco = Sound {
            syncopation: 0.7,
            ..MUSIC
        };
        let offbeat = |i: usize| Hits {
            snare: if i % 30 == 20 { 1.0 } else { 0.0 },
            ..Default::default()
        };
        let a = run_with(&mut Pulse::default(), 4.0, straight, offbeat);
        let b = run_with(&mut Pulse::default(), 4.0, synco, offbeat);
        assert!(b.last().unwrap().morph_b > a.last().unwrap().morph_b + 3.0);
    }

    #[test]
    fn hits_never_cause_flashing() {
        // Every instrument hitting on every frame, at full intensity and heaviness.
        let all = |_| Hits {
            kick: 1.0,
            snare: 1.0,
            hat: 1.0,
        };
        let wild = Sound {
            bass_sustain: 1.0,
            intensity: 1.0,
            heaviness: 1.0,
            poly: 1.0,
            syncopation: 1.0,
            snare_rate: 4.0,
        };
        let vs = run_with(&mut Pulse::default(), 8.0, wild, all);
        let mut prev = 1.0;
        for v in &vs {
            assert!(v.expo <= MAX_EXPO + 1e-6);
            assert!((v.expo - prev).abs() <= EXPO_SLEW * DT + 1e-6);
            assert!(v.zoom >= 1.0 - MAX_ZOOM - 1e-6);
            assert!(v.ang_off.abs() <= MAX_SWAY + 1e-6);
            prev = v.expo;
        }
    }

    #[test]
    fn drop_detection() {
        let mut l = loud();
        assert!(!is_drop(&l));
        l.level_fast = 0.5;
        l.level_slow = 0.1;
        assert!(is_drop(&l));
        l.level_slow = 0.001;
        assert!(!is_drop(&l), "near-silence never counts as a drop");
    }
}
