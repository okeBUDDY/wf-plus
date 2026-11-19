#!/usr/bin/env python3
"""Warning Forever icon generator: icon-only W and F glyphs (assets/icon/glyphs,
the etc0 W and F redrawn to the original icon's proportions by glyphs.py),
both turned 45 degrees and cropped as in wf.exe's original 32x32 icon, over the
game's green background grid.

    python3 icon.py            # everything: Windows sizes + wf.ico, Android, icon-sheet.png
    python3 icon.py --quick    # only 32 and 256 next to the original -> quick.png (a second or two)
    python3 icon.py --out DIR  # write somewhere else (default: icon-out in the current folder)

All composition parameters live in PARAMS below; glyphs.py derives them,
together with the glyphs, from the original icon's pixel lines, so that the
32 px aliased rendering reproduces orig_0.png exactly.  Needs resvg, Pillow and numpy.
"""
import argparse, math, os, re, struct, subprocess, sys, tempfile, time

# ---------------------------------------------------------------------------
# Composition, in the original icon's 32 px space (from glyphs.py).
PARAMS = dict(
    ROTATE=45.0,    # degrees, both glyphs
    SCALE=0.176777, # icon px per glyph unit (1/4 px per unit along the diagonals)
    WX=17.5,        # where the W glyph's (0, 0) lands, icon px
    WY=-19.25,
    FDX=142.0,      # F offset from the W, glyph units
    FDY=86.0,
    GRID_D=3.0,     # the background grid, read off every green pixel of the original:
    GRID_S=12.0,    # lines x - y = GRID_D and x + y = GRID_S (continuous coords;
    GRID_STEP=16.0, # pixel index i - j = 3, i + j = 11) every GRID_STEP px: big diamonds
    MIN_GAP=2,      # aliased sizes: a stroke's two edges stay at least this many px apart
)
# Colours of the original (16-colour palette) for the aliased sizes, and the
# smooth sizes' equivalents.
CRISP = dict(bg='#000000', grid='#008000', line='#c0c0c0', vline='#808080')
SMOOTH = dict(bg='#000000', grid='#0b6e3e', line='#ffffff', vline='#a8a8a8')
SIZES = [16, 20, 24, 30, 32, 36, 40, 48, 60, 64, 72, 80, 96, 256]
CRISP_MAX = 48          # up to here: aliased 1 px lines like the original
NO_GRID_BELOW = 32      # 16 to 30: black background, no grid
ANDROID = 432           # xxxhdpi 108 dp layer
# ---------------------------------------------------------------------------

HERE = os.path.dirname(os.path.abspath(__file__))
VEC = os.path.join(os.environ.get('WF_ART_DIR', os.path.join(HERE, '..', '..', 'assets', '1.08', 'sprites', 'vector')), 'etc0', '')
RESVG = os.environ.get('RESVG', 'resvg')
DEFAULT_OUT = os.path.join(os.getcwd(), 'icon-out', '')
ORIG = next((p for p in (os.path.join(HERE, 'orig_0.png'), DEFAULT_OUT + 'tools/orig_0.png') if os.path.exists(p)), None)


GLYPHS = next((p for p in (os.path.join(HERE, '..', '..', 'assets', 'icon', 'glyphs'),   # wf-plus repo
                            os.path.join(HERE, '..', 'glyphs'), os.path.join(HERE, 'glyphs'))
               if os.path.exists(os.path.join(p, 'icon_w.svg'))), None)


def load(name):
    t = open(os.path.join(GLYPHS, name)).read()
    return (re.findall(r'd="([^"]+)"', t)[0], float(re.search(r'data-stroke="([\d.]+)"', t).group(1)),
            float(re.search(r'data-vgap="([\d.]+)"', t).group(1)))


(W, STROKE, VGAP), (F, _, _) = load('icon_w.svg'), load('icon_f.svg')


def polygon(d):
    """Vertices of a single-subpath 'M x y L x y ... Z' outline."""
    return [tuple(map(float, p)) for p in re.findall(r'([-\d.]+) ([-\d.]+)', d)]


