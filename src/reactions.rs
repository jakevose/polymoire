//! Turns audio (levels + beats) and the Energy trait into per-frame visual modifiers.
//!
//! Two comfort rules shape everything here, and tests enforce them:
//! - **No camera motion.** Nothing zooms, pans or spins the whole picture on the beat;
//!   whole-field motion is what triggers motion sickness. Movement is local and
//!   incoherent: the image wobbles in many directions at once, shapes re-form in eased
//!   steps on the beat, and motes orbit in place.
//! - **No flashing.** Brightness may rise at most [`MAX_EXPO`] above normal and changes no
//!   faster than [`EXPO_SLEW`] per second, well inside photosensitivity guidance.

use crate::analysis::Levels;
use crate::dsp::smooth_k;
use crate::traits::ReactionWeights;

/// Highest brightness multiplier a beat may cause.
pub const MAX_EXPO: f32 = 1.10;
/// Fastest brightness change, per second.
pub const EXPO_SLEW: f32 = 0.6;
/// How long a beat-stepped re-form takes, in seconds.
pub const MORPH_EASE: f32 = 0.22;

/// Per-frame visual modifiers handed to the renderer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Visual {
    /// Always 1.0: kept in the uniforms for the shader, never driven by audio.
    pub zoom: f32,
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
        warp_add: 0.0,
        chroma_add: 0.0,
        expo: 1.0,
        hue: 0.0,
        spark: 0.0,
        vig: 0.85,
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
        }
    }
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
        }
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
        let [w_br, w_pu, w_gl, w_sh, w_hu, w_je, w_mo, w_mt] = *f.weights;
        let (low, mid, high, kk) = (l.low * g, l.mid * g, l.high * g, self.kick * g);
        let e = f.energy.clamp(0.0, 1.0);

        // Beat-stepped re-forming. With music, steps land on beats the reaction asks for;
        // without music, Energy sets an ambient step every couple of seconds.
        self.morph_age += dt;
        let music_morph = g * w_mo;
        if f.beat && music_morph > 0.25 {
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

        // Local wobble and motes: always some with Energy, more with the music.
        let jelly = calm * (e * 0.12 + w_je * (kk * 0.22 + mid * 0.08) + w_pu * kk * 0.08);
        self.jelly_t += (dt * (0.25 + e * 0.5 + g * (mid * 0.5 + w_je * kk * 0.8)) * calm) as f64;
        let motes = ((e - 0.3).max(0.0) * 0.55 + w_mt * g * 0.25).min(0.6);
        self.mote_t += (dt * (0.5 + e * 0.7 + w_mt * high * 1.0) * calm) as f64;
        let mote_size = 2.5 + e * 1.5 + w_mt * high * 2.0;

        // Brightness: small target, slew-limited so it can never flash.
        let expo_target =
            (1.0 + w_pu * kk * 0.07 + w_gl * self.glow * g * 0.08 + w_br * low * 0.04)
                .min(MAX_EXPO);
        let max_step = EXPO_SLEW * dt;
        self.expo += (expo_target - self.expo).clamp(-max_step, max_step);

        Visual {
            zoom: 1.0,
            warp_add: calm * w_br * mid * 0.1,
            chroma_add: w_sh * high * 0.3,
            expo: self.expo,
            hue: g * self.hue + w_hu * kk * 0.03 + w_sh * high * 0.015,
            spark: w_sh * high * 0.06,
            vig: 0.85 - (w_br + 0.2) * low * 0.25,
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
    const ALL: ReactionWeights = [1.0; 8];

    fn loud() -> Levels {
        Levels {
            low: 1.0,
            mid: 1.0,
            high: 1.0,
            level_fast: 0.2,
            level_slow: 0.2,
            bpm: 120.0,
            present: true,
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
            reduced_motion: false,
        }
    }

    /// Runs `secs` of loud music with beats every `period` seconds; returns all visuals.
    fn run_music(p: &mut Pulse, secs: f32, period: f32, energy: f32) -> Vec<Visual> {
        let l = loud();
        let frames = (secs / DT) as usize;
        let per = (period / DT).round() as usize;
        (0..frames)
            .map(|i| p.frame(input(&l, true, i % per == 0, energy)))
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
    fn no_camera_motion_ever() {
        let mut p = Pulse::default();
        for v in run_music(&mut p, 10.0, 0.33, 1.0) {
            assert_eq!(v.zoom, 1.0);
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
