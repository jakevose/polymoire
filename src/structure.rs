//! A track's shape, from its beat map and the band energies the mapper already measures:
//! where the bars start, and where the song changes section (intro, verse, drop,
//! breakdown…) and how loud each section is.
//!
//! Each beat gets a timbre-and-level fingerprint: the mean log energy in the mapper's
//! eight bands over that beat. Music changes on bar lines and, more, at section starts,
//! so:
//! - **Bars:** the downbeat is the beat phase (of four) where the fingerprint changes most
//!   between the bar before and the bar after.
//! - **Sections:** a section starts at a downbeat where the four bars after differ
//!   clearly from the four before, more than anywhere nearby. Sections are at least
//!   four bars long.
//!
//! The pacing uses it to change the look with the song's phrasing instead of on a
//! running count of beats.

use serde::{Deserialize, Serialize};

pub const BANDS: usize = 8;
/// Beats per bar. Nearly all music the app meets is in four; a waltz gets bars of four
/// beats anyway, which only shifts where the look changes.
pub const BAR: usize = 4;
/// Shortest section, in beats (four bars).
pub const MIN_SECTION: usize = 16;
/// A section boundary must stand this far above the track's typical change between
/// four-bar spans...
const PEAK_OVER_TYPICAL: f32 = 1.6;
/// ...and change the bands by at least this much on average (natural log units: 0.25 is
/// about 1 dB per band).
const MIN_CHANGE: f32 = 0.25;

/// One section of a track.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Section {
    /// Index of its first beat in the beat map.
    pub beat: usize,
    /// Its loudness against the loudest section, dB (0 for the loudest, negative below).
    pub level: f32,
}

/// The bars and sections of a track.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Structure {
    /// Beat `i` starts a bar when `i % BAR == bar`.
    pub bar: usize,
    /// In order; the first starts at the first downbeat.
    pub sections: Vec<Section>,
}

/// Where a beat sits in the song.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Place {
    /// 0 on the downbeat.
    pub beat_in_bar: usize,
    /// Bars since the section started (0 in its first bar).
    pub bar_in_section: usize,
    /// This beat starts a section.
    pub section_start: bool,
    /// This section's level and the one before it, dB against the loudest section.
    pub level: f32,
    pub prev_level: f32,
}

impl Structure {
    /// Bars and sections from per-beat band fingerprints (`feats[i]` covers beat `i`
    /// to beat `i + 1`) and per-beat power (linear, any scale).
    pub fn find(feats: &[[f32; BANDS]], power: &[f32]) -> Structure {
        let n = feats.len();
        if n < 2 * MIN_SECTION {
            return Structure {
                bar: 0,
                sections: vec![Section {
                    beat: 0,
                    level: 0.0,
                }],
            };
        }
        let bar_change = novelty(feats, BAR);
        let two_bars = novelty(feats, 2 * BAR);
        let phase_score = |p: usize| -> f32 {
            (p..n)
                .step_by(BAR)
                .map(|i| bar_change[i] + two_bars[i])
                .sum::<f32>()
                / (n - p).div_ceil(BAR) as f32
        };
        let bar = (0..BAR)
            .max_by(|&a, &b| phase_score(a).total_cmp(&phase_score(b)))
            .unwrap_or(0);

        // Section starts: four bars against four bars, at downbeats only.
        let span = novelty(feats, MIN_SECTION);
        let downbeats: Vec<usize> = (bar..n).step_by(BAR).collect();
        let mut typical: Vec<f32> = downbeats
            .iter()
            .map(|&i| span[i])
            .filter(|&v| v > 0.0)
            .collect();
        typical.sort_by(f32::total_cmp);
        let typical = typical.get(typical.len() / 2).copied().unwrap_or(0.0);
        let threshold = (typical * PEAK_OVER_TYPICAL).max(MIN_CHANGE);
        let mut starts: Vec<usize> = Vec::new();
        for &i in &downbeats {
            let v = span[i];
            // The biggest change within half a section either side.
            let lo = i.saturating_sub(MIN_SECTION / 2);
            let hi = (i + MIN_SECTION / 2).min(n - 1);
            let biggest = downbeats
                .iter()
                .filter(|&&j| j >= lo && j <= hi && j != i)
                .all(|&j| span[j] < v || (span[j] == v && j > i));
            if v >= threshold && biggest {
                starts.push(i);
            }
        }
        // Keep sections at least MIN_SECTION long: of two close starts, the stronger wins.
        let mut kept: Vec<usize> = vec![bar];
        for s in starts {
            let last = *kept.last().unwrap_or(&bar);
            if s < last + MIN_SECTION {
                if kept.len() > 1 && span[s] > span[last] {
                    kept.pop();
                    kept.push(s);
                }
                continue;
            }
            if n - s >= MIN_SECTION / 2 {
                kept.push(s);
            }
        }
        // Each section's level: mean power, in dB against the loudest section.
        let db: Vec<f32> = kept
            .iter()
            .enumerate()
            .map(|(k, &s)| {
                let e = kept.get(k + 1).copied().unwrap_or(n);
                let p = power[s..e].iter().sum::<f32>() / (e - s).max(1) as f32;
                10.0 * p.max(1e-12).log10()
            })
            .collect();
        let top = db.iter().copied().fold(f32::MIN, f32::max);
        Structure {
            bar,
            sections: kept
                .iter()
                .zip(&db)
                .map(|(&beat, &d)| Section {
                    beat,
                    level: d - top,
                })
                .collect(),
        }
    }

