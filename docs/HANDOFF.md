# Handoff notes

What a new session needs to pick up the work: where things stand, why things are the way
they are, and what is still open. The code and git history say *what*; this file says
*why* and *what's next*. Keep it current (see `CLAUDE.md`).

Last updated: 2026-11-22 (v0.3.0: local folder, map wait, problem files).

## Where things stand

- **Repo:** `github.com/jakevose/polymoire` (private). It was renamed from `driftfield` on
  2026-11-10, and the old URL redirects. Work is committed straight to `main`; push only
  when asked. CI (`.github/workflows/ci.yml`) runs `mise run ci` on Linux, Windows and
  macOS and takes about 20 minutes.
- **Current focus: making the picture feel tied to the music.** The path so far:
  1. The user said it "doesn't feel responsive". Measured: the visual changes were
     tiny (warp added at most 0.03 on a 0–1.05 trait), arrived 50–170 ms late, and
     flattened out on compressed masters.
  2. Added per-band "punch" (each band against its own average, instant attack).
     The user found it **"too twitchy and less pulse-y"**, so it was removed.
  3. Kick detection fixed for loud masters; a single **kick-driven thump** drives the
     surge, warp and wobble.
  4. The user asked for **"more direct" beat tracking**. Measured against true kick
     times, the visual peaks landed 67–83 ms after the kick. Now the beat clock leads
     (see Decisions) and peaks land at 33–50 ms, before display latency, which
     `BEAT_LEAD` offsets. The user tested it locally and approved it; it shipped as
     v0.1.1.
  5. On `media-player`, with whole-track beat maps: the look follows the song's
     sections and phrases, and reaction strength knows how loud the song gets (see
     Decisions). The user said "I like where we're at" and released it as v0.2.0.
- **Media player: a local folder (since 2026-11-21, released in v0.3.0).** v0.2.0 shipped it
  on Dropbox (the user's "counterintuitive direction": a player *inside* the
  visualizer, with a catalog, playlists and beat maps made ahead). The user then
  dropped Dropbox: **its rate limits made it impractical** for a big library. The
  player now points at a folder on this computer (any path: external drive, network
  share, or a folder a cloud app syncs, which is how Dropbox users carry on). Scanning,
  tags, beat maps, loudness, catalog, views, playlists and the backup file all work as
  before. The app has **no network code at all** now (ureq, keyring, sha2, getrandom
  removed), and the site says so (its copy says "Version 0.3 dropped it").
- **Trait changes** now go through a shuffle bag (every trait changes once per round,
  never twice in a row). The user asked about "effects"; this assumes they meant the
  trait rows, not the options within the Effect trait. Not confirmed.

## Releases

- **v0.3.0 released 2026-11-22**: a local music folder instead of Dropbox (no network
  code left), waiting up to 2 s for a new track's beat map with a progress bar, and
  Problem files (decoder crashes caught, crash journal). The user tried it on their
  16k-track thumb drive and said it "felt great".
- **v0.2.1 released 2026-11-14**: Dropbox sign-in moved to the system keychain, a
  Dropbox setup guide on the site and in the README, a 4-box hero.
- **v0.2.0 released 2026-11-14**: the media player (Dropbox library, whole-track beat
  maps, loudness leveling, song-structure pacing) and the silence halt. A minor bump
  because it adds a whole feature.
- The release workflow no longer reads `POLYMOIRE_DROPBOX_KEY`. If the user ever set
  that repo secret, it can be deleted. The user wants anything they set kept as a
  secret (repo secrets, not variables).
