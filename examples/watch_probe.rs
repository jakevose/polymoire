//! Plays real tracks through the whole chain the way the media player does (beat map,
//! analyzer, vibe, reactions, director, traits), without a window or sound card, and
//! reports how the look changes against the music: how often, where in the bar, how
//! much is in transition at once, and what the whole-view camera does.
//!
//!     cargo run --release --example watch_probe -- song.mp3 [more files…] [--log]
//!
//! `--live` paces as for system audio (live beat clock, no map); `--no-place` uses the
//! map's beats but not its bars and sections (the pacing before song structure).
//!
//! Use it before and after changing the pacing so the change is measured.

use polymoire::analysis::{Analyzer, BEAT_LEAD};
use polymoire::director::{Director, Moment};
use polymoire::library::media::{self, Decoder};
use polymoire::reactions::{is_drop, is_sounding, FrameInput, Pulse, Sound};
use polymoire::rng::Rng;
use polymoire::traits::{Key, Traits};
use polymoire::vibe::{self, Mood, Vibe};
use std::sync::Arc;

const FPS: f64 = 60.0;

struct Shift {
    t: f64,
    traits: Vec<&'static str>,
    dur: f64,
    big: bool,
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flag = |f: &str| args.iter().any(|a| a == f);
    let mode = Mode {
        log: flag("--log"),
        live: flag("--live"),
        no_place: flag("--no-place"),
    };
    let mut totals = Totals::default();
    for path in args.iter().filter(|a| !a.starts_with("--")) {
        for seed in 0..3 {
            run(path, seed, &mode, &mut totals);
        }
    }
    totals.print();
}

struct Mode {
    log: bool,
    live: bool,
    no_place: bool,
}

#[derive(Default)]
struct Totals {
    sections: usize,
    sections_marked: usize,
    minutes: f64,
    shifts: usize,
    big: usize,
    on_one: usize,
    with_beat: usize,
    scene: usize,
    in_motion: f64,
    two_moving: f64,
    frames: f64,
    gaps: Vec<f64>,
}

impl Totals {
    fn print(&self) {
        let mut g = self.gaps.clone();
        g.sort_by(f64::total_cmp);
        let pct = |p: f64| g.get(((g.len().max(1) - 1) as f64 * p) as usize).copied();
        println!("\nALL: {:.1} min", self.minutes);
        println!(
            "  shifts/min {:.1} (big {:.1}), scene-wide (Field/Palette) {:.1}/min",
            self.shifts as f64 / self.minutes,
            self.big as f64 / self.minutes,
            self.scene as f64 / self.minutes
        );
        println!(
            "  gap p10/50/90: {:.1} / {:.1} / {:.1} s",
            pct(0.1).unwrap_or(0.0),
            pct(0.5).unwrap_or(0.0),
            pct(0.9).unwrap_or(0.0)
        );
        println!(
            "  on a bar's first beat (of beat-timed shifts): {:.0}%",
            100.0 * self.on_one as f64 / self.with_beat.max(1) as f64
        );
        println!(
            "  section starts met by a scene change: {} of {}",
            self.sections_marked, self.sections
        );
        println!(
            "  time with a trait mid-change {:.0}%, two or more {:.0}%",
            100.0 * self.in_motion / self.frames,
            100.0 * self.two_moving / self.frames
        );
    }
}

