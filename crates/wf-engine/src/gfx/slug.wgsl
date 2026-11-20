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
const FLAG_STROKE: u32 = 0x400u;

struct Instance {
    @location(0) bounds: vec4<f32>,   // dilated local quad (min, max)
    @location(1) m0: vec4<f32>,       // m00 m01 m10 m11
    @location(2) m1: vec2<f32>,       // m20 m21
    @location(3) tint: vec4<f32>,     // straight RGBA
    @location(4) layers: vec4<u32>,   // first layer, count, compose, line width (f32 bits)
};

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) local: vec2<f32>,
    @location(1) @interpolate(flat) tint: vec4<f32>,
    @location(2) @interpolate(flat) layers: vec4<u32>,
    // The instance's linear part, for the strokes' constant pixel width.
    @location(3) @interpolate(flat) m0: vec4<f32>,
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
    out.m0 = inst.m0;
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
        // The roots are (b.y -+ d) / a.y. One of the two differences
        // cancels when a.y is small next to b.y, as it is for a straight
        // edge whose a.y is only the rounding left by subtracting the
        // sample (an ulp of a 640-unit coordinate is far above 1/65536), so
        // that root comes from the product of the roots, p1.y / a.y,
        // instead.
        let d = sqrt(max(b.y * b.y - a.y * p1.y, 0.0));
        let q = b.y + select(-d, d, b.y >= 0.0);
        let near = p1.y / q;
        let far = q / a.y;
        if (b.y >= 0.0) {
            t1 = near;
            t2 = far;
        } else {
            t1 = far;
            t2 = near;
        }
    }
    return vec2<f32>((a.x * t1 - b.x * 2.0) * t1 + p1.x, (a.x * t2 - b.x * 2.0) * t2 + p1.x);
}

// A curve relative to the sample. A stroke edge is stored as centre points
// and signed unit normals; its corners go half the line width (in pixels of
// the 640x480 screen) out along the normals, converted to local units under the
// instance's transform, and its control point is their midpoint.
struct Quad {
    p1: vec2<f32>,
    p2: vec2<f32>,
    p3: vec2<f32>,
};

fn stroke_offset(n: vec2<f32>, m0: vec4<f32>, half_px: f32) -> vec2<f32> {
    // Logical (640x480) pixels per local unit along n: lines are one pixel
    // of the original screen wide, scaled with the window like the rest.
    let px_per_unit = length(n.x * m0.xy + n.y * m0.zw);
    return n * (half_px / max(px_per_unit, 1.0 / 65536.0));
}

fn load_curve(ci: u32, rc: vec2<f32>, stroke: bool, m0: vec4<f32>, half_px: f32) -> Quad {
    let a = textureLoad(curve_tex, tex2d(ci, CURVE_W), 0);
    let c = textureLoad(curve_tex, tex2d(ci + 1u, CURVE_W), 0);
    var q: Quad;
    if (stroke) {
        q.p1 = a.xy + stroke_offset(c.xy, m0, half_px) - rc;
        q.p3 = a.zw + stroke_offset(c.zw, m0, half_px) - rc;
        q.p2 = (q.p1 + q.p3) * 0.5;
    } else {
        q.p1 = a.xy - rc;
        q.p2 = a.zw - rc;
        q.p3 = c.xy - rc;
    }
    return q;
}

// The perf bench's counting renderer: the fragment outputs the curves its
// rays visited (in 64ths) instead of a colour.
override count_visits: bool = false;
var<private> visits: f32;