# The W's V (notch and bottom outline) runs at 45 deg in the glyph, so it is
# axis-aligned in the icon; the original draws those edges grey.
def _diag_edges(d):
    g = polygon(d); n = len(g)
    return ' '.join(f'M{g[e][0]:g} {g[e][1]:g} L{g[(e + 1) % n][0]:g} {g[(e + 1) % n][1]:g}' for e in range(n)
                    if abs(abs(g[(e + 1) % n][0] - g[e][0]) - abs(g[(e + 1) % n][1] - g[e][1])) < 1e-3)


W_V = _diag_edges(W)


def svg(S, view=None, layers=('bg', 'grid', 'glyph'), mono=False, grid=None, P=PARAMS, lw=None):
    """An S px SVG.  `view` = (x, y, size) of the original's 32-unit space shown."""
    vx, vy, vs = view or (0.0, 0.0, 32.0)
    u = S / vs                                   # px per original-icon pixel
    crisp = S <= CRISP_MAX and view is None
    C = CRISP if crisp else SMOOTH
    if grid is None:
        grid = S >= NO_GRID_BELOW
    o = [f'<svg xmlns="http://www.w3.org/2000/svg" width="{S}" height="{S}" viewBox="0 0 {S} {S}"'
         + (' shape-rendering="crispEdges"' if crisp else '') + '>']
    if 'bg' in layers:
        o.append(f'<rect width="{S}" height="{S}" fill="{C["bg"]}"/>')
    if grid and 'grid' in layers:
        st = P['GRID_STEP']; gw = lw or (1 if crisp else max(1.0, S / 192)); d = []
        n = int(vs / st) + 8
        for i in range(-n, n + 1):
            c = (P['GRID_D'] - vx + vy + i * st) * u
            d.append(f'M{c - 2 * S:.3f} {-2 * S} L{c + 2 * S:.3f} {2 * S}')
            c2 = (P['GRID_S'] - vx - vy + i * st) * u
            d.append(f'M{c2 + 2 * S:.3f} {-2 * S} L{c2 - 2 * S:.3f} {2 * S}')
        o.append(f'<path d="{" ".join(d)}" stroke="{C["grid"]}" stroke-width="{gw}" fill="none"/>')
    if 'glyph' in layers:
        k = P['SCALE'] * u
        base = f'translate({(P["WX"] - vx) * u:.4f} {(P["WY"] - vy) * u:.4f}) rotate({P["ROTATE"]}) scale({k:.6f})'
        lw = lw or (1.0 if crisp else max(1.0, S / 128))
        for d, dx, dy, isw in ((F, P['FDX'], P['FDY'], False), (W, 0, 0, True)):
            t = base + f' translate({dx} {dy})'
            if mono:
                o.append(f'<path transform="{t}" d="{d}" fill="none" stroke="#fff" stroke-width="{2 * lw / k:.4f}"/>')
                continue
            o.append(f'<path transform="{t}" d="{d}" fill="#000" stroke="{C["line"]}" stroke-width="{lw / k:.4f}" stroke-linejoin="miter"/>')
            if isw:
                o.append(f'<path transform="{t}" d="{W_V}" fill="none" stroke="{C["vline"]}" stroke-width="{lw / k:.4f}" stroke-linecap="square"/>')
    o.append('</svg>')
    return '\n'.join(o)


def render(text, path):
    fd, tmp = tempfile.mkstemp(suffix='.svg'); os.write(fd, text.encode()); os.close(fd)
    try:
        subprocess.run([RESVG, tmp, path], check=True)
    finally:
        os.remove(tmp)


def crisp_grid(S, P=PARAMS):
    """The original's grid per pixel, scaled to S: at 32 exactly the original's
    i - j = 3 and i + j = 11 (mod 16) diagonals (numpy, no resvg)."""
    import numpy as np
    from PIL import Image
    a = np.zeros((S, S, 4), np.uint8); a[..., 3] = 255
    if S >= NO_GRID_BELOW:
        # Even period and even offset sum, so the two families cross on a pixel
        # (a clean X) rather than beside each other (a 2x2 blob); at most 1 px off.
        m = S / 32; p = 2 * round(P['GRID_STEP'] * m / 2)
        od = round(P['GRID_D'] * m); t = P['GRID_S'] * m - 1
        os_ = min((c for c in (math.floor(t) - 1, math.floor(t), math.ceil(t), math.ceil(t) + 1)
                   if (c + od) % 2 == 0), key=lambda c: abs(c - t))
        j, i = np.mgrid[0:S, 0:S]
        on = ((i - j - od) % p == 0) | ((i + j - os_) % p == 0)
        a[on] = (0, 128, 0, 255)
    return Image.fromarray(a, 'RGBA')


