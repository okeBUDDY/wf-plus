#!/usr/bin/env python3
"""Icon-only W and F glyphs, drawn to the original wf.exe icon's proportions.

The game's etc0 W and F have 14-unit strokes; the hand-drawn icon's strokes
are much thicker relative to the letters.  These glyphs keep the etc0 style
(128x208 units, a silhouette at 20% plus a 1-unit border, edges through unit
centres, axis-aligned and 45-degree edges only) but take every visible edge
from the original's pixel lines.

With both glyphs turned 45 deg at a = k*sqrt(2) = 1/4 icon px per unit, an
edge lands on an exact pixel line (pixel index i, j; u = i + j, v = j - i):
    glyph x = gx  ->  u = U0 + gx/4          glyph y = gy  ->  v = V0 + gy/4
    gx - gy = c   ->  i = X0 + c/8           gx + gy = c   ->  j = Y0 + c/8
Lines read off orig_0.png (see original-annotated.png):
    silver /  i+j = 23, 29 (W right stroke), 33, 39 (F stem)
    silver \\  j-i = -15, -9 (F top bar), 9, 15 (F middle bar and W bottom)
    grey   rows j = 5, 9 and columns i = 8, 4 (the W's V: notch and bottom)
Choosing W right stroke outer x = 127 and W bottom y = 207 fixes U0 = -2.75,
V0 = -36.75, hence X0 = 17, Y0 = -19.75 and the composition below; every other
edge then follows exactly:
    W right stroke 103..127 (24 units = 6 px), V: notch arms gx+gy = 198,
    gx-gy = -72 (apex 63,135), bottom arms gx+gy = 230, gx-gy = -104 (apex 63,167)
    F (offset 142, 86): stem 1..25, top bar 1..25, middle bar 97..121
Two edges the original leaves out of its crop pin parts that are not seen:
the W's left stroke must have its inner edge left of u = -0.5 (gx < 9), or it
would draw i + j = 3 across the top-left corner, where the original has grid
and black; so the left leg is 1..8 wide (the hand-drawn W is lopsided, its
left leg is off the icon).  The F's top-right drop must end above v = -4
(y <= 41) or it shows at the right corner (pixels (31,28), (30,29), (31,30));
so it is 16 units deep.  Unseen parts keep the etc0 proportions: the F's bar
runs 72 past the stem.

    python3 glyphs.py OUTDIR    # writes icon_w.svg, icon_f.svg; prints PARAMS
"""
import math, os, sys

A = 0.25
U0, V0 = -2.75, -36.75
WX = (U0 + 1 - V0) / 2                # u = WX + WY - 1 + a gx,  v = WY - WX + a gy
WY = (U0 + 1 + V0) / 2
PARAMS = dict(ROTATE=45.0, SCALE=round(A / math.sqrt(2), 6), WX=WX, WY=WY, FDX=142.0, FDY=86.0)

W = [(1, 207), (1, 1), (8, 1), (8, 190), (63, 135), (103, 175), (103, 1), (127, 1),
     (127, 207), (103, 207), (63, 167), (23, 207)]
F = [(127, 41), (103, 41), (103, 25), (25, 25), (25, 97), (97, 97), (97, 121), (25, 121),
     (25, 207), (1, 207), (1, 1), (127, 1)]
STROKE, VGAP = 24, 32                 # stroke width; the V's gap in gx +- gy


def area(p):
    return sum(p[i][0] * p[(i + 1) % len(p)][1] - p[(i + 1) % len(p)][0] * p[i][1] for i in range(len(p))) / 2


def inset(p, d=1.0):
    """The polygon moved inwards by d (each edge offset, consecutive lines met)."""
    n = len(p); sgn = 1 if area(p) > 0 else -1; lines = []
    for i in range(n):
        (x0, y0), (x1, y1) = p[i], p[(i + 1) % n]
        L = math.hypot(x1 - x0, y1 - y0); nx, ny = -(y1 - y0) / L * sgn, (x1 - x0) / L * sgn
        lines.append(((x0 + nx * d, y0 + ny * d), (x1 - x0, y1 - y0)))
    out = []
    for i in range(n):
        (p1, d1), (p2, d2) = lines[i - 1], lines[i]
        det = d1[0] * d2[1] - d1[1] * d2[0]
        t = ((p2[0] - p1[0]) * d2[1] - (p2[1] - p1[1]) * d2[0]) / det
        out.append((p1[0] + t * d1[0], p1[1] + t * d1[1]))
    return out


def fmt(p):
    return ' '.join(('M' if i == 0 else 'L') + f'{x:g} {y:g}'.replace('.000', '') for i, (x, y) in
                    enumerate((round(x, 3), round(y, 3)) for x, y in p)) + ' Z'


def svg(p, note):
    d = fmt(p)
    return (f'<svg xmlns="http://www.w3.org/2000/svg" width="128" height="208" viewBox="0 0 128 208" '
            f'data-stroke="{STROKE}" data-vgap="{VGAP}" data-note="{note}">\n'
            f'<path fill="#ffffff" fill-opacity="0.200" d="{d}"/>\n'
            f'<path fill="#ffffff" fill-rule="evenodd" d="{d} {fmt(inset(p)[::-1])}"/>\n</svg>\n')


if __name__ == '__main__':
    out = sys.argv[1] if len(sys.argv) > 1 else '.'
    os.makedirs(out, exist_ok=True)
    note = 'icon-only glyph: the etc0 %s redrawn to the original wf.exe icon proportions (tools/icon/glyphs.py)'
    open(os.path.join(out, 'icon_w.svg'), 'w').write(svg(W, note % 'W'))
    open(os.path.join(out, 'icon_f.svg'), 'w').write(svg(F, note % 'F'))
    print(PARAMS)
