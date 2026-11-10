//! CPU-side mirror of the shader's uniform block. Its layout is checked against the WGSL
//! by a test, so the two can't silently drift apart.

use bytemuck::{Pod, Zeroable};

use crate::reactions::Visual;
use crate::traits::{Key, Traits};

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
    /// Builds the uniforms for one frame: the traits at sim time `sim`, the audio
    /// reactions in `v`, the flow clock and view angle, for a `size` pixel target whose
    /// logical width is `logical_w`. Shared by the app and the screenshot tool.
    pub fn for_frame(
        t: &Traits,
        sim: f64,
        v: &Visual,
        flow_t: f64,
        ang: f64,
        size: [u32; 2],
        logical_w: f32,
    ) -> Self {
        let (pa, pb, pm) = t.discrete(Key::Field, sim);
        let (ta, tb, tm) = t.discrete(Key::Texture, sim);
        let (fa, fb, fm) = t.discrete(Key::Effect, sim);
        let (sa, sb, sm) = t.scalar_pair(Key::Symmetry, sim);
        let mut u = Uniforms {
            res: [size[0] as f32, size[1] as f32],
            px: size[0] as f32 / logical_w.max(1.0),
            t: flow_t as f32,
            clock: sim as f32,
            ang: ang as f32 + v.ang_off,
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

    pub fn set_palette(&mut self, p: &crate::traits::Palette) {
        let v = |c: [f32; 3]| [c[0], c[1], c[2], 0.0];
        self.pa = v(p[0]);
        self.pb = v(p[1]);
        self.pc = v(p[2]);
        self.pd = v(p[3]);
    }
}