- **v0.1.2 released 2026-11-11** (PR #1): settles when the music stops; fewer flat looks.
- **v0.1.1 released 2026-11-10** with the more direct beat tracking (v0.1.0, the first
  release, came out the same day). Releases: github.com/jakevose/polymoire/releases.
  For the next release, bump `version` in `Cargo.toml`, then tag `vX.Y.Z` on that commit
  and push the tag. Run the workflow by hand first if packaging changed.
- `.github/workflows/release.yml` builds Linux x64 and arm64 (`.deb`, `.AppImage`),
  Windows x64 (NSIS `.exe`) and macOS arm64 and x64 (`.dmg`). A `v*` tag publishes a
  GitHub release; running it by hand (workflow_dispatch) builds artifacts only.
- **macOS signing:** cargo-packager leaves the app unsigned unless given an Apple
  identity, and Apple Silicon calls an unsigned download "damaged". The workflow signs
  it ad hoc (`codesign --deep -s -`, no hardened runtime, matching `scripts/mac-app.sh`),
  then builds the DMG with `hdiutil`. Don't switch to `signing-identity = "-"`:
  cargo-packager would add `--options runtime` (the hardened runtime), which blocks
  audio capture unless extra entitlements are declared.
- **No paid certificates**, so users must allow the app once on each OS (steps are in
  the README's Releases section). Real Developer ID signing and notarization need the
  `APPLE_*` secrets that cargo-packager reads.
- **Icons:** `packaging/icons/`, generated by `scripts/make-icons.py`. The AppImage
  format needs a square PNG, and the first dry run failed without one.

## Website

- **Hero:** exactly four feature boxes in a fixed grid (4 across, 2×2 under 980 px, 1
  column on phones). Five boxes in an auto-fit grid wrapped 3 + 2 and looked awkward
  (user). Keep it at four, or change the grid with it.
- **`#player`:** what the media player does, the one write, and a note for Dropbox
  sign-in users. v0.2.x apps link to `#dropbox`, so that id stays (on the note's
  heading).

- https://jakevose.github.io/polymoire/ is built by `scripts/build-site.py` (standard
  library only) and deployed by `.github/workflows/pages.yml`: on pushes to `main`, after
  Release runs (`workflow_run`, because a release made by the workflow token can't trigger
  other workflows), and weekly for the advisory scan.
- **Generated, not hand-written:**
  - download links: the latest release's assets, matched by name pattern in `PLATFORMS`;
  - dependency versions and tree sizes: `Cargo.lock` and `cargo tree` per desktop target;
  - the no-networking claim: checked against `NETWORK_CRATES` on every build;
  - advisories: `cargo audit`;
  - the test count;
  - screenshot captions: `site/screenshots/shots.json`.
- **Risk register:** `site/dependencies.toml`. `site-check` (in CI) enforces that it
  matches `Cargo.toml` exactly.
- **Screenshots:** `examples/screenshots.rs` renders the scene pass headlessly (wgpu, no
  window) while simulating the demo loop through the real analyzer and reactions. It
  captures 3 frames after a kick. It shares `Uniforms::for_frame` with the app, so frames
  match what the app draws. Needs a GPU, so it runs locally, not in CI; the results are
  committed as WebP.
- **Link previews** (Slack and social sites) use `site/og.jpg`: a 1200×630 JPEG cropped by
  `scripts/compress-screenshots.py`, referenced by absolute URL. Previewers ignore
  relative URLs and often WebP; the first version had both problems and showed a blank
  image.
- `tokio` appears in `Cargo.lock` only through `wasm-bindgen-futures` (web builds), not
  in desktop builds.

## Map

| File | Role |
|---|---|
| `src/analysis.rs` | Per-sample analyzer: band levels, kick/snare/hat hits, rhythm stats, beat clock |
| `src/tempo.rs` | Autocorrelation tempo and beat phase |
| `src/vibe.rs` | Intensity, heaviness, mood, `level` (loudness against the last minute) |
| `src/reactions.rs` | `Pulse::frame`: audio → per-frame `Visual` modifiers. Comfort rules live here |
| `src/traits.rs` | Trait catalog, crossfades, shuffle-bag scheduling, variety guard (`flatness`) |
| `src/director.rs` | *When* shifts happen: with a song map, by section and phrase; with live music, on the beat; a slow drift without it; `Halt` stops everything in silence |
| `src/structure.rs` | A mapped track's bars (downbeat phase) and sections (start beat, level) from per-beat band fingerprints |
| `src/loudness.rs` | BS.1770 integrated loudness and peak, measured in the beat-map decode |
| `src/shader.wgsl` / `src/uniforms.rs` | Rendering; the uniform layout is checked against the WGSL by `tests/shader.rs` |
| `src/demo.rs` | 120 bpm house loop. **The only ground truth for the analysis tests** |
| `src/app/` | Window, audio devices, HUD, frame loop (`mod.rs::step`) |
| `src/beatmap.rs` | Offline whole-track beat mapping (onset bands → tempo → DP beats → refine) |
| `src/library/` | `catalog` (model, views, playlists, move detection), `folder` (reads, picker listing, the one write), `scan` (walks the folder), `media` (Symphonia), `store` (app data files) |
| `src/app/player.rs` | Queue, output stream that decodes + resamples in the callback, background read/analysis |
| `src/app/library.rs` / `library_ui.rs` | Folder choice, scan, tag crawl, cache + backup / folder picker, library window, now-playing bar |
| `examples/library_probe.rs` | Tag coverage and browse-view grouping of a saved catalog (the user's `library.json`) |
| `examples/beatmap_probe.rs` | Beat-map local files; `--write-demo` + `--demo-grid` score encoded demo files |
| `examples/pulse_probe.rs` | Measures kick accuracy and how well each visual signal pulses |
| `examples/watch_probe.rs` | Plays local files through the whole chain (map, vibe, director, traits) and measures pacing: changes/min, bar alignment, sections met, time mid-change, reaction gain per section (`--log`) |
| `examples/look_probe.rs` | Renders the scheduler's looks on the GPU and measures how often they're flat |
| `examples/common/headless.rs` | Headless scene rendering shared by `screenshots` and `look_probe` |
| `tests/music_stops.rs` | Demo, then the audio stops three ways; the picture must settle |

The frame loop: `AudioEngine::frame` polls beats and hits at the time the listener is
hearing, then `Vibe` and `Pulse::frame` turn them into a `Visual`, and `uniforms()` sends
it to the shader. The flow clock `flow_t` advances by the Tempo trait plus
`Pulse::flow_speed_add` (this is where the surge acts).

## Decisions and why

- **Comfort rules (the user's hard requirement, enforced by tests in `reactions.rs`).**
  - Whole-view zoom and sway happen only at half tempo, with small eased caps.
  - Brightness rises at most 10% and changes at most 60% per second (no flashing).
  - Reduce motion turns the camera moves off.
  - Responsiveness has to come from *local* motion: the flow clock, warp, jelly, motes.
  - A pattern-scale pulse on each kick was rejected because it's effectively a
    per-beat whole-view zoom.
- **Kicks are judged against the track's own kicks.** On loud masters the bass line rose
  almost as sharply as the kick: 61 false kicks per 40 s of demo, plus 16 real ones lost
  to the cooldown. So a low-band onset is:
  - judged at its peak, about 15 ms in;
  - compared with `kick_ref`, the track's recent kick strength;
  - required to reach 0.7× on the beat grid, or 0.85× off it or alongside a snare
    (the snare case also has an absolute floor);
  - not allowed to start the cooldown if rejected.

  Real kicks score about 0.9–1.0 and bass notes and snare bodies about 0.5–0.7, so
  there is margin. Guarded by `tests/demo_tempo.rs`.
- **The beat clock leads the thump, confirmed by kicks.** Detection can't report a kick
  until ~33 ms after it: the onset window plus a 3-hop peak check. Shortening that
  check to 2 hops lost half the kicks on loud masters, so it stays at 3. The beat
  clock is phase-accurate to within a frame, so `Pulse` fires the thump on the beat
  when a kick confirmed the previous beat (`KICK_MATCH` window; a kick just ahead of
  the beat also counts). A breakdown therefore gets one led thump, then stops. The
  app also polls beats `BEAT_LEAD` (25 ms) early to offset the frame→screen delay.
  Hits are polled at the true time, since they can't be early.
- **Thump, not punch.** Only detected kicks set the thump: not hats, stabs or bass notes,
  and not the predicted beat grid, so breakdowns stay still. Per-band punch reacted to
  everything, which read as twitchy.
- **The loudness gain (`loudness_gain`)** scales every music-driven effect by how loud it
  is now against the last minute: 0.3× in quiet passages up to 1.25× on swells. The
  comfort caps apply after it.
- **Shuffle bag for traits.** The old scheduler sorted by "least recently changed" plus
  jitter, which could repeat a trait. The bag skips pinned, still-fading and (without
  audio) Reaction traits; they keep their place. On refill, whatever just changed is
  kept out of the front for the rest of the current shift plus a full next shift. An
  arrow click counts as that trait's turn.

- **With a song map, the look follows the song (`structure.rs`, `Director::with_map`).**
  Measured on 40 min of local tracks, the old beat-count pacing changed the look 18.7
  times a minute, kept a trait mid-change 48% of the time (two at once 27%), landed on
  a bar line no more than chance, and fired "big" changes on live mood flips (up to 10
  per track). Now:
  - *Structure:* each beat's fingerprint is the mean log energy in the mapper's 8
    bands. The downbeat is the phase (of 4) where the bar-to-bar change is largest.
    Sections start at downbeats where 4 bars after differ from 4 bars before by
    ≥1.6× the track's median and ≥0.25 ln/band, the biggest within ±2 bars, ≥4 bars
    apart. Levels are dB under the loudest section. On real tracks the starts fall on
    8-bar phrase lines (e.g. 15.2/30.5/61.0 s at 126 bpm).
  - *Pacing:* a section start brings a Scene shift (Field, Palette, Symmetry, Effect,
    Scale, Mosaic): 2 traits over 2 beats into a rise of ≥3 dB, 2 over 8 beats into a
    fall, else 1 over 4. A Detail shift (the other traits) every 4 bars, 8 in sections
    >3 dB down; a Scene shift every 16 bars of a long section. Only on downbeats.
    Live drop/mood guesses are ignored. Result: 7.2/min, 100% on bar lines, 95% of
    section starts met by a new scene, mid-change 25% (two at once 4%).
  - *Level:* `Vibe::know_section` raises the loudness reference to what the section's
    dB implies, so a quiet intro reacts gently from the start (−9 dB intro: gain 1.14 →
    0.59) and the drop stands out. It waits 4 bars after a fall (the slow level would
    overstate the peak).
  - Mood is still the live vibe's, so a scene change at a drop is picked with the
    previous section's mood bias (see Open items).
- **Pacing without music is a comfort rule (`src/director.rs`).** With live music:
  shifts on the beat every 8 or 16 beats (was 4 or 8: 17.6/min → 11.3/min, mid-change
  46% → 30%), never two within `MIN_GAP`; a drop or mood change (at most every
  `BIG_GAP`, 16 s) restarts the beat count so it doesn't stack. Pacing counts as music
  only while `is_sounding` (fast level within 10 dB of the slow one): the beat clock
  ticks on after a stop, and the longer count once landed a shift 0.5 s after the
  music ended. Music without a beat drifts on a 7-11 s timer (it
  used to freeze). Without music: hold still `SETTLE` (12 s), then one trait every 12-20 s
  with 6-9 s crossfades, picks biased to the Ambient mood (calm energy, slow tempo).
  Reduce motion doubles the holds. Startup without audio still makes its first change at
  3 s so the app visibly comes alive.
- **Flat looks: fix the causes, then guard the combinations.**
  - Six fields (Smoke, Plasma, Quasicrystal, Ridges, Truchet, Interference) bunched
    around their middle and only ever showed a slice of the palette. `spread()` in the
    shader stretches them to about the spread of the sine-based fields.
  - Ten palettes were near-monochrome or low-contrast in their middle. Their phase and
    amplitude were nudged (base color kept, so each keeps its identity), found by a small
    search against a hue and contrast floor.
  - Riso now prints with two inks; Duotone lifts its light ink on dark palettes.
  - The **variety guard** (`flatness()` table in `traits.rs`): options that flatten
    (two-ink effects, dark or mono palettes, smooth fields, big scales) are fine alone but
    not stacked. Picks that would push the look past `FLAT_BUDGET` are `GUARD_WEIGHT`
    times as likely, and a look already over budget changes its flattest trait next,
    even out of its turn (never twice in a row). That is the one exception to "every trait
    once per round". Without it, a flat look could sit for most of a round (30-60 s).
  - Guarded options get rarer, not gone: Duotone and Ink are on screen about 2% of the
    time (a fair share is ~7%), Riso about 3%.

- **Media player design.**
  - *One source at a time.* Player mode stops system capture (and the demo); the app
    doesn't even start capture at launch in player mode, so no macOS permission prompt.
  - *What it plays is what it analyzes.* The output callback pushes its own mono mix
    (pre-volume, so quiet listening still moves the picture) into a `Feed`, like the demo.
  - *Beats from the map, everything else live.* `AudioEngine::frame(Some(Tap))` replaces
    the beat clock with the track's beat map (same `BEAT_LEAD`); kicks, levels and vibe
    still come from the live analyzer. No map yet → live beat clock.
  - *Beat mapping:* the whole decoded track, never a BPM tag. 8 band-pass onset bands
    at hop 256; a tempo curve (autocorrelation in 8 s windows every 1 s, mild 120 bpm
    prior plus a 0.3 pull toward the whole-track tempo, Viterbi with a cost per squared
    log tempo change); Ellis DP placement using the local period, then sub-frame refinement, and beats
    without a clear onset (breakdowns) spread evenly between heard ones. "Clear" is
    relative to the track's median beat; a fixed threshold let hats break it. Hop 128 was
    tried: kicks got sharper but breakdowns fell apart (DP effectively looser).
  - *Fetching:* the current and next track are read whole (3 kept in memory) and
    analyzed on a thread once.
  - *Beat storage:* ms times as second differences (interval changes), zero runs
    collapsed, zigzag varints, base64url: ~1.2 chars/beat on the demo, a few bytes for a
    perfectly steady track. `StoredBeats.v` = `MAP_VERSION`; older maps are redone on
    next play. One tempo was the old weakness: 128→96 hit only 72% of beats.
  - *Browsing names (`names.rs`):* artist credits split on feat./ft. always, and on
    "/", ", ", " & ", " x " only when the first part is an artist in this library on
    its own (keeps "Simon & Garfunkel", "AC/DC"). Albums without an album artist file
    under the main credit, or "Various Artists" with the compilation flag. Genres split
    on "/;," and merge by alphanumerics, minus a trailing "music", plus a few aliases
    (DnB, R&B). Labels are each group's most common spelling. Views are cached in
    `Library` against a revision bumped on every catalog change (16k tracks: 25 ms).
  - *Path guesses* also handle disc-track file names ("2-05", "1.09", "03-01"),
    numbered set folders ("07 - Show" → artist from the box set above), year-first
    folders ("2014 - Artist - Album"), and leave dates ("5-7-77 Boston") alone. On
    the user's library numeric "artists" went 25 → 2 (both real). Tags read 4 at a time.
  - *Path guesses* read release folder names ("Artist - Album (2018) [FLAC]", scene
    "VA-Album-WEB-FLAC-2022", disc subfolders) and are redone on load for unread files.
    On the user's library: "Unknown artist" 3,524 → 193, "No year" 9,087 → 3,702.
  - *Leveling:* ReplayGain is rare in the user's files (2.3%), so loudness (BS.1770
    integrated, per channel, gated) is measured in the same decode as the beat map
    and stored on the track (`Track.loudness`; tracks without it are re-analyzed on
    play). Played at `TARGET_LUFS` −14, boosts limited to 1 dB under the sample peak,
    ±15 dB. Gain glides over 150 ms when the measurement arrives mid-track. Until
    measured: ReplayGain + 4 dB, cut only. Matches ffmpeg's ebur128 within 0.1 dB on
    mono, stereo and wide-stereo files (an earlier mono-mix version read wide stereo
    3 dB low).
  - *Track change and pause hold:* in player mode the picture freezes (like Pause)
    from a change of track, or a pause of the player, until the track is heard and the
    analyzer reports sound (user request). While held, the frame carries the last
    sound heard (no beats or hits) and the vibe doesn't update: feeding it the pause's
    silence made the resume read as a drop and "go crazy" (user report). Tags crawl 4
    files at a time from the first 256 KB (a second read if the ID3 block is bigger), and the whole file if that fails: m4a with the index at the end, and some mp3s
    Symphonia won't probe from a partial read even with a 55 KB tag (6 of 16,422 on the
    user's drive).
  - *The one write:* `<folder>/polymoire-library.json`, via `Folder::write_backup`
    (temp file, then rename). A test in `folder.rs` counts every file-writing call in
    `folder.rs`, `scan.rs` and `media.rs`. The website promises it. Keep both true.
  - *Catalog:* one JSON file (`VERSION` 2), local cache in the app data folder (saved
    5 s after a change), backed up to the library folder after a scan that changed
    something, every 10 min if changed, and on exit; restored from there when the folder
    has no local cache. Version 1 (Dropbox) catalogs and settings are ignored.
  - *Track identity:* the path inside the library folder, `/`-separated. Relative, so a
    backup still fits where the folder has another path (another computer). The change
    "hash" is size + mtime (hashing would mean reading every file).
  - *Scan:* one thread walks the whole folder each sync (local listing is fast: 15
    files in 0.35 ms; the user's ~16k-track library untested). Skips dot-files (incl.
    macOS `._` files), doesn't follow linked folders (loops). Files go to the catalog
    per folder; tracks not seen are removed only after a clean walk, so an unreadable
    subfolder or unplugged drive removes nothing. The Dropbox design (top-level walk,
    per-folder cursors, resumable scan, change cursor) was dropped with it: a local walk
    is cheap enough to redo.
  - *Moves:* at the end of a scan, a vanished track and a new one with the same size
    and mtime (unique on both sides) are one file moved: it keeps tags, beat map, plays
    and playlist places. Replaces Dropbox's stable file ids.
  - *Problem files (2026-11-22):* release builds now **unwind** on panic (was abort)
    so `media::guarded` can catch a Symphonia panic around tag reads, analysis and
    opening; the audio callback catches one around `Deck::fill`. A decode failure on
    the whole file or a caught crash sets `Track.problem`: out of every view but
    Problem files (shown only when non-empty), never read again until the file
    changes or "Try again". Hard crashes (OOM, abort): `journal.rs` keeps the files
    being decoded in `reading.json` in app data; a launch after an unclean exit gives
    each a strike, two strikes make a problem, and struck files are re-read alone so
    the second strike lands on the right one. Never deletes (one-write promise).
    Fuzzing 600 corrupted mp3/m4a copies: 0 Symphonia panics (it has no `unsafe` in
    the crates used), so this is insurance, not a known hole. Hangs aren't caught.
  - *Waiting for the map:* a track with no map holds (picture still, as between
    tracks) until mapped or `MAP_WAIT` (2 s) after play was asked; the now-playing bar
    shows progress guessed from file size × `secs_per_mb` (starts at 0.1, a running
    average of this machine's read+map time for files over 1 MB). Measured on the
    user's drive (M3 Max): ~87 ms per track-minute, 0.3 s per song, 7 s per 85 min mix.
    The user turned down mapping the whole library in the background: too much
    thumb-drive churn.
  - *Folder picker:* in-app (no native dialog crate), starts at home, with a path box
    (`~` expands). macOS Info.plist carries usage strings for Documents, Desktop,
    Downloads, removable and network volumes.

- **Silence stops the picture (`Halt`, user request 2026-11-12).** After 1 s without
  sound (not live, or below `PRESENCE_RMS`), the frame loop's motion speed eases to 0
  over 2 s (smoothstep) and stays there; sound back for 0.25 s brings it up in 0.5 s.
  The 0.25 s guards against tails hovering at the presence threshold, which flicker
  `present` (no hysteresis in the analyzer) and were the likely "rapid effect" the user
  saw as levels fell toward zero. This supersedes the settle-then-drift idle for real
  silence; a steady noise floor above the threshold still drifts (`music_stops` tests
  still pass since they run the director directly). A player hold counts as sound.
  Side effect: with no audio at launch the picture comes to rest after ~3 s.

## Tuning dials

| Constant | Where | Effect |
|---|---|---|
| `THUMP_DECAY` (0.3 beats) | reactions.rs | How long each kick's pulse lasts. Longer is smoother, shorter is snappier |
| `THUMP_ATTACK` (60/s, ~17 ms) | reactions.rs | How fast the pulse rises. Higher is punchier but risks twitchiness |
| `BEAT_LEAD` (25 ms) | analysis.rs | How early beats fire to offset display delay. Raise it if visuals still trail the sound |
| `KICK_MATCH` (0.1 s) | reactions.rs | How long after a beat a detected kick still confirms it (and lets the next beat lead) |
| `MAX_SURGE` (3.0) | reactions.rs | How hard kicks push the patterns forward |
| warp `0.2 * thump`, jelly `0.08 + 0.18 * w_je + 0.1 * w_pu` | `Pulse::frame` | How strongly kicks warp and wobble the picture |
| `KICK_ON_GRID` / `KICK_OFF_GRID` (0.7 / 0.85) | analysis.rs | Kick strictness. Raise them if bass leaks through; lower them if real kicks are missed |
| `KICK_REF_TAU` (8 s) | analysis.rs | How fast the kick reference forgets (recovery after a quieter section) |
| `SETTLE` (12 s), `IDLE_HOLD` (12-20 s), `IDLE_FADE` (6-9 s) | director.rs | How calm the picture is without music |
| `MIN_GAP` (2 s), `DRIFT_HOLD` (7-11 s) | director.rs | Shortest gap between shifts with music; pacing for beatless music |
| `FLAT_BUDGET` (1.3), `GUARD_WEIGHT` (0.3) | traits.rs | Variety guard. Lower budget or weight means fewer flat looks but rarer two-ink looks |
| `spread()` factors | shader.wgsl | How much of the palette the narrow fields reach |
| `TIGHTNESS` (100), `STRONG` (0.5× median), `HOP` (256) | beatmap.rs | Beat map grid rigidity; what counts as a heard beat; time resolution |
| `TEMPO_WINDOW` (8 s), `TEMPO_STEP` (1 s), `TEMPO_CHANGE` (40), `GLOBAL_PULL` (0.3) | beatmap.rs | Tempo curve: resolution, how readily it changes, how firmly breakdowns hold |
| `TAG_BYTES` (256 KB), `BACKUP_EVERY` (10 min) | app/library.rs | Tag crawl read size; how often the backup is written |
| `TARGET_LUFS` (−14) | app/player.rs | Playback loudness every track is leveled to |
| `MAP_WAIT` (2 s), `SECS_PER_MB` (0.1) | app/player.rs | Longest wait for a new track's beat map; first guess for its progress bar |
| `PHRASE_BARS` (4), `QUIET_PHRASE_BARS` (8), `QUIET_SECTION_DB` (3), `SCENE_BARS` (16), `SECTION_JUMP_DB` (3) | director.rs | Mapped pacing: bars between details, when a section counts as quiet, scene refresh in long sections, what counts as a drop or breakdown |
| `PEAK_OVER_TYPICAL` (1.6), `MIN_CHANGE` (0.25), `MIN_SECTION` (16 beats) | structure.rs | How readily a section boundary is found |
| `BIG_GAP` (16 s), beats per live shift (8 or 16) | director.rs | Live pacing |

Baseline from `cargo run --release --example pulse_probe` at this point. Since the "more
direct" work, the probe measures against the demo's **true kick times** (earlier
baselines used the detected kick and aren't comparable). "Pulse" is how much a signal
rises in the 80 ms after a kick, as a share of its range. "Peak" is the median time
from the kick to the signal's peak, before display latency.

| Signal | Pulse (clean / loud) | Peak | Before (same method) |
|---|---|---|---|
| Kicks found | 64/64, 0 extra (both) | hits arrive 33 ms after | — |
| Beat clock | — | fires 17 ms early (BEAT_LEAD) | 17 ms late |
| thump | 82% / 81% | 33 ms | 67 ms |
| surge | 74% / 73% | 33 ms | 83 ms |
| jelly | 72% / 69% | 50 ms | 83 ms |
| warp_add | 71% / 68% | 33 ms | 83 ms |

Look baseline from `cargo run --release --example look_probe` (1500 looks from the
scheduler, 30 runs). *Flat* means dull (fewer than 2.5 effective hues and contrast < 0.15
or detail < 0.09) or blurry (detail < 0.06), thresholds set by eye on contact sheets.

| | Flat | Dull | Blurry | Median hues |
|---|---|---|---|---|
| v0.1.1 | 23.3% | 18.3% | 13.9% | 3.3 |
| v0.1.2 | 6.3% | 2.5% | 4.5% | 4.6 |

Beat-map baseline from `beatmap_probe --demo-grid` on 60 s of the demo, encoded (beat
error against the true grid):

| File | Median | 95th | Note |
|---|---|---|---|
| mp3 (lame 192k) | 1.7 ms | 3.6 ms | also through the player's 48 kHz resampler: 1.7 ms |
| m4a (ffmpeg / afconvert) | 25 / 50 ms | — | constant: AAC priming isn't trimmed (see Open items) |

Unchanged by the tempo curve (0.06 s analysis per minute). Synthetic tempo changes
(`follows_a_tempo_change`): 112→136, ramp 118→134, 128→96 all 100%, 100→140 98% of
beats within 15 ms.

Pacing baseline from `watch_probe` on the 16 local tracks in `~/TheoTown/music` plus
the demo (40 min, 3 seeds):

| Mode | Changes/min | Median gap | On a bar line | Sections met | Mid-change (2+) |
|---|---|---|---|---|---|
| Before (map beats, beat count) | 18.7 | 3.3 s | 27% | 27/258 | 48% (27%) |
| Media player (structure) | 7.2 | 7.9 s | 100% | 245/258 | 25% (4%) |
| System audio (8/16 beats) | 11.3 | 4.4 s | 42% | 27/258 | 30% (18%) |

## Verifying

- `mise run ci` runs the formatting check, clippy and tests (what CI runs), on the Rust
  version `mise.toml` pins (1.97). The shell's default `cargo` here is 1.98, and its
  clippy missed a `manual_range_contains` lint that 1.97 fails on.
- `cargo run --release --example pulse_probe` measures the pulse. Compare against the
  baseline above when tuning.
- `cargo run --release --example look_probe` measures how often looks are flat (needs a
  GPU). `LOOK_SAVE=<dir>` saves frames to eyeball; add `LOOK_FLAT=1` for flat ones only.
- `mise run smoke` opens a window for 20 s and **plays the demo beat on the speakers**.
- On macOS, system audio needs the app bundle: `mise run app` / `mise run app-smoke`.
- `scripts/dev-runner.sh` lets a sandboxed tool run allowlisted `mise` tasks through
  `.runner/` (gitignored).
- Automated tests only cover the demo plus a soft-clipped copy of it. **Nothing checks
  real music**; the user's own listening is the real test.

## Open items

- **Local media player:** it has run on the user's 16k-track thumb drive (scan, tags,
  playback, backup). Ask how the picker, scan speed and Rescan felt; nothing has been
  measured on screen.
- The "Mapping beats" progress bar and the Problem files view haven't been seen on
  screen yet. Ask how the wait before an unmapped track feels.
- Users of v0.2.x keep a Dropbox refresh token in their keychain ("Polymoire" /
  `dropbox-refresh-token`); the app can no longer delete it. The README says how.
- The player was checked offline (decode, resample, seek) but not through a sound card.
- Ask the user: are the browse views right (Artists, Albums, Genres, Years by decade,
  Tempo bands, Folders, Playlists, Recently added, Most played, Songs)? Album art, an
  "Up next" queue view and drag-to-reorder playlists aren't built.
- AAC priming delay (25–50 ms) isn't trimmed by Symphonia's MP4 reader. Map and playback
  share the timeline, so beats still line up, but seek positions are off by that much.
- A track is read fully into memory before it plays. Locally that's quick; on a slow
  network share a long mix may take a moment.
- The player's output doesn't follow a change of default output device yet.
- Online metadata lookup (user asked 2026-11-12): not built. Read tags were clean
  (7 odd artists in 10,364, all real credits like "1991"; numeric titles real, e.g.
  Sakamoto's *12*); the weird names were path guesses, now fixed. If it's wanted for
  files whose tags are missing or junk: MusicBrainz text search (no key, 1 req/s,
  User-Agent with the repo URL) for files with a usable title; AcoustID fingerprints
  (pure-Rust chromaprint port, needs an app key, whole-file download) for the rest.
  Either breaks the "never goes online" claim on the site: make it opt-in, and update
  the site (its no-networking line is generated from the crate list).
- The website screenshots don't show the player yet (`mise run screenshots`).
- Mapped pacing: ask the user how it feels (calmer, changes at the right moments?).
  Next steps if wanted: a mood per section computed offline (the live mood lags a
  section change, so the new scene is picked with the old mood), and a 3/4 bar.
- Maps from before `MAP_VERSION` 3 have no structure; they're redone on next play.

- Confirm with the user that the silence fix matches what they saw, and that the looks
  feel less flat in practice (the probe's thresholds are a proxy).
- Plasma is still the field most often in the remaining flat looks (soft by nature). If
  it keeps coming up, raise its frequencies.
- Confirm the thump on real tracks, then tune the dials.
- If no kicks are detected (ambient music, or 808 hip-hop where kick and bass merge),
  nothing produces a pulse. Consider a fallback, e.g. low-band onsets when there have
  been no kicks for a few beats.
- `Pulse.kick_raw` is still set by *predicted* beats as well as kicks. It feeds
  brightness and jelly lightly, so the picture still breathes a little in breakdowns.
  Consider moving it to kicks only.
- Idea not yet built: send the band envelopes to the shader as uniforms so each field
  can map bass, mids and highs to its own geometry. This changes `uniforms.rs`, the WGSL
  struct and `tests/shader.rs` together.

## Session log

- **2026-11-10**
  - Measured responsiveness. Tried per-band punch, then replaced it with the kick
    thump.
  - Fixed bass-as-kick false detections on loud masters.
  - Added shuffle-bag trait rotation.
  - Renamed the repo `driftfield` → `polymoire` on GitHub.
  - Added these notes, `CLAUDE.md` and `examples/pulse_probe.rs`.
  - Fixed a CI lint failure: the code had been linted with the wrong toolchain (see
    Verifying).
  - Release prep: added the app icon set, ad-hoc macOS signing plus DMG build, and
    install steps for users. After two Release dry runs (the first failed on the
    missing icon), checked the artifacts (macOS: ad-hoc signed without hardened
    runtime, correct architectures, audio permission text present) and published
    v0.1.0 with 7 installers.
  - Website: headless screenshot tool, site generator with a risk register enforced in
    CI, and a Pages workflow. Made the repo public for it (the user's decision).
  - "More direct" beat tracking: the probe now measures against true kick times. The
    beat clock leads the thump when kicks confirm it, beats fire early to offset
    display delay (`BEAT_LEAD`), and the attacks are faster. Peaks moved from 67–83 ms
    to 33–50 ms after the kick. The user tested and approved it; released v0.1.1.
- **2026-11-11**
  - User reports: rapid shifting after system audio stops; too much time in flat looks.
  - Moved shift pacing into `src/director.rs` with a calm idle drift, `MIN_GAP`, and a
    beatless-music timer; end-to-end test `tests/music_stops.rs`.
  - Built `examples/look_probe.rs`. Fixed narrow fields and palettes, two-ink Riso,
    lifted Duotone, and added the variety guard: flat looks 23% → 6%.
  - Work went to a PR for the user's review (their request) instead of straight to
    `main`; v0.1.2 to be tagged after merge.
  - User approved and merged PR #1; tagged `v0.1.2` on the merge commit.
  - Media player on branch `media-player`: Dropbox (PKCE, read plus one backup write),
    Symphonia decoding, catalog with ten views, playlists, offline beat maps (demo
    encoded to mp3: 1.7 ms median), system-audio/player toggle. Not run against Dropbox.
  - User test: sign-in works; listing folders failed with HTTP 400. Sent the exact JSON
    content type and surfaced Dropbox's error text for every call.
  - The real 400 was a missing app scope (`files.metadata.read`). "Use this folder" is
    now disabled until Dropbox has listed the folder (it had let a failed listing set
    the whole Dropbox as the library).
  - User: sync sat on "Reading the library folder…" with no sense of progress. Added
    `scan.rs`: folder-count progress bar, tracks found, time left, tracks appear as
    folders finish, 4 parallel listings.
  - User asked for scan progress to be checkpointed into the catalog and Dropbox for
    resume. Made the scan resumable (per-folder cursors in the catalog, backup every
    2 min while scanning) and later syncs incremental via a Dropbox change cursor.
- **2026-11-12**
  - Beat maps: tempo curve (Viterbi over windowed tempo) replaces one tempo per
    track; compact versioned storage. User request: freeze the picture between tracks
    until the new one is heard. Looked at tag quality in the user's library.
  - Tags (user said continue): artist-credit splitting, genre merging, compilation
    flag, ReplayGain leveling, release-folder path guesses, cached views,
    `examples/library_probe.rs`.
  - User: loves the new beat tracking; pausing/resuming the player made the picture go
    crazy. The player pause now holds the picture and keeps the analysis state.
  - User: still a rapid effect as the level falls toward zero; asked to fade to a stop
    after a second of silence. Added `Halt` (with threshold-flicker guard). A fade-out
    simulation of the demo showed reactions themselves decay smoothly, so the flicker
    at the presence threshold is the suspect; confirm with the user.
  - User: weird numeric/title-like artists; asked about online lookup. They were path
    guesses (disc-track numbers, numbered box-set folders, year-first folders): fixed,
    25 → 2 (real). Tag reads 4 at a time so real tags replace guesses sooner. Online
    lookup deferred (see Open items).
  - ReplayGain is rare (2.3%); user agreed to measuring loudness during beat-map
    analysis. Added `src/loudness.rs` and leveling to −14 LUFS.
  - User: with better beat tracking, revisit how the picture reacts: engaging with the
    vibe, no weird hard-to-watch sequences. Measured with a new `watch_probe`: the look
    was mid-change half the time. Added song structure to the map (bars, sections),
    section-and-phrase pacing for mapped tracks, section-aware reaction strength, and
    calmer live pacing.
- **2026-11-14**
  - User: "I like where we're at." Bumped to 0.2.0 (the media player is a new feature),
    merged `media-player` into `main`, tagged `v0.2.0`. Release builds now read
    `POLYMOIRE_DROPBOX_KEY` from a repo variable (not set yet). Screenshots regenerated:
    byte-identical, so the site images stand.
  - User: five hero boxes looked awkward; the Dropbox setup had no end-user docs; secrets
    they set must be secured. Merged the hero to four boxes in a fixed grid; wrote the
    Dropbox steps (site `#dropbox`, README, linked from the app); moved the refresh token
    from `settings.json` to the OS keychain (`keyring` 4, pure-Rust D-Bus on Linux; old
    plain-text tokens migrate on first launch); release builds read the app key from a
    repo secret. Released v0.2.1.
  - Added `mise run site-serve` (build, then serve `target/site` on 127.0.0.1:8765) for
    previewing site changes locally.
- **2026-11-21**
  - User: drop Dropbox (rate limits made it impractical), point at a local folder, keep
    the rest of scanning and processing. Replaced `dropbox.rs` with `folder.rs`, rewrote
    `scan.rs` as a local walk, path-based track ids with move detection, an in-app
    folder picker; removed ureq, keyring, sha2, getrandom. Site, README and macOS usage
    strings updated; the site now generates "No networking library" truthfully.
- **2026-11-22**
  - First run on the user's thumb drive (`/Volumes/MPTHREE/Music`, 16,422 tracks, one
    exit, no errors): tags read for 16,416, the backup written (11.6 MB). The 6 left
    failed Symphonia's partial-read probe; the tag crawl now falls back to the whole file.
  - User: mapping is fast enough to do before each track; no background mapping of
    the whole library (thumb-drive churn). Built: wait for the map (2 s cap) with a
    size-based progress bar, and problem files (panic guard, unwind builds, crash
    journal with two strikes, Problem files view). The user asked whether media
    files are a malware risk: not realistically (pure-Rust decoder, no `unsafe`; fuzz
    found no crashes); robustness was the real gap.
  - The user tried the map wait on their library: "felt great". Released v0.3.0.
