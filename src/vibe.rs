//! The vibe: what the music feels like right now, beyond its tempo.
//!
//! From loudness, how busy the hits are, how much of the mix is bass and how bright it
//! is, we derive an **intensity** and a **heaviness**, then a **mood**. The mood only
//! changes after the new one has held for a few seconds, so a fill or a single loud hit
//! doesn't flip the picture. Moods steer which looks get picked; rhythm character
//! (polyrhythm, triplet feel) adds its own preferences on top.

use crate::analysis::{Levels, Rhythm};
use crate::dsp::smooth_k;
use crate::traits::Key;

/// How long a new mood must hold before it takes over, seconds.
pub const MOOD_HOLD: f32 = 3.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Mood {
    /// Quiet, sparse: soft fields, slow drift.
    Ambient,
    /// Mid-energy, swinging: playful motion.
    Groove,
    /// Bass-weighted and loud: dark, deep, weighty.
    Heavy,
    /// Loud and steady: forward motion.
    Driving,
    /// Very busy: sharp, fast, bright detail.
    Frantic,
    /// Bright with little bass: light, glassy.
    Airy,
}

impl Mood {
    pub fn label(self) -> &'static str {
        match self {
            Mood::Ambient => "Ambient",
            Mood::Groove => "Groove",
            Mood::Heavy => "Heavy",
            Mood::Driving => "Driving",
            Mood::Frantic => "Frantic",
            Mood::Airy => "Airy",
        }
    }
}

/// Highest [`Vibe::level`]: a peak may run a little above the recent loud average.
pub const MAX_LEVEL: f32 = 1.25;
/// How fast [`Vibe::level`] follows the music, per second (about 0.7 s to settle).
const LEVEL_RATE: f32 = 1.5;

/// Instantaneous features, smoothed by [`Vibe`].
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Features {
    /// Loudness relative to the loudest the track has been lately, 0..1.
    pub loudness: f32,
    /// Hits per second, scaled so ~14/s is 1 (a house loop with 16th hats is ~0.6).
    pub density: f32,
    pub bass_share: f32,
    pub bright_share: f32,
    pub bass_sustain: f32,
}

#[derive(Clone, Debug)]
pub struct Vibe {
    loud_peak: f32,
    /// How loud it is right now against the loudest the track has been lately: about 0..1,
    /// up to [`MAX_LEVEL`] on peaks, smoothed over about 0.7 s. 0 in silence.
    pub level: f32,
    pub intensity: f32,
    pub heaviness: f32,
    pub density: f32,
    pub brightness: f32,
    pub poly: f32,
    pub triplet: f32,
    pub syncopation: f32,
    pub mood: Mood,
    candidate: Mood,
    held: f32,
}

impl Default for Vibe {
    fn default() -> Self {
        Vibe {
            loud_peak: 0.02,
            level: 0.0,
            intensity: 0.0,
            heaviness: 0.0,
            density: 0.0,
            brightness: 0.0,
            poly: 0.0,
            triplet: 0.0,
            syncopation: 0.0,
            mood: Mood::Ambient,
            candidate: Mood::Ambient,
            held: 0.0,
        }
    }
}

/// Picks a mood from smoothed features.
pub fn classify(intensity: f32, heaviness: f32, density: f32, brightness: f32) -> Mood {
    if intensity < 0.3 {
        Mood::Ambient
    } else if heaviness > 0.55 && intensity >= 0.45 {
        Mood::Heavy
    } else if density > 0.7 && intensity > 0.55 {
        Mood::Frantic
    } else if brightness > 0.35 && heaviness < 0.3 {
        Mood::Airy
    } else if intensity > 0.62 {
        Mood::Driving
    } else {
        Mood::Groove
    }
}

impl Vibe {
    /// Extracts features from the analyzer's outputs.
    pub fn features(&mut self, dt: f32, l: &Levels, r: &Rhythm, live: bool) -> Features {
        let k = smooth_k(LEVEL_RATE, dt);
        if !live || !l.present {
            self.level -= self.level * k;
            return Features::default();
        }
        self.loud_peak = l
            .level_slow
            .max(self.loud_peak * (-dt / 60.0).exp())
            .max(0.02);
        let level = (l.level_fast / self.loud_peak).clamp(0.0, MAX_LEVEL);
        self.level += (level - self.level) * k;
        Features {
            loudness: (l.level_slow / self.loud_peak).clamp(0.0, 1.0),
            density: (r.density / 14.0).clamp(0.0, 1.0),
            bass_share: l.share[0],
            bright_share: l.share[2],
            bass_sustain: l.bass_sustain,
        }
    }

