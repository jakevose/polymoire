//! Beat-map local mp3/m4a files the way the media player does, and print what it found.
//!
//!     cargo run --release --example beatmap_probe -- song.mp3 [more files…]
//!
//! With `--demo-grid`, also scores the beats against the demo loop's true 120 bpm grid
//! (for files rendered from `DemoSynth`; see `--write-demo`).
//!
//!     cargo run --release --example beatmap_probe -- --write-demo demo.wav

use polymoire::demo::{DemoSynth, BPM};
use polymoire::library::media;
use std::sync::Arc;
use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(i) = args.iter().position(|a| a == "--write-demo") {
        write_demo(args.get(i + 1).expect("--write-demo <file.wav>"));
        return;
    }
    let grid = args.iter().any(|a| a == "--demo-grid");
    for path in args.iter().filter(|a| !a.starts_with("--")) {
        let bytes = Arc::new(std::fs::read(path).expect("read file"));
        let ext = path.rsplit('.').next().unwrap_or("").to_lowercase();
        match media::read_tags(bytes.clone(), &ext) {
            Ok((t, d)) => println!(
                "{path}\n  tags: {:?} / {:?} / {:?} ({}), {:.1} s",
                t.title, t.artist, t.album, t.year, d
            ),
            Err(e) => println!("{path}\n  tags: {e}"),
        }
        let started = Instant::now();
        match media::analyze(bytes, &ext) {
            Ok(media::Analysis {
                map: Some(m),
                duration: secs,
                loudness,
            }) => {
                if let Some(l) = loudness {
                    println!("  loudness {:.1} LUFS, peak {:.1} dBFS", l.lufs, l.peak_db);
                }
                println!(
                    "  {:.2} bpm, {} beats over {secs:.1} s, analyzed in {:.2} s",
                    m.bpm,
                    m.beats.len(),
                    started.elapsed().as_secs_f32()
                );
                let first: Vec<String> =
                    m.beats.iter().take(8).map(|b| format!("{b:.3}")).collect();
                println!("  first beats: {}", first.join(" "));
                let st = &m.structure;
                let secs: Vec<String> = st
                    .sections
                    .iter()
                    .map(|s| {
                        let t = m.beats.get(s.beat).copied().unwrap_or(0.0);
                        format!("{t:.1}s#{}({:+.0})", s.beat, s.level)
                    })
                    .collect();
                println!(
                    "  first downbeat #{} {:.2} s; sections: {}",
                    st.bar,
                    m.beats.get(st.bar).copied().unwrap_or(0.0),
                    secs.join(" ")
                );
                let code = m.encode();
                println!(
                    "  stored as {} chars ({:.2} per beat)",
                    code.len(),
                    code.len() as f64 / m.beats.len().max(1) as f64
                );
                if grid {
                    let period = 60.0 / BPM as f64;
                    let mut errs: Vec<f64> = m
                        .beats
                        .iter()
                        .map(|b| (b / period).round() * period - b)
                        .map(|e| e.abs() * 1000.0)
                        .collect();
                    errs.sort_by(f64::total_cmp);
                    let pct = |p: f64| errs[((errs.len() - 1) as f64 * p) as usize];
                    let signed: f64 = m
                        .beats
                        .iter()
                        .map(|b| b - (b / period).round() * period)
                        .sum::<f64>()
                        / m.beats.len() as f64;
                    println!(
                        "  vs true grid: median {:.1} ms, 95th {:.1} ms, worst {:.1} ms, mean offset {:+.1} ms",
                        pct(0.5),
                        pct(0.95),
                        pct(1.0),
                        signed * 1000.0
                    );
                }
            }
            Ok(a) => println!("  no steady beat found ({:.1} s)", a.duration),
            Err(e) => println!("  {e}"),
        }
    }
}

/// 60 s of the demo loop as 16-bit mono WAV at 44.1 kHz.
fn write_demo(path: &str) {
    let sr = 44_100u32;
    let mut buf = vec![0.0f32; sr as usize * 60];
    DemoSynth::new(sr as f32).render(&mut buf);
    let data: Vec<u8> = buf
        .iter()
        .flat_map(|s| ((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes())
        .collect();
    let mut out = Vec::new();
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    for v in [16u32, 1 | 1 << 16, sr, sr * 2, 2 | 16 << 16] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(&data);
    std::fs::write(path, out).expect("write wav");
    println!("wrote {path}");
}
