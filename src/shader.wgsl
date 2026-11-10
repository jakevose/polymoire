// Driftfield scene and blit shaders.
// The scene pass renders offscreen at an adaptive resolution; the blit pass scales it to the window.
// Each field, texture and effect is tagged `// F<n>`, `// T<n>`, `// E<n>`; a test checks that
// every option in the Rust catalog has an implementation here.

struct Uniforms {
    res: vec2f, px: f32, t: f32,
    clock: f32, ang: f32, pat_m: f32, sym_a: f32,
    sym_b: f32, sym_m: f32, tex_m: f32, fx_m: f32,
    warp: f32, scale: f32, chroma: f32, mosaic: f32,
    zoom: f32, expo: f32, hue: f32, spark: f32,
    vig: f32, ring_r: f32, ring_a: f32, pat_a: f32,
    pat_b: f32, tex_a: f32, tex_b: f32, fx_a: f32,
    fx_b: f32, jelly: f32, jelly_t: f32, morph_a: f32,
    morph_b: f32, morph_m: f32, morph_amp: f32, motes: f32,
    mote_t: f32, mote_size: f32, _pad0: f32, _pad1: f32,
    pa: vec4f, pb: vec4f, pc: vec4f, pd: vec4f,
}

@group(0) @binding(0) var<uniform> u: Uniforms;
@group(0) @binding(1) var src_tex: texture_2d<f32>;
@group(0) @binding(2) var src_samp: sampler;

struct VsOut {
    @builtin(position) pos: vec4f,
    @location(0) uv: vec2f,
}

// One triangle that covers the whole target.
@vertex
fn vs_full(@builtin(vertex_index) vi: u32) -> VsOut {
    let x = f32((vi << 1u) & 2u);
    let y = f32(vi & 2u);
    var o: VsOut;
    o.pos = vec4f(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
    o.uv = vec2f(x, y);
    return o;
}

const TAU: f32 = 6.28318;

fn h21(p_in: vec2f) -> f32 {
    var p = fract(p_in * vec2f(123.34, 456.21));
    p = p + dot(p, p + 45.32);
    return fract(p.x * p.y);
}

fn h22(p: vec2f) -> vec2f {
    let n = h21(p);
    return vec2f(n, h21(p + n + 17.1));
}

fn vn(p: vec2f) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let w = f * f * (3.0 - 2.0 * f);
    return mix(
        mix(h21(i), h21(i + vec2f(1.0, 0.0)), w.x),
        mix(h21(i + vec2f(0.0, 1.0)), h21(i + vec2f(1.0, 1.0)), w.x),
        w.y
    );
}

fn fbm(p_in: vec2f) -> f32 {
    var p = p_in;
    var s = 0.0;
    var a = 0.5;
    let m = mat2x2f(1.6, 1.2, -1.2, 1.6);
    for (var i = 0; i < 4; i++) {
        s += a * vn(p);
        p = m * p;
        a *= 0.5;
    }
    return s / 0.9375;
}

fn pal(x: f32) -> vec3f {
    return u.pa.xyz + u.pb.xyz * cos(TAU * (u.pc.xyz * x + u.pd.xyz + u.hue));
}

fn lum(c: vec3f) -> f32 {
    return dot(c, vec3f(0.299, 0.587, 0.114));
}

// GLSL-style mod (floored), unlike WGSL's truncating %.
fn fmod(a: f32, b: f32) -> f32 {
    return a - b * floor(a / b);
}

fn fmod2(a: vec2f, b: vec2f) -> vec2f {
    return a - b * floor(a / b);
}

// Symmetry: n >= 2 is an n-fold kaleidoscope, -1 mirrors left/right, -2 mirrors both axes.
fn kal(p: vec2f, n: f32) -> vec2f {
    if (n < -1.5) {
        return abs(p);
    }
    if (n < -0.5) {
        return vec2f(abs(p.x), p.y);
    }
    if (n < 1.5) {
        return p;
    }
    let s = TAU / n;
    var a = atan2(p.y, p.x);
    a = fmod(a, s);
    a = abs(a - s * 0.5);
    return length(p) * vec2f(cos(a), sin(a));
}

// Zero-mean displacement whose direction varies from place to place, so the picture
// moves locally in many directions at once instead of as a whole.
fn local_disp(p: vec2f, seed: vec2f) -> vec2f {
    return (vec2f(vn(p + seed), vn(p + seed.yx + vec2f(9.2, 4.4))) - 0.5) * 2.0;
}