    /// Advances the vibe by `dt` seconds. Returns true on the frame the mood changes.
    pub fn update(&mut self, dt: f32, f: &Features, r: &Rhythm) -> bool {
        let k = smooth_k(0.4, dt);
        let intensity = (0.55 * f.loudness + 0.45 * f.density).clamp(0.0, 1.0);
        let heaviness = ((f.bass_share * 1.3) * (0.4 + 0.6 * f.loudness) + 0.3 * f.bass_sustain
            - 0.2)
            .clamp(0.0, 1.0);
        self.intensity += (intensity - self.intensity) * k;
        self.heaviness += (heaviness - self.heaviness) * k;
        self.density += (f.density - self.density) * k;
        self.brightness += (f.bright_share - self.brightness) * k;
        self.poly += (r.poly - self.poly) * k;
        self.triplet += (r.triplet - self.triplet) * k;
        self.syncopation += (r.syncopation - self.syncopation) * k;

        let now = classify(
            self.intensity,
            self.heaviness,
            self.density,
            self.brightness,
        );
        if now == self.mood {
            self.held = 0.0;
            self.candidate = now;
            return false;
        }
        if now != self.candidate {
            self.candidate = now;
            self.held = 0.0;
        }
        self.held += dt;
        if self.held >= MOOD_HOLD {
            self.mood = now;
            self.held = 0.0;
            return true;
        }
        false
    }

    /// Short description for the HUD, e.g. "Heavy · 72% · poly".
    pub fn describe(&self) -> String {
        let mut s = format!("{} · {:.0}%", self.mood.label(), self.intensity * 100.0);
        if self.poly > 0.45 {
            s.push_str(" · poly");
        } else if self.triplet > 0.4 {
            s.push_str(" · triplet");
        }
        if self.syncopation > 0.5 {
            s.push_str(" · syncopated");
        }
        s
    }
}

