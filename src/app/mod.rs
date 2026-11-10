//! The window: drives the trait engine, audio reactions and renderer each frame.

mod audio;
mod capture;
mod hud;
mod render;

use audio::{AudioEngine, AudioFrame};
use driftfield::quality::Quality;
use driftfield::reactions::{is_drop, FrameInput, Pulse, Sound, Visual};
use driftfield::rng::Rng;
use driftfield::traits::{Key, Traits};
use driftfield::uniforms::Uniforms;
use driftfield::vibe::{self, Vibe};
use eframe::egui;
use eframe::egui_wgpu;
use std::time::Instant;

pub fn run() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Driftfield")
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([480.0, 360.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Driftfield",
        options,
        Box::new(|cc| Ok(Box::new(DriftApp::new(cc)))),
    )
}

pub struct DriftApp {
    traits: Traits,
    pulse: Pulse,
    quality: Quality,
    audio: AudioEngine,
    last_audio: AudioFrame,
    visual: Visual,
    sim: f64,
    flow_t: f64,
    ang: f64,
    next_at: f64,
    beats_to_shift: u32,
    last_drop: f64,
    pub vibe: Vibe,
    last_frame: Instant,
    /// Sim time each trait last started changing, for the HUD flash.
    flashed: Vec<f64>,
    pub paused: bool,
    pub hud_visible: bool,
    pub reduced_motion: bool,
    pub fullscreen: bool,
    smoke: Option<Smoke>,
}

/// Self-test mode (`DRIFTFIELD_SMOKE_SECS`): run for a while, save a screenshot, print a
/// report and quit. Used by `mise run smoke`.
struct Smoke {
    secs: f64,
    started: Instant,
    frames: u64,
    beats: u64,
    shifts: u64,
    max_dt: f64,
    shot_requested: bool,
    shot_saved: Option<String>,
}