fn pat(id: i32, p: vec2f) -> f32 {
    let t = u.t;
    if (id == 0) { // F0 Smoke
        let q = vec2f(fbm(p + vec2f(0.0, t * 0.3)), fbm(p + vec2f(5.2, 1.3) - t * 0.2));
        return fbm(p + 2.5 * q);
    }
    if (id == 1) { // F1 Rings
        let r = length(p);
        return 0.5 + 0.5 * sin(r * 5.0 - t * 0.55 + fbm(p * 1.5 + t * 0.15) * 3.0);
    }
    if (id == 2) { // F2 Cells
        let i = floor(p);
        let f = fract(p);
        var d1 = 8.0;
        var d2 = 8.0;
        for (var y = -1; y <= 1; y++) {
            for (var x = -1; x <= 1; x++) {
                let g = vec2f(f32(x), f32(y));
                var o = h22(i + g);
                o = 0.5 + 0.4 * sin(t * 0.8 + TAU * o);
                let d = length(g + o - f);
                if (d < d1) {
                    d2 = d1;
                    d1 = d;
                } else if (d < d2) {
                    d2 = d;
                }
            }
        }
        return smoothstep(0.0, 0.55, d2 - d1) * 0.8 + d1 * 0.25;
    }
    if (id == 3) { // F3 Moiré
        let a = sin(p.x * 3.0 + sin(p.y * 1.7 + t) * 1.5);
        let b = sin((p.x * 0.6 - p.y * 0.8) * 3.3 + t * 0.7);
        return 0.5 + 0.5 * a * b;
    }
    if (id == 4) { // F4 Plasma
        let v = sin(p.x * 1.3 + t) + sin(p.y * 1.1 - t * 0.8) + sin((p.x + p.y) * 0.9 + t * 0.6)
            + sin(length(p * 1.2 + vec2f(sin(t * 0.3), cos(t * 0.4)) * 3.0) * 2.0);
        return 0.5 + 0.125 * v;
    }
    if (id == 5) { // F5 Spiral
        let r = length(p);
        let a = atan2(p.y, p.x);
        return 0.5 + 0.5 * sin(a * 3.0 + log(r + 0.001) * 4.0 - t * 0.45);
    }
    if (id == 6) { // F6 Quasicrystal: five plane waves at 36° steps
        var s = 0.0;
        for (var k = 0; k < 5; k++) {
            let a = f32(k) * 0.6283185;
            s += cos(dot(p, vec2f(cos(a), sin(a))) * 3.0 + t * 0.6);
        }
        return 0.5 + 0.1 * s;
    }
    if (id == 7) { // F7 Marble
        let n = fbm(p * 0.8 + vec2f(t * 0.1, -t * 0.07));
        return 0.5 + 0.5 * sin(p.x * 2.0 + p.y * 0.7 + n * 6.0);
    }
    if (id == 8) { // F8 Ridges: ridged fractal noise
        var q = p;
        var s = 0.0;
        var a = 0.5;
        let m = mat2x2f(1.6, 1.2, -1.2, 1.6);
        for (var k = 0; k < 4; k++) {
            let n = 1.0 - abs(vn(q + t * 0.1) * 2.0 - 1.0);
            s += a * n * n;
            q = m * q;
            a *= 0.5;
        }
        return s / 0.9375;
    }
    if (id == 9) { // F9 Truchet: quarter-circle tiles
        let g = p * 1.5;
        let cell = floor(g);
        var f = fract(g) - 0.5;
        let h = h21(cell);
        if (h > 0.5) {
            f.x = -f.x;
        }
        let d = min(abs(length(f - vec2f(0.5)) - 0.5), abs(length(f + vec2f(0.5)) - 0.5));
        return smoothstep(0.0, 0.12 + 0.05 * sin(t + h * TAU), d) * 0.8 + 0.2 * fbm(p + t * 0.1);
    }
    if (id == 10) { // F10 Honeycomb
        let q = p * 1.2;
        let r = vec2f(1.0, 1.7320508);
        let hr = r * 0.5;
        let a = fmod2(q, r) - hr;
        let b = fmod2(q - hr, r) - hr;
        let gv = select(b, a, dot(a, a) < dot(b, b));
        let v = abs(gv);
        let hd = max(dot(v, vec2f(0.5, 0.8660254)), v.x);
        return 0.5 + 0.5 * sin(hd * 18.0 - t * 1.2 + fbm(q * 0.5) * 4.0);
    }
    if (id == 11) { // F11 Petals
        let r = length(p);
        let a = atan2(p.y, p.x);
        return 0.5 + 0.5 * sin(cos(a * 5.0 + t * 0.3) * 2.0 - r * 3.0 + fbm(p + t * 0.1) * 2.0);
    }
    if (id == 12) { // F12 Currents: streaks whose direction varies across the picture
        let n = fbm(p * 0.7 + t * 0.05);
        let dir = vec2f(cos(n * TAU), sin(n * TAU));
        return 0.5 + 0.5 * sin(dot(p, dir) * 6.0 + t * 0.4);
    }
    // F13 Interference: ripples from three slowly wandering sources
    var s = 0.0;
    for (var k = 0; k < 3; k++) {
        let fk = f32(k);
        let c = vec2f(sin(t * 0.21 + fk * 2.1), cos(t * 0.17 + fk * 1.7)) * 1.5;
        s += sin(length(p - c) * 6.0 - t * 1.5);
    }
    return 0.5 + s / 6.0;
}

