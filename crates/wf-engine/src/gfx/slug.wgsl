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
    // Device pixels per logical pixel.
    px: f32,
    // The logical point at the target's top left corner.
    origin: vec2<f32>,
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

// The sprite cache (gfx/cache.rs): a sprite's Slug layers rasterized once
// per scale bucket into an atlas page (premultiplied), drawn as a textured
// quad under the instance's transform. The page is at least as fine as the
// screen (the bucket is the next 1.25x step at or above the instance's
// largest scale), so linear filtering only ever shrinks it. The tint
// modulates it as it does the Slug path's output.
@group(1) @binding(0) var atlas_tex: texture_2d<f32>;
@group(1) @binding(1) var atlas_sampler: sampler;

struct CachedOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) @interpolate(flat) tint: vec4<f32>,
};

@vertex
fn vs_cached(@builtin(vertex_index) vi: u32, inst: Instance) -> CachedOut {
    let corner = vec2<f32>(f32(vi & 1u), f32((vi >> 1u) & 1u));
    let local = mix(inst.bounds.xy, inst.bounds.zw, corner);
    let screen = vec2<f32>(
        local.x * inst.m0.x + local.y * inst.m0.z + inst.m1.x,
        local.x * inst.m0.y + local.y * inst.m0.w + inst.m1.y,
    );
    var out: CachedOut;
    out.pos = vec4<f32>(screen * view.scale + view.offset, 0.5, 1.0);
    // layers = the page texel of the quad's min corner (x, y) and texels
    // per local unit (f32 bits).
    let texel = bitcast<vec2<f32>>(inst.layers.xy) + (local - inst.bounds.xy) * bitcast<f32>(inst.layers.z);
    out.uv = texel / vec2<f32>(textureDimensions(atlas_tex));
    out.tint = inst.tint;
    return out;
}

@fragment
fn fs_cached(in: CachedOut) -> @location(0) vec4<f32> {
    // A one-pixel box: four taps a quarter pixel from the centre, each
    // bilinear over half-pixel texels (the page is 2x the screen or more).
    let dx = dpdx(in.uv) * 0.25;
    let dy = dpdy(in.uv) * 0.25;
    let c = (textureSampleLevel(atlas_tex, atlas_sampler, in.uv + dx + dy, 0.0)
        + textureSampleLevel(atlas_tex, atlas_sampler, in.uv + dx - dy, 0.0)
        + textureSampleLevel(atlas_tex, atlas_sampler, in.uv - dx + dy, 0.0)
        + textureSampleLevel(atlas_tex, atlas_sampler, in.uv - dx - dy, 0.0)) * 0.25;
    if (count_visits) {
        // Four texel fetches, counted apart from the curve visits (green).
        return vec4<f32>(0.0, 4.0 / 64.0, 0.0, 0.0);
    }
    return vec4<f32>(c.rgb * in.tint.rgb * in.tint.a, c.a * in.tint.a);
}

// Line shapes (gfx/lines.rs): one thin quad per segment. The coverage is
// Slug's for the segment's stroke rectangle (the same corners, crossings,
// weights and blend as `coverage` above, on the four edges directly), plus
// the rectangles of the neighbours sharing its endpoints, so that joints
// saturate as in Slug; the lowest-numbered segment covering a pixel draws
// it.
struct LineIn {
    @location(0) seg: vec4<f32>,   // a, b (local)
    @location(1) nbr: vec4<f32>,   // the neighbours' other endpoints
    @location(2) m0: vec4<f32>,
    @location(3) m1: vec2<f32>,
    @location(4) flags: u32,
    @location(5) tint: vec4<f32>,
};

struct LineOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) local: vec2<f32>,
    @location(1) @interpolate(flat) seg: vec4<f32>,
    @location(2) @interpolate(flat) nbr: vec4<f32>,
    @location(3) @interpolate(flat) m0: vec4<f32>,
    @location(4) @interpolate(flat) flags: u32,
    @location(5) @interpolate(flat) tint: vec4<f32>,
};

const LINE_HALF: f32 = 0.5;
const HAS_NBR_A: u32 = 1u;
const HAS_NBR_B: u32 = 2u;
const NBR_A_FIRST: u32 = 4u;
const NBR_B_FIRST: u32 = 8u;

// Logical pixels per local unit along unit vector `v`.
fn px_along(v: vec2<f32>, m0: vec4<f32>) -> f32 {
    return length(v.x * m0.xy + v.y * m0.zw);
}

