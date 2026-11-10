# Polymoire

A generative visualizer for Windows, macOS and Linux. It drifts between looks one or two
traits at a time (palette, field, symmetry, texture, effect, warp, scale, tempo, spin,
chroma, mosaic) and moves to whatever your computer is playing.

## Using it

- **System audio.** Polymoire listens to your computer's sound output, so any player
  (Spotify, a browser tab, a DAW) drives it. Nothing is recorded or saved.
- **Demo beat.** A built-in 120 bpm house loop; toggle it with the button or `D`.
- **Change a trait** with the ◀ ▶ arrows on its row; **pin it** with the dot on the right
  so it stops changing on its own (the arrows still work on pinned traits).
- **Energy** sets how lively the picture is even without music: Calm, Lively, Restless,
  Electric.
- Keys: `Space` pause · `N` shift now · `H` hide controls · `F` fullscreen · `D` demo beat.
  Double-click the picture for fullscreen; click it to bring hidden controls back.
- **Reduce motion** slows everything and damps the beat reactions.
- **Loudness sets the strength.** How loud the music is right now, compared with the
  loudest it has been in the last minute, scales every beat reaction: quiet intros and
  breakdowns move gently (down to 0.3x), steady loud sections run at about 1.1x and
  sudden swells and drops hit hardest (up to 1.25x).

### Comfort: kinetic without motion sickness or flashing

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

The library (`src/lib.rs`) has no window or audio-device code, so the whole test suite
runs headless in CI on all three platforms.

### Why Rust + egui + wgpu

- One native binary of a few MB per platform, no browser engine, starts instantly.
- wgpu picks Metal, Direct3D 12, Vulkan or OpenGL, so the shader runs on old integrated
  GPUs too. The scene renders offscreen at an adaptive resolution (35–100%) and is scaled
  up, which keeps low-end machines smooth.
- cpal gives system-audio loopback on Windows and macOS; Linux uses the PulseAudio monitor.

## Releases

Push a tag like `v0.1.0`. The **Release** workflow builds:

- Windows x64 installer (`.exe`, NSIS)
- macOS arm64 and x64 (`.dmg`)
- Linux x64 and arm64 (`.deb`, `.AppImage`)

and attaches them to a GitHub release. The builds are unsigned: on macOS, right-click →
Open the first time (or sign and notarize with an Apple Developer ID); on Windows,
SmartScreen asks for confirmation.
