//! Turns audio (levels + beats) into visual parameters. Reactions change light and color
//! only; nothing moves the "camera", which is what caused motion sickness.

use crate::analysis::Levels;
use crate::dsp::smooth_k;
use crate::traits::ReactionWeights;

/// Envelopes that persist between frames.
#[derive(Clone, Debug, Default)]
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
    ring_age: f32,
}

/// Per-frame visual modifiers handed to the renderer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Visual {
    pub zoom: f32,
    pub warp_add: f32,
    pub chroma_add: f32,
    pub expo: f32,
    pub hue: f32,
    pub spark: f32,
    pub vig: f32,
    pub ring_r: f32,
    pub ring_a: f32,
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
        ring_r: 0.0,
        ring_a: 0.0,
    };
}

pub const MAX_EXPO: f32 = 1.3;

impl Pulse {
    /// Advance envelopes by `dt` seconds. `beat` is true on frames where a beat lands.
    pub fn update(&mut self, dt: f32, levels: &Levels, live: bool, beat: bool) {
        if beat {
            self.kick_raw = 1.0;
            self.glow_raw = 1.0;
            self.hue_acc += 0.012;
            self.ring_age = 0.0;
        }
        self.kick_raw *= (-dt * 5.0).exp();
        self.kick += (self.kick_raw - self.kick) * smooth_k(22.0, dt);
        self.glow_raw *= (-dt * 1.8).exp();
        self.glow += (self.glow_raw - self.glow) * smooth_k(6.0, dt);
        self.hue += (self.hue_acc - self.hue) * smooth_k(5.0, dt);
        self.ring_age += dt;
        let level = if live { levels.level_fast } else { 0.0 };
        self.flow_boost += (level - self.flow_boost) * smooth_k(0.6, dt);
        let present = live && levels.present;
        let target = if present { 1.0 } else { 0.0 };
        let rate = if present { 3.0 } else { 0.5 };
        self.gate += (target - self.gate) * smooth_k(rate, dt);
    }

    /// Audio is considered "driving" once the gate is past halfway.
    pub fn active(&self) -> bool {
        self.gate > 0.5
    }

    /// How much to add to the flow speed (scaled by the gate).
    pub fn flow_speed_add(&self, reduced_motion: bool) -> f32 {
        let m = if reduced_motion { 0.3 } else { 1.0 };
        self.gate * m * self.flow_boost * 0.8
    }

    pub fn visual(&self, w: &ReactionWeights, levels: &Levels, reduced_motion: bool) -> Visual {
        let g = self.gate * if reduced_motion { 0.3 } else { 1.0 };
        let [w_br, w_pu, w_gl, w_sh, w_hu, w_fl, w_rp] = *w;
        let (low, mid, high, kk) = (levels.low, levels.mid, levels.high, self.kick);
        Visual {
            zoom: 1.0 - g * w_br * low * 0.03,
            warp_add: g * w_br * mid * 0.1,
            chroma_add: g * w_sh * high * 0.3,
            expo: (1.0
                + g * (w_pu * kk * 0.16
                    + w_fl * kk * 0.22
                    + w_gl * self.glow * 0.22
                    + w_br * low * 0.08))
                .min(MAX_EXPO),
            hue: g * (self.hue + w_hu * kk * 0.035 + w_sh * high * 0.015),
            spark: g * w_sh * high * 0.07,
            vig: 0.85 - g * (w_br + 0.2) * low * 0.3,
            ring_r: self.ring_age * 0.55,
            ring_a: g * w_rp * (-self.ring_age * 3.0).exp() * 0.45,
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

    const ALL: ReactionWeights = [1.0; 7];

    #[test]
    fn silence_is_neutral() {
        let mut p = Pulse::default();
        for _ in 0..600 {
            p.update(1.0 / 60.0, &Levels::default(), false, false);
        }
        let v = p.visual(&ALL, &Levels::default(), false);
        assert_eq!(v.zoom, 1.0);
        assert_eq!(v.expo, 1.0);
        assert_eq!(v.ring_a, 0.0);
        assert!(!p.active());
    }

    #[test]
    fn gate_opens_fast_and_closes_slowly() {
        let mut p = Pulse::default();
        for _ in 0..60 {
            p.update(1.0 / 60.0, &loud(), true, false);
        }
        assert!(p.active(), "gate after 1 s: {}", p.gate);
        for _ in 0..60 {
            p.update(1.0 / 60.0, &Levels::default(), true, false);
        }
        assert!(p.gate > 0.3, "should fade, not snap: {}", p.gate);
    }

    #[test]
    fn beats_brighten_within_safe_limits_and_decay() {
        let mut p = Pulse::default();
        for _ in 0..120 {
            p.update(1.0 / 60.0, &loud(), true, false);
        }
        p.update(1.0 / 60.0, &loud(), true, true);
        for _ in 0..3 {
            p.update(1.0 / 60.0, &loud(), true, false);
        }
        let hit = p.visual(&ALL, &loud(), false);
        assert!(hit.expo > 1.1 && hit.expo <= MAX_EXPO, "{}", hit.expo);
        for _ in 0..120 {
            p.update(1.0 / 60.0, &loud(), true, false);
        }
        let later = p.visual(&ALL, &loud(), false);
        assert!(later.expo < hit.expo);
    }

    #[test]
    fn camera_barely_moves() {
        let mut p = Pulse::default();
        for _ in 0..120 {
            p.update(1.0 / 60.0, &loud(), true, true);
        }
        let v = p.visual(&ALL, &loud(), false);
        assert!(v.zoom >= 0.96, "zoom swing too large: {}", v.zoom);
    }

    #[test]
    fn reduced_motion_damps_reactions() {
        let mut p = Pulse::default();
        for _ in 0..120 {
            p.update(1.0 / 60.0, &loud(), true, false);
        }
        p.update(1.0 / 60.0, &loud(), true, true);
        let full = p.visual(&ALL, &loud(), false);
        let calm = p.visual(&ALL, &loud(), true);
        assert!(calm.expo - 1.0 < (full.expo - 1.0) * 0.5);
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
