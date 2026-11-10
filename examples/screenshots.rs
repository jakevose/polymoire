//! Renders the website's screenshots headlessly: no window, no sound card, no speakers.
//!
//! Each shot pins every trait, plays the demo loop through the same analyzer, vibe and
//! reactions the app uses, and captures the frame just after a kick (while the pulse is
//! up). The scene pass is rendered on the GPU exactly as in the app, then saved as PNG.
//!
//! Run with `mise run screenshots` (or `cargo run --release --example screenshots`).
//! Writes `site/screenshots/*.png` and `site/screenshots/shots.json` (the trait settings
//! of each shot, which the site generator shows as captions). Needs a GPU, so it runs on
//! a developer machine, not in CI.

use eframe::egui_wgpu::wgpu;
use polymoire::analysis::Analyzer;
use polymoire::demo::DemoSynth;
use polymoire::reactions::{FrameInput, Pulse, Sound};
use polymoire::rng::Rng;
use polymoire::traits::{Key, Traits};
use polymoire::uniforms::Uniforms;
use polymoire::vibe::Vibe;
use polymoire::SHADER_WGSL;
use std::path::Path;

const SR: f32 = 48_000.0;
const FPS: f32 = 60.0;
const W: u32 = 1600;
const H: u32 = 1000;
/// Logical width the shot is "displayed" at: sets the size of grain, dots and motes.
const LOGICAL_W: f32 = 1000.0;

