// Driftfield scene and blit shaders.
// The scene pass renders offscreen at an adaptive resolution; the blit pass scales it to the window.

struct Uniforms {
    res: vec2f, px: f32, t: f32,
    clock: f32, ang: f32, pat_m: f32, sym_a: f32,
    sym_b: f32, sym_m: f32, tex_m: f32, fx_m: f32,
    warp: f32, scale: f32, chroma: f32, mosaic: f32,
    zoom: f32, expo: f32, hue: f32, spark: f32,
    vig: f32, ring_r: f32, ring_a: f32, pat_a: f32,
    pat_b: f32, tex_a: f32, tex_b: f32, fx_a: f32,
    fx_b: f32, _pad0: f32, _pad1: f32, _pad2: f32,
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
    return u.pa.xyz + u.pb.xyz * cos(6.28318 * (u.pc.xyz * x + u.pd.xyz + u.hue));
}

fn lum(c: vec3f) -> f32 {
    return dot(c, vec3f(0.299, 0.587, 0.114));
}

// GLSL-style mod (floored), unlike WGSL's truncating %.
fn fmod(a: f32, b: f32) -> f32 {
    return a - b * floor(a / b);
}

fn kal(p: vec2f, n: f32) -> vec2f {
    if (n < 1.5) {
        return p;
    }
    let s = 6.28318 / n;
    var a = atan2(p.y, p.x);
    a = fmod(a, s);
    a = abs(a - s * 0.5);
    return length(p) * vec2f(cos(a), sin(a));
}

fn pat(id: i32, p: vec2f) -> f32 {
    let t = u.t;
    if (id == 0) { // Smoke
        let q = vec2f(fbm(p + vec2f(0.0, t * 0.3)), fbm(p + vec2f(5.2, 1.3) - t * 0.2));
        return fbm(p + 2.5 * q);
    }
    if (id == 1) { // Rings
        let r = length(p);
        return 0.5 + 0.5 * sin(r * 5.0 - t * 0.55 + fbm(p * 1.5 + t * 0.15) * 3.0);
    }
    if (id == 2) { // Cells
        let i = floor(p);
        let f = fract(p);
        var d1 = 8.0;
        var d2 = 8.0;
        for (var y = -1; y <= 1; y++) {
            for (var x = -1; x <= 1; x++) {
                let g = vec2f(f32(x), f32(y));
                var o = h22(i + g);
                o = 0.5 + 0.4 * sin(t * 0.8 + 6.28318 * o);
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
    if (id == 3) { // Moiré
        let a = sin(p.x * 3.0 + sin(p.y * 1.7 + t) * 1.5);
        let b = sin((p.x * 0.6 - p.y * 0.8) * 3.3 + t * 0.7);
        return 0.5 + 0.5 * a * b;
    }
    if (id == 4) { // Plasma
        let v = sin(p.x * 1.3 + t) + sin(p.y * 1.1 - t * 0.8) + sin((p.x + p.y) * 0.9 + t * 0.6)
            + sin(length(p * 1.2 + vec2f(sin(t * 0.3), cos(t * 0.4)) * 3.0) * 2.0);
        return 0.5 + 0.125 * v;
    }
    // Spiral
    let r = length(p);
    let a = atan2(p.y, p.x);
    return 0.5 + 0.5 * sin(a * 3.0 + log(r + 0.001) * 4.0 - t * 0.45);
}

fn field(p: vec2f) -> f32 {
    var a = pat(i32(u.pat_a + 0.5), p);
    if (u.pat_m > 0.001) {
        a = mix(a, pat(i32(u.pat_b + 0.5), p), u.pat_m);
    }
    return a;
}

fn fx(id: i32, c: vec3f, x: f32) -> vec3f {
    if (id == 1) { // Contour
        let k = 7.0;
        let f = fract(x * k);
        let q = pal(floor(x * k) / k + 0.5 / k);
        let e = smoothstep(0.0, 0.07, f) * (1.0 - smoothstep(0.93, 1.0, f));
        return q * (0.3 + 0.7 * e);
    }
    if (id == 2) { // Solarize
        return 0.5 + 0.5 * cos(6.28318 * (c * 1.4 + vec3f(0.0, 0.12, 0.27) + u.clock * 0.015));
    }
    if (id == 3) { // Bloom
        let hi = max(c - 0.42, vec3f(0.0));
        return c * 0.8 + hi * hi * 3.2;
    }
    if (id == 4) { // Duotone
        let l = lum(c);
        return mix(pal(0.05) * 0.18, pal(0.72) * 1.1, smoothstep(0.12, 0.88, l));
    }
    if (id == 5) { // Invert bands
        return mix(c, 1.0 - c, step(0.5, fract(x * 3.0 + u.clock * 0.03)));
    }
    return c;
}

fn tex(id: i32, c: vec3f, fc: vec2f) -> vec3f {
    if (id == 1) { // Grain, refreshed ~8 times a second so it doesn't fizz
        return c + (h21(fc + fract(floor(u.clock * 8.0) * 0.137) * 173.0) - 0.5) * 0.18;
    }
    if (id == 2) { // Halftone
        let r = mat2x2f(0.7071, -0.7071, 0.7071, 0.7071);
        let g = fract(r * fc / 7.0) - 0.5;
        let rad = sqrt(clamp(lum(c), 0.0, 1.0)) * 0.66;
        let d = 1.0 - smoothstep(rad - 0.12, rad, length(g));
        return mix(c * 0.1, c * 1.2, d);
    }
    if (id == 3) { // Scanline
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
    if (id == 4) { // Hatch
        let l = lum(c);
        let a = abs(fract((fc.x + fc.y) / 6.0) - 0.5);
        let b = abs(fract((fc.x - fc.y) / 6.0) - 0.5);
        let la = (1.0 - smoothstep(0.06, 0.12, a)) * step(l, 0.62);
        let lb = (1.0 - smoothstep(0.06, 0.12, b)) * step(l, 0.34);
        return c * (1.15 - 0.75 * max(la, lb));
    }
    if (id == 5) { // Dither
        let ign = fract(52.9829 * fract(0.06711 * fc.x + 0.00583 * fc.y));
        return floor(c * 4.0 + ign) / 4.0;
    }
    if (id == 6) { // Paper
        let f = vn(fc * vec2f(0.7, 0.04)) * 0.5 + vn(fc * vec2f(0.04, 0.7)) * 0.5;
        let s = h21(floor(fc));
        return c * (0.8 + 0.28 * f + 0.06 * s);
    }
    return c;
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