fn field(p: vec2f) -> f32 {
    var a = pat(i32(u.pat_a + 0.5), p);
    if (u.pat_m > 0.001) {
        a = mix(a, pat(i32(u.pat_b + 0.5), p), u.pat_m);
    }
    return a;
}

fn fx(id: i32, c: vec3f, x: f32) -> vec3f {
    // E0 None falls through to the end.
    if (id == 1) { // E1 Contour
        let k = 7.0;
        let f = fract(x * k);
        let q = pal(floor(x * k) / k + 0.5 / k);
        let e = smoothstep(0.0, 0.07, f) * (1.0 - smoothstep(0.93, 1.0, f));
        return q * (0.3 + 0.7 * e);
    }
    if (id == 2) { // E2 Solarize
        return 0.5 + 0.5 * cos(TAU * (c * 1.4 + vec3f(0.0, 0.12, 0.27) + u.clock * 0.015));
    }
    if (id == 3) { // E3 Bloom
        let hi = max(c - 0.42, vec3f(0.0));
        return c * 0.8 + hi * hi * 3.2;
    }
    if (id == 4) { // E4 Duotone
        let l = lum(c);
        return mix(pal(0.05) * 0.18, pal(0.72) * 1.1, smoothstep(0.12, 0.88, l));
    }
    if (id == 5) { // E5 Invert bands
        return mix(c, 1.0 - c, step(0.5, fract(x * 3.0 + u.clock * 0.03)));
    }
    if (id == 6) { // E6 Posterize
        return floor(c * 4.0 + 0.5) / 4.0;
    }
    if (id == 7) { // E7 Neon: glowing isolines on a dark ground
        let f = abs(fract(x * 6.0) - 0.5) * 2.0;
        let glow = pow(1.0 - f, 6.0);
        return c * 0.22 + pal(x + 0.3) * glow * 1.2;
    }
    if (id == 8) { // E8 Thermal
        let l = clamp(lum(c), 0.0, 1.0);
        let heat = vec3f(l * 3.0, l * 3.0 - 1.0, l * 3.0 - 2.0);
        return clamp(heat + vec3f(0.0, 0.0, 0.45) * max(1.0 - l * 2.5, 0.0), vec3f(0.0), vec3f(1.0));
    }
    if (id == 9) { // E9 Chrome
        let s = 0.5 + 0.5 * cos(x * 14.0);
        let metal = mix(vec3f(0.08, 0.09, 0.11), vec3f(0.92, 0.94, 0.97), pow(s, 1.5));
        return metal * mix(vec3f(1.0), pal(x), 0.25);
    }
    if (id == 10) { // E10 Iridescent
        return 0.5 + 0.5 * cos(TAU * (x * 2.0 + vec3f(0.0, 0.33, 0.67) + lum(c) * 0.5));
    }
    if (id == 11) { // E11 Ink
        let ink = smoothstep(0.35, 0.55, lum(c));
        return mix(vec3f(0.06, 0.06, 0.08), vec3f(0.94, 0.92, 0.86), ink) + (pal(x) - 0.5) * 0.08;
    }
    return c;
}