@vertex
fn vs_line(@builtin(vertex_index) vi: u32, inst: LineIn) -> LineOut {
    let a = inst.seg.xy;
    let b = inst.seg.zw;
    let d = b - a;
    let len = max(length(d), 1e-12);
    let dir = d / len;
    let n = vec2<f32>(-dir.y, dir.x);
    // Half the line across, plus a device pixel and a half of margin for
    // the antialiasing (both ways along the segment too).
    let pn = max(px_along(n, inst.m0), 1.0 / 65536.0);
    let pd = max(px_along(dir, inst.m0), 1.0 / 65536.0);
    let k = max(view.px, 1e-6);
    let across = LINE_HALF / pn + 1.5 / (pn * k);
    let ext = 1.5 / (pd * k);
    let corner = vec2<f32>(f32(vi & 1u), f32((vi >> 1u) & 1u));
    let local = a + dir * mix(-ext, len + ext, corner.x) + n * mix(-across, across, corner.y);
    let screen = vec2<f32>(
        local.x * inst.m0.x + local.y * inst.m0.z + inst.m1.x,
        local.x * inst.m0.y + local.y * inst.m0.w + inst.m1.y,
    );
    var out: LineOut;
    out.pos = vec4<f32>(screen * view.scale + view.offset, 0.5, 1.0);
    out.local = local;
    out.seg = inst.seg;
    out.nbr = inst.nbr;
    out.m0 = inst.m0;
    out.flags = inst.flags;
    out.tint = inst.tint;
    return out;
}

struct Crossings {
    xcov: f32,
    xwgt: f32,
    ycov: f32,
    ywgt: f32,
};

// One straight stroke edge (p1 to p3, relative to the sample) on both rays,
// as `coverage` does for a curve whose control point is the midpoint.
fn cross_edge(c: ptr<function, Crossings>, p1: vec2<f32>, p3: vec2<f32>, ppe: vec2<f32>) {
    cross_curve(c, p1, (p1 + p3) * 0.5, p3, ppe);
}

// One curve (relative to the sample) on both rays.
fn cross_curve(c: ptr<function, Crossings>, p1: vec2<f32>, p2: vec2<f32>, p3: vec2<f32>, ppe: vec2<f32>) {
    let hcode = root_code(p1.y, p2.y, p3.y);
    if (hcode != 0u) {
        let r = solve(p1, p2, p3) * ppe.x;
        if ((hcode & 1u) != 0u) {
            (*c).xcov += clamp(r.x + 0.5, 0.0, 1.0);
            (*c).xwgt = max((*c).xwgt, clamp(1.0 - abs(r.x) * 2.0, 0.0, 1.0));
        }
        if (hcode > 1u) {
            (*c).xcov -= clamp(r.y + 0.5, 0.0, 1.0);
            (*c).xwgt = max((*c).xwgt, clamp(1.0 - abs(r.y) * 2.0, 0.0, 1.0));
        }
    }
    let s1 = p1.yx;
    let s2 = p2.yx;
    let s3 = p3.yx;
    let vcode = root_code(s1.y, s2.y, s3.y);
    if (vcode != 0u) {
        let r = solve(s1, s2, s3) * ppe.y;
        if ((vcode & 1u) != 0u) {
            (*c).ycov -= clamp(r.x + 0.5, 0.0, 1.0);
            (*c).ywgt = max((*c).ywgt, clamp(1.0 - abs(r.x) * 2.0, 0.0, 1.0));
        }
        if (vcode > 1u) {
            (*c).ycov += clamp(r.y + 0.5, 0.0, 1.0);
            (*c).ywgt = max((*c).ywgt, clamp(1.0 - abs(r.y) * 2.0, 0.0, 1.0));
        }
    }
}

// The stroke rectangle of segment a..b: the cooker's four edges (a to b on
// the left, b's end, b to a on the right, a's end), their corners offset
// half a line along the segment's normal as `load_curve` does.
fn cross_rect(a: vec2<f32>, b: vec2<f32>, rc: vec2<f32>, m0: vec4<f32>, ppe: vec2<f32>) -> Crossings {
    var c = Crossings(0.0, 0.0, 0.0, 0.0);
    let d = b - a;
    let len = length(d);
    let n = vec2<f32>(-d.y / len, d.x / len);
    let o = stroke_offset(n, m0, LINE_HALF);
    let al = a + o - rc;
    let bl = b + o - rc;
    let br = b - o - rc;
    let ar = a - o - rc;
    cross_edge(&c, al, bl, ppe);
    cross_edge(&c, bl, br, ppe);
    cross_edge(&c, br, ar, ppe);
    cross_edge(&c, ar, al, ppe);
    return c;
}

