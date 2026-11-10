//! CPU-side mirror of the shader's uniform block. Its layout is checked against the WGSL
//! by a test, so the two can't silently drift apart.

use bytemuck::{Pod, Zeroable};

/// Must match `struct Uniforms` in `shader.wgsl` field for field.
/// Discrete selections (field, texture, effect) travel as f32 indices.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable, PartialEq)]
pub struct Uniforms {
    pub res: [f32; 2],
    pub px: f32,
    pub t: f32,

    pub clock: f32,
    pub ang: f32,
    pub pat_m: f32,
    pub sym_a: f32,

    pub sym_b: f32,
    pub sym_m: f32,
    pub tex_m: f32,
    pub fx_m: f32,

    pub warp: f32,
    pub scale: f32,
    pub chroma: f32,
    pub mosaic: f32,

    pub zoom: f32,
    pub expo: f32,
    pub hue: f32,
    pub spark: f32,

    pub vig: f32,
    pub ring_r: f32,
    pub ring_a: f32,
    pub pat_a: f32,

    pub pat_b: f32,
    pub tex_a: f32,
    pub tex_b: f32,
    pub fx_a: f32,

    pub fx_b: f32,
    pub jelly: f32,
    pub jelly_t: f32,
    pub morph_a: f32,

    pub morph_b: f32,
    pub morph_m: f32,
    pub morph_amp: f32,
    pub motes: f32,

    pub mote_t: f32,
    pub mote_size: f32,
    pub _pad: [f32; 2],

    pub pa: [f32; 4],
    pub pb: [f32; 4],
    pub pc: [f32; 4],
    pub pd: [f32; 4],
}

impl Uniforms {
    pub fn set_palette(&mut self, p: &crate::traits::Palette) {
        let v = |c: [f32; 3]| [c[0], c[1], c[2], 0.0];
        self.pa = v(p[0]);
        self.pb = v(p[1]);
        self.pc = v(p[2]);
        self.pd = v(p[3]);
    }
}