# Edge families after the 45 deg turn, as lines A*i + B*j = c in pixel-index
# space: glyph verticals -> '/' (i + j = c), glyph horizontals -> '\\' (j - i = c),
# the W's 45 deg V edges -> columns (i = c) or rows (j = c).
FAM = {'u': (1, 1), 'v': (-1, 1), 'x': (1, 0), 'y': (0, 1)}


def lattice_edges(S, P=PARAMS):
    """Each glyph edge snapped to the pixel lattice.  Every edge becomes an exact
    pixel line (one pixel per row on the diagonals); the two edges of a stroke
    are snapped together so their spacing is the same everywhere at this size;
    corners are the intersections of the snapped lines, so they always meet.
    Returns [(polygon vertices, [(family, c, P0, P1, grey)])] per glyph."""
    m = S / 32; k = P['SCALE'] * m; r = math.radians(P['ROTATE']); cs, sn = math.cos(r), math.sin(r)
    def scr(gx, gy):  # continuous screen coords
        return P['WX'] * m + k * (cs * gx - sn * gy), P['WY'] * m + k * (sn * gx + cs * gy)
    out = []
    for d, ox, oy, isw in ((F, P['FDX'], P['FDY'], False), (W, 0, 0, True)):
        g = [(x + ox, y + oy) for x, y in polygon(d)]; n = len(g)
        edges = []
        for e in range(n):
            (x0, y0), (x1, y1) = g[e], g[(e + 1) % n]
            dx, dy = x1 - x0, y1 - y0
            if abs(dx) < 1e-6: fam, gc = 'u', x0
            elif abs(dy) < 1e-6: fam, gc = 'v', y0
            elif abs(dx - dy) < 1e-3: fam, gc = 'x', x0 - y0
            else: fam, gc = 'y', x0 + y0
            X, Y = scr(x0, y0)
            sc = {'u': X + Y - 1, 'v': Y - X, 'x': X - 0.5, 'y': Y - 0.5}[fam]
            edges.append([fam, gc, sc, fam in 'xy' and isw])
        # Snap: the second edge of a stroke follows the first at a fixed spacing.
        snapped = {}
        for fam in 'uvxy':
            fe = sorted({(round(e[1], 3), e[2]) for e in edges if e[0] == fam})
            for gc, sc in fe:
                part = [(g2, s2) for g2, s2 in fe if (g2, s2) != (gc, sc) and g2 < gc
                        and abs((gc - g2) - (STROKE if fam in 'uv' else VGAP)) < 0.6]
                if part and (fam, part[0][0]) in snapped:
                    g2, s2 = part[0]
                    snapped[(fam, gc)] = snapped[(fam, g2)] + math.copysign(max(P['MIN_GAP'], round(abs(sc - s2))), sc - s2)
                else:
                    snapped[(fam, gc)] = round(sc)
        lines = [(e[0], snapped[(e[0], round(e[1], 3))], e[3]) for e in edges]
        def meet(l1, l2):
            (a1, b1), c1 = FAM[l1[0]], l1[1]; (a2, b2), c2 = FAM[l2[0]], l2[1]
            det = a1 * b2 - a2 * b1
            return ((c1 * b2 - c2 * b1) / det, (a1 * c2 - a2 * c1) / det)
        verts = [meet(lines[e - 1], lines[e]) for e in range(n)]   # vertex e starts edge e
        out.append((verts, [(lines[e][0], lines[e][1], verts[e], verts[(e + 1) % n], lines[e][2]) for e in range(n)]))
    return out


