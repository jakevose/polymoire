//! The shader is validated on every platform without a GPU, and its uniform layout is
//! checked against the Rust struct so the two can't drift apart.

use driftfield::uniforms::Uniforms;
use driftfield::SHADER_WGSL;
use std::mem::{offset_of, size_of};

fn module() -> naga::Module {
    naga::front::wgsl::parse_str(SHADER_WGSL)
        .unwrap_or_else(|e| panic!("WGSL failed to parse:\n{}", e.emit_to_string(SHADER_WGSL)))
}

#[test]
fn shader_parses_and_validates() {
    let m = module();
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::empty(),
    )
    .validate(&m)
    .unwrap_or_else(|e| panic!("WGSL failed validation:\n{}", e.emit_to_string(SHADER_WGSL)));
}

#[test]
fn entry_points_used_by_the_renderer_exist() {
    let m = module();
    let names: Vec<&str> = m.entry_points.iter().map(|e| e.name.as_str()).collect();
    for want in ["vs_full", "fs_scene", "fs_blit", "fs_blit_srgb"] {
        assert!(
            names.contains(&want),
            "missing entry point {want}; have {names:?}"
        );
    }
}

#[test]
fn uniform_layout_matches_rust_struct() {
    let m = module();
    let (_, ty) = m
        .types
        .iter()
        .find(|(_, t)| t.name.as_deref() == Some("Uniforms"))
        .expect("struct Uniforms in WGSL");
    let naga::TypeInner::Struct { members, span } = &ty.inner else {
        panic!("Uniforms is not a struct");
    };
    assert_eq!(*span as usize, size_of::<Uniforms>(), "struct size");

    macro_rules! fields {
        ($($f:ident),*) => { [$((stringify!($f), offset_of!(Uniforms, $f))),*] };
    }
    let rust = fields!(
        res, px, t, clock, ang, pat_m, sym_a, sym_b, sym_m, tex_m, fx_m, warp, scale, chroma,
        mosaic, zoom, expo, hue, spark, vig, ring_r, ring_a, pat_a, pat_b, tex_a, tex_b, fx_a,
        fx_b, pa, pb, pc, pd
    );
    for (name, off) in rust {
        let member = members
            .iter()
            .find(|m| m.name.as_deref() == Some(name))
            .unwrap_or_else(|| panic!("WGSL Uniforms has no field {name}"));
        assert_eq!(member.offset as usize, off, "offset of {name}");
    }
    assert_eq!(
        size_of::<Uniforms>() % 16,
        0,
        "uniform blocks must be 16-byte multiples"
    );
}
