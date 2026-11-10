//! End to end without a sound card: the demo synth feeds the analyzer exactly as the app
//! does, and the beat clock must find 120 bpm and keep time through the breakdown.

use driftfield::analysis::Analyzer;
use driftfield::demo::{DemoSynth, BPM};

const SR: f32 = 48_000.0;

fn run(seconds: f32, block: usize) -> (Analyzer, Vec<f64>) {
    let mut synth = DemoSynth::new(SR);
    let mut a = Analyzer::new(SR);
    let mut buf = vec![0.0; block];
    let mut beats = Vec::new();
    let blocks = (SR * seconds) as usize / block;
    for _ in 0..blocks {
        synth.render(&mut buf);
        a.push(&buf);
        if a.poll_beat(a.time()) {
            beats.push(a.time());
        }
    }
    (a, beats)
}

#[test]
fn demo_beat_is_detected_at_120_bpm() {
    let (a, _) = run(10.0, 800);
    let bpm = a.levels().bpm;
    assert!((bpm - BPM).abs() < 2.0, "detected {bpm} bpm");
    assert!(a.is_locked());
}

#[test]
fn beats_stay_on_the_grid_through_the_breakdown() {
    // Bars are 2 s at 120 bpm; bars 6–7 of each 8-bar phrase (12–16 s) drop the kick.
    let (_, beats) = run(20.0, 800);
    let late: Vec<f64> = beats.into_iter().filter(|&t| t > 4.0).collect();
    for w in late.windows(2) {
        let d = w[1] - w[0];
        assert!((d - 0.5).abs() < 0.06, "beat spacing {d:.3}s: {late:?}");
    }
    let in_breakdown = late.iter().filter(|&&t| (12.5..15.5).contains(&t)).count();
    assert!(
        in_breakdown >= 5,
        "beat clock stalled in the breakdown: {late:?}"
    );
}

#[test]
fn works_with_typical_audio_buffer_sizes() {
    for block in [128, 441, 1024, 4096] {
        let (a, _) = run(8.0, block);
        let bpm = a.levels().bpm;
        assert!((bpm - BPM).abs() < 2.0, "block {block}: {bpm} bpm");
    }
}

#[test]
fn demo_triplet_bars_read_as_polyrhythm() {
    // Bars are 2 s: bars 1–4 (0–8 s) are straight, bars 5–6 (8–12 s) put triplet hats
    // over the straight kick.
    let mut synth = DemoSynth::new(SR);
    let mut a = Analyzer::new(SR);
    let mut buf = vec![0.0; 800];
    let mut at = |secs: f32, a: &mut Analyzer| {
        while a.time() < secs as f64 {
            synth.render(&mut buf);
            a.push(&buf);
        }
        a.rhythm()
    };
    let straight = at(8.0, &mut a);
    let poly = at(12.0, &mut a);
    assert!(straight.poly < 0.3, "straight bars: {straight:?}");
    assert!(poly.poly > 0.5, "triplet bars: {poly:?}");
    assert!(poly.triplet > straight.triplet + 0.3);
    let [kick, snare, hat] = a.hit_counts();
    assert!(
        kick > 15 && snare > 4 && hat > 30,
        "kick {kick} snare {snare} hat {hat}"
    );
}