def edge_pixels(fam, c, p0, p1):
    """Integer pixels of line `fam` = c between p0 and p1, inclusive; at a corner
    whose vertex is off this line's pixel parity the line stops one short, so
    two diagonals meet as a flat two-pixel corner rather than crossing."""
    eps = 1e-6
    if fam in 'xy':
        t0, t1 = (p0[1], p1[1]) if fam == 'x' else (p0[0], p1[0])
        lo, hi = math.ceil(min(t0, t1) - eps), math.floor(max(t0, t1) + eps)
        return [(c, t) if fam == 'x' else (t, c) for t in range(lo, hi + 1)]
    T = (lambda p: p[1] - p[0]) if fam == 'u' else (lambda p: p[0] + p[1])
    lo, hi = math.ceil(min(T(p0), T(p1)) - eps), math.floor(max(T(p0), T(p1)) + eps)
    px = []
    for t in range(lo, hi + 1):
        if (t + c) % 2: continue
        px.append(((c - t) // 2, (c + t) // 2) if fam == 'u' else ((t - c) // 2, (t + c) // 2))
    return px


def crisp_glyphs(S, P=PARAMS):
    """Aliased glyphs on the pixel lattice over a transparent image: black
    interiors (hiding the grid, as in the original), silver edges, grey V."""
    import numpy as np
    from PIL import Image
    a = np.zeros((S, S, 4), np.uint8)
    rgb = lambda h: tuple(int(h[q:q + 2], 16) for q in (1, 3, 5)) + (255,)
    j, i = np.mgrid[0:S, 0:S].astype(float)
    for verts, edges in lattice_edges(S, P):
        inside = np.zeros((S, S), bool)              # even-odd test at pixel centres
        n = len(verts)
        for e in range(n):
            (x0, y0), (x1, y1) = verts[e], verts[(e + 1) % n]
            if y0 == y1: continue
            cross = ((y0 > j) != (y1 > j)) & (i < (x1 - x0) * (j - y0) / (y1 - y0) + x0)
            inside ^= cross
        a[inside] = (0, 0, 0, 255)
        for fam, c, p0, p1, grey in edges:
            col = rgb(CRISP['vline'] if grey else CRISP['line'])
            for x, y in edge_pixels(fam, int(c), p0, p1):
                if 0 <= x < S and 0 <= y < S:
                    a[y, x] = col
        # Where a grey V edge meets a silver one, the corner pixel is grey at a
        # reflex corner (the V notch) and silver at a convex one, as in the original.
        ar = sum(verts[q][0] * verts[(q + 1) % n][1] - verts[(q + 1) % n][0] * verts[q][1] for q in range(n))
        for q in range(n):
            g0, g1 = edges[q - 1][4], edges[q][4]
            x, y = verts[q]
            if g0 == g1 or x != int(x) or y != int(y) or not (0 <= x < S and 0 <= y < S): continue
            (xa, ya), (xb, yb) = verts[q - 1], verts[(q + 1) % n]
            turn = (x - xa) * (yb - y) - (y - ya) * (xb - x)
            a[int(y), int(x)] = rgb(CRISP['vline'] if turn * ar < 0 else CRISP['line'])
    return Image.fromarray(a, 'RGBA')


def windows_png(S, path):
    from PIL import Image
    if S > CRISP_MAX:
        render(svg(S), path); return
    Image.alpha_composite(crisp_grid(S), crisp_glyphs(S)).convert('RGB').save(path)


def write_ico(out):
    imgs = [open(f'{out}windows/wf_{s}.png', 'rb').read() for s in SIZES]
    ico = bytearray(struct.pack('<HHH', 0, 1, len(SIZES))); off = 6 + 16 * len(SIZES)
    for s, b in zip(SIZES, imgs):
        ico += struct.pack('<BBBBHHII', s % 256, s % 256, 0, 0, 1, 32, len(b), off); off += len(b)
    for b in imgs:
        ico += b
    open(out + 'wf.ico', 'wb').write(ico)


def android(out):
    """Adaptive icon, full bleed: the original's 32 px crop fills the 72 dp
    visible area (288 px) and the layers carry on 18 dp past it on every side."""
    from PIL import Image, ImageDraw
    A = out + 'android/'; os.makedirs(A, exist_ok=True)
    S = ANDROID; view = (-8.0, -8.0, 48.0)       # 32 units over 288 px, 8 more each side
    render(svg(S, view=view, layers=('bg', 'grid'), grid=True), A + 'ic_launcher_background.png')
    render(svg(S, view=view, layers=('glyph',)), A + 'ic_launcher_foreground.png')
    render(svg(S, view=view, layers=('glyph',), mono=True), A + 'ic_launcher_monochrome.png')
    full = Image.alpha_composite(Image.open(A + 'ic_launcher_background.png').convert('RGBA'),
                                 Image.open(A + 'ic_launcher_foreground.png').convert('RGBA'))
    c = S // 6; vis = full.crop((c, c, S - c, S - c))
    vis.resize((192, 192), Image.LANCZOS).save(A + 'ic_launcher.png')
    m = Image.new('L', (768, 768), 0); ImageDraw.Draw(m).ellipse((0, 0, 767, 767), fill=255)
    r = vis.resize((192, 192), Image.LANCZOS); r.putalpha(m.resize((192, 192), Image.LANCZOS))
    r.save(A + 'ic_launcher_round.png')
    vis.resize((512, 512), Image.LANCZOS).save(A + 'play_store_512.png')
    open(A + 'ic_launcher.xml', 'w').write('''<?xml version="1.0" encoding="utf-8"?>
<adaptive-icon xmlns:android="http://schemas.android.com/apk/res/android">
    <background android:drawable="@mipmap/ic_launcher_background"/>
    <foreground android:drawable="@mipmap/ic_launcher_foreground"/>
    <monochrome android:drawable="@mipmap/ic_launcher_monochrome"/>
</adaptive-icon>
''')


def diff_panel(mine, orig, n=8):
    """Exact per-pixel diff at n x: matching pixels dimmed, mismatches magenta.
    Returns (image, summary) with mismatch counts by the original's colour."""
    import numpy as np
    from PIL import Image
    a = np.asarray(mine.convert('RGB')).astype(int); b = np.asarray(orig.convert('RGB')).astype(int)
    bad = (a != b).any(-1)
    out = (b * 0.45).astype(np.uint8); out[bad] = (255, 0, 255)
    names = {(192, 192, 192): 'silver', (128, 128, 128): 'grey', (0, 128, 0): 'green', (0, 0, 0): 'black'}
    cnt = {k: 0 for k in names.values()}
    for y, x in zip(*np.nonzero(bad)):
        cnt[names.get(tuple(b[y, x]), 'black')] += 1
    tot = {v: int((b == np.array(k)).all(-1).sum()) for k, v in names.items()}
    summary = (f'exact diff: {int(bad.sum())} px differ ('
               + ', '.join(f'{k} {cnt[k]}/{tot[k]}' for k in ('silver', 'grey', 'green', 'black')) + ')')
    return Image.fromarray(out).resize((32 * n, 32 * n), Image.NEAREST), summary


def crisp_vs_smooth(sizes=None, n=112):
    """Each aliased size enlarged (nearest) above a smooth 8x render
    downscaled to the same size and enlarged: the shapes should read the same."""
    from PIL import Image
    sizes = sizes or [s for s in SIZES if s <= CRISP_MAX]
    im = Image.new('RGB', (len(sizes) * (n + 14), 2 * n + 10), '#2a2a2a')
    tmp = tempfile.mkdtemp()
    for q, S in enumerate(sizes):
        windows_png(S, f'{tmp}/c.png'); render(svg(8 * S, view=(0.0, 0.0, 32.0), lw=8 * S / 32, grid=S >= NO_GRID_BELOW), f'{tmp}/s.png')
        c = Image.open(f'{tmp}/c.png').convert('RGB').resize((n, n), Image.NEAREST)
        sm = Image.open(f'{tmp}/s.png').convert('RGB').resize((S, S), Image.LANCZOS).resize((n, n), Image.NEAREST)
        im.paste(c, (q * (n + 14), 0)); im.paste(sm, (q * (n + 14), n + 10))
    return im


def label(d, xy, s, font, fill='#ddd'):
    d.text(xy, s, font=font, fill=fill)


def fonts():
    from PIL import ImageFont
    try:
        return (ImageFont.truetype('/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf', 15),
                ImageFont.truetype('/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf', 20))
    except Exception:
        f = ImageFont.load_default(); return f, f


def comparison(m32, m256, orig):
    """Original | ours | diff at 8x (256 px each), plus our 256 rendering."""
    from PIL import Image, ImageDraw
    font, big = fonts()
    im = Image.new('RGB', (4 * 256 + 5 * 24, 256 + 120), '#2a2a2a'); d = ImageDraw.Draw(im)
    dp, summary = diff_panel(m32, orig)
    cols = [(orig.convert('RGB').resize((256, 256), Image.NEAREST), 'original wf.exe 32 px (8x)'),
            (m32.convert('RGB').resize((256, 256), Image.NEAREST), 'ours 32 px (8x)'),
            (dp, summary + '; magenta = differs, dimmed = matches'),
            (m256.convert('RGB'), 'ours 256 px')]
    x = 24
    for i, (p, s) in enumerate(cols):
        im.paste(p, (x, 60)); label(d, (x, 324), s if i != 2 else 'diff overlay', font)
        x += 256 + 24
    label(d, (24, 350), cols[2][1], font, '#bbb')
    im.paste(orig.convert('RGB'), (24, 16)); im.paste(m32.convert('RGB'), (64, 16))
    P = PARAMS
    label(d, (112, 20), f"SCALE {P['SCALE']}  W at ({P['WX']}, {P['WY']})  F offset ({P['FDX']}, {P['FDY']})  rotate {P['ROTATE']}", big, '#fff')
    return im


def sheet(out, orig):
    from PIL import Image, ImageDraw
    font, big = fonts()
    WD = 1500; sh = Image.new('RGB', (WD, 2400), '#2a2a2a'); d = ImageDraw.Draw(sh)
    label(d, (24, 18), 'Windows icon (wf.ico): actual size', big, '#fff')
    x = 24; y = 56
    for s in SIZES:
        im = Image.open(f'{out}windows/wf_{s}.png').convert('RGB')
        sh.paste(im, (x, y + 256 - s)); label(d, (x, y + 262), f'{s}', font, '#bbb'); x += s + 22
    label(d, (24, 350), 'Windows icon: each size enlarged to 128 px with nearest-neighbour (pixel grid visible)', big, '#fff')
    x = 24; y = 386
    for i, s in enumerate(SIZES[:-1]):
        im = Image.open(f'{out}windows/wf_{s}.png').convert('RGB').resize((128, 128), Image.NEAREST)
        if i == 9: x = 24; y += 166
        sh.paste(im, (x, y)); label(d, (x, y + 132), f'{s} px', font, '#bbb'); x += 128 + 18
    label(d, (24, 730), 'Android adaptive icon (432 px layers, xxxhdpi) under launcher masks: full bleed, 72 dp visible', big, '#fff')
    A = out + 'android/'
    bg = Image.open(A + 'ic_launcher_background.png').convert('RGBA'); fg = Image.open(A + 'ic_launcher_foreground.png').convert('RGBA')
    full = Image.alpha_composite(bg, fg)
    c = 72; vis = full.crop((c, c, 432 - c, 432 - c)).resize((288, 288), Image.LANCZOS)

    def mask(kind, n=288):
        N = n * 4; m = Image.new('L', (N, N), 0); md = ImageDraw.Draw(m)
        if kind == 'circle': md.ellipse((0, 0, N - 1, N - 1), fill=255)
        elif kind == 'squircle': md.rounded_rectangle((0, 0, N - 1, N - 1), radius=N * 0.32, fill=255)
        elif kind == 'rounded': md.rounded_rectangle((0, 0, N - 1, N - 1), radius=N * 0.12, fill=255)
        elif kind == 'teardrop':
            md.rounded_rectangle((0, 0, N - 1, N - 1), radius=N // 2, fill=255); md.rectangle((N // 2, N // 2, N - 1, N - 1), fill=255)
        return m.resize((n, n), Image.LANCZOS)
    x = 24; y = 770
    for kind in ('circle', 'squircle', 'rounded', 'teardrop'):
        p = Image.new('RGBA', (288, 288), (0, 0, 0, 0)); p.paste(vis, (0, 0), mask(kind)); sh.paste(p, (x, y), p)
        label(d, (x, y + 294), kind, font, '#bbb'); x += 288 + 40
    mono = Image.open(A + 'ic_launcher_monochrome.png').convert('RGBA')
    y2 = 1140; x = 24
    label(d, (24, y2 - 34), 'Layers (full 108 dp; yellow = 72 dp visible area, dashed = 66 dp safe zone) and the themed icon', big, '#fff')
    for im, name in ((bg, 'background'), (fg, 'foreground'), (mono, 'monochrome')):
        t = Image.alpha_composite(Image.new('RGBA', (432, 432), (60, 60, 60, 255)), im).resize((300, 300), Image.LANCZOS)
        td = ImageDraw.Draw(t); r = 300 * 66 / 108 / 2; q = 300 * 18 / 108
        td.rectangle((q, q, 300 - q, 300 - q), outline='#f0c040')
        for a in range(0, 360, 10):
            td.arc((150 - r, 150 - r, 150 + r, 150 + r), a, a + 5, fill='#f0c040')
        sh.paste(t, (x, y2)); label(d, (x, y2 + 306), name, font, '#bbb'); x += 340
    th = Image.new('RGBA', (288, 288), (0, 0, 0, 0)); circ = Image.new('RGBA', (288, 288), (205, 225, 200, 255))
    alpha = mono.crop((c, c, 432 - c, 432 - c)).resize((288, 288), Image.LANCZOS).split()[3]
    circ.paste(Image.new('RGBA', (288, 288), (40, 70, 40, 255)), (0, 0), alpha)
    th.paste(circ, (0, 0), mask('circle')); t2 = th.resize((200, 200), Image.LANCZOS)
    sh.paste(t2, (x + 20, y2 + 50), t2); label(d, (x + 20, y2 + 306), 'themed (Android 13+)', font, '#bbb')
    label(d, (24, 1500), 'Aliased sizes 16-48 (top) against a smooth 8x render downscaled to the same size (bottom)', big, '#fff')
    sh.paste(crisp_vs_smooth(), (24, 1536))
    y3 = 1800
    label(d, (24, y3), 'Ours against the original wf.exe icon (32 px, actual size and 8x) with a diff overlay', big, '#fff')
    if orig is not None:
        cmp = comparison(Image.open(f'{out}windows/wf_32.png'), Image.open(f'{out}windows/wf_256.png'), orig)
        sh.paste(cmp, (0, y3 + 30))
    sh = sh.crop((0, 0, WD, y3 + 30 + 380))
    sh.save(out + 'icon-sheet.png')


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--quick', action='store_true', help='only 32 and 256 beside the original -> <out>quick.png')
    ap.add_argument('--out', default=DEFAULT_OUT)
    args = ap.parse_args()
    out = args.out if args.out.endswith('/') else args.out + '/'
    from PIL import Image
    orig = Image.open(ORIG) if ORIG else None
    t0 = time.time()
    os.makedirs(out + 'windows', exist_ok=True)
    if args.quick:
        tmp = tempfile.mkdtemp()
        windows_png(32, tmp + '/32.png'); windows_png(256, tmp + '/256.png')
        cmp = comparison(Image.open(tmp + '/32.png'), Image.open(tmp + '/256.png'),
                         orig if orig is not None else Image.new('RGB', (32, 32)))
        cmp.save(out + 'quick.png')
        print(f'quick: {out}quick.png in {time.time() - t0:.2f}s')
        return
    for s in SIZES:
        windows_png(s, f'{out}windows/wf_{s}.png')
    write_ico(out)
    android(out)
    sheet(out, orig)
    print(f'all: {len(SIZES)} Windows sizes + wf.ico, Android, icon-sheet.png in {time.time() - t0:.2f}s')


if __name__ == '__main__':
    main()