fn tex(id: i32, c: vec3f, fc: vec2f) -> vec3f {
    // T0 Clean falls through to the end.
    if (id == 1) { // T1 Grain, refreshed ~8 times a second so it doesn't fizz
        return c + (h21(fc + fract(floor(u.clock * 8.0) * 0.137) * 173.0) - 0.5) * 0.18;
    }
    if (id == 2) { // T2 Halftone
        let r = mat2x2f(0.7071, -0.7071, 0.7071, 0.7071);
        let g = fract(r * fc / 7.0) - 0.5;
        let rad = sqrt(clamp(lum(c), 0.0, 1.0)) * 0.66;
        let d = 1.0 - smoothstep(rad - 0.12, rad, length(g));
        return mix(c * 0.1, c * 1.2, d);
    }
    if (id == 3) { // T3 Scanline
        let sl = 0.66 + 0.34 * sin(fc.y * 1.5708);
        let k = fmod(floor(fc.x), 3.0);
        var m = vec3f(0.72, 0.72, 1.0);
        if (k < 0.5) {
            m = vec3f(1.0, 0.72, 0.72);
        } else if (k < 1.5) {
            m = vec3f(0.72, 1.0, 0.72);
        }
        return c * sl * m * 1.3;
    }
    if (id == 4) { // T4 Hatch
        let l = lum(c);
        let a = abs(fract((fc.x + fc.y) / 6.0) - 0.5);
        let b = abs(fract((fc.x - fc.y) / 6.0) - 0.5);
        let la = (1.0 - smoothstep(0.06, 0.12, a)) * step(l, 0.62);
        let lb = (1.0 - smoothstep(0.06, 0.12, b)) * step(l, 0.34);
        return c * (1.15 - 0.75 * max(la, lb));
    }
    if (id == 5) { // T5 Dither
        let ign = fract(52.9829 * fract(0.06711 * fc.x + 0.00583 * fc.y));
        return floor(c * 4.0 + ign) / 4.0;
    }
    if (id == 6) { // T6 Paper
        let f = vn(fc * vec2f(0.7, 0.04)) * 0.5 + vn(fc * vec2f(0.04, 0.7)) * 0.5;
        let s = h21(floor(fc));
        return c * (0.8 + 0.28 * f + 0.06 * s);
    }
    if (id == 7) { // T7 Stipple: dot density follows brightness
        let r = h21(floor(fc / 3.0) + 0.37);
        return select(c * 0.25, c * 1.1, r < lum(c) * 1.1);
    }
    if (id == 8) { // T8 Weave
        let g = fc / 6.0;
        let i = floor(g);
        let f = fract(g);
        let over = fmod(i.x + i.y, 2.0) < 0.5;
        let thread = select(sin(f.y * 3.14159), sin(f.x * 3.14159), over);
        return c * (0.7 + 0.4 * thread);
    }
    if (id == 9) { // T9 LCD subpixels
        let g = fract(fc / 4.0);
        let col = floor(g.x * 3.0);
        var m = vec3f(0.25, 0.25, 1.0);
        if (col < 0.5) {
            m = vec3f(1.0, 0.25, 0.25);
        } else if (col < 1.5) {
            m = vec3f(0.25, 1.0, 0.25);
        }
        return c * m * 1.6 * step(0.15, g.y);
    }
    if (id == 10) { // T10 Riso: two inks on cream stock, with grain
        let l = lum(c);
        let ink1 = pal(0.15);
        let a = smoothstep(0.25, 0.6, l + (h21(fc * 0.73) - 0.5) * 0.25);
        let b = smoothstep(0.6, 0.9, l + (h21(fc * 0.71 + 3.0) - 0.5) * 0.25);
        let base = mix(ink1 * 0.3 + 0.04, ink1, a);
        return mix(base, vec3f(0.95, 0.93, 0.88), b * 0.8);
    }
    if (id == 11) { // T11 Engrave: line weight follows darkness
        let l = clamp(lum(c), 0.0, 1.0);
        let d = abs(fract((fc.x + fc.y) * 0.7071 / 5.0) - 0.5) * 2.0;
        let ink = 1.0 - smoothstep(1.0 - l - 0.1, 1.0 - l + 0.1, d);
        return mix(c * 1.15, c * 0.15, ink * 0.9);
    }
    return c;
}