    /// Where beat `i` sits.
    pub fn place(&self, i: usize) -> Place {
        let k = self.sections.partition_point(|s| s.beat <= i);
        let (start, level, prev_level) = match k {
            0 => (0, 0.0, 0.0),
            _ => {
                let s = self.sections[k - 1];
                let prev = if k >= 2 {
                    self.sections[k - 2].level
                } else {
                    s.level
                };
                (s.beat, s.level, prev)
            }
        };
        Place {
            beat_in_bar: (i + BAR - self.bar % BAR) % BAR,
            bar_in_section: i.saturating_sub(start) / BAR,
            section_start: k > 0 && i == start,
            level,
            prev_level,
        }
    }

    /// Compact text for storage: "bar|beat:level,beat:level…" (levels in whole dB).
    pub fn encode(&self) -> String {
        let s: Vec<String> = self
            .sections
            .iter()
            .map(|s| format!("{}:{}", s.beat, s.level.round() as i32))
            .collect();
        format!("{}|{}", self.bar, s.join(","))
    }

    pub fn decode(text: &str) -> Option<Structure> {
        let (bar, rest) = text.split_once('|')?;
        let sections = rest
            .split(',')
            .filter(|s| !s.is_empty())
            .map(|s| {
                let (b, l) = s.split_once(':')?;
                Some(Section {
                    beat: b.parse().ok()?,
                    level: l.parse().ok()?,
                })
            })
            .collect::<Option<Vec<_>>>()?;
        Some(Structure {
            bar: bar.parse().ok()?,
            sections,
        })
    }
}