/// How much more (or less) likely an option is under the current vibe. 1.0 = neutral.
pub fn bias(mood: Mood, poly: f32, triplet: f32, key: Key, name: &str) -> f32 {
    let has = |names: &[&str]| names.contains(&name);
    let mut w = 1.0;
    match (mood, key) {
        (Mood::Heavy, Key::Palette)
            if has(&["Ember", "Abyss", "Nocturne", "Copper", "Ultraviolet"]) =>
        {
            w *= 2.5
        }
        (Mood::Heavy, Key::Field) if has(&["Ridges", "Cells", "Honeycomb", "Smoke", "Marble"]) => {
            w *= 2.0
        }
        (Mood::Heavy, Key::Effect) if has(&["Bloom", "Thermal", "Chrome", "Contour"]) => w *= 2.0,
        (Mood::Heavy, Key::Energy) if has(&["Restless", "Electric"]) => w *= 2.0,
        (Mood::Heavy, Key::Energy) if has(&["Calm"]) => w *= 0.3,
        (Mood::Heavy, Key::Tempo) if has(&["Still", "Drift"]) => w *= 1.6,
        (Mood::Heavy, Key::Warp) if has(&["Fold", "Melt"]) => w *= 2.0,
        (Mood::Heavy, Key::Reaction) if has(&["Punch", "Pump", "Jelly"]) => w *= 2.2,

        (Mood::Ambient, Key::Palette) if has(&["Glacier", "Moss", "Abyss", "Dune", "Rose"]) => {
            w *= 2.0
        }
        (Mood::Ambient, Key::Field) if has(&["Smoke", "Marble", "Petals", "Plasma"]) => w *= 2.0,
        (Mood::Ambient, Key::Energy) if has(&["Calm"]) => w *= 3.0,
        (Mood::Ambient, Key::Energy) if has(&["Electric"]) => w *= 0.2,
        (Mood::Ambient, Key::Tempo) if has(&["Still", "Drift"]) => w *= 2.0,
        (Mood::Ambient, Key::Texture) if has(&["Paper", "Grain", "Clean"]) => w *= 1.5,
        (Mood::Ambient, Key::Reaction) if has(&["Breathe", "Glow"]) => w *= 2.5,
        (Mood::Ambient, Key::Effect) if has(&["None", "Duotone"]) => w *= 2.0,
        (Mood::Ambient, Key::Mosaic) if has(&["Off"]) => w *= 2.0,

        (Mood::Groove, Key::Reaction) if has(&["Bop", "Sway", "Re-form"]) => w *= 2.0,
        (Mood::Groove, Key::Energy) if has(&["Lively"]) => w *= 2.0,
        (Mood::Groove, Key::Field) if has(&["Plasma", "Moiré", "Cells"]) => w *= 1.5,
        (Mood::Groove, Key::Palette) if has(&["Coral", "Peach", "Sunset", "Citrus"]) => w *= 1.6,

        (Mood::Driving, Key::Energy) if has(&["Restless"]) => w *= 2.0,
        (Mood::Driving, Key::Tempo) if has(&["Flow", "Surge"]) => w *= 2.0,
        (Mood::Driving, Key::Reaction) if has(&["Pump", "Punch", "Re-form"]) => w *= 2.0,
        (Mood::Driving, Key::Field) if has(&["Rings", "Interference", "Quasicrystal"]) => w *= 1.5,
        (Mood::Driving, Key::Effect) if has(&["Neon", "Contour"]) => w *= 1.5,

        (Mood::Frantic, Key::Energy) if has(&["Electric"]) => w *= 3.0,
        (Mood::Frantic, Key::Reaction) if has(&["Motes", "Shimmer", "Re-form"]) => w *= 2.0,
        (Mood::Frantic, Key::Field) if has(&["Quasicrystal", "Interference", "Truchet"]) => {
            w *= 2.0
        }
        (Mood::Frantic, Key::Texture) if has(&["Dither", "LCD", "Stipple"]) => w *= 1.5,
        (Mood::Frantic, Key::Palette) if has(&["Acid", "Vapor", "Prism", "Lime"]) => w *= 2.0,

        (Mood::Airy, Key::Palette) if has(&["Glacier", "Prism", "Rose", "Vapor"]) => w *= 2.0,
        (Mood::Airy, Key::Reaction) if has(&["Shimmer", "Motes"]) => w *= 2.0,
        (Mood::Airy, Key::Texture) if has(&["Clean"]) => w *= 1.5,
        (Mood::Airy, Key::Effect) if has(&["Iridescent"]) => w *= 2.0,
        _ => {}
    }
    // Rhythm character, on top of the mood.
    if poly > 0.45 {
        match key {
            Key::Symmetry if has(&["3-fold", "5-fold", "7-fold"]) => w *= 1.0 + 2.0 * poly,
            Key::Field if has(&["Quasicrystal", "Interference", "Truchet"]) => {
                w *= 1.0 + 1.5 * poly
            }
            Key::Reaction if has(&["Motes", "Re-form", "Bop"]) => w *= 1.0 + poly,
            _ => {}
        }
    } else if triplet > 0.4 {
        if let Key::Symmetry = key {
            if has(&["3-fold", "6-fold", "12-fold"]) {
                w *= 1.0 + 2.0 * triplet;
            }
        }
    }
    w
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(v: &mut Vibe, secs: f32, f: Features, r: Rhythm) -> bool {
        let mut changed = false;
        for _ in 0..(secs * 60.0) as usize {
            changed |= v.update(1.0 / 60.0, &f, &r);
        }
        changed
    }

    fn heavy() -> Features {
        Features {
            loudness: 0.95,
            density: 0.4,
            bass_share: 0.8,
            bright_share: 0.05,
            bass_sustain: 0.8,
        }
    }

    #[test]
    fn classification_covers_the_moods() {
        assert_eq!(classify(0.1, 0.0, 0.0, 0.0), Mood::Ambient);
        assert_eq!(classify(0.8, 0.7, 0.4, 0.1), Mood::Heavy);
        assert_eq!(classify(0.8, 0.3, 0.9, 0.2), Mood::Frantic);
        assert_eq!(classify(0.5, 0.1, 0.3, 0.5), Mood::Airy);
        assert_eq!(classify(0.7, 0.4, 0.4, 0.2), Mood::Driving);
        assert_eq!(classify(0.5, 0.4, 0.4, 0.2), Mood::Groove);
    }

    #[test]
    fn bass_heavy_loud_music_turns_heavy() {
        let mut v = Vibe::default();
        assert!(feed(&mut v, 12.0, heavy(), Rhythm::default()));
        assert_eq!(v.mood, Mood::Heavy);
        assert!(v.heaviness > 0.55 && v.intensity > 0.5, "{v:?}");
    }

    #[test]
    fn busy_music_is_more_intense_than_sparse_music() {
        let mut a = Vibe::default();
        let mut b = Vibe::default();
        let mut sparse = heavy();
        sparse.density = 0.1;
        let mut busy = heavy();
        busy.density = 1.0;
        feed(&mut a, 10.0, sparse, Rhythm::default());
        feed(&mut b, 10.0, busy, Rhythm::default());
        assert!(b.intensity > a.intensity + 0.3);
    }

    #[test]
    fn a_brief_change_does_not_flip_the_mood() {
        let mut v = Vibe::default();
        feed(&mut v, 12.0, heavy(), Rhythm::default());
        // One second of near-silence (a fill or a pause) is not enough.
        let changed = feed(&mut v, 1.0, Features::default(), Rhythm::default());
        assert!(!changed);
        assert_eq!(v.mood, Mood::Heavy);
    }

    #[test]
    fn mood_steers_the_picks() {
        assert!(bias(Mood::Heavy, 0.0, 0.0, Key::Palette, "Abyss") > 2.0);
        assert!(bias(Mood::Heavy, 0.0, 0.0, Key::Energy, "Calm") < 0.5);
        assert!(bias(Mood::Ambient, 0.0, 0.0, Key::Energy, "Calm") > 2.0);
        assert_eq!(bias(Mood::Groove, 0.0, 0.0, Key::Mosaic, "Coarse"), 1.0);
    }

    #[test]
    fn polyrhythm_prefers_odd_symmetry() {
        let odd = bias(Mood::Groove, 0.8, 0.0, Key::Symmetry, "5-fold");
        let even = bias(Mood::Groove, 0.8, 0.0, Key::Symmetry, "4-fold");
        assert!(odd > even * 2.0);
        let trip = bias(Mood::Groove, 0.0, 0.7, Key::Symmetry, "6-fold");
        assert!(trip > 2.0);
    }

    #[test]
    fn silence_reads_as_ambient() {
        let mut v = Vibe::default();
        let f = v.features(1.0 / 60.0, &Levels::default(), &Rhythm::default(), false);
        assert_eq!(f, Features::default());
        feed(&mut v, 6.0, f, Rhythm::default());
        assert_eq!(v.mood, Mood::Ambient);
    }

    #[test]
    fn level_follows_loudness_against_the_recent_peak() {
        let mut v = Vibe::default();
        let r = Rhythm::default();
        let at = |rms: f32| Levels {
            level_fast: rms,
            level_slow: rms,
            present: true,
            ..Default::default()
        };
        let run = |v: &mut Vibe, l: &Levels, secs: f32| {
            for _ in 0..(secs * 60.0) as usize {
                v.features(1.0 / 60.0, l, &r, true);
            }
        };
        run(&mut v, &at(0.2), 6.0);
        assert!(v.level > 0.95, "loud section: {}", v.level);
        // A quiet verse at a quarter of the volume reads well below the chorus.
        run(&mut v, &at(0.05), 4.0);
        assert!((0.2..0.35).contains(&v.level), "quiet verse: {}", v.level);
        // It eases rather than snaps: half a second into the return it is still rising.
        run(&mut v, &at(0.2), 0.5);
        assert!(v.level < 0.9, "should ease back up: {}", v.level);
        run(&mut v, &at(0.2), 4.0);
        assert!(v.level > 0.95);
        // Silence fades it out.
        v.features(1.0 / 60.0, &Levels::default(), &r, true);
        run(&mut v, &Levels::default(), 5.0);
        assert!(v.level < 0.01, "silence: {}", v.level);
        // Peaks above the recent level are allowed, but bounded.
        let mut p = Vibe::default();
        run(&mut p, &at(0.2), 6.0);
        let spike = Levels {
            level_fast: 2.0,
            ..at(0.2)
        };
        run(&mut p, &spike, 3.0);
        assert!(p.level <= MAX_LEVEL + 1e-4);
    }
}