// Motes: small soft points, each orbiting inside its own cell. Every mote takes its own
// path, so they add life without any shared drift across the screen.
fn motes(c: vec3f, fc: vec2f) -> vec3f {
    let cell = 46.0;
    let g = fc / cell;
    let id = floor(g);
    let f = fract(g);
    let h = h22(id + 3.7);
    if (h.x > u.motes) {
        return c;
    }
    let ph = h * TAU;
    let sp = 0.6 + h.y * 0.8;
    let pos = vec2f(0.5) + 0.32 * vec2f(sin(u.mote_t * sp + ph.x), cos(u.mote_t * sp * 0.83 + ph.y));
    let d = length(f - pos) * cell;
    let r = u.mote_size * (0.6 + 0.8 * h.y);
    let a = 1.0 - smoothstep(r * 0.3, r, d);
    let mc = pal(h.x * 0.7 + 0.3);
    return mix(c, max(c, mc * 1.05), a * 0.85);
}

@fragment
fn fs_scene(@builtin(position) pos: vec4f) -> @location(0) vec4f {
    let fc = pos.xy / u.px;
    let res = u.res / u.px;
    var qc = fc;
    if (u.mosaic > 1.01) {
        qc = (floor(fc / u.mosaic) + 0.5) * u.mosaic;
    }
    var uv = (qc - 0.5 * res) / res.y;
    uv.y = -uv.y;
    let ca = cos(u.ang);
    let sa = sin(u.ang);
    uv = mat2x2f(ca, sa, -sa, ca) * uv;
    if (u.ring_a > 0.001) {
        let dd = length(uv) - u.ring_r;
        uv = uv * (1.0 + u.ring_a * 0.14 * exp(-dd * dd * 70.0));
    }
    uv = mix(kal(uv, u.sym_a), kal(uv, u.sym_b), u.sym_m);
    var p = uv * u.scale * u.zoom;
    if (u.warp > 0.001) {
        p = p + u.warp * (vec2f(fbm(p * 0.7 + u.t * 0.15), fbm(p * 0.7 + vec2f(3.1, 7.7) - u.t * 0.12)) - 0.5) * 2.0;
    }
    if (u.jelly > 0.001) {
        p = p + local_disp(p * 0.9, vec2f(u.jelly_t, u.jelly_t * 0.7)) * u.jelly;
    }
    if (u.morph_amp > 0.001) {
        let da = local_disp(p * 0.7, vec2f(u.morph_a * 17.3, u.morph_a * 3.1));
        let db = local_disp(p * 0.7, vec2f(u.morph_b * 17.3, u.morph_b * 3.1));
        p = p + mix(da, db, u.morph_m) * u.morph_amp;
    }
    let x = field(p);
    var col: vec3f;
    if (u.chroma > 0.001) {
        let o = normalize(uv + vec2f(1e-4)) * u.chroma * 0.09;
        col = vec3f(pal(field(p + o)).r, pal(x).g, pal(field(p - o)).b);
    } else {
        col = pal(x);
    }
    var c1 = fx(i32(u.fx_a + 0.5), col, x);
    if (u.fx_m > 0.001) {
        c1 = mix(c1, fx(i32(u.fx_b + 0.5), col, x), u.fx_m);
    }
    c1 = c1 * u.expo;
    let vu = fc / res - 0.5;
    c1 = c1 * (1.0 - dot(vu, vu) * u.vig);
    var t1 = tex(i32(u.tex_a + 0.5), c1, fc);
    if (u.tex_m > 0.001) {
        t1 = mix(t1, tex(i32(u.tex_b + 0.5), c1, fc), u.tex_m);
    }
    if (u.motes > 0.001) {
        t1 = motes(t1, fc);
    }
    if (u.spark > 0.001) {
        t1 = t1 + (h21(fc * 1.3 + fract(floor(u.clock * 8.0) * 0.291) * 91.0) - 0.5) * u.spark;
    }
    return vec4f(clamp(t1, vec3f(0.0), vec3f(1.0)), 1.0);
}

@fragment
fn fs_blit(in: VsOut) -> @location(0) vec4f {
    return vec4f(textureSample(src_tex, src_samp, in.uv).rgb, 1.0);
}

// For sRGB targets: the scene is authored in display (gamma) space, so decode before writing.
@fragment
fn fs_blit_srgb(in: VsOut) -> @location(0) vec4f {
    let c = textureSample(src_tex, src_samp, in.uv).rgb;
    return vec4f(pow(c, vec3f(2.2)), 1.0);
}
