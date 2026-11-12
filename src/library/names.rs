//! Tidying names for browsing: artist credits split into artists, genre spellings
//! merged. Tags are kept as the files have them; this only decides where a track is
//! listed.

use std::collections::HashSet;

/// Words that start the featured part of a credit ("A feat. B").
const FEATURING: [&str; 7] = [
    " feat. ",
    " feat ",
    " ft. ",
    " ft ",
    " featuring ",
    "(feat",
    "(ft",
];
/// Separators between artists. Also used inside names ("Simon & Garfunkel",
/// "AC/DC"), so a main credit is only split when its first part is an artist in the
/// library in its own right.
const JOINERS: [&str; 8] = ["/", ";", ", ", " & ", " and ", " x ", " vs. ", " vs "];

/// Splits artist credits using the library's own artists as the guide.
pub struct Credits {
    known: HashSet<String>,
}

impl Credits {
    /// `artists`: every artist and album-artist credit in the library, as tagged.
    pub fn new<'a>(artists: impl Iterator<Item = &'a str>) -> Self {
        Credits {
            known: artists.map(|a| a.trim().to_lowercase()).collect(),
        }
    }

    /// The artists in a credit, main artist first, no duplicates.
    pub fn split(&self, credit: &str) -> Vec<String> {
        let lower = credit.to_lowercase();
        let (main, featured) = match FEATURING.iter().filter_map(|f| lower.find(f)).min() {
            Some(i) => {
                let rest = &credit[i..];
                // Drop the marker word itself.
                let rest = rest
                    .trim_start_matches(['(', ' '])
                    .split_once([' ', '.'])
                    .map_or("", |(_, r)| r);
                (&credit[..i], rest.trim_end_matches(')'))
            }
            None => (credit, ""),
        };
        let mut out: Vec<String> = Vec::new();
        let parts = split_all(main);
        let first_known = parts
            .first()
            .is_some_and(|p| self.known.contains(&p.to_lowercase()));
        if parts.len() > 1 && first_known {
            out.extend(parts);
        } else if !main.trim().is_empty() {
            out.push(main.trim().to_string());
        }
        // A featured list is always a list.
        out.extend(split_all(featured));
        let mut seen = HashSet::new();
        out.retain(|a| seen.insert(a.to_lowercase()));
        out
    }
}

fn split_all(s: &str) -> Vec<String> {
    let mut s = s.to_string();
    for j in JOINERS {
        s = replace_ignore_case(&s, j, "\u{1f}");
    }
    s.split('\u{1f}')
        .map(|p| {
            p.trim()
                .trim_start_matches("& ")
                .trim_start_matches("and ")
                .trim()
                .to_string()
        })
        .filter(|p| !p.is_empty())
        .collect()
}

fn replace_ignore_case(s: &str, from: &str, to: &str) -> String {
    let lower = s.to_lowercase();
    // Lowercasing can change byte lengths outside ASCII; only then fall back to exact.
    if lower.len() != s.len() {
        return s.replace(from, to);
    }
    let mut out = String::new();
    let mut last = 0;
    for (i, _) in lower.match_indices(from) {
        if i >= last {
            out.push_str(&s[last..i]);
            out.push_str(to);
            last = i + from.len();
        }
    }
    out.push_str(&s[last..]);
    out
}

/// The genres in a genre tag ("Rap/Hip Hop" is two), each as (key, label). Spellings
/// that differ only in case, spacing, punctuation or a trailing "music" share a key.
pub fn genres(tag: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for g in tag.split(['/', ';', ',']) {
        let label = g.trim();
        let mut key: String = label
            .to_lowercase()
            .chars()
            .filter(|c| c.is_alphanumeric())
            .collect();
        if let Some(k) = key.strip_suffix("music").filter(|k| !k.is_empty()) {
            key = k.to_string();
        }
        let key = match key.as_str() {
            "drumnbass" | "drumbass" | "dnb" | "drumandbass" => "drumandbass".to_string(),
            "rnb" | "rb" | "rhythmandblues" => "rnb".to_string(),
            "hiphoprap" | "raphiphop" => "hiphop".to_string(),
            _ => key,
        };
        if !key.is_empty() && !out.iter().any(|(k, _)| *k == key) {
            out.push((key, label.to_string()));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn credits() -> Credits {
        Credits::new(
            [
                "Skrillex",
                "Calibre",
                "Congo Natty",
                "Joe Goddard",
                "Simon & Garfunkel",
                "AC/DC",
                "Earth, Wind & Fire",
            ]
            .into_iter(),
        )
    }

    #[test]
    fn collaborations_split_into_known_artists() {
        let c = credits();
        assert_eq!(c.split("Skrillex/Bladee"), ["Skrillex", "Bladee"]);
        assert_eq!(
            c.split("Skrillex, PinkPantheress, & Trippie Redd"),
            ["Skrillex", "PinkPantheress", "Trippie Redd"]
        );
        assert_eq!(c.split("Calibre & Cimone"), ["Calibre", "Cimone"]);
        assert_eq!(
            c.split("Congo Natty feat. Rebel MC & Sista Mary"),
            ["Congo Natty", "Rebel MC", "Sista Mary"]
        );
        assert_eq!(
            c.split("Joe Goddard (feat. Barrie)"),
            ["Joe Goddard", "Barrie"]
        );
        assert_eq!(c.split("$uicideboy$"), ["$uicideboy$"]);
    }

    #[test]
    fn band_names_with_joiners_stay_whole() {
        let c = credits();
        assert_eq!(c.split("Simon & Garfunkel"), ["Simon & Garfunkel"]);
        assert_eq!(c.split("AC/DC"), ["AC/DC"]);
        assert_eq!(c.split("Earth, Wind & Fire"), ["Earth, Wind & Fire"]);
        assert_eq!(
            c.split("Crosby, Stills, Nash & Young"),
            ["Crosby, Stills, Nash & Young"]
        );
        // "Feat" inside a word isn't a featuring credit.
        assert_eq!(c.split("Featurecast"), ["Featurecast"]);
    }

    #[test]
    fn genre_spellings_merge() {
        let keys = |s: &str| genres(s).into_iter().map(|(k, _)| k).collect::<Vec<_>>();
        assert_eq!(keys("Hip-Hop"), keys("Hip Hop"));
        assert_eq!(keys("Rap/Hip Hop"), ["rap", "hiphop"]);
        assert_eq!(keys("World Music"), keys("world"));
        assert_eq!(keys("Drum & Bass"), keys("Drum n Bass"));
        assert_eq!(keys("Electronic"), ["electronic"]);
        assert_ne!(keys("Electronica"), keys("Electronic"));
        assert!(keys("").is_empty());
        assert_eq!(genres("Hip-Hop")[0].1, "Hip-Hop");
    }
}
