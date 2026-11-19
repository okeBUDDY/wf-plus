# Icon

The game's icon: the W and F logo vectors (etc0_001, etc0_002) in the
original icon's composition, turned 45° and cropped, over the game's grid.

- `wf.ico`: 14 sizes, 16 to 256 px, PNG entries. Embedded as the Windows
  exe's icon by `crates/wf/build.rs` (winresource, Windows hosts only).
- `windows/wf_<size>.png`: the same sizes as PNGs. `wf_256.png` is the
  window icon, set at start-up through the platform layer.

`glyphs/icon_w.svg`, `glyphs/icon_f.svg`: icon-only W and F, the etc0 letters
redrawn to the original icon's proportions (24-unit strokes, its V depth and
bar positions); the game's own etc0 glyphs are unchanged. Rendered by the
32 px aliased path they reproduce the original wf.exe icon pixel for pixel.

`tools/icon/` has the generator (Python with Pillow and numpy, and resvg at
the path the script names). `glyphs.py` derives the icon glyphs and the
composition from the original's pixel lines and writes the two SVGs.
`icon.py` makes every size, the .ico, the Android adaptive icon and a
contact sheet with an exact diff against the original; `icon.py --quick`
renders only 32 and 256 px beside the original. All composition parameters
are in `PARAMS` at its top. The scripts read the original icon (not in this
repository) from beside themselves; pass `--out` for the output folder.
