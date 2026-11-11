//! End to end: the demo plays, then the audio stops, and the picture must settle instead
//! of shifting faster. Runs the analyzer, vibe, reactions and director exactly as the app's
//! frame loop does (`src/app/mod.rs::step`), for the ways system audio can "stop".

use polymoire::analysis::{Analyzer, BEAT_LEAD};
use polymoire::demo::DemoSynth;
use polymoire::director::{Director, Moment, MIN_GAP, SETTLE};
use polymoire::reactions::{is_drop, FrameInput, Pulse, Sound};
use polymoire::rng::Rng;

const SR: f32 = 48_000.0;
const FPS: f64 = 60.0;
const MUSIC: f64 = 30.0;
const AFTER: f64 = 90.0;

#[derive(Clone, Copy)]
enum After {
    /// Capture keeps delivering digital silence.
    Zeros,
    /// Capture stops delivering samples altogether.
    Nothing,
    /// A faint noise floor, loud enough to count as "present" (about -50 dBFS).
    Hiss,
}

/// Returns the times of every shift.
fn run(after: After, seed: u64) -> Vec<f64> {
    let mut synth = DemoSynth::new(SR);
    let mut an = Analyzer::new(SR);
    let mut vibe = polymoire::vibe::Vibe::default();
    let mut pulse = Pulse::default();
    let mut director = Director::new(Rng::new(seed));
    let mut noise = Rng::new(seed + 100);
    let mut buf = vec![0.0f32; (SR as f64 / FPS) as usize];
    let mut shifts = Vec::new();
    let mut last_push = 0.0;
    let dt = 1.0 / FPS as f32;
    for f in 0..((MUSIC + AFTER) * FPS) as usize {
        let sim = f as f64 / FPS;
        let pushed = if sim < MUSIC {
            synth.render(&mut buf);
            true
        } else {
            match after {
                After::Zeros => {
                    buf.fill(0.0);
                    true
                }
                After::Nothing => false,
                After::Hiss => {
                    buf.iter_mut()
                        .for_each(|x| *x = (noise.f32() - 0.5) * 0.011);
                    true
                }
            }
        };
        if pushed {
            an.push(&buf);
            last_push = sim;
        }
        // As `Feed::heard_now`: no samples for half a second means audio stopped.
        let live = sim - last_push < 0.5;
        let heard = an.time();
        let beat = live && an.poll_beat(heard + BEAT_LEAD);
        let hits = if live {
            an.poll_hits(heard)
        } else {
            Default::default()
        };
        let l = if live {
            an.levels()
        } else {
            Default::default()
        };
        let r = an.rhythm();
        let feats = vibe.features(dt, &l, &r, live);
        let mood_changed = vibe.update(dt, &feats, &r);
        pulse.frame(FrameInput {
            dt,
            levels: &l,
            live,
            beat,
            weights: &[0.5; 10],
            energy: 0.55,
            period: an.period() as f32,
            hits,
            sound: Sound {
                level: vibe.level,
                intensity: vibe.intensity,
                ..Default::default()
            },
            reduced_motion: false,
        });
        let cue = director.step(Moment {
            sim,
            music: pulse.active(),
            beat,
            period: an.period(),
            big_change: is_drop(&l) || mood_changed,
            reduced_motion: false,
        });
        if cue.is_some() {
            shifts.push(sim);
        }
    }
    shifts
}

fn check(after: After, name: &str) {
    for seed in 0..3 {
        let shifts = run(after, seed);
        let during: Vec<f64> = shifts.iter().copied().filter(|&t| t < MUSIC).collect();
        assert!(
            during.len() >= 5,
            "{name}: the demo should drive shifts: {shifts:?}"
        );
        let tail: Vec<f64> = shifts.iter().copied().filter(|&t| t >= MUSIC).collect();
        for w in shifts.windows(2) {
            assert!(w[1] - w[0] >= MIN_GAP - 1e-9, "{name}: {shifts:?}");
        }
        // After the music: never faster than one change every seven seconds.
        assert!(
            tail.len() as f64 <= AFTER / 7.0,
            "{name}: {} shifts in {AFTER} s after the music: {tail:?}",
            tail.len()
        );
        if !matches!(after, After::Hiss) {
            // Real silence settles first.
            let gap = tail.first().copied().unwrap_or(f64::MAX) - MUSIC;
            assert!(gap >= SETTLE * 0.9, "{name}: first change {gap:.1} s after");
        }
    }
}

#[test]
fn digital_silence_settles() {
    check(After::Zeros, "zeros");
}

#[test]
fn audio_that_stops_arriving_settles() {
    check(After::Nothing, "nothing");
}

#[test]
fn a_faint_noise_floor_drifts_slowly_and_does_not_freeze() {
    check(After::Hiss, "hiss");
    let tail = run(After::Hiss, 0)
        .into_iter()
        .filter(|&t| t >= MUSIC + 15.0)
        .count();
    assert!(tail >= 3, "the look froze: {tail} shifts");
}