fn run(path: &str, seed: u64, mode: &Mode, totals: &mut Totals) {
    let log = mode.log && seed == 0;
    let bytes = Arc::new(std::fs::read(path).expect("read file"));
    let ext = path.rsplit('.').next().unwrap_or("").to_lowercase();
    let analysis = media::analyze(bytes.clone(), &ext).expect("analyze");
    let map = analysis.map.filter(|_| !mode.live);
    let mut dec = Decoder::new(bytes, &ext).expect("decode");
    let mut mono = Vec::new();
    while let Some((block, ch)) = dec.next_block() {
        mono.extend(block.chunks(ch).map(|f| f.iter().sum::<f32>() / ch as f32));
    }
    let sr = dec.rate as f64;

    let mut an = Analyzer::new(sr as f32);
    let mut vibe = Vibe::default();
    let mut pulse = Pulse::default();
    let mut director = Director::new(Rng::new(seed));
    let mut traits = Traits::new(Rng::new(seed + 7));
    let mut shifts: Vec<Shift> = Vec::new();
    let mut sim = 0.0;
    let mut polled: Option<f64> = None;
    let mut beats_seen: Vec<f64> = Vec::new();
    let mut moods = Vec::new();
    let per = (sr / FPS) as usize;
    let dt = 1.0 / FPS as f32;
    let frames = mono.len() / per;
    let (mut zoom_moves, mut last_cam) = (0usize, 1.0f32);
    let mut gains: Vec<(f64, f32)> = Vec::new();
    for f in 0..frames {
        an.push(&mono[f * per..(f + 1) * per]);
        let heard = an.time();
        let mut beat = an.poll_beat(heard + BEAT_LEAD);
        let mut period = an.period();
        let mut l = an.levels();
        let mut place = None;
        if let Some(m) = &map {
            if !mode.no_place {
                place = m.place_at(heard + BEAT_LEAD);
            }
            let from = polled.filter(|&p| p <= heard && heard - p < 1.0);
            beat = from.is_some_and(|p| m.beat_in(p + BEAT_LEAD, heard + BEAT_LEAD));
            period = m.period_at(heard);
            l.bpm = m.bpm;
            polled = Some(heard);
        }
        if beat {
            beats_seen.push(heard);
        }
        let hits = an.poll_hits(heard);
        let r = an.rhythm();
        if let Some(p) = &place {
            vibe.know_section(&l, p);
        }
        let feats = vibe.features(dt, &l, &r, true);
        let mood_changed = vibe.update(dt, &feats, &r);
        if mood_changed {
            moods.push((heard, vibe.mood));
        }
        let weights = traits.reaction(sim);
        let trait_energy = traits.scalar(Key::Energy, sim);
        let energy = trait_energy + (vibe.intensity - trait_energy).max(0.0) * pulse.gate * 0.7;
        let v = pulse.frame(FrameInput {
            dt,
            levels: &l,
            live: true,
            beat,
            weights: &weights,
            energy,
            period: period as f32,
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
        gains.push((heard, polymoire::reactions::loudness_gain(vibe.level)));
        if last_cam >= 0.995 && v.zoom < 0.995 {
            zoom_moves += 1;
        }
        last_cam = v.zoom;
        sim += dt as f64;
        let big = is_drop(&l) || mood_changed;
        let cue = director.step(Moment {
            sim,
            music: pulse.active() && is_sounding(&l),
            beat,
            period,
            big_change: big,
            reduced_motion: false,
            place,
        });
        if let Some(c) = cue {
            let (mood, poly, triplet) = if pulse.active() {
                (vibe.mood, vibe.poly, vibe.triplet)
            } else {
                (Mood::Ambient, 0.0, 0.0)
            };
            let changed =
                traits.shift_with(sim, c.n, c.scope, c.dur, pulse.active(), false, &|k, n| {
                    vibe::bias(mood, poly, triplet, k, n)
                });
            shifts.push(Shift {
                t: heard,
                traits: changed.iter().map(|&i| traits.defs[i].label).collect(),
                dur: c.dur.unwrap_or(0.0),
                big: c.n == Some(2) && c.dur.is_some(),
            });
        }
        let moving = traits.state.iter().filter(|s| s.moving()).count();
        totals.frames += 1.0;
        if moving >= 1 {
            totals.in_motion += 1.0;
        }
        if moving >= 2 {
            totals.two_moving += 1.0;
        }
        traits.settle(sim);
    }

    let secs = frames as f64 / FPS;
    totals.minutes += secs / 60.0;
    totals.shifts += shifts.len();
    totals.big += shifts.iter().filter(|s| s.big).count();
    totals.scene += shifts
        .iter()
        .flat_map(|s| &s.traits)
        .filter(|t| matches!(**t, "Field" | "Palette"))
        .count();
    totals
        .gaps
        .extend(shifts.windows(2).map(|w| w[1].t - w[0].t));
    // Bar position from the map's structure (measured the same way in every mode).
    let shape = media::analyze(Arc::new(std::fs::read(path).unwrap()), &ext)
        .ok()
        .and_then(|a| a.map);
    if let Some(m) = &shape {
        let bar = m.structure.bar;
        for s in &shifts {
            if let Some(i) = m.beats.iter().position(|&b| (b - s.t).abs() < 0.06) {
                totals.with_beat += 1;
                if (i + 4 - bar % 4).is_multiple_of(4) {
                    totals.on_one += 1;
                }
            }
        }
        for sec in m.structure.sections.iter().skip(1) {
            totals.sections += 1;
            let t = m.beats[sec.beat];
            if shifts.iter().any(|s| {
                (s.t - t).abs() < 0.1
                    && s.traits.iter().any(|n| {
                        matches!(
                            *n,
                            "Field" | "Palette" | "Symmetry" | "Effect" | "Scale" | "Mosaic"
                        )
                    })
            }) {
                totals.sections_marked += 1;
            }
        }
    }
    if seed == 0 {
        println!(
            "{path}\n  {:.0} s, {} bpm, {} shifts, {} moods, {} zoom moves",
            secs,
            map.as_ref().map_or(0.0, |m| m.bpm).round(),
            shifts.len(),
            moods.len(),
            zoom_moves
        );
    }
    if log {
        for s in &shifts {
            let idx = shape
                .as_ref()
                .and_then(|m| m.beats.iter().position(|&b| (b - s.t).abs() < 0.06));
            println!(
                "    {:6.1}s beat {idx:?} {}{:?} over {:.1}s",
                s.t,
                if s.big { "BIG " } else { "" },
                s.traits,
                s.dur
            );
        }
        if let Some(m) = &shape {
            let secs = &m.structure.sections;
            for (k, sec) in secs.iter().enumerate() {
                let a = m.beats[sec.beat];
                let b = secs.get(k + 1).map_or(f64::MAX, |n| m.beats[n.beat]);
                let g: Vec<f32> = gains
                    .iter()
                    .filter(|(t, _)| *t >= a && *t < b)
                    .map(|x| x.1)
                    .collect();
                println!(
                    "    section {a:6.1}s {:+4.0} dB: reaction gain {:.2}",
                    sec.level,
                    g.iter().sum::<f32>() / g.len().max(1) as f32
                );
            }
        }
        for (t, m) in &moods {
            println!("    mood {t:6.1}s → {}", m.label());
        }
    }
}