struct Shot {
    file: &'static str,
    title: &'static str,
    /// Seconds of the demo to play before capturing (on the next kick after this).
    at: f32,
    traits: [(Key, &'static str); 13],
}

const SHOTS: [Shot; 5] = [
    Shot {
        file: "vapor-petals",
        title: "Vapor petals",
        at: 6.0,
        traits: [
            (Key::Palette, "Vapor"),
            (Key::Field, "Petals"),
            (Key::Symmetry, "12-fold"),
            (Key::Texture, "Clean"),
            (Key::Effect, "Iridescent"),
            (Key::Energy, "Lively"),
            (Key::Reaction, "Pump"),
            (Key::Warp, "Ripple"),
            (Key::Scale, "Mid"),
            (Key::Tempo, "Flow"),
            (Key::Spin, "Slow"),
            (Key::Chroma, "Prism"),
            (Key::Mosaic, "Off"),
        ],
    },
    Shot {
        file: "ember-smoke",
        title: "Ember smoke",
        at: 9.0,
        traits: [
            (Key::Palette, "Ember"),
            (Key::Field, "Smoke"),
            (Key::Symmetry, "Free"),
            (Key::Texture, "Grain"),
            (Key::Effect, "None"),
            (Key::Energy, "Restless"),
            (Key::Reaction, "Jelly"),
            (Key::Warp, "Melt"),
            (Key::Scale, "Wide"),
            (Key::Tempo, "Surge"),
            (Key::Spin, "Still"),
            (Key::Chroma, "Clean"),
            (Key::Mosaic, "Off"),
        ],
    },
    Shot {
        file: "glacier-cells",
        title: "Glacier cells",
        at: 7.0,
        traits: [
            (Key::Palette, "Glacier"),
            (Key::Field, "Cells"),
            (Key::Symmetry, "6-fold"),
            (Key::Texture, "Halftone"),
            (Key::Effect, "Chrome"),
            (Key::Energy, "Calm"),
            (Key::Reaction, "Breathe"),
            (Key::Warp, "Calm"),
            (Key::Scale, "Close"),
            (Key::Tempo, "Drift"),
            (Key::Spin, "Counter"),
            (Key::Chroma, "Clean"),
            (Key::Mosaic, "Off"),
        ],
    },
    Shot {
        file: "ultraviolet-spiral",
        title: "Ultraviolet spiral",
        at: 10.0,
        traits: [
            (Key::Palette, "Ultraviolet"),
            (Key::Field, "Spiral"),
            (Key::Symmetry, "Free"),
            (Key::Texture, "Scanline"),
            (Key::Effect, "Neon"),
            (Key::Energy, "Electric"),
            (Key::Reaction, "Shimmer"),
            (Key::Warp, "Fold"),
            (Key::Scale, "Vast"),
            (Key::Tempo, "Flow"),
            (Key::Spin, "Slow"),
            (Key::Chroma, "Split"),
            (Key::Mosaic, "Off"),
        ],
    },
    Shot {
        file: "coral-truchet",
        title: "Coral truchet",
        at: 8.0,
        traits: [
            (Key::Palette, "Coral"),
            (Key::Field, "Truchet"),
            (Key::Symmetry, "Quad mirror"),
            (Key::Texture, "Dither"),
            (Key::Effect, "Duotone"),
            (Key::Energy, "Lively"),
            (Key::Reaction, "Bop"),
            (Key::Warp, "Calm"),
            (Key::Scale, "Close"),
            (Key::Tempo, "Flow"),
            (Key::Spin, "Still"),
            (Key::Chroma, "Clean"),
            (Key::Mosaic, "Off"),
        ],
    },
];

/// Pins trait `key` to the option called `name`.
fn pin(t: &mut Traits, key: Key, name: &str) {
    let i = t.index_of(key);
    let k = t.defs[i]
        .opts
        .iter()
        .position(|o| o.name == name)
        .unwrap_or_else(|| panic!("{:?} has no option {name:?}", key));
    t.state[i].from = k;
    t.state[i].to = k;
    t.state[i].locked = true;
}

/// Plays the demo with the shot's traits and returns the uniforms of the captured frame.
fn simulate(shot: &Shot) -> Uniforms {
    let mut traits = Traits::new(Rng::new(1));
    for &(key, name) in &shot.traits {
        pin(&mut traits, key, name);
    }
    let mut synth = DemoSynth::new(SR);
    let mut an = Analyzer::new(SR);
    let mut vibe = Vibe::default();
    let mut pulse = Pulse::default();
    let mut buf = vec![0.0; (SR / FPS) as usize];
    let dt = 1.0 / FPS;
    let (mut sim, mut flow_t, mut ang) = (0.0f64, 0.0f64, 0.0f64);
    let mut since_kick: Option<u32> = None;
    // Mirrors PolymoireApp::step (src/app/mod.rs), minus trait shifts and pausing.
    loop {
        synth.render(&mut buf);
        an.push(&buf);
        let heard = an.time();
        let beat = an.poll_beat(heard);
        let hits = an.poll_hits(heard);
        let (l, r) = (an.levels(), an.rhythm());
        let feats = vibe.features(dt, &l, &r, true);
        vibe.update(dt, &feats, &r);
        let trait_energy = traits.scalar(Key::Energy, sim);
        let energy = trait_energy + (vibe.intensity - trait_energy).max(0.0) * pulse.gate * 0.7;
        let visual = pulse.frame(FrameInput {
            dt,
            levels: &l,
            live: true,
            beat,
            weights: &traits.reaction(sim),
            energy,
            period: an.period() as f32,
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
        sim += dt as f64;
        let tempo = traits.scalar(Key::Tempo, sim) as f64;
        flow_t += dt as f64 * (tempo + pulse.flow_speed_add(false) as f64);
        ang += dt as f64 * traits.scalar(Key::Spin, sim) as f64;
        if sim >= shot.at as f64 {
            if hits.kick > 0.0 && since_kick.is_none() {
                since_kick = Some(0);
            }
            if let Some(n) = since_kick.as_mut() {
                *n += 1;
                // A few frames after the kick: the thump is near its peak.
                if *n == 3 {
                    return Uniforms::for_frame(
                        &traits,
                        sim,
                        &visual,
                        flow_t,
                        ang,
                        [W, H],
                        LOGICAL_W,
                    );
                }
            }
        }
        assert!(
            sim < shot.at as f64 + 10.0,
            "no kick found for {}",
            shot.file
        );
    }
}

struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::RenderPipeline,
    uniform_buf: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
}

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

fn gpu() -> Gpu {
    let instance = wgpu::Instance::default();
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        ..Default::default()
    }))
    .expect("no GPU adapter");
    let (device, queue) =
        pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
            .expect("no GPU device");
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("polymoire"),
        source: wgpu::ShaderSource::Wgsl(SHADER_WGSL.into()),
    });
    let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("scene"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }],
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("scene"),
        bind_group_layouts: &[Some(&bgl)],
        immediate_size: 0,
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("scene"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: Some("vs_full"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        fragment: Some(wgpu::FragmentState {
            module: &module,
            entry_point: Some("fs_scene"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: FORMAT,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    });
    let uniform_buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("uniforms"),
        size: std::mem::size_of::<Uniforms>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("scene"),
        layout: &bgl,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: uniform_buf.as_entire_binding(),
        }],
    });
    Gpu {
        device,
        queue,
        pipeline,
        uniform_buf,
        bind_group,
    }
}