impl DriftApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let rs = cc
            .wgpu_render_state
            .as_ref()
            .expect("Driftfield needs the wgpu renderer");
        render::init(rs);
        cc.egui_ctx.set_visuals(egui::Visuals::dark());
        let traits = Traits::new(Rng::from_time());
        let n = traits.defs.len();
        let smoke = std::env::var("DRIFTFIELD_SMOKE_SECS")
            .ok()
            .and_then(|v| v.parse::<f64>().ok())
            .map(|secs| Smoke {
                secs,
                started: Instant::now(),
                frames: 0,
                beats: 0,
                shifts: 0,
                max_dt: 0.0,
                shot_requested: false,
                shot_saved: None,
            });
        let mut audio = AudioEngine::new();
        if std::env::var("DRIFTFIELD_DEMO").is_ok_and(|v| v == "1") {
            audio.set_demo(true);
        }
        DriftApp {
            traits,
            pulse: Pulse::default(),
            quality: Quality::default(),
            audio,
            last_audio: AudioFrame::default(),
            visual: Visual::NEUTRAL,
            sim: 0.0,
            flow_t: 0.0,
            ang: 0.0,
            next_at: 3.0,
            beats_to_shift: 8,
            last_drop: -99.0,
            vibe: Vibe::default(),
            last_frame: Instant::now(),
            flashed: vec![-99.0; n],
            paused: false,
            hud_visible: true,
            reduced_motion: false,
            fullscreen: false,
            smoke,
        }
    }

    fn smoke_frame(&mut self, ctx: &egui::Context, dt: f64) {
        let beat = self.last_audio.beat;
        let Some(s) = self.smoke.as_mut() else { return };
        s.frames += 1;
        s.beats += beat as u64;
        if s.frames > 30 {
            s.max_dt = s.max_dt.max(dt);
        }
        let elapsed = s.started.elapsed().as_secs_f64();
        let events = ctx.input(|i| i.raw.events.clone());
        for e in events {
            if let egui::Event::Screenshot { image, .. } = e {
                let dir = std::env::var("DRIFTFIELD_SMOKE_DIR").unwrap_or_else(|_| "target".into());
                let path = std::path::Path::new(&dir).join("smoke.ppm");
                let mut out =
                    format!("P6\n{} {}\n255\n", image.size[0], image.size[1]).into_bytes();
                for p in &image.pixels {
                    out.extend_from_slice(&[p.r(), p.g(), p.b()]);
                }
                if std::fs::write(&path, out).is_ok() {
                    s.shot_saved = Some(path.display().to_string());
                }
            }
        }
        if elapsed > s.secs - 1.5 && !s.shot_requested {
            s.shot_requested = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
        }
        if elapsed > s.secs {
            let levels = self.last_audio.levels;
            println!("SMOKE REPORT");
            println!("frames: {}", s.frames);
            println!("avg_fps: {:.1}", s.frames as f64 / elapsed);
            println!("worst_frame_ms: {:.1}", s.max_dt * 1000.0);
            println!("render_scale: {:.2}", self.quality.scale());
            println!("audio_status: {}", self.audio.status(levels.present));
            println!("demo_on: {}", self.audio.demo_on());
            println!("system_audio_ok: {}", self.audio.system_ok());
            let (n, peak, sr) = self.audio.system_stats();
            println!("system_samples: {n} at {sr} Hz, loudest_rms: {peak:.4}");
            println!("bpm: {:.1}", levels.bpm);
            println!("vibe: {}", self.vibe.describe());
            println!(
                "intensity: {:.2}, heaviness: {:.2}, bass_sustain: {:.2}",
                self.vibe.intensity, self.vibe.heaviness, levels.bass_sustain
            );
            let r = self.last_audio.rhythm;
            println!(
                "rhythm: density {:.1}/s, syncopation {:.2}, triplet {:.2}, dotted {:.2}, poly {:.2}",
                r.density, r.syncopation, r.triplet, r.dotted, r.poly
            );
            let [k, sn, h] = self.last_audio.hit_counts;
            println!("hits: kick {k}, snare {sn}, hats {h}");
            println!("beats_seen: {}", s.beats);
            println!("gate: {:.2}", self.pulse.gate);
            println!("trait_shifts: {}", s.shifts);
            println!(
                "screenshot: {}",
                s.shot_saved.clone().unwrap_or_else(|| "none".into())
            );
            for i in 0..self.traits.defs.len() {
                println!(
                    "trait {}: {}",
                    self.traits.defs[i].label,
                    self.traits.describe(i, true)
                );
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            self.smoke = None;
        }
    }

    fn shift(&mut self, n: Option<usize>, dur: Option<f64>) {
        let active = self.pulse.active();
        // While music drives the picture, its vibe steers what gets picked.
        let (mood, poly, triplet) = (self.vibe.mood, self.vibe.poly, self.vibe.triplet);
        let changed = self.traits.shift_with(
            self.sim,
            n,
            dur,
            active,
            self.reduced_motion,
            &|key, name| {
                if active {
                    vibe::bias(mood, poly, triplet, key, name)
                } else {
                    1.0
                }
            },
        );
        for i in changed {
            self.flashed[i] = self.sim;
            if let Some(s) = self.smoke.as_mut() {
                s.shifts += 1;
            }
        }
    }

    /// User clicked an arrow: step trait `i` to its previous or next option.
    pub fn step_trait(&mut self, i: usize, dir: i32) {
        self.traits.step_option(i, dir, self.sim, 1.2);
        self.flashed[i] = self.sim;
    }

    /// Shift now: on the next beat grid when music drives timing, otherwise immediately.
    pub fn force_shift(&mut self) {
        self.paused = false;
        if self.pulse.active() {
            let p = self.last_audio.period;
            self.shift(None, Some(p * 4.0));
        } else {
            self.next_at = self.sim;
        }
    }

    fn step(&mut self, dt: f64) {
        self.quality.frame(dt as f32);
        let a = self.audio.frame();
        let weights = self.traits.reaction(self.sim);
        let feats = self.vibe.features(dt as f32, &a.levels, &a.rhythm, a.live);
        let mood_changed = self.vibe.update(dt as f32, &feats, &a.rhythm);
        // The music's intensity lifts the Energy trait; it never lowers it.
        let trait_energy = self.traits.scalar(Key::Energy, self.sim);
        let energy =
            trait_energy + (self.vibe.intensity - trait_energy).max(0.0) * self.pulse.gate * 0.7;
        let sound = Sound {
            bass_sustain: a.levels.bass_sustain,
            intensity: self.vibe.intensity,
            heaviness: self.vibe.heaviness,
            poly: self.vibe.poly,
            syncopation: self.vibe.syncopation,
            snare_rate: a.rhythm.snare_rate,
        };
        self.visual = self.pulse.frame(FrameInput {
            // Pausing freezes the picture, including its local motion.
            dt: if self.paused { 0.0 } else { dt as f32 },
            levels: &a.levels,
            live: a.live,
            beat: a.beat && !self.paused,
            weights: &weights,
            energy,
            period: a.period as f32,
            hits: if self.paused {
                Default::default()
            } else {
                a.hits
            },
            sound,
            reduced_motion: self.reduced_motion,
        });
        let active = self.pulse.active();
        let motion = if self.reduced_motion { 0.35 } else { 1.0 };

        if !self.paused {
            self.sim += dt;
            let tempo = self.traits.scalar(Key::Tempo, self.sim) as f64 * motion;
            self.flow_t += dt * (tempo + self.pulse.flow_speed_add(self.reduced_motion) as f64);
            self.ang += dt * self.traits.scalar(Key::Spin, self.sim) as f64 * motion;

            if active {
                self.next_at = self.next_at.max(self.sim + 4.0);
                if a.beat {
                    self.beats_to_shift = self.beats_to_shift.saturating_sub(1);
                    if self.beats_to_shift == 0 {
                        let beats = self.traits.beat_fade();
                        self.shift(None, Some(a.period * beats));
                        self.beats_to_shift = self.traits.beats_until_next();
                    }
                }
                // A drop or a change of mood brings a bigger scene change.
                if (is_drop(&a.levels) || mood_changed) && self.sim - self.last_drop > 8.0 {
                    self.last_drop = self.sim;
                    self.shift(Some(2), Some(a.period * 4.0));
                }
            } else if self.sim >= self.next_at {
                self.shift(None, None);
                self.next_at = self.sim + self.traits.next_hold(self.reduced_motion);
            }
        }
        self.traits.settle(self.sim);
        self.last_audio = a;
    }

    fn uniforms(&self, size: [u32; 2], logical_w: f32) -> Uniforms {
        let t = &self.traits;
        let sim = self.sim;
        let v = self.visual;
        let (pa, pb, pm) = t.discrete(Key::Field, sim);
        let (ta, tb, tm) = t.discrete(Key::Texture, sim);
        let (fa, fb, fm) = t.discrete(Key::Effect, sim);
        let (sa, sb, sm) = t.scalar_pair(Key::Symmetry, sim);
        let mut u = Uniforms {
            res: [size[0] as f32, size[1] as f32],
            px: size[0] as f32 / logical_w.max(1.0),
            t: self.flow_t as f32,
            clock: sim as f32,
            ang: self.ang as f32 + v.ang_off,
            pat_a: pa as f32,
            pat_b: pb as f32,
            pat_m: pm,
            sym_a: sa,
            sym_b: sb,
            sym_m: sm,
            tex_a: ta as f32,
            tex_b: tb as f32,
            tex_m: tm,
            fx_a: fa as f32,
            fx_b: fb as f32,
            fx_m: fm,
            warp: t.scalar(Key::Warp, sim) + v.warp_add,
            scale: t.scalar(Key::Scale, sim),
            chroma: t.scalar(Key::Chroma, sim) + v.chroma_add,
            mosaic: t.scalar(Key::Mosaic, sim),
            zoom: v.zoom,
            expo: v.expo,
            hue: v.hue,
            spark: v.spark,
            vig: v.vig,
            jelly: v.jelly,
            jelly_t: v.jelly_t,
            morph_a: v.morph_a,
            morph_b: v.morph_b,
            morph_m: v.morph_m,
            morph_amp: v.morph_amp,
            motes: v.motes,
            mote_t: v.mote_t,
            mote_size: v.mote_size,
            ..Default::default()
        };
        u.set_palette(&t.palette(sim));
        u
    }

    fn handle_keys(&mut self, ctx: &egui::Context) {
        let (space, n, h, f, d, esc) = ctx.input(|i| {
            (
                i.key_pressed(egui::Key::Space),
                i.key_pressed(egui::Key::N),
                i.key_pressed(egui::Key::H),
                i.key_pressed(egui::Key::F),
                i.key_pressed(egui::Key::D),
                i.key_pressed(egui::Key::Escape),
            )
        });
        if space {
            self.paused = !self.paused;
        }
        if n {
            self.force_shift();
        }
        if h {
            self.hud_visible = !self.hud_visible;
        }
        if d {
            let on = !self.audio.demo_on();
            self.audio.set_demo(on);
        }
        if f || (esc && self.fullscreen) {
            self.set_fullscreen(ctx, !self.fullscreen);
        }
    }

    pub fn set_fullscreen(&mut self, ctx: &egui::Context, on: bool) {
        self.fullscreen = on;
        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(on));
    }
}

impl eframe::App for DriftApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let now = Instant::now();
        let dt = now.duration_since(self.last_frame).as_secs_f64().min(0.1);
        self.last_frame = now;
        self.handle_keys(ctx);
        self.step(dt);
        self.smoke_frame(ctx, dt);
        // ui() requests the next frame while the window is visible. When it's hidden or
        // minimized, only logic runs, so tick slowly instead of spinning the CPU.
        ctx.request_repaint_after(std::time::Duration::from_millis(50));
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let rect = ui.max_rect();
        let ppp = ctx.pixels_per_point();
        let size = self
            .quality
            .render_size(rect.width() * ppp, rect.height() * ppp);
        let uniforms = self.uniforms(size, rect.width());
        ui.painter().add(egui_wgpu::Callback::new_paint_callback(
            rect,
            render::SceneCallback { uniforms, size },
        ));
        let bg = ui.interact(rect, ui.id().with("scene"), egui::Sense::click());
        if bg.clicked() && !self.hud_visible {
            self.hud_visible = true;
        }
        if bg.double_clicked() {
            self.set_fullscreen(&ctx, !self.fullscreen);
        }
        hud::show(&ctx, self);
        ctx.request_repaint();
    }
}
