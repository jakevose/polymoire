//! Measures how responsive the reactions are, without a window or sound card.
//!
//! Runs the demo loop (clean, and through a soft clipper standing in for a loud commercial
//! master) through the analyzer, vibe and reactions exactly as the app does, then prints:
//! - kick detection against the demo's true kick times (found / missed / extra), and
//! - for each visual signal: its range, how much it rises in the 80 ms after a kick
//!   ("pulse": higher reads as more on-the-beat), and how long after a kick it peaks.
//!
//! Run with `cargo run --release --example pulse_probe`. Use it before and after tuning a
//! reaction so the change is measured, not guessed.

use polymoire::analysis::Analyzer;
use polymoire::demo::DemoSynth;
use polymoire::reactions::{FrameInput, Pulse, Sound};
use polymoire::vibe::Vibe;

const SR: f32 = 48_000.0;
const FPS: f32 = 60.0;
const SECS: f32 = 40.0;
/// The "Pump" reaction weights.
const WEIGHTS: [f32; 10] = [0.3, 1.0, 0.0, 0.0, 0.2, 0.6, 0.0, 0.0, 1.0, 0.2];

fn pct(v: &[f32], p: f32) -> f32 {
    let mut s = v.to_vec();
    s.sort_by(f32::total_cmp);
    s[((s.len() - 1) as f32 * p) as usize]
}

fn run(label: &str, master: impl Fn(f32) -> f32) {
    let mut synth = DemoSynth::new(SR);
    let mut an = Analyzer::new(SR);
    let mut vibe = Vibe::default();
    let mut pulse = Pulse::default();
    let per_frame = (SR / FPS) as usize;
    let frames = (SECS * FPS) as usize;
    let mut buf = vec![0.0; per_frame];
    let names = [
        "thump",
        "surge",
        "jelly",
        "warp_add",
        "chroma_add",
        "expo",
        "zoom",
    ];
    let mut series: Vec<Vec<f32>> = vec![Vec::new(); names.len()];
    let mut kicks = Vec::new();
    for f in 0..frames {
        synth.render(&mut buf);
        buf.iter_mut().for_each(|x| *x = master(*x));
        an.push(&buf);
        let heard = an.time();
        let beat = an.poll_beat(heard);
        let hits = an.poll_hits(heard);
        let (l, r) = (an.levels(), an.rhythm());
        let dt = 1.0 / FPS;
        let feats = vibe.features(dt, &l, &r, true);
        vibe.update(dt, &feats, &r);
        let v = pulse.frame(FrameInput {
            dt,
            levels: &l,
            live: true,
            beat,
            weights: &WEIGHTS,
            energy: 0.55,
            period: an.period() as f32,
            hits,
            sound: Sound {
                bass_sustain: l.bass_sustain,
                intensity: vibe.intensity,
                heaviness: vibe.heaviness,
                poly: vibe.poly,
                syncopation: vibe.syncopation,
                snare_rate: r.snare_rate,
                level: vibe.level,
            },
            reduced_motion: false,
        });
        if hits.kick > 0.0 {
            kicks.push((f, heard));
        }
        let vals = [
            pulse.thump,
            pulse.flow_speed_add(false),
            v.jelly,
            v.warp_add,
            v.chroma_add,
            v.expo,
            1.0 - v.zoom,
        ];
        for (s, x) in series.iter_mut().zip(vals) {
            s.push(x);
        }
    }

    // True kicks: every beat (0.5 s) except bars 6–7 of each 8-bar phrase (the breakdown).
    let truth: Vec<f64> = (0..(SECS * 2.0) as usize)
        .map(|k| k as f64 * 0.5)
        .filter(|t| ((t / 2.0) as u64 % 8) < 6)
        .collect();
    let real = |t: f64| truth.iter().any(|&k| (-0.01..0.05).contains(&(t - k)));
    let matched = kicks.iter().filter(|&&(_, t)| real(t)).count();
    println!(
        "\n=== {label}: kicks found {matched}/{}, extra {}, bpm {:.1}",
        truth.len(),
        kicks.len() - matched,
        an.levels().bpm
    );
    println!(
        "{:<11} {:>7} {:>7} {:>7} {:>7} {:>8}",
        "signal", "p5", "p50", "p95", "pulse", "peak ms"
    );
    let start = (8.0 * FPS) as usize;
    let at: Vec<usize> = kicks
        .iter()
        .map(|&(f, _)| f)
        .filter(|&f| f > start && f + 30 < frames)
        .collect();
    for (name, s) in names.iter().zip(&series) {
        let v = &s[start..];
        let full = (pct(v, 1.0) - pct(v, 0.0)).max(1e-6);
        let (mut post, mut pre, mut lags) = (0.0, 0.0, Vec::new());
        for &k in &at {
            post += (0..5).map(|i| s[k + i]).sum::<f32>() / 5.0;
            pre += (2..7).map(|i| s[k - i]).sum::<f32>() / 5.0;
            let peak = (0..18)
                .max_by(|&a, &b| s[k + a].total_cmp(&s[k + b]))
                .unwrap();
            lags.push(peak as f32 * 1000.0 / FPS);
        }
        lags.sort_by(f32::total_cmp);
        let n = at.len().max(1) as f32;
        println!(
            "{:<11} {:>7.3} {:>7.3} {:>7.3} {:>6.0}% {:>8.0}",
            name,
            pct(v, 0.05),
            pct(v, 0.5),
            pct(v, 0.95),
            (post - pre) / n / full * 100.0,
            lags.get(lags.len() / 2).copied().unwrap_or(f32::NAN)
        );
    }
}

fn main() {
    run("demo loop", |x| x);
    run("demo loop, loud master (+12 dB into a soft clipper)", |x| {
        (x * 4.0).tanh() * 0.9
    });
}