/// Renders one frame and returns tightly packed RGB bytes.
fn render(g: &Gpu, u: &Uniforms) -> Vec<u8> {
    let tex = g.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("shot"),
        size: wgpu::Extent3d {
            width: W,
            height: H,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = tex.create_view(&Default::default());
    let row = (W * 4).div_ceil(256) * 256;
    let out = g.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback"),
        size: (row * H) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    g.queue
        .write_buffer(&g.uniform_buf, 0, bytemuck::bytes_of(u));
    let mut enc = g.device.create_command_encoder(&Default::default());
    {
        let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("shot"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&g.pipeline);
        pass.set_bind_group(0, &g.bind_group, &[]);
        pass.draw(0..3, 0..1);
    }
    enc.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &tex,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &out,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row),
                rows_per_image: Some(H),
            },
        },
        wgpu::Extent3d {
            width: W,
            height: H,
            depth_or_array_layers: 1,
        },
    );
    g.queue.submit([enc.finish()]);
    let slice = out.slice(..);
    slice.map_async(wgpu::MapMode::Read, |r| r.expect("map readback buffer"));
    g.device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("GPU poll");
    let data = slice.get_mapped_range().expect("read back the frame");
    let mut rgb = Vec::with_capacity((W * H * 3) as usize);
    for y in 0..H {
        let line = &data[(y * row) as usize..(y * row + W * 4) as usize];
        for px in line.chunks_exact(4) {
            rgb.extend_from_slice(&px[..3]);
        }
    }
    rgb
}

fn save_png(path: &Path, rgb: &[u8]) {
    let file = std::fs::File::create(path).expect("create png");
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), W, H);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    enc.set_compression(png::Compression::High);
    let mut w = enc.write_header().expect("png header");
    w.write_image_data(rgb).expect("png data");
}

fn main() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("site/screenshots");
    std::fs::create_dir_all(&dir).expect("create site/screenshots");
    let g = gpu();
    let mut manifest = String::from("[\n");
    for (n, shot) in SHOTS.iter().enumerate() {
        let u = simulate(shot);
        let path = dir.join(format!("{}.png", shot.file));
        save_png(&path, &render(&g, &u));
        println!("wrote {}", path.display());
        let traits: Vec<String> = shot
            .traits
            .iter()
            .map(|(k, v)| format!("{{\"trait\": \"{k:?}\", \"option\": \"{v}\"}}"))
            .collect();
        manifest.push_str(&format!(
            "  {{\"file\": \"{}.png\", \"title\": \"{}\", \"traits\": [{}]}}{}\n",
            shot.file,
            shot.title,
            traits.join(", "),
            if n + 1 < SHOTS.len() { "," } else { "" }
        ));
    }
    manifest.push_str("]\n");
    std::fs::write(dir.join("shots.json"), manifest).expect("write shots.json");
}
