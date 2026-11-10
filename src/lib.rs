//! Driftfield: a generative visualizer that drifts between looks one or two traits at a time
//! and reacts to whatever the computer is playing.
//!
//! The library holds everything that can be tested without a window or a sound card:
//! the trait engine, the beat analysis, the demo synth, the reaction mapping and the
//! shader's uniform layout. The binary (`src/main.rs` and `src/app/`) wires it to a
//! window, the GPU and the audio devices.

pub mod analysis;
pub mod demo;
pub mod dsp;
pub mod quality;
pub mod reactions;
pub mod rng;
pub mod tempo;
pub mod traits;
pub mod uniforms;
pub mod vibe;

/// The WGSL source for the scene and blit passes.
pub const SHADER_WGSL: &str = include_str!("shader.wgsl");