fn coverage(rc: vec2<f32>, ppe: vec2<f32>, layer: u32, m0: vec4<f32>, half_px: f32) -> f32 {
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
    // Stroke edges are sorted by their widest extent, not their drawn one:
    // no early exit for them.
    let stroke = (t1.w & FLAG_STROKE) != 0u;

    // A fill's ray goes to the nearer side of the layer's box: past the
    // curves sorted from that side (the band's second list), and stops at
    // the first curve wholly beyond the pixel's filter. The far side's
    // crossings sum to the near side's (a closed outline crosses the line
    // up as often as down), so the coverage is the same either way.
    var xcov = 0.0;
    var xwgt = 0.0;
    let hb = u32(clamp(i32(floor((rc.y - bbox.y) * band_scale.y)), 0, i32(hbands) - 1));
    let hh = band(band_base + hb);
    let hlow = !stroke && rc.x < (bbox.x + bbox.z) * 0.5;
    let hfirst = select(hh.y, hh.y + hh.x, hlow);
    for (var k = 0u; k < hh.x; k++) {
        visits += 1.0;
        let q = load_curve(band(hfirst + k).x, rc, stroke, m0, half_px);
        let p1 = q.p1;
        let p2 = q.p2;
        let p3 = q.p3;
        if (hlow) {
            if (min(min(p1.x, p2.x), p3.x) * ppe.x > 0.5) {
                break;
            }
        } else if (max(max(p1.x, p2.x), p3.x) * ppe.x < -0.5) {
            if (stroke) {
                continue;
            }
            break;
        }
        let code = root_code(p1.y, p2.y, p3.y);
        if (code != 0u) {
            let r = solve(p1, p2, p3) * ppe.x;
            if ((code & 1u) != 0u) {
                xcov += select(clamp(r.x + 0.5, 0.0, 1.0), -clamp(0.5 - r.x, 0.0, 1.0), hlow);
                xwgt = max(xwgt, clamp(1.0 - abs(r.x) * 2.0, 0.0, 1.0));
            }
            if (code > 1u) {
                xcov -= select(clamp(r.y + 0.5, 0.0, 1.0), -clamp(0.5 - r.y, 0.0, 1.0), hlow);
                xwgt = max(xwgt, clamp(1.0 - abs(r.y) * 2.0, 0.0, 1.0));
            }
        }
    }

    var ycov = 0.0;
    var ywgt = 0.0;
    let vb = u32(clamp(i32(floor((rc.x - bbox.x) * band_scale.x)), 0, i32(vbands) - 1));
    let vh = band(band_base + hbands + vb);
    let vlow = !stroke && rc.y < (bbox.y + bbox.w) * 0.5;
    let vfirst = select(vh.y, vh.y + vh.x, vlow);
    for (var k = 0u; k < vh.x; k++) {
        visits += 1.0;
        let q = load_curve(band(vfirst + k).x, rc, stroke, m0, half_px);
        // Swap the axes: the vertical ray is the horizontal case on (y, x).
        let p1 = q.p1.yx;
        let p2 = q.p2.yx;
        let p3 = q.p3.yx;
        if (vlow) {
            if (min(min(p1.x, p2.x), p3.x) * ppe.y > 0.5) {
                break;
            }
        } else if (max(max(p1.x, p2.x), p3.x) * ppe.y < -0.5) {
            if (stroke) {
                continue;
            }
            break;
        }
        let code = root_code(p1.y, p2.y, p3.y);
        if (code != 0u) {
            let r = solve(p1, p2, p3) * ppe.y;
            // The swap mirrors the plane, so the winding sign flips.
            if ((code & 1u) != 0u) {
                ycov -= select(clamp(r.x + 0.5, 0.0, 1.0), -clamp(0.5 - r.x, 0.0, 1.0), vlow);
                ywgt = max(ywgt, clamp(1.0 - abs(r.x) * 2.0, 0.0, 1.0));
            }
            if (code > 1u) {
                ycov += select(clamp(r.y + 0.5, 0.0, 1.0), -clamp(0.5 - r.y, 0.0, 1.0), vlow);
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

// SVG feTurbulence (SVG 1.1, 15.24) on the lattice the cooker baked into
// the layer texture at texel `base`: 256 texels of (selector, gradient x
// bits, gradient y bits, 0).
fn lattice(base: u32, i: i32) -> vec4<u32> {
    return textureLoad(layer_tex, tex2d(base + u32(i & 255), LAYER_W), 0);
}

fn grad(base: u32, b: i32) -> vec2<f32> {
    return bitcast<vec2<f32>>(lattice(base, b).yz);
}

fn s_curve(t: vec2<f32>) -> vec2<f32> {
    return t * t * (3.0 - 2.0 * t);
}

// The spec's noise2 for one channel, without stitching.
fn noise2(base: u32, v: vec2<f32>) -> f32 {
    let t = v + vec2<f32>(4096.0);
    let b0 = vec2<i32>(floor(t)) & vec2<i32>(255);
    let b1 = (b0 + vec2<i32>(1)) & vec2<i32>(255);
    let r0 = t - floor(t);
    let r1 = r0 - vec2<f32>(1.0);
    let i = i32(lattice(base, b0.x).x);
    let j = i32(lattice(base, b1.x).x);
    let b00 = i32(lattice(base, i + b0.y).x);
    let b10 = i32(lattice(base, j + b0.y).x);
    let b01 = i32(lattice(base, i + b1.y).x);
    let b11 = i32(lattice(base, j + b1.y).x);
    let s = s_curve(r0);
    let a = mix(dot(r0, grad(base, b00)), dot(vec2<f32>(r1.x, r0.y), grad(base, b10)), s.x);
    let b = mix(dot(vec2<f32>(r0.x, r1.y), grad(base, b01)), dot(r1, grad(base, b11)), s.x);
    return mix(a, b, s.y);
}

// type="fractalNoise" in 0..1. `params` is (base frequency, octaves, seed,
// amplitude); `p` is in the SVG's user space.
fn fractal(base: u32, p: vec2<f32>, params: vec4<f32>) -> f32 {
    var sum = 0.0;
    var ratio = 1.0;
    var v = p * params.x;
    let octaves = i32(params.y);
    for (var o = 0; o < octaves; o++) {
        sum += noise2(base, v) / ratio;
        v *= 2.0;
        ratio *= 2.0;
    }
    return (sum + 1.0) * 0.5;
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
        let base = layer_texel(layer, 5u).w;
        let origin = bitcast<vec2<f32>>(layer_texel(layer, 7u).zw);
        let k = 1.0 + n.w * (clamp(fractal(base, p + origin, n), 0.0, 1.0) - 0.52);
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
    // "Over" composes front to back, top layer first, and stops once the
    // pixel is opaque (to well under an 8-bit step): the layers below can
    // no longer show, so their coverage is never computed.
    for (var j = 0u; j < count; j++) {
        let i = select(j, count - 1u - j, !compose_max);
        let layer = first + i;
        let cov = coverage(rc, ppe, layer, in.m0, bitcast<f32>(in.layers.w) * 0.5);
        if (cov <= 0.0) {
            continue;
        }
        let c = paint(layer, rc);
        let p = vec4<f32>(c.rgb * c.a, c.a) * cov;
        if (compose_max) {
            out = max(out, p);
        } else {
            out += p * (1.0 - out.a);
            if (out.a >= 1.0 - 1.0 / 2048.0) {
                break;
            }
        }
    }
    if (count_visits) {
        return vec4<f32>(visits / 64.0, 0.0, 0.0, 0.0);
    }
    // Tint (the D3D diffuse colour modulates the texel, straight alpha).
    return vec4<f32>(out.rgb * in.tint.rgb * in.tint.a, out.a * in.tint.a);
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
    if (count_visits) {
        return vec4<f32>(0.0);
    }
    return vec4<f32>(c.rgb * in.tint.rgb * in.tint.a, c.a * in.tint.a);
}
