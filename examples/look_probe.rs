//! Measures how varied the looks are: how often a random scene is "flat" (few, similar
//! colors, low contrast, little structure), and which trait options make it so.
//!
//! Renders random trait combinations headlessly (the scene pass, exactly as the app draws
//! it) at a small size and scores each frame:
//! - **hues**: the effective number of hues (exp of the entropy of a 12-bin hue
//!   histogram weighted by chroma: one hue is 1, an even rainbow 12, grey 0);
//! - **contrast**: standard deviation of luminance;
//! - **detail**: mean luminance gradient after a 4× box downsample, so structure counts
//!   and fine texture (grain, halftone) doesn't.
//!
//! A frame is **flat** when it is *dull* (hues < 2.5 with contrast < 0.15 or detail
//! < 0.09) or *blurry* (detail < 0.06). The thresholds were set by eye against contact
//! sheets of the frames (`LOOK_SAVE=<dir>` saves the first 48 with their scores; add
//! `LOOK_FLAT=1` for the first 48 flat ones).
//! Run with `cargo run --release --example look_probe`. Needs a GPU.

#[path = "common/headless.rs"]
mod headless;

use polymoire::reactions::{FrameInput, Pulse, Sound};
use polymoire::rng::Rng;
use polymoire::traits::{flatness, Key, Traits};
use polymoire::uniforms::Uniforms;
use std::collections::BTreeMap;

const W: u32 = 480;
const H: u32 = 300;
const RUNS: u64 = 30;
const SAMPLES_PER_RUN: u64 = 50;

#[derive(Clone, Copy, Default)]
struct Score {
    hues: f32,
    contrast: f32,
    detail: f32,
}

impl Score {
    /// One or two similar colors, without the contrast or structure to make up for it.
    fn dull(&self) -> bool {
        self.hues < 2.5 && (self.contrast < 0.15 || self.detail < 0.09)
    }
    /// Soft blobs with little structure, whatever the colors.
    fn blurry(&self) -> bool {
        self.detail < 0.06
    }
    fn flat(&self) -> bool {
        self.dull() || self.blurry()
    }
}

fn lum(c: [f32; 3]) -> f32 {
    0.299 * c[0] + 0.587 * c[1] + 0.114 * c[2]
}

fn score(rgb: &[u8]) -> Score {
    let px: Vec<[f32; 3]> = rgb
        .chunks_exact(3)
        .map(|p| {
            [
                p[0] as f32 / 255.0,
                p[1] as f32 / 255.0,
                p[2] as f32 / 255.0,
            ]
        })
        .collect();
    let mut bins = [0.0f32; 12];
    let (mut sum, mut sum2) = (0.0f64, 0.0f64);
    for c in &px {
        let (mx, mn) = (c[0].max(c[1]).max(c[2]), c[0].min(c[1]).min(c[2]));
        let chroma = mx - mn;
        if chroma > 0.06 {
            let h = if mx == c[0] {
                ((c[1] - c[2]) / chroma).rem_euclid(6.0)
            } else if mx == c[1] {
                (c[2] - c[0]) / chroma + 2.0
            } else {
                (c[0] - c[1]) / chroma + 4.0
            };
            bins[((h / 6.0 * 12.0) as usize).min(11)] += chroma;
        }
        let l = lum(*c) as f64;
        sum += l;
        sum2 += l * l;
    }
    // Effective number of hues: exp(entropy) of the chroma-weighted hue histogram. One
    // hue scores 1, an even rainbow 12; grey scores 0.
    let total: f32 = bins.iter().sum();
    let hues = if total < px.len() as f32 * 0.03 {
        0.0
    } else {
        let h: f32 = bins
            .iter()
            .filter(|&&b| b > 0.0)
            .map(|&b| -(b / total) * (b / total).ln())
            .sum();
        h.exp()
    };
    let n = px.len() as f64;
    let contrast = (sum2 / n - (sum / n).powi(2)).max(0.0).sqrt() as f32;
    // 4x box downsample of luminance, then mean gradient.
    let (w4, h4) = ((W / 4) as usize, (H / 4) as usize);
    let mut small = vec![0.0f32; w4 * h4];
    for y in 0..h4 * 4 {
        for x in 0..w4 * 4 {
            small[(y / 4) * w4 + x / 4] += lum(px[y * W as usize + x]) / 16.0;
        }
    }
    let mut g = 0.0;
    for y in 0..h4 - 1 {
        for x in 0..w4 - 1 {
            let v = small[y * w4 + x];
            g += (small[y * w4 + x + 1] - v).abs() + (small[(y + 1) * w4 + x] - v).abs();
        }
    }
    let detail = g / ((w4 - 1) * (h4 - 1)) as f32;
    Score {
        hues,
        contrast,
        detail,
    }
}

/// Uniforms for the look `t` shows at `sim`, a couple of seconds into its ambient motion.
fn uniforms(t: &Traits, sim: f64, rng: &mut Rng) -> Uniforms {
    let mut pulse = Pulse::default();
    let dt = 1.0 / 60.0;
    let energy = t.scalar(Key::Energy, sim);
    let mut v = polymoire::reactions::Visual::NEUTRAL;
    for _ in 0..120 {
        v = pulse.frame(FrameInput {
            dt,
            levels: &Default::default(),
            live: false,
            beat: false,
            weights: &t.reaction(sim),
            energy,
            period: 0.5,
            hits: Default::default(),
            sound: Sound::default(),
            reduced_motion: false,
        });
    }
    let flow_t = rng.range(0.0, 200.0) as f64;
    let ang = rng.range(0.0, std::f32::consts::TAU) as f64;
    Uniforms::for_frame(t, sim, &v, flow_t, ang, [W, H], 1000.0 * W as f32 / 1600.0)
}

