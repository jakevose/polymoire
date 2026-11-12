//! Summarize a saved catalog (the app's `library.json`, or the Dropbox backup): tag
//! coverage and how the browse views group it, with timings.
//!
//!     cargo run --release --example library_probe -- ~/Library/Application\ Support/Polymoire/library.json

use polymoire::library::catalog::{Catalog, View};
use std::time::Instant;

fn main() {
    let path = std::env::args().nth(1).expect("path to library.json");
    let mut c: Catalog =
        serde_json::from_slice(&std::fs::read(&path).expect("read")).expect("parse catalog");
    c.refresh_guesses();
    let tagged: Vec<_> = c.tracks.values().filter(|t| t.tagged).collect();
    println!("{} tracks, {} with tags read", c.tracks.len(), tagged.len());
    let pct = |f: &dyn Fn(&&polymoire::library::catalog::Track) -> bool| {
        100.0 * tagged.iter().filter(|t| f(t)).count() as f64 / tagged.len().max(1) as f64
    };
    println!(
        "  album artist {:.0}%, genre {:.0}%, year {:.0}%, compilation {:.1}%, ReplayGain {:.1}% (of {} read by the new reader)",
        pct(&|t| !t.tags.album_artist.is_empty()),
        pct(&|t| !t.tags.genre.is_empty()),
        pct(&|t| t.tags.year > 0),
        pct(&|t| t.tags.compilation),
        pct(&|t| t.tags.gain.is_some()),
        tagged.iter().filter(|t| t.tags.v >= polymoire::library::catalog::TAGS_VERSION).count(),
    );
    for v in [
        View::Artists,
        View::Albums,
        View::Genres,
        View::Years,
        View::Folders,
    ] {
        let start = Instant::now();
        let groups = c.groups(v, "");
        let ms = start.elapsed().as_secs_f64() * 1000.0;
        let mut top = groups.clone();
        top.sort_by_key(|g| std::cmp::Reverse(g.count));
        let top: Vec<String> = top
            .iter()
            .take(12)
            .map(|g| format!("{} ({})", g.label, g.count))
            .collect();
        println!(
            "{:?}: {} groups in {ms:.0} ms; biggest: {}",
            v,
            groups.len(),
            top.join(", ")
        );
    }
}
