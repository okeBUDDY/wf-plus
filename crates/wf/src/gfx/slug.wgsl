// Slug rendering (E. Lengyel, "GPU-Centered Font Rendering Directly from
// Glyph Outlines", JCGT 2017) of the cooked art. Each instance is one sprite:
// a dilated quad over its bounds. For every pixel and every layer of the
// sprite, the fragment shader casts a horizontal and a vertical ray against
// the curves of the pixel's band, turns the crossings into fractional
// coverage, paints the layer, and composes the layers (over or max). The
// result is premultiplied, tinted by the instance colour and blended once.

struct View {
    // logical (640x480) -> clip: clip = pos * scale + offset
    scale: vec2<f32>,
    offset: vec2<f32>,
    time: f32,
    _p0: f32,
    _p1: f32,
    _p2: f32,
};

@group(0) @binding(0) var<uniform> view: View;
@group(0) @binding(1) var curve_tex: texture_2d<f32>;
@group(0) @binding(2) var band_tex: texture_2d<u32>;
@group(0) @binding(3) var layer_tex: texture_2d<u32>;
@group(0) @binding(4) var ramp_tex: texture_2d<f32>;
@group(0) @binding(5) var bitmap_tex: texture_2d<f32>;
@group(0) @binding(6) var bitmap_sampler: sampler;

const CURVE_W: u32 = 4096u;
const BAND_W: u32 = 4096u;
const LAYER_W: u32 = 1024u;
const LAYER_TEXELS: u32 = 8u;
const RAMP_W: u32 = 256u;

const PAINT_SOLID: u32 = 0u;
const PAINT_LINEAR: u32 = 1u;
const PAINT_RADIAL: u32 = 2u;
const FLAG_COMBINE: u32 = 0x100u;
const FLAG_NOISE: u32 = 0x200u;

struct Instance {
    @location(0) bounds: vec4<f32>,   // dilated local quad (min, max)
    @location(1) m0: vec4<f32>,       // m00 m01 m10 m11
    @location(2) m1: vec2<f32>,       // m20 m21
    @location(3) tint: vec4<f32>,     // straight RGBA
    @location(4) layers: vec4<u32>,   // first layer, count, compose, flags
};

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) local: vec2<f32>,
    @location(1) @interpolate(flat) tint: vec4<f32>,
    @location(2) @interpolate(flat) layers: vec4<u32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vi: u32, inst: Instance) -> VsOut {
    let corner = vec2<f32>(f32(vi & 1u), f32((vi >> 1u) & 1u));
    let local = mix(inst.bounds.xy, inst.bounds.zw, corner);
    let screen = vec2<f32>(
        local.x * inst.m0.x + local.y * inst.m0.z + inst.m1.x,
        local.x * inst.m0.y + local.y * inst.m0.w + inst.m1.y,
    );
    var out: VsOut;
    out.pos = vec4<f32>(screen * view.scale + view.offset, 0.5, 1.0);
    out.local = local;
    out.tint = inst.tint;
    out.layers = inst.layers;
    return out;
}

fn tex2d(i: u32, w: u32) -> vec2<i32> {
    return vec2<i32>(i32(i % w), i32(i / w));
}

fn layer_texel(layer: u32, k: u32) -> vec4<u32> {
    return textureLoad(layer_tex, tex2d(layer * LAYER_TEXELS + k, LAYER_W), 0);
}

fn band(i: u32) -> vec2<u32> {
    return textureLoad(band_tex, tex2d(i, BAND_W), 0).xy;
}

// Which roots of a curve (relative to the sample, along the ray's
// perpendicular axis) contribute, from the signs of its three y values.
fn root_code(y1: f32, y2: f32, y3: f32) -> u32 {
    let shift = select(0u, 2u, y1 > 0.0) + select(0u, 4u, y2 > 0.0) + select(0u, 8u, y3 > 0.0);
    return (0x2E74u >> shift) & 3u;
}