/// The looks the app would show: the trait scheduler running as it does with music,
/// sampled after every other shift, across several runs.
fn looks() -> Vec<(Traits, f64)> {
    let mut out = Vec::new();
    for seed in 0..RUNS {
        let mut t = Traits::new(Rng::new(seed));
        for k in 0..SAMPLES_PER_RUN * 2 {
            let sim = k as f64 * 10.0;
            t.shift(sim, None, Some(1.0), true, false);
            t.settle(sim + 5.0);
            if k % 2 == 1 {
                out.push((t.clone(), sim + 5.0));
            }
        }
    }
    out
}

fn key_of(label: &str) -> Key {
    polymoire::traits::catalog()
        .into_iter()
        .find(|d| d.label == label)
        .map(|d| d.key)
        .expect("trait label")
}

fn main() {
    let g = headless::gpu();
    let mut by_opt: BTreeMap<(String, String), (u32, u32)> = BTreeMap::new();
    let mut all = Vec::new();
    let mut rng = Rng::new(0x9e37);
    let mut saved = 0;
    for (seed, (t, sim)) in looks().into_iter().enumerate() {
        let u = uniforms(&t, sim, &mut rng);
        let rgb = headless::render(&g, &u, W, H);
        let s = score(&rgb);
        all.push(s);
        for (i, d) in t.defs.iter().enumerate() {
            let e = by_opt
                .entry((
                    d.label.to_string(),
                    d.opts[t.state[i].from].name.to_string(),
                ))
                .or_default();
            e.0 += 1;
            e.1 += s.flat() as u32;
        }
        // LOOK_SAVE=<dir> saves frames for a contact sheet: the first 48, or with
        // LOOK_FLAT=1 the first 48 flat ones.
        if let Ok(dir) = std::env::var("LOOK_SAVE") {
            let only_flat = std::env::var("LOOK_FLAT").is_ok();
            if saved < 48 && (!only_flat || s.flat()) {
                saved += 1;
                let names: Vec<&str> = t
                    .defs
                    .iter()
                    .enumerate()
                    .map(|(i, d)| d.opts[t.state[i].from].name)
                    .collect();
                save_png(&format!("{dir}/{seed:04}.png"), &rgb);
                println!(
                    "{seed:4} hues={:.1} contrast={:.3} detail={:.3} flat={} {:?}",
                    s.hues,
                    s.contrast,
                    s.detail,
                    s.flat(),
                    names
                );
            }
        }
    }
    let n = all.len() as f32;
    let flat = all.iter().filter(|s| s.flat()).count() as f32 / n;
    let pct = |f: &dyn Fn(&Score) -> f32, p: f32| {
        let mut v: Vec<f32> = all.iter().map(f).collect();
        v.sort_by(f32::total_cmp);
        v[((v.len() - 1) as f32 * p) as usize]
    };
    println!("samples: {}", all.len());
    println!("flat: {:.1}%", flat * 100.0);
    println!(
        "dull: {:.1}%   blurry: {:.1}%   hues < 2.5: {:.1}%",
        all.iter().filter(|s| s.dull()).count() as f32 / n * 100.0,
        all.iter().filter(|s| s.blurry()).count() as f32 / n * 100.0,
        all.iter().filter(|s| s.hues < 2.5).count() as f32 / n * 100.0,
    );
    for (name, f) in [
        ("hues", &(|s: &Score| s.hues) as &dyn Fn(&Score) -> f32),
        ("contrast", &|s: &Score| s.contrast),
        ("detail", &|s: &Score| s.detail),
    ] {
        println!(
            "{name:9} p10 {:.3}  p25 {:.3}  p50 {:.3}  p75 {:.3}",
            pct(f, 0.1),
            pct(f, 0.25),
            pct(f, 0.5),
            pct(f, 0.75)
        );
    }
    // The variety guard should make the flattening options rarer, not drop them.
    let mut seen: Vec<String> = by_opt
        .iter()
        .filter(|((k, o), _)| flatness(key_of(k), o) >= 0.6)
        .map(|((k, o), (c, _))| format!("{k} {o} {:.1}%", *c as f32 / n * 100.0))
        .collect();
    seen.sort();
    println!(
        "\nOn screen, the most flattening options:\n  {}",
        seen.join(" · ")
    );
    println!(
        "\nOptions most often in flat looks (flat share when present, vs {:.0}% overall):",
        flat * 100.0
    );
    let mut rows: Vec<_> = by_opt
        .iter()
        .filter(|(_, (c, _))| *c >= 20)
        .map(|((k, o), (c, f))| (*f as f32 / *c as f32, k, o, *c))
        .collect();
    rows.sort_by(|a, b| b.0.total_cmp(&a.0));
    for (share, k, o, c) in rows.iter().take(25) {
        println!("  {:5.1}%  {k:9} {o:14} (n={c})", share * 100.0);
    }
}

fn save_png(path: &str, rgb: &[u8]) {
    let file = std::fs::File::create(path).expect("create png");
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), W, H);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    let mut w = enc.write_header().expect("png header");
    w.write_image_data(rgb).expect("png data");
}
