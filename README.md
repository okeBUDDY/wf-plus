# Warning Forever in Rust

A port of Warning Forever 1.08 (Hikoza T. Ohkubo, 2005) to Rust, 64-bit and
platform independent: SDL3 for the window, input and audio, wgpu for the GPU.
The decompilation in `../decomp` is the reference for behaviour; no Win32,
DirectX or HL-library code is emulated. The port starts above those layers,
at what the game asked of them.

The original game data is not part of this repository. You need `data.bin`
from the 1.08 release (in `wf108.zip`).

## Build and run

```
cargo build --release
target/release/wf --data path/to/data.bin
```

`data.bin` can also be given with `WF_DATA`, or put in the working directory
or next to the executable. `bgm.ogg` next to `data.bin` is played as the
music, as in the original.

Building needs the vector art and the font sheets. They are cooked into the
executable by `crates/wf/build.rs`. By default they are taken from the
project folder next to this repository (`../software/warning-forever/1.08`).
Override with `WF_ART_DIR` (the vector art: `<sheet>/<sheet>_<id>.svg`),
`WF_FONT_DIR` (`font0.png` .. `font2.png`) and `WF_BITMAP_DIR` (the cut
sprites, for the art that is still a bitmap). SDL3 is built from source
(`sdl3` crate, static), which needs CMake and a C compiler. On Linux it also
needs the X11/Wayland/PulseAudio development headers.

Other options: `--size WxH` (window size), `--frames N --screenshot out.png`
(save frame N), and `--headless --script FRAME:KEY,...` (no window: run N
frames with scripted keys and save the last one, used for the images in
`docs/`).

Controls: arrows or numpad to move. Z, X, S, D, A, W, E and F are logical
triggers 0-7 (Z shot, X slow, D weapon select, F pause). On a gamepad, the
left stick or d-pad moves and the face buttons, shoulders, Back and Start are
triggers 0-7. Esc quits and F12 resets.

## Layout

| Crate | What it is |
|---|---|
| `crates/wf-art` | The cooked art format: the GPU half (texture images uploaded as they are) and the CPU half (sprite table). |
| `crates/wf-cook` | The build-time cooker: SVG art -> Slug curves and bands, font sheets -> traced glyph outlines, bitmaps -> atlas. Also a `wf-cook` binary for inspecting a pack. |
| `crates/wf` | The game. |

Inside `crates/wf/src`:

| Module | |
|---|---|
| `game/` | The port. `app.rs` is Application::Run's frame. `task.rs` is TaskList with its RemoveUpTo/RemoveFrom sweeps. `fade.rs` is the screen-fade overlay, `text.rs` is HLSTxtSys, `input.rs` is the pads (HLInpDev) and `assets.rs` is what cGlobal loads. `viewer.rs` is a temporary first stage. |
| `gfx/` | **Seam**: `DrawList` (sprites under a 2D transform with an ARGB tint and normal/additive blend, untextured shapes, screen fill) in 640x480 space. `gpu.rs` + `slug.wgsl` is the wgpu backend. |
| `platform/` | **Seam**: `RawInput` (keys and gamepads held), `Event`. `sdl.rs` is the SDL3 backend (window, keyboard, SDL_Gamepad, audio stream). |
| `sound/` | The sound system (channels, groups, fades, the original's volume and pan curves) on a software mixer that the audio callback pulls from. Ogg via lewton. |
| `data/` | data.bin and the .pat pattern files. |
| `art.rs` | The built-in art pack. |

## Rendering

The art is drawn with the Slug algorithm (Lengyel, JCGT 2017). The cooker
turns each SVG path into quadratic curves and builds, per layer, horizontal
and vertical bands. Each band lists the curves that overlap it, sorted for
early exit. The fragment shader takes the pixel's band, casts a ray along
each axis, and turns the crossings into fractional coverage. Each instance is
one sprite. Its layers are painted (solid colour, linear or radial ramp, the
sys0_008 combined ramps, the add0_027 noise) and composed in the shader
(source-over, or max for `data-blend-layers="max"`). The result is
premultiplied, tinted like the original's diffuse colour, and blended once as
normal or additive. Blending is in gamma space, as on the original's
Direct3D 7.

The fonts have no hand-made vectors yet. The cooker traces their pixels into
exact outlines (one layer per alpha level), so they render pixel-exact at 1:1
and stay sharp when scaled. The title character (`etc2_001_character`) has no
faithful vector version. It is cooked as the original pixels into a bitmap
atlas and drawn as a textured quad with sharp-bilinear sampling, through the
same sprite call.

## Status

Done: the cooker and the Slug renderer, the main loop with the original
17 ms frame (58.8 fps), the task lists and stages, the screen fade, text,
pads, sound, and data.bin, .pat and .ogg loading. The viewer stage shows every
sprite and font and plays the sounds.

Next: the game's own stages (title, game, options, results), then the actors
and the boss generator, ported from the decompilation file by file.
`bgm.seq` and `bgm.wav` music, the profile (`wf.ini`) and the score and replay
files are not ported yet.