// The two crossings (along x) of y = 0 by the curve p1, p2, p3.
fn solve(p1: vec2<f32>, p2: vec2<f32>, p3: vec2<f32>) -> vec2<f32> {
    let a = p1 - 2.0 * p2 + p3;
    let b = p1 - p2;
    var t1: f32;
    var t2: f32;
    if (abs(a.y) < 1.0 / 65536.0) {
        t1 = p1.y * 0.5 / b.y;
        t2 = t1;
    } else {
        let ra = 1.0 / a.y;
        let d = sqrt(max(b.y * b.y - a.y * p1.y, 0.0));
        t1 = (b.y - d) * ra;
        t2 = (b.y + d) * ra;
    }
    return vec2<f32>((a.x * t1 - b.x * 2.0) * t1 + p1.x, (a.x * t2 - b.x * 2.0) * t2 + p1.x);
}

fn coverage(rc: vec2<f32>, ppe: vec2<f32>, layer: u32) -> f32 {
    let t0 = layer_texel(layer, 0u);
    let bbox = bitcast<vec4<f32>>(t0);
    let margin = 0.5 / ppe;
    if (any(rc < bbox.xy - margin) || any(rc > bbox.zw + margin)) {
        return 0.0;
    }
    let t1 = layer_texel(layer, 1u);
    let band_base = t1.x;
    let hbands = t1.y;
    let vbands = t1.z;
    let band_scale = bitcast<vec2<f32>>(layer_texel(layer, 7u).xy);

    var xcov = 0.0;
    var xwgt = 0.0;
    let hb = u32(clamp(i32(floor((rc.y - bbox.y) * band_scale.y)), 0, i32(hbands) - 1));
    let hh = band(band_base + hb);
    for (var k = 0u; k < hh.x; k++) {
        let ci = band(hh.y + k).x;
        let a = textureLoad(curve_tex, tex2d(ci, CURVE_W), 0);
        let c = textureLoad(curve_tex, tex2d(ci + 1u, CURVE_W), 0);
        let p1 = a.xy - rc;
        let p2 = a.zw - rc;
        let p3 = c.xy - rc;
        if (max(max(p1.x, p2.x), p3.x) * ppe.x < -0.5) {
            break;
        }
        let code = root_code(p1.y, p2.y, p3.y);
        if (code != 0u) {
            let r = solve(p1, p2, p3) * ppe.x;
            if ((code & 1u) != 0u) {
                xcov += clamp(r.x + 0.5, 0.0, 1.0);
                xwgt = max(xwgt, clamp(1.0 - abs(r.x) * 2.0, 0.0, 1.0));
            }
            if (code > 1u) {
                xcov -= clamp(r.y + 0.5, 0.0, 1.0);
                xwgt = max(xwgt, clamp(1.0 - abs(r.y) * 2.0, 0.0, 1.0));
            }
        }
    }

    var ycov = 0.0;
    var ywgt = 0.0;
    let vb = u32(clamp(i32(floor((rc.x - bbox.x) * band_scale.x)), 0, i32(vbands) - 1));
    let vh = band(band_base + hbands + vb);
    for (var k = 0u; k < vh.x; k++) {
        let ci = band(vh.y + k).x;
        let a = textureLoad(curve_tex, tex2d(ci, CURVE_W), 0);
        let c = textureLoad(curve_tex, tex2d(ci + 1u, CURVE_W), 0);
        // Swap the axes: the vertical ray is the horizontal case on (y, x).
        let p1 = (a.xy - rc).yx;
        let p2 = (a.zw - rc).yx;
        let p3 = (c.xy - rc).yx;
        if (max(max(p1.x, p2.x), p3.x) * ppe.y < -0.5) {
            break;
        }
        let code = root_code(p1.y, p2.y, p3.y);
        if (code != 0u) {
            let r = solve(p1, p2, p3) * ppe.y;
            // The swap mirrors the plane, so the winding sign flips.
            if ((code & 1u) != 0u) {
                ycov -= clamp(r.x + 0.5, 0.0, 1.0);
                ywgt = max(ywgt, clamp(1.0 - abs(r.x) * 2.0, 0.0, 1.0));
            }
            if (code > 1u) {
                ycov += clamp(r.y + 0.5, 0.0, 1.0);
                ywgt = max(ywgt, clamp(1.0 - abs(r.y) * 2.0, 0.0, 1.0));
            }
        }
    }

    let blended = abs(xcov * xwgt + ycov * ywgt) / max(xwgt + ywgt, 1.0 / 65536.0);
    return clamp(max(blended, min(abs(xcov), abs(ycov))), 0.0, 1.0);
}