fn crossings_coverage(c: Crossings) -> f32 {
    let blended = abs(c.xcov * c.xwgt + c.ycov * c.ywgt) / max(c.xwgt + c.ywgt, 1.0 / 65536.0);
    return clamp(max(blended, min(abs(c.xcov), abs(c.ycov))), 0.0, 1.0);
}

fn add_crossings(a: Crossings, b: Crossings) -> Crossings {
    return Crossings(a.xcov + b.xcov, max(a.xwgt, b.xwgt), a.ycov + b.ycov, max(a.ywgt, b.ywgt));
}

@fragment
fn fs_line(in: LineOut) -> @location(0) vec4<f32> {
    let rc = in.local;
    let ppe = 1.0 / max(fwidth(rc), vec2<f32>(1.0 / 65536.0));
    let a = in.seg.xy;
    let b = in.seg.zw;
    let own = cross_rect(a, b, rc, in.m0, ppe);
    var total = own;
    var drawn = crossings_coverage(own) > 0.0;
    var rects = 1.0;
    if ((in.flags & HAS_NBR_A) != 0u) {
        let c = cross_rect(in.nbr.xy, a, rc, in.m0, ppe);
        total = add_crossings(total, c);
        rects += 1.0;
        if ((in.flags & NBR_A_FIRST) != 0u && crossings_coverage(c) > 0.0) {
            drawn = false;
        }
    }
    if ((in.flags & HAS_NBR_B) != 0u) {
        let c = cross_rect(b, in.nbr.zw, rc, in.m0, ppe);
        total = add_crossings(total, c);
        rects += 1.0;
        if ((in.flags & NBR_B_FIRST) != 0u && crossings_coverage(c) > 0.0) {
            drawn = false;
        }
    }
    if (count_visits) {
        // Edge tests (four a rectangle), counted apart (blue).
        return vec4<f32>(0.0, 0.0, rects * 4.0 / 64.0, 0.0);
    }
    if (!drawn) {
        return vec4<f32>(0.0);
    }
    let cov = crossings_coverage(total);
    return vec4<f32>(in.tint.rgb * in.tint.a * cov, in.tint.a * cov);
}

// Fill shapes (gfx/fills.rs): one instance per triangle, rasterized grown
// by a pixel and a quarter. The coverage sums the crossings of all the
// shape's triangles' edges, as Slug does for the cooked outline; the
// lowest-numbered triangle whose triangle grown by three quarters of a
// pixel holds the pixel draws it. The triangles (local points) are in the
// curve texture's fill row, two texels each.
struct FillIn {
    @location(0) m0: vec4<f32>,
    @location(1) m1: vec2<f32>,
    @location(2) tint: vec4<f32>,
    @location(3) tris: vec4<u32>,  // this triangle, the shape's first, count
};

struct FillOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) local: vec2<f32>,
    @location(2) @interpolate(flat) m0: vec4<f32>,
    @location(3) @interpolate(flat) m1: vec2<f32>,
    @location(4) @interpolate(flat) tint: vec4<f32>,
    @location(5) @interpolate(flat) tris: vec4<u32>,
};

// Triangle `t`'s points under the transform (logical pixels).
fn fill_screen(t: u32, m0: vec4<f32>, m1: vec2<f32>) -> array<vec2<f32>, 3> {
    let a = textureLoad(curve_tex, tex2d(2u * t, CURVE_W), 0);
    let c = textureLoad(curve_tex, tex2d(2u * t + 1u, CURVE_W), 0);
    return array<vec2<f32>, 3>(
        a.x * m0.xy + a.y * m0.zw + m1,
        a.z * m0.xy + a.w * m0.zw + m1,
        c.x * m0.xy + c.y * m0.zw + m1,
    );
}

// The outward unit normal of edge p -> q of a triangle of orientation `o`.
fn out_normal(p: vec2<f32>, q: vec2<f32>, o: f32) -> vec2<f32> {
    let e = q - p;
    return vec2<f32>(e.y, -e.x) * (o / max(length(e), 1e-12));
}

fn orientation(s: array<vec2<f32>, 3>) -> f32 {
    let area = (s[1].x - s[0].x) * (s[2].y - s[0].y) - (s[1].y - s[0].y) * (s[2].x - s[0].x);
    return select(-1.0, 1.0, area > 0.0);
}

