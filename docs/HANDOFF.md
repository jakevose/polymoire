# Handoff notes

What a new session needs to pick up the work: where things stand, why things are the way
they are, and what is still open. The code and git history say *what*; this file says
*why* and *what's next*. Keep it current (see `CLAUDE.md`).

Last updated: 2026-11-11 (media player branch).

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
- **v0.1.2 (in review as a PR, not yet tagged)** fixes two user reports:
  1. *"If system audio stops, the app may enter a desperation phase of rapid visualizer
     shifting."* Simulated (`tests/music_stops.rs`): after the music stopped, the old
     timer kept shifting 1-2 traits every 4-8 s with unbiased picks (Electric energy as
     likely as Calm), and a mood or drop shift could land right on top of a beat shift.
     A faint noise floor instead froze the look (gate open, tempo lost, so no beats and no
     timer). The exact burst the user saw wasn't reproduced; this is the closest mechanism
     found. Now `src/director.rs` owns the pacing (see Decisions). **Ask the user whether
     it's fixed on their machine.**
  2. *"Too much time with only a couple of similar colors and a fairly flat, blurry
     scene."* Measured with `examples/look_probe.rs`: 23% of on-screen looks were flat.
     Now 6% (see Decisions and the baseline below).
- **Media player (branch `media-player`, not merged).** The user asked for the
  "counterintuitive direction": a Dropbox mp3/m4a player *inside* the visualizer, with a
  catalog, playlists and beat maps made ahead of playback. They want a "low golf score"
  first pass they'll refine, kept modular. The user has signed in to a real Dropbox
  account; the folder listing then failed with a bare "http status: 400". Fixed blind
  (no account here): JSON calls now send exactly `application/json` (ureq adds
  `; charset=utf-8`), and every error shows Dropbox's own explanation. **Ask whether
  the folder list works now; if not, the new message says why.** The surfaced error
  showed the real cause: the user's Dropbox app lacked the `files.metadata.read` scope
  (the content type wasn't the problem). Scopes are fixed when the token is granted, so
  enabling one in the App Console also needs Disconnect then Connect.
- **Trait changes** now go through a shuffle bag (every trait changes once per round,
  never twice in a row). The user asked about "effects"; this assumes they meant the
  trait rows, not the options within the Effect trait. Not confirmed.

## Releases

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
| `src/director.rs` | *When* shifts happen: on the beat with music, a slow drift without it |
| `src/shader.wgsl` / `src/uniforms.rs` | Rendering; the uniform layout is checked against the WGSL by `tests/shader.rs` |
| `src/demo.rs` | 120 bpm house loop. **The only ground truth for the analysis tests** |
| `src/app/` | Window, audio devices, HUD, frame loop (`mod.rs::step`) |
| `src/beatmap.rs` | Offline whole-track beat mapping (onset bands → tempo → DP beats → refine) |
| `src/library/` | `catalog` (model, views, playlists), `dropbox` (API, the one write), `scan` (resumable full scan, then changes only), `media` (Symphonia), `store` (app data files) |
| `src/app/player.rs` | Queue, output stream that decodes + resamples in the callback, background fetch/analysis |
| `src/app/library.rs` / `library_ui.rs` | Sign-in, folder choice, sync, tag crawl, cache + backup / library window, now-playing bar |
| `examples/beatmap_probe.rs` | Beat-map local files; `--write-demo` + `--demo-grid` score encoded demo files |
| `examples/pulse_probe.rs` | Measures kick accuracy and how well each visual signal pulses |
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

- **Pacing without music is a comfort rule (`src/director.rs`).** With music: shifts on
  the beat every 4 or 8 beats, never two within `MIN_GAP`; a drop or mood change restarts
  the beat count so it doesn't stack. Music without a beat drifts on a 7-11 s timer (it
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
  - *Fetching:* the current and next track are downloaded whole (3 kept in memory) and
    analyzed on a thread once.
  - *Beat storage:* ms times as second differences (interval changes), zero runs
    collapsed, zigzag varints, base64url: ~1.2 chars/beat on the demo, a few bytes for a
    perfectly steady track. `StoredBeats.v` = `MAP_VERSION`; older maps are redone on
    next play. One tempo was the old weakness: 128→96 hit only 72% of beats.
  - *Track change hold:* in player mode the picture freezes (like Pause) from a change
    of track until the new track is heard and the analyzer reports sound (user request). Tags crawl one file
    at a time from the first 256 KB (a second request if the ID3 block is bigger).
  - *The one Dropbox write:* scopes can't restrict writes to a file, so the app asks for
    `files.content.write` and `dropbox.rs` enforces it; a test scans that file for every
    `files/…` endpoint. The website now says so. Keep both true.
  - *Catalog:* one JSON file, local cache in the app data folder (saved 5 s after a
    change), backed up to `<root>/polymoire-library.json` every 10 min and on exit, and
    restored from there when a folder has no local cache. Track identity is the Dropbox
    file id (survives moves); a changed content hash drops tags and beat map.
  - *Sync = resumable full scan, then changes only (`scan.rs`).* Dropbox gives no folder
    size, so the scan walks the top levels (one request per folder) until it has 24+
    folders or is 3 deep, then lists each whole on 4 threads; "folders done of total"
    drives the bar and the time-left guess. Everything goes to the UI thread as ordered
    `Step`s and lands in `Catalog::scan` (per-folder cursors, walk depth, pass number)
    together with the files, so every catalog save is a checkpoint. Local save 5 s
    after a change; Dropbox backup every 2 min during a scan (10 min otherwise). A
    resumed scan reuses the saved depth so the folders line up.
  - *Pruning:* tracks carry the full-scan `pass` that saw them; only the end of a full
    scan removes unseen ones. Before listing anything the scan takes a recursive
    `get_latest_cursor`, so later syncs `list_folder/continue` from it and fetch only
    changes (applied in order; deleted tracks wait in `scan.gone` until the sync ends,
    so a move keeps tags, beats and plays). An expired cursor (`409 reset`) starts a
    new full scan. Retries rate limits and dropped connections, not refusals.
  - *Sign-in:* PKCE with a pasted code (no loopback server). Refresh token stored in
    plain text in `settings.json`.

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

- **Media player, untested pieces:** sign-in and folder listing work for the user;
  the scan on a big library, downloads, Range reads and backup upload/restore haven't
  been seen yet. Folder-count progress is uneven if one folder holds most of the
  library (e.g. one giant "Compilations"). The player was checked offline
  (decode, resample, seek) but not through a sound card. Ask the user to try it.
- Ask the user: are the browse views right (Artists, Albums, Genres, Years by decade,
  Tempo bands, Folders, Playlists, Recently added, Most played, Songs)? Album art, an
  "Up next" queue view and drag-to-reorder playlists aren't built.
- AAC priming delay (25–50 ms) isn't trimmed by Symphonia's MP4 reader. Map and playback
  share the timeline, so beats still line up, but seek positions are off by that much.
- m4a files with the index at the end (ffmpeg's default) can't give tags from a partial
  read; they keep path-guessed tags until played. A suffix Range read could fix it.
- A track downloads fully before it plays (a second or two; long mixes longer).
  Progressive streaming would need a growing buffer as the media source.
- The player's output doesn't follow a change of default output device yet.
- Refresh token is plain text in the app data folder; move it to the OS keychain.
- Tags, from the user's library (16,422 mp3s, 9,105 tag-read so far): title, artist,
  album, track 100% when read; album artist 75%, year 80%, genre 68% (163 genres with
  near-duplicates: Hip-Hop / Rap/Hip Hop, World / World Music). 113 artists split
  across collaboration credits ("Skrillex/Bladee", "feat."). Proposed: split artist
  credits so a track lists under each artist, fold genre spellings, read the
  compilation flag and ReplayGain. Waiting on the user.
- The website screenshots don't show the player yet (`mise run screenshots`).

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
