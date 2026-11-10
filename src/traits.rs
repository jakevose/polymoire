//! The trait engine: each visual trait holds one option at a time and crossfades to a new
//! one when the scheduler picks it. Only one or two traits change at once.

use crate::rng::Rng;

pub type Palette = [[f32; 3]; 4];
/// Reaction weights: [breathe, pump, glow, shimmer, hue pulse, jelly, morph, motes].
/// Reactions change light gently and move the picture locally; they never move the
/// whole view (no zoom, pan or spin) and never flash.
pub type ReactionWeights = [f32; 8];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Key {
    Palette,
    Field,
    Symmetry,
    Texture,
    Effect,
    Energy,
    Reaction,
    Warp,
    Scale,
    Tempo,
    Spin,
    Chroma,
    Mosaic,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Value {
    /// Discrete choice; the shader receives the option index.
    Index,
    Scalar(f32),
    Palette(Palette),
    Reaction(ReactionWeights),
}

#[derive(Clone, Debug)]
pub struct Opt {
    pub name: &'static str,
    pub weight: f32,
    pub value: Value,
}

#[derive(Clone, Debug)]
pub struct TraitDef {
    pub key: Key,
    pub label: &'static str,
    pub opts: Vec<Opt>,
}

#[derive(Clone, Debug)]
pub struct TraitState {
    pub from: usize,
    pub to: usize,
    pub start: f64,
    pub dur: f64,
    pub locked: bool,
    pub last: f64,
}

impl TraitState {
    pub fn moving(&self) -> bool {
        self.from != self.to
    }
}

fn o(name: &'static str, weight: f32, value: Value) -> Opt {
    Opt {
        name,
        weight,
        value,
    }
}
fn idx(name: &'static str) -> Opt {
    o(name, 1.0, Value::Index)
}
fn sc(name: &'static str, v: f32) -> Opt {
    o(name, 1.0, Value::Scalar(v))
}
fn scw(name: &'static str, v: f32, w: f32) -> Opt {
    o(name, w, Value::Scalar(v))
}
fn pal(name: &'static str, a: [f32; 3], b: [f32; 3], c: [f32; 3], d: [f32; 3]) -> Opt {
    o(name, 1.0, Value::Palette([a, b, c, d]))
}
fn re(name: &'static str, w: ReactionWeights) -> Opt {
    o(name, 1.0, Value::Reaction(w))
}

pub fn catalog() -> Vec<TraitDef> {
    let h = [0.5, 0.5, 0.5];
    let one = [1.0, 1.0, 1.0];
    vec![
        TraitDef {
            key: Key::Palette,
            label: "Palette",
            opts: vec![
                pal("Prism", h, h, one, [0.0, 0.33, 0.67]),
                pal("Ember", h, h, one, [0.0, 0.1, 0.2]),
                pal("Coral", h, h, one, [0.3, 0.2, 0.2]),
                pal("Lime", h, h, [1.0, 1.0, 0.5], [0.8, 0.9, 0.3]),
                pal("Dune", h, h, [1.0, 0.7, 0.4], [0.0, 0.15, 0.2]),
                pal("Acid", h, h, [2.0, 1.0, 0.0], [0.5, 0.2, 0.25]),
                pal(
                    "Peach",
                    [0.8, 0.5, 0.4],
                    [0.2, 0.4, 0.2],
                    [2.0, 1.0, 1.0],
                    [0.0, 0.25, 0.25],
                ),
                pal(
                    "Abyss",
                    [0.2, 0.3, 0.45],
                    [0.25, 0.3, 0.35],
                    one,
                    [0.6, 0.55, 0.5],
                ),
                pal(
                    "Nocturne",
                    [0.3, 0.18, 0.38],
                    [0.4, 0.25, 0.4],
                    one,
                    [0.85, 0.1, 0.3],
                ),
                pal(
                    "Glacier",
                    [0.6, 0.7, 0.8],
                    [0.3, 0.25, 0.2],
                    one,
                    [0.5, 0.55, 0.6],
                ),
                pal(
                    "Moss",
                    [0.4, 0.5, 0.3],
                    [0.25, 0.3, 0.2],
                    one,
                    [0.1, 0.15, 0.35],
                ),
                pal(
                    "Rose",
                    [0.75, 0.55, 0.6],
                    [0.25, 0.25, 0.25],
                    one,
                    [0.0, 0.1, 0.05],
                ),
                pal(
                    "Sunset",
                    [0.6, 0.4, 0.4],
                    [0.4, 0.3, 0.3],
                    [1.0, 0.8, 0.6],
                    [0.0, 0.15, 0.35],
                ),
                pal("Mono", h, h, one, [0.0, 0.0, 0.0]),
                pal(
                    "Vapor",
                    [0.6, 0.5, 0.75],
                    [0.4, 0.35, 0.25],
                    one,
                    [0.9, 0.5, 0.3],
                ),
                pal(
                    "Copper",
                    [0.5, 0.35, 0.25],
                    [0.45, 0.3, 0.2],
                    one,
                    [0.0, 0.05, 0.1],
                ),
                pal(
                    "Ultraviolet",
                    [0.35, 0.2, 0.55],
                    [0.35, 0.25, 0.45],
                    one,
                    [0.6, 0.75, 0.95],
                ),
                pal(
                    "Citrus",
                    [0.7, 0.65, 0.3],
                    [0.3, 0.3, 0.3],
                    one,
                    [0.0, 0.08, 0.45],
                ),
            ],
        },
        TraitDef {
            key: Key::Field,
            label: "Field",
            opts: [
                "Smoke",
                "Rings",
                "Cells",
                "Moiré",
                "Plasma",
                "Spiral",
                "Quasicrystal",
                "Marble",
                "Ridges",
                "Truchet",
                "Honeycomb",
                "Petals",
                "Currents",
                "Interference",
            ]
            .into_iter()
            .map(idx)
            .collect(),
        },
        TraitDef {
            key: Key::Symmetry,
            label: "Symmetry",
            opts: vec![
                scw("Free", 1.0, 2.0),
                sc("Mirror", -1.0),
                sc("Quad mirror", -2.0),
                sc("3-fold", 3.0),
                sc("4-fold", 4.0),
                sc("5-fold", 5.0),
                sc("6-fold", 6.0),
                sc("7-fold", 7.0),
                sc("8-fold", 8.0),
                sc("10-fold", 10.0),
                sc("12-fold", 12.0),
                scw("16-fold", 16.0, 0.5),
            ],
        },
        TraitDef {
            key: Key::Texture,
            label: "Texture",
            opts: vec![
                o("Clean", 1.5, Value::Index),
                idx("Grain"),
                idx("Halftone"),
                idx("Scanline"),
                idx("Hatch"),
                idx("Dither"),
                idx("Paper"),
                idx("Stipple"),
                idx("Weave"),
                idx("LCD"),
                idx("Riso"),
                idx("Engrave"),
            ],
        },
        TraitDef {
            key: Key::Effect,
            label: "Effect",
            opts: vec![
                o("None", 1.5, Value::Index),
                idx("Contour"),
                idx("Solarize"),
                idx("Bloom"),
                idx("Duotone"),
                idx("Invert bands"),
                idx("Posterize"),
                idx("Neon"),
                idx("Thermal"),
                idx("Chrome"),
                idx("Iridescent"),
                idx("Ink"),
            ],
        },
        TraitDef {
            key: Key::Energy,
            label: "Energy",
            opts: vec![
                sc("Calm", 0.2),
                sc("Lively", 0.55),
                scw("Restless", 0.8, 0.8),
                scw("Electric", 1.0, 0.4),
            ],
        },
        TraitDef {
            key: Key::Reaction,
            label: "Reaction",
            opts: vec![
                re("Breathe", [1.0, 0.0, 0.4, 0.0, 0.0, 0.3, 0.0, 0.0]),
                re("Pump", [0.3, 1.0, 0.0, 0.0, 0.2, 0.6, 0.0, 0.0]),
                re("Glow", [0.3, 0.0, 1.0, 0.0, 0.2, 0.2, 0.3, 0.0]),
                re("Shimmer", [0.2, 0.0, 0.2, 1.0, 0.0, 0.0, 0.0, 1.0]),
                re("Hue pulse", [0.2, 0.2, 0.0, 0.0, 1.0, 0.3, 0.4, 0.0]),
                re("Jelly", [0.2, 0.3, 0.0, 0.0, 0.0, 1.0, 0.3, 0.0]),
                re("Re-form", [0.2, 0.2, 0.2, 0.0, 0.3, 0.2, 1.0, 0.0]),
                re("Motes", [0.2, 0.0, 0.3, 0.4, 0.0, 0.2, 0.0, 1.0]),
                re("Bop", [0.2, 0.6, 0.0, 0.0, 0.2, 0.8, 0.8, 0.3]),
            ],
        },
        TraitDef {
            key: Key::Warp,
            label: "Warp",
            opts: vec![
                sc("Calm", 0.0),
                sc("Ripple", 0.35),
                sc("Wave", 0.55),
                sc("Fold", 0.8),
                scw("Melt", 1.05, 0.5),
            ],
        },
        TraitDef {
            key: Key::Scale,
            label: "Scale",
            opts: vec![
                scw("Vast", 1.1, 0.6),
                sc("Wide", 1.6),
                sc("Mid", 3.0),
                sc("Close", 4.8),
                scw("Dense", 6.0, 0.6),
                scw("Micro", 9.0, 0.3),
            ],
        },
        TraitDef {
            key: Key::Tempo,
            label: "Tempo",
            opts: vec![
                sc("Still", 0.08),
                sc("Drift", 0.16),
                sc("Flow", 0.3),
                scw("Surge", 0.5, 0.6),
            ],
        },
        TraitDef {
            key: Key::Spin,
            label: "Spin",
            opts: vec![
                scw("Still", 0.0, 2.5),
                sc("Slow", 0.025),
                sc("Counter", -0.025),
            ],
        },
        TraitDef {
            key: Key::Chroma,
            label: "Chroma",
            opts: vec![
                scw("Clean", 0.0, 2.0),
                sc("Prism", 0.25),
                sc("Split", 0.5),
                scw("Smear", 0.8, 0.6),
            ],
        },
        TraitDef {
            key: Key::Mosaic,
            label: "Mosaic",
            opts: vec![
                scw("Off", 1.0, 4.0),
                scw("Tiny", 2.5, 0.6),
                sc("Fine", 4.0),
                scw("Coarse", 8.0, 0.6),
            ],
        },
    ]
}

/// Weighted pick, never returning `exclude`.
pub fn pick_weighted(opts: &[Opt], exclude: Option<usize>, rng: &mut Rng) -> usize {
    let weight = |i: usize| {
        if Some(i) == exclude {
            0.0
        } else {
            opts[i].weight.max(0.0)
        }
    };
    let total: f32 = (0..opts.len()).map(weight).sum();
    if total <= 0.0 {
        return exclude.map(|e| (e + 1) % opts.len()).unwrap_or(0);
    }
    let mut r = rng.f32() * total;
    for i in 0..opts.len() {
        let w = weight(i);
        if w > 0.0 {
            r -= w;
            if r <= 0.0 {
                return i;
            }
        }
    }
    (0..opts.len())
        .rev()
        .find(|&i| weight(i) > 0.0)
        .unwrap_or(0)
}

/// Cubic ease in-out.
pub fn ease(t: f32) -> f32 {
    if t < 0.5 {
        4.0 * t * t * t
    } else {
        1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
    }
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

pub struct Traits {
    pub defs: Vec<TraitDef>,
    pub state: Vec<TraitState>,
    rng: Rng,
}

impl Traits {
    pub fn new(mut rng: Rng) -> Self {
        let defs = catalog();
        let state = defs
            .iter()
            .map(|d| {
                let i = pick_weighted(&d.opts, None, &mut rng);
                TraitState {
                    from: i,
                    to: i,
                    start: 0.0,
                    dur: 1.0,
                    locked: false,
                    last: -99.0,
                }
            })
            .collect();
        Traits { defs, state, rng }
    }

    pub fn index_of(&self, key: Key) -> usize {
        self.defs
            .iter()
            .position(|d| d.key == key)
            .expect("trait exists")
    }

    pub fn progress(&self, i: usize, sim: f64) -> f32 {
        let s = &self.state[i];
        if !s.moving() {
            return 1.0;
        }
        (((sim - s.start) / s.dur) as f32).clamp(0.0, 1.0)
    }

    /// Eased crossfade amount from `from` (0) to `to` (1).
    pub fn mix(&self, i: usize, sim: f64) -> f32 {
        if self.state[i].moving() {
            ease(self.progress(i, sim))
        } else {
            0.0
        }
    }

    /// Finishes crossfades that have run their course.
    pub fn settle(&mut self, sim: f64) {
        for i in 0..self.state.len() {
            if self.state[i].moving() && self.progress(i, sim) >= 1.0 {
                self.state[i].from = self.state[i].to;
            }
        }
    }

    pub fn scalar(&self, key: Key, sim: f64) -> f32 {
        let i = self.index_of(key);
        let (s, d) = (&self.state[i], &self.defs[i]);
        let get = |k: usize| match d.opts[k].value {
            Value::Scalar(v) => v,
            _ => 0.0,
        };
        lerp(get(s.from), get(s.to), self.mix(i, sim))
    }

    /// (from index, to index, mix) for discrete traits.
    pub fn discrete(&self, key: Key, sim: f64) -> (usize, usize, f32) {
        let i = self.index_of(key);
        let s = &self.state[i];
        (s.from, s.to, self.mix(i, sim))
    }

    /// (from value, to value, mix) for scalar traits that must not be interpolated directly,
    /// such as symmetry, which the shader blends as two kaleidoscopes.
    pub fn scalar_pair(&self, key: Key, sim: f64) -> (f32, f32, f32) {
        let i = self.index_of(key);
        let (s, d) = (&self.state[i], &self.defs[i]);
        let get = |k: usize| match d.opts[k].value {
            Value::Scalar(v) => v,
            _ => 0.0,
        };
        (get(s.from), get(s.to), self.mix(i, sim))
    }

    pub fn palette(&self, sim: f64) -> Palette {
        let i = self.index_of(Key::Palette);
        let (s, d) = (&self.state[i], &self.defs[i]);
        let get = |k: usize| match d.opts[k].value {
            Value::Palette(p) => p,
            _ => [[0.5; 3]; 4],
        };
        let (a, b, m) = (get(s.from), get(s.to), self.mix(i, sim));
        let mut out = a;
        for (row, (ra, rb)) in out.iter_mut().zip(a.iter().zip(b.iter())) {
            for (v, (x, y)) in row.iter_mut().zip(ra.iter().zip(rb.iter())) {
                *v = lerp(*x, *y, m);
            }
        }
        out
    }

    pub fn reaction(&self, sim: f64) -> ReactionWeights {
        let i = self.index_of(Key::Reaction);
        let (s, d) = (&self.state[i], &self.defs[i]);
        let get = |k: usize| match d.opts[k].value {
            Value::Reaction(w) => w,
            _ => [0.0; 8],
        };
        let (a, b, m) = (get(s.from), get(s.to), self.mix(i, sim));
        let mut out = a;
        for (v, (x, y)) in out.iter_mut().zip(a.iter().zip(b.iter())) {
            *v = lerp(*x, *y, m);
        }
        out
    }

    /// Steps trait `i` to its previous (`dir < 0`) or next option, wrapping, with a short
    /// crossfade. Works on pinned traits too: pinning keeps whatever the user chose.
    pub fn step_option(&mut self, i: usize, dir: i32, sim: f64, dur: f64) {
        let n = self.defs[i].opts.len() as i32;
        let progress = self.progress(i, sim);
        let s = &mut self.state[i];
        if s.from != s.to && progress > 0.5 {
            s.from = s.to;
        }
        let next = ((s.to as i32 + dir.signum()) % n + n) % n;
        s.to = next as usize;
        s.start = sim;
        s.dur = dur;
        s.last = sim;
        if s.from == s.to {
            s.dur = 1.0;
        }
    }

    pub fn toggle_lock(&mut self, i: usize) {
        self.state[i].locked = !self.state[i].locked;
    }

    /// Starts crossfades on `n` traits (one or two at random when `None`). Pinned traits and
    /// traits already moving are skipped, as is Reaction while no audio is driving it.
    /// Returns the indices that started moving.
    pub fn shift(
        &mut self,
        sim: f64,
        n: Option<usize>,
        dur: Option<f64>,
        audio_active: bool,
        reduced_motion: bool,
    ) -> Vec<usize> {
        let mut free: Vec<(usize, f64)> = (0..self.state.len())
            .filter(|&i| {
                let s = &self.state[i];
                !s.locked && !s.moving() && (audio_active || self.defs[i].key != Key::Reaction)
            })
            .map(|i| (i, self.state[i].last + (self.rng.f32() as f64 - 0.5) * 14.0))
            .collect();
        free.sort_by(|a, b| a.1.total_cmp(&b.1));
        let n = n.unwrap_or_else(|| if self.rng.chance(0.8) { 1 } else { 2 });
        let mut changed = Vec::new();
        for &(i, _) in free.iter().take(n) {
            let to = pick_weighted(&self.defs[i].opts, Some(self.state[i].from), &mut self.rng);
            let base = dur.unwrap_or_else(|| self.rng.range(4.0, 7.0) as f64);
            let s = &mut self.state[i];
            s.to = to;
            s.start = sim;
            s.dur = base * if reduced_motion { 1.6 } else { 1.0 };
            s.last = sim;
            changed.push(i);
        }
        changed
    }

    /// Seconds to hold before the next timed shift (used when no audio is playing).
    pub fn next_hold(&mut self, reduced_motion: bool) -> f64 {
        self.rng.range(7.0, 12.0) as f64 * if reduced_motion { 2.0 } else { 1.0 }
    }

    /// What the HUD shows for trait `i`.
    pub fn describe(&self, i: usize, audio_active: bool) -> String {
        let (s, d) = (&self.state[i], &self.defs[i]);
        if s.moving() {
            return format!("{} → {}", d.opts[s.from].name, d.opts[s.to].name);
        }
        let mut t = d.opts[s.from].name.to_string();
        if s.locked {
            t.push_str(" · pinned");
        }
        if d.key == Key::Reaction && !audio_active {
            t.push_str(" · needs audio");
        }
        t
    }

    /// Beats between shifts while music drives the timing.
    pub fn beats_until_next(&mut self) -> u32 {
        if self.rng.chance(0.6) {
            32
        } else {
            16
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn traits() -> Traits {
        Traits::new(Rng::new(42))
    }

    #[test]
    fn catalog_is_well_formed() {
        let defs = catalog();
        assert_eq!(defs.len(), 13);
        for d in &defs {
            assert!(d.opts.len() >= 2, "{} needs at least two options", d.label);
            assert!(d.opts.iter().all(|o| o.weight > 0.0));
        }
        let names: Vec<&str> = defs
            .iter()
            .flat_map(|d| d.opts.iter().map(|o| o.name))
            .collect();
        assert!(names.len() > 100, "lists should be broad: {}", names.len());
    }

    #[test]
    fn pick_never_returns_excluded_and_follows_weights() {
        let opts = vec![
            o("a", 1.0, Value::Index),
            o("b", 3.0, Value::Index),
            o("c", 1.0, Value::Index),
        ];
        let mut rng = Rng::new(9);
        let mut counts = [0; 3];
        for _ in 0..10_000 {
            let i = pick_weighted(&opts, Some(2), &mut rng);
            assert_ne!(i, 2);
            counts[i] += 1;
        }
        let ratio = counts[1] as f32 / counts[0] as f32;
        assert!((2.6..3.4).contains(&ratio), "ratio {ratio}");
    }

    #[test]
    fn ease_is_monotonic_with_fixed_ends() {
        assert_eq!(ease(0.0), 0.0);
        assert_eq!(ease(1.0), 1.0);
        let mut prev = 0.0;
        for k in 1..=100 {
            let v = ease(k as f32 / 100.0);
            assert!(v >= prev);
            prev = v;
        }
    }

    #[test]
    fn shift_moves_one_or_two_traits_and_crossfades() {
        let mut t = traits();
        for round in 0..50 {
            let sim = round as f64 * 100.0;
            let changed = t.shift(sim, None, None, true, false);
            assert!((1..=2).contains(&changed.len()));
            for &i in &changed {
                assert!(t.state[i].moving());
                assert_eq!(t.mix(i, sim), 0.0);
            }
            t.settle(sim + 50.0);
            assert!(t.state.iter().all(|s| !s.moving()));
        }
    }

    #[test]
    fn pinned_traits_never_change() {
        let mut t = traits();
        let p = t.index_of(Key::Palette);
        t.toggle_lock(p);
        let before = t.state[p].from;
        for round in 0..200 {
            t.shift(round as f64 * 20.0, Some(3), Some(1.0), true, false);
            t.settle(round as f64 * 20.0 + 5.0);
        }
        assert_eq!(t.state[p].from, before);
        assert!(!t.state[p].moving());
    }

    #[test]
    fn reaction_waits_for_audio() {
        let mut t = traits();
        let r = t.index_of(Key::Reaction);
        for round in 0..200 {
            let changed = t.shift(round as f64 * 20.0, Some(12), Some(1.0), false, false);
            assert!(!changed.contains(&r));
            t.settle(round as f64 * 20.0 + 5.0);
        }
        assert!(t.describe(r, false).ends_with("needs audio"));
    }

    #[test]
    fn moving_traits_are_not_restarted() {
        let mut t = traits();
        let first = t.shift(0.0, Some(12), Some(10.0), true, false);
        let second = t.shift(1.0, Some(12), Some(10.0), true, false);
        assert!(!first.is_empty());
        assert!(second.iter().all(|i| !first.contains(i)));
    }

    #[test]
    fn scalar_interpolates_between_options() {
        let mut t = traits();
        let i = t.index_of(Key::Scale);
        t.state[i].from = 1; // Wide 1.6
        t.state[i].to = 2; // Mid 3.0
        t.state[i].start = 0.0;
        t.state[i].dur = 2.0;
        assert!((t.scalar(Key::Scale, 0.0) - 1.6).abs() < 1e-6);
        assert!((t.scalar(Key::Scale, 1.0) - 2.3).abs() < 1e-6);
        assert!((t.scalar(Key::Scale, 2.0) - 3.0).abs() < 1e-6);
    }

    #[test]
    fn arrows_step_and_wrap_both_ways() {
        let mut t = traits();
        let i = t.index_of(Key::Field);
        let n = t.defs[i].opts.len();
        t.state[i].from = 0;
        t.state[i].to = 0;
        t.step_option(i, -1, 10.0, 1.0);
        assert_eq!(
            t.state[i].to,
            n - 1,
            "left from the first wraps to the last"
        );
        t.settle(20.0);
        t.step_option(i, 1, 20.0, 1.0);
        assert_eq!(t.state[i].to, 0, "right from the last wraps to the first");
        assert_eq!(t.state[i].last, 20.0);
    }

    #[test]
    fn arrows_work_on_pinned_traits() {
        let mut t = traits();
        let i = t.index_of(Key::Palette);
        t.toggle_lock(i);
        let before = t.state[i].from;
        t.step_option(i, 1, 0.0, 1.0);
        t.settle(2.0);
        assert_ne!(t.state[i].from, before);
        assert!(t.state[i].locked);
    }

    #[test]
    fn rapid_clicks_keep_moving_forward() {
        let mut t = traits();
        let i = t.index_of(Key::Effect);
        t.state[i].from = 0;
        t.state[i].to = 0;
        for k in 0..5 {
            t.step_option(i, 1, k as f64 * 0.1, 1.2);
        }
        assert_eq!(t.state[i].to, 5);
    }

    #[test]
    fn reduced_motion_stretches_transitions() {
        let mut t = traits();
        let c = t.shift(0.0, Some(1), Some(5.0), true, true);
        assert!((t.state[c[0]].dur - 8.0).abs() < 1e-9);
    }
}