fn ramp(row: u32, t: f32) -> vec4<f32> {
    let x = clamp(t, 0.0, 1.0) * f32(RAMP_W) - 0.5;
    let i0 = u32(clamp(floor(x), 0.0, f32(RAMP_W - 1u)));
    let i1 = min(i0 + 1u, RAMP_W - 1u);
    let f = clamp(x - floor(x), 0.0, 1.0);
    let a = textureLoad(ramp_tex, vec2<i32>(i32(i0), i32(row)), 0);
    let b = textureLoad(ramp_tex, vec2<i32>(i32(i1), i32(row)), 0);
    return mix(a, b, select(f, 0.0, x < 0.0));
}

fn ramp_t(kind: u32, geom: vec4<f32>, p: vec2<f32>) -> f32 {
    if (kind == PAINT_RADIAL) {
        return length(p - geom.xy) / geom.z;
    }
    let d = geom.zw - geom.xy;
    return dot(p - geom.xy, d) / max(dot(d, d), 1e-6);
}

fn hash(p: vec2<f32>, seed: f32) -> f32 {
    let h = dot(p, vec2<f32>(127.1, 311.7)) + seed * 74.7;
    return fract(sin(h) * 43758.5453);
}

fn value_noise(p: vec2<f32>, seed: f32) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = hash(i, seed);
    let b = hash(i + vec2<f32>(1.0, 0.0), seed);
    let c = hash(i + vec2<f32>(0.0, 1.0), seed);
    let d = hash(i + vec2<f32>(1.0, 1.0), seed);
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

// Fractal noise in 0..1 (stands in for feTurbulence fractalNoise).
fn fractal(p: vec2<f32>, params: vec4<f32>) -> f32 {
    var sum = 0.0;
    var amp = 0.5;
    var freq = params.x;
    var norm = 0.0;
    let octaves = i32(params.y);
    for (var o = 0; o < octaves; o++) {
        sum += amp * value_noise(p * freq, params.z + f32(o));
        norm += amp;
        amp *= 0.5;
        freq *= 2.0;
    }
    return sum / max(norm, 1e-6);
}

