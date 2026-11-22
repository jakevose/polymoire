# Polymoire

A music player and generative visualizer for Windows, macOS and Linux. It plays the music
in a folder on your computer, with every beat mapped before the song starts, or moves to
whatever else your computer is playing. The picture drifts between looks one or two traits
at a time (palette, field, symmetry, texture, effect, warp, scale, tempo, spin, chroma,
mosaic).

**Website, screenshots and downloads: https://jakevose.github.io/polymoire/**

## Using it

- **System audio.** Polymoire listens to your computer's sound output, so any player
  (Spotify, a browser tab, a DAW) drives it. Nothing is recorded or saved.
- **Media player.** Switch the audio source to *Media player* (or press `L`) to play mp3
  and m4a files from a music folder instead. Only one source runs at a time: the media
  player turns system-audio listening off. See [Media player](#media-player).
- **Demo beat.** A built-in 120 bpm house loop; toggle it with the button or `D`.
- **Changes on the beat.** With system audio, one or two traits change every two or
  four bars' worth of beats; a drop or a change of mood brings a bigger change. With the
  media player, changes follow the song's sections (see below).
- **Every trait gets a turn.** Automatic changes walk through all the traits in a
  shuffled order, one or two at a time, before any changes again, and never change the
  same trait twice in a row. The one exception keeps the picture from going flat: if the
  look has drifted into a couple of similar colors or soft, featureless blobs, the trait
  most to blame changes next.
- **When the music stops, the picture comes to rest.** After a second of silence (or
  sound too faint to count, like the tail of a fade), all motion eases to a stop over
  two seconds and waits. When sound comes back, it picks up again within half a
  second. A faint but steady noise floor keeps a slow drift instead.
- **Change a trait** with the ◀ ▶ arrows on its row; **pin it** with the dot on the right
  so it stops changing on its own (the arrows still work on pinned traits).
- **Energy** sets how lively the picture is even without music: Calm, Lively, Restless,
  Electric.
- Keys: `Space` pause · `N` shift now · `H` hide controls · `F` fullscreen · `D` demo beat
  · `L` library.
  Double-click the picture for fullscreen; click it to bring hidden controls back.
- **Reduce motion** slows everything and damps the beat reactions.
- **Loudness sets the strength.** How loud the music is right now, compared with the
  loudest it has been in the last minute, scales every beat reaction: quiet intros and
  breakdowns move gently (down to 0.3x), steady loud sections run at about 1.1x and
  sudden swells and drops hit hardest (up to 1.25x).

### Media player

No account or setup:

1. Click **Media player** (or press `L`).
2. Pick the folder your music lives in: browse from your home folder, or type or paste a
   path (`~` means your home folder). Any folder this computer can see works, including
   an external drive, a network share, or a folder a cloud app (Dropbox, OneDrive,
   Google Drive) keeps in sync. Polymoire lists every mp3 and m4a under it. On macOS,
   the first time you choose a folder in Documents, Desktop, Downloads or on another
   drive, macOS asks whether Polymoire may read it.
3. Click **Use this folder**.
4. Browse by artist, album, genre, decade, tempo, folder, playlist, recently added or
   most played, or search. Double-click to play; right-click a track for *Play next*,
   *Add to queue* and playlists. A collaboration ("Skrillex/Bladee", "A feat. B") is
   listed under each artist, and genre spellings like *Hip-Hop* and *Hip Hop* are
   merged. Until a file's tags are read, its artist, album and year are guessed from
   folder names such as `Artist - Album (2018) [FLAC]`.

How it behaves:

- **Beat-mapped ahead of time.** Before a track plays (and while the one before it is
  playing), Polymoire reads it and maps every beat in the whole track, so the
  visuals land on the beat from the first bar, through breakdowns, syncopation and
  tempo changes. The map is saved (about a byte per beat), so each track is analyzed
  once. A track that hasn't been mapped yet waits for its map, with a progress bar:
  about a third of a second for a song on a recent computer. Long mixes start after
  two seconds with live beat tracking and switch to the map when it's done.
- **The look follows the song.** The map also finds where bars start and where the song
  changes section (intro, drop, breakdown). A new section brings a new scene (pattern,
  colors, symmetry): quickly into a drop, slowly into a breakdown. In between, one
  detail varies every four bars (eight in quieter sections), always on the first beat
  of a bar. A quiet intro reacts gently from its first beat, because the map knows how
  loud the song will get.
- **Even volume.** Each track's loudness is measured when it's analyzed for beats, and
  it plays at the same level as the others: loud masters come down, quiet ones come up
  only as far as they can without clipping. Before a track is measured, ReplayGain tags
  are used if it has them.
- **A still picture between songs.** When the track changes, the picture holds still
  until the next one starts making sound.
- **Quick to keep up to date.** Opening the library (or clicking **Rescan**) walks the
  folder, usually in a few seconds even for a big library; tracks appear as each
  folder is read. Files that moved or were renamed keep their tags, beat map and play
  count. If part of the folder can't be read (an unplugged drive), nothing is removed.
  Tags are read in the background from the start of each file, and everything is
  cached on your computer.
- **A broken file can't crash it.** If a file won't decode, or makes the decoder
  crash, Polymoire skips it and lists it under **Problem files** (right-click: show
  it in Finder, try again, or hide it). It never deletes anything. If the app ever
  quits unexpectedly twice while reading the same file, that file goes on the list
  too. Your music files can't run code in Polymoire: its decoder is written in safe
  Rust.
- **One write, ever.** Polymoire only reads your music. The one file it writes is
  `polymoire-library.json` in your music folder, a backup of its catalog (tags, beat
  maps, play counts, playlists), updated at most every 10 minutes and when you quit.
  Another computer that sees the same folder (a shared drive, or one a cloud app syncs)
  picks up from that backup.

### Comfort: kinetic without motion sickness or flashing

Kicks push the patterns' own motion: each one swells them forward, with the warp and
wobble, and they coast until the next. Only real kicks do this (not bass notes, hats or
the predicted beat), so the picture pulses with the drums and rests in breakdowns.
Most movement is local: the image wobbles in many directions at once ("jelly"), shapes
re-form in eased steps on the beat ("re-form"), and motes orbit inside their own small
cells. Whole-view zooms and spins happen only at **half the tempo** (every other beat):
the view eases in by at most 8% and rocks at most 4° to alternating sides, never
spinning on. **Reduce motion** turns the camera moves off. Brightness may rise at most
10% and changes no faster than 60% per second, so nothing flashes. Tests in
`src/reactions.rs` enforce all of this.

### System audio by platform

| Platform | How | Notes |
| --- | --- | --- |
| Windows 10/11 | WASAPI loopback of the default output | Works out of the box. |
| macOS 14.6+ | Core Audio process tap | First run asks for **System Audio Recording** permission (System Settings → Privacy & Security). Only an app bundle can ask, so run `Polymoire.app` (`mise run app` in development), not the bare binary. Older macOS runs the visuals and demo beat without system audio. |
| Linux | PulseAudio / PipeWire monitor of the default output | Needs `libpulse` (present on standard desktops with PulseAudio or PipeWire). |

## Development

Tooling is managed with [mise](https://mise.jdx.dev):

```sh
mise install        # pinned Rust toolchain
mise run dev        # run a debug build
mise run run        # run a release build
mise run test       # automated tests
mise run lint       # rustfmt + clippy
mise run ci         # everything CI runs
mise run smoke      # launch for 20 s on the demo beat, save target/smoke.ppm, print a report
mise run app        # macOS: build and open Polymoire.app (needed for system audio)
mise run app-smoke  # macOS: 25 s smoke test of the app bundle against system audio
mise run package    # installers for this platform → target/dist
```

`scripts/dev-runner.sh` lets an assistant that can only write files in this folder run the
tasks above on your machine: start it in a terminal, and it runs `mise run <task>` for each
allowlisted task name written to `.runner/request`, logging to `.runner/log`.

Linux needs these system packages to build:

```sh
sudo apt-get install libasound2-dev libpulse-dev libxkbcommon-dev libwayland-dev \
  libx11-dev libxcursor-dev libxrandr-dev libxi-dev libgl1-mesa-dev
```

### Layout

| Path | What |
| --- | --- |
| `src/traits.rs` | Trait catalog, weighted picks, crossfades, the one-or-two-at-a-time scheduler |
| `src/analysis.rs` | Band levels, sustained bass, kick/snare/hat hits, rhythm character |
| `src/tempo.rs` | Tempo and beat phase by autocorrelation of drum onsets |
| `src/vibe.rs` | Intensity, heaviness and mood from the sound; biases trait picks |
| `src/demo.rs` | Demo beat synth |
| `src/reactions.rs` | Audio → light/color modifiers, drop detection |
| `src/quality.rs` | Adaptive render resolution for slow GPUs |
| `src/uniforms.rs`, `src/shader.wgsl` | GPU uniform block and the scene/blit shaders |
| `src/app/` | Window, renderer, HUD, audio devices, system capture |
| `tests/` | Shader validation + layout, demo-beat tempo tracking end to end |
| `examples/` | `pulse_probe` (measures how well reactions pulse), `screenshots` (renders the website's screenshots headlessly) |
| `site/`, `scripts/build-site.py` | The website: template, styles, screenshots, and the dependency risk register |

The library (`src/lib.rs`) has no window or audio-device code, so the whole test suite
runs headless in CI on all three platforms.

### Why Rust + egui + wgpu

- One native binary of a few MB per platform, no browser engine, starts instantly.
- wgpu picks Metal, Direct3D 12, Vulkan or OpenGL, so the shader runs on old integrated
  GPUs too. The scene renders offscreen at an adaptive resolution (35–100%) and is scaled
  up, which keeps low-end machines smooth.
- cpal gives system-audio loopback on Windows and macOS; Linux uses the PulseAudio monitor.

## Website

The site at https://jakevose.github.io/polymoire/ is generated by `scripts/build-site.py`
and deployed by `.github/workflows/pages.yml` on every push to `main`, after every release
(so download links follow the newest installers) and weekly (fresh advisory scan).

- `mise run site` builds it into `target/site`; `mise run site-check` (part of `mise run ci`
  and CI) fails if a dependency has no entry in `site/dependencies.toml` or a screenshot
  is missing.
- `mise run site-serve` builds it and serves it at http://127.0.0.1:8765/ to look at
  locally (`PORT=9000 mise run site-serve` for another port). Rerun it after changes.
- `mise run screenshots` re-renders the screenshots from the demo loop (needs a GPU and
  Pillow). Rerun it when the visuals change noticeably.

## Releases

Push a tag like `v0.1.0`. The **Release** workflow builds:

- Windows x64 installer (`.exe`, NSIS)
- macOS arm64 and x64 (`.dmg`)
- Linux x64 and arm64 (`.deb`, `.AppImage`)

and attaches them to a GitHub release. Run the workflow by hand (Actions → Release → Run
workflow) to build everything as downloadable artifacts without publishing.

The builds aren't signed with a paid developer certificate, so each OS asks once:

- **macOS:** open the `.dmg` and drag Polymoire to Applications. The first launch is
  blocked: go to System Settings → Privacy & Security, click **Open Anyway** next to
  Polymoire, then confirm. (On macOS 14 and older, right-click the app → Open.)
- **Windows:** SmartScreen shows "Windows protected your PC": click **More info → Run
  anyway**.
- **Linux:** install the `.deb` (`sudo apt install ./polymoire_*.deb`), or make the
  `.AppImage` executable (`chmod +x`) and run it.