/// How much the fingerprint changes at each beat: the distance between the mean over the
/// `w` beats before and the `w` after, per band (root mean square). Zero within `w` of
/// either end.
pub fn novelty(feats: &[[f32; BANDS]], w: usize) -> Vec<f32> {
    let n = feats.len();
    let mut prefix = vec![[0.0f64; BANDS]; n + 1];
    for i in 0..n {
        for b in 0..BANDS {
            prefix[i + 1][b] = prefix[i][b] + feats[i][b] as f64;
        }
    }
    (0..n)
        .map(|i| {
            if i < w || i + w > n {
                return 0.0;
            }
            let d: f64 = (0..BANDS)
                .map(|b| {
                    let before = (prefix[i][b] - prefix[i - w][b]) / w as f64;
                    let after = (prefix[i + w][b] - prefix[i][b]) / w as f64;
                    (after - before).powi(2)
                })
                .sum();
            (d / BANDS as f64).sqrt() as f32
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A made-up song: `bars` of four beats; each bar's fingerprint from `bar_feat`, with
    /// a small wobble every beat so nothing is perfectly flat.
    fn song(
        bars: usize,
        offset: usize,
        bar_feat: impl Fn(usize) -> [f32; BANDS],
    ) -> Vec<[f32; BANDS]> {
        let n = bars * BAR + offset;
        (0..n)
            .map(|i| {
                // Beats before the first downbeat belong to a pickup bar.
                let bar = (i + BAR - offset) / BAR;
                let mut f = bar_feat(bar);
                f[i % BANDS] += 0.05 * ((i * 7 % 5) as f32 - 2.0);
                f
            })
            .collect()
    }

    fn power(feats: &[[f32; BANDS]]) -> Vec<f32> {
        feats
            .iter()
            .map(|f| f.iter().map(|v| v.exp()).sum())
            .collect()
    }

    /// Chords change every bar (the bands shift a little); the song has an intro (8
    /// bars), a loud drop (16), a quiet breakdown (8) and the drop again (16).
    fn shaped(bar: usize) -> [f32; BANDS] {
        let mut f = [-4.0; BANDS];
        let chord = (bar % 4) as f32 * 0.3;
        f[2] += chord;
        f[3] -= chord;
        let (level, highs) = match bar {
            1..=8 => (-1.5, 0.0),
            9..=24 => (0.0, 1.5),
            25..=32 => (-3.0, -1.0),
            _ => (0.0, 1.5),
        };
        f.iter_mut().for_each(|v| *v += level);
        f[6] += highs;
        f[7] += highs;
        f
    }

    #[test]
    fn finds_the_downbeat_after_a_pickup() {
        for offset in 0..BAR {
            let feats = song(48, offset, shaped);
            let s = Structure::find(&feats, &power(&feats));
            assert_eq!(s.bar, offset, "pickup of {offset} beats");
        }
    }

    #[test]
    fn finds_sections_on_their_downbeats() {
        let offset = 2;
        let feats = song(49, offset, shaped);
        let s = Structure::find(&feats, &power(&feats));
        let starts: Vec<usize> = s.sections.iter().map(|s| s.beat).collect();
        // Bar b (1-based) starts at beat offset + (b - 1) * 4.
        let at = |b: usize| offset + (b - 1) * BAR;
        assert_eq!(starts, [at(1), at(9), at(25), at(33)], "{s:?}");
        let levels: Vec<f32> = s.sections.iter().map(|s| s.level).collect();
        assert!(levels[1] > -0.5 && levels[3] > -0.5, "{levels:?}");
        assert!(levels[2] < levels[0] && levels[0] < levels[1], "{levels:?}");
        let p = s.place(at(25));
        assert!(p.section_start && p.beat_in_bar == 0 && p.level < p.prev_level);
        let p = s.place(at(26) + 1);
        assert_eq!((p.beat_in_bar, p.bar_in_section), (1, 1));
        assert!(!p.section_start);
    }

    #[test]
    fn a_steady_track_is_one_section() {
        let feats = song(40, 0, |bar| {
            let mut f = [-4.0; BANDS];
            f[2] += (bar % 4) as f32 * 0.3;
            f
        });
        let s = Structure::find(&feats, &power(&feats));
        assert_eq!(s.sections.len(), 1, "{s:?}");
    }

    #[test]
    fn stores_compactly_and_reads_back() {
        let s = Structure {
            bar: 3,
            sections: vec![
                Section {
                    beat: 3,
                    level: -4.0,
                },
                Section {
                    beat: 67,
                    level: 0.0,
                },
            ],
        };
        let text = s.encode();
        assert_eq!(text, "3|3:-4,67:0");
        assert_eq!(Structure::decode(&text), Some(s));
    }
}