// Whether target pixel `p` is inside triangle `s0` (logical pixels) grown
// by `r` target pixels. Every fragment of a pixel runs it on the same
// numbers (the pixel's exact centre, not an interpolated point), so the
// triangles agree on which one owns the pixel.
fn in_grown(p: vec2<f32>, s0: array<vec2<f32>, 3>, r: f32) -> bool {
    let s = array<vec2<f32>, 3>((s0[0] - view.origin) * view.px, (s0[1] - view.origin) * view.px, (s0[2] - view.origin) * view.px);
    let o = orientation(s);
    return dot(p - s[0], out_normal(s[0], s[1], o)) <= r
        && dot(p - s[1], out_normal(s[1], s[2], o)) <= r
        && dot(p - s[2], out_normal(s[2], s[0], o)) <= r;
}

@vertex
fn vs_fill(@builtin(vertex_index) vi: u32, inst: FillIn) -> FillOut {
    var s = fill_screen(inst.tris.x, inst.m0, inst.m1);
    let o = orientation(s);
    let i = vi % 3u;
    let prev = s[(i + 2u) % 3u];
    let v = s[i];
    let next = s[(i + 1u) % 3u];
    let n1 = out_normal(prev, v, o);
    let n2 = out_normal(v, next, o);
    // The corner where both edges, moved out by a pixel and a quarter, meet.
    let r = 1.25 / max(view.px, 1e-6);
    let screen = v + (n1 + n2) * (r / max(1.0 + dot(n1, n2), 1e-4));
    // Back to local units.
    let d = screen - inst.m1;
    let det = inst.m0.x * inst.m0.w - inst.m0.z * inst.m0.y;
    let local = vec2<f32>(inst.m0.w * d.x - inst.m0.z * d.y, inst.m0.x * d.y - inst.m0.y * d.x) / det;
    var out: FillOut;
    out.pos = vec4<f32>(screen * view.scale + view.offset, 0.5, 1.0);
    out.local = local;
    out.m0 = inst.m0;
    out.m1 = inst.m1;
    out.tint = inst.tint;
    out.tris = inst.tris;
    return out;
}

@fragment
fn fs_fill(in: FillOut) -> @location(0) vec4<f32> {
    let rc = in.local;
    let ppe = 1.0 / max(fwidth(rc), vec2<f32>(1.0 / 65536.0));
    let r = 0.75;
    let own = in.tris.x;
    let first = in.tris.y;
    let count = in.tris.z;
    var drawn = in_grown(in.pos.xy, fill_screen(own, in.m0, in.m1), r);
    for (var t = first; t < own; t++) {
        if (in_grown(in.pos.xy, fill_screen(t, in.m0, in.m1), r)) {
            drawn = false;
        }
    }
    var c = Crossings(0.0, 0.0, 0.0, 0.0);
    for (var t = first; t < first + count; t++) {
        let a = textureLoad(curve_tex, tex2d(2u * t, CURVE_W), 0);
        let b = textureLoad(curve_tex, tex2d(2u * t + 1u, CURVE_W), 0);
        // The control points as the cooker makes them (the midpoints, in
        // local units), so that a sample on a shared edge counts as in Slug.
        let p0 = a.xy - rc;
        let p1 = a.zw - rc;
        let p2 = b.xy - rc;
        // An edge two triangles share (once each way) cancels: skipped, as
        // a sample right on it could count one way and not the other.
        let skip = u32(b.z);
        if ((skip & 1u) == 0u) {
            cross_curve(&c, p0, (a.xy + a.zw) * 0.5 - rc, p1, ppe);
        }
        if ((skip & 2u) == 0u) {
            cross_curve(&c, p1, (a.zw + b.xy) * 0.5 - rc, p2, ppe);
        }
        if ((skip & 4u) == 0u) {
            cross_curve(&c, p2, (b.xy + a.xy) * 0.5 - rc, p0, ppe);
        }
    }
    if (count_visits) {
        // Edge and ownership tests, counted apart (blue).
        return vec4<f32>(0.0, 0.0, f32(3u * count + own - first + 1u) / 64.0, 0.0);
    }
    if (!drawn) {
        return vec4<f32>(0.0);
    }
    let cov = crossings_coverage(c);
    return vec4<f32>(in.tint.rgb * in.tint.a * cov, in.tint.a * cov);
}