// The layer's straight colour at `p`.
fn paint(layer: u32, p: vec2<f32>) -> vec4<f32> {
    let t1 = layer_texel(layer, 1u);
    let kind = t1.w & 0xffu;
    let flags = t1.w & 0xff00u;
    var c: vec4<f32>;
    if (kind == PAINT_SOLID) {
        c = bitcast<vec4<f32>>(layer_texel(layer, 2u));
    } else {
        let geom = bitcast<vec4<f32>>(layer_texel(layer, 3u));
        let rows = layer_texel(layer, 5u);
        c = ramp(rows.x, ramp_t(kind, geom, p));
        if ((flags & FLAG_COMBINE) != 0u) {
            let geom2 = bitcast<vec4<f32>>(layer_texel(layer, 4u));
            let stripe = ramp(rows.y, ramp_t(PAINT_LINEAR, geom2, p)).a;
            // base in alpha, depth in red; the colour is the green/blue grey.
            let alpha = c.a * (1.0 - c.r * (1.0 - stripe));
            c = vec4<f32>(c.g, c.g, c.b, alpha);
        }
    }
    if ((flags & FLAG_NOISE) != 0u) {
        let n = bitcast<vec4<f32>>(layer_texel(layer, 6u));
        let k = 1.0 + n.w * (fractal(p, n) - 0.52);
        c = vec4<f32>(clamp(c.rgb * k, vec3<f32>(0.0), vec3<f32>(1.0)), c.a);
    }
    return c;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let rc = in.local;
    let ppe = 1.0 / max(fwidth(rc), vec2<f32>(1.0 / 65536.0));
    var out = vec4<f32>(0.0);
    let first = in.layers.x;
    let count = in.layers.y;
    let compose_max = in.layers.z == 1u;
    for (var i = 0u; i < count; i++) {
        let layer = first + i;
        let cov = coverage(rc, ppe, layer);
        if (cov <= 0.0) {
            continue;
        }
        let c = paint(layer, rc);
        let p = vec4<f32>(c.rgb * c.a, c.a) * cov;
        if (compose_max) {
            out = max(out, p);
        } else {
            out = p + out * (1.0 - p.a);
        }
    }
    // Tint (the D3D diffuse colour modulates the texel, straight alpha).
    return vec4<f32>(out.rgb * in.tint.rgb * in.tint.a, out.a * in.tint.a);
}

// Untextured shapes: screen-space triangles with per-vertex colour.
struct ShapeOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs_shape(@location(0) pos: vec2<f32>, @location(1) color: vec4<f32>) -> ShapeOut {
    var out: ShapeOut;
    out.pos = vec4<f32>(pos * view.scale + view.offset, 0.5, 1.0);
    out.color = color;
    return out;
}

@fragment
fn fs_shape(in: ShapeOut) -> @location(0) vec4<f32> {
    return vec4<f32>(in.color.rgb * in.color.a, in.color.a);
}

// Bitmap sprites (original pixel art without a faithful vector version):
// a textured quad, one atlas texel per local unit, sampled "sharp
// bilinear": nearest inside a texel, linear across its edge over about one
// screen pixel, so it stays crisp at any scale without shimmering. The
// atlas is premultiplied, like the Slug path's output.
struct BitmapOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) texel: vec2<f32>,
    @location(1) @interpolate(flat) tint: vec4<f32>,
};

@vertex
fn vs_bitmap(@builtin(vertex_index) vi: u32, inst: Instance) -> BitmapOut {
    let corner = vec2<f32>(f32(vi & 1u), f32((vi >> 1u) & 1u));
    let local = mix(inst.bounds.xy, inst.bounds.zw, corner);
    let screen = vec2<f32>(
        local.x * inst.m0.x + local.y * inst.m0.z + inst.m1.x,
        local.x * inst.m0.y + local.y * inst.m0.w + inst.m1.y,
    );
    var out: BitmapOut;
    out.pos = vec4<f32>(screen * view.scale + view.offset, 0.5, 1.0);
    // layers = atlas rect (x, y, w, h)
    out.texel = vec2<f32>(inst.layers.xy) + corner * vec2<f32>(inst.layers.zw);
    out.tint = inst.tint;
    return out;
}

@fragment
fn fs_bitmap(in: BitmapOut) -> @location(0) vec4<f32> {
    let size = vec2<f32>(textureDimensions(bitmap_tex));
    let t = in.texel;
    let px_per_texel = 1.0 / max(fwidth(t), vec2<f32>(1.0 / 65536.0));
    let uv = (floor(t) + 0.5 + clamp((fract(t) - 0.5) * px_per_texel, vec2<f32>(-0.5), vec2<f32>(0.5))) / size;
    let c = textureSampleLevel(bitmap_tex, bitmap_sampler, uv, 0.0);
    return vec4<f32>(c.rgb * in.tint.rgb * in.tint.a, c.a * in.tint.a);
}
