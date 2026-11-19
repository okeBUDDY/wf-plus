# Warning Forever in Rust

A port of Warning Forever 1.08 (Hikoza T. Ohkubo, 2005) to Rust, 64-bit and
platform independent: SDL3 for the window, input and audio, wgpu for the GPU.
The decompilation of the original is the reference for behaviour (read only,
not part of this repository). No Win32, DirectX or HL-library code is
emulated: the port starts above those layers, at what the game asked of them.

The whole game is ported: title, options, the game with every boss part and
the boss generator, the HUD, game over and name entry, the score table,
replays, and the music sequencer. It plays the original's own replays back
frame-exact (see [Verification](#verification)).

| | |
|---|---|
| ![title](docs/title.png) | ![classic arena in a 16:9 window](docs/game.png) |
| ![HD arena](docs/hd.png) | ![boss](docs/boss.png) |
| ![explosions](docs/explosions.png) | ![name entry, HD scores](docs/name-entry.png) |
| ![options: game](docs/options.png) | ![options: display](docs/options-display.png) |
| ![options: controls](docs/options-controls.png) | ![fonts](docs/viewer-fonts.png) |

The Classic arena in a 21:9 window, with its border:
![classic arena border](docs/arena-border.png)

The screenshots are made by `scripts/docs-screenshots.sh`.

## The game data

The original game data is not part of this repository. You need `data.bin`
from the 1.08 release (`wf108.zip`). It is the only runtime input; `wf.exe`
is never read, at run time or at build time. Constants that came from
`wf.exe` (tables, literals) are in the source as `const` tables with their
address cited.

`data.bin` is looked up in this order: `--data <path>`, `WF_DATA`,
`./data.bin`, then next to the executable. Music is `bgm.seq`, `bgm.ogg` or
`bgm.wav` next to `data.bin`, as in the original.

Settings (`wf.ini`), the score table (`wf.sco`, the original's format) and
replays (`rec/*.rec`, the original's format) live in the home folder:
`--home <dir>`, else `WF_HOME`, else the working directory. The original's
own `wf.ini`, `wf.sco` and `.rec` files work as they are. The port's own
settings (display, mouse and touch, the last arena) go to `wf-plus.ini` beside
them, and HD scores to `wf-hd.sco`; `wf.ini` keeps only the
original's keys and values.

## Arena modes

After GAME START the title asks for the arena; the choice is remembered.

* **Classic** is the original's 640x480 arena, exactly: its replays and
  scores are the original's formats, byte for byte. In a window of another
  shape it is drawn 4:3 in the middle, the background grid running into the
  margins and a faint border around the arena.
* **HD** is 480 high and as wide as the window's shape (clamped to
  270..1920). Every screen-derived value (player and boss bounds, spawn and
  aim points, culling, the grid, the HUD's anchors) comes from the arena in
  one place (`game/arena.rs`). The arena is locked when the game starts and
  stored in the recording (version 1103, which the original refuses), so
  replays and resizes keep it, letter- or pillarboxed. HD scores have
  their own table with each entry's arena size.

## Build and run

```
cargo build --release
target/release/wf --data path/to/data.bin
```

SDL3 is built from source (`sdl3` crate, static), which needs CMake and a C
compiler; on Linux also the X11/Wayland/PulseAudio development headers.

Building needs the vector art and the font sheets, which `crates/wf/build.rs`
cooks into the executable. By default they come from the project folder next
to this repository (`../software/warning-forever/1.08`). Overrides:

| Variable | What |
|---|---|
| `WF_ART_DIR` | The vector art, `<sheet>/<sheet>_<id>.svg`. |
| `WF_FONT_DIR` | `font0.png` .. `font2.png`, traced when a glyph has no SVG. |
| `WF_BITMAP_DIR` | The cut sprites, for the art that stays a bitmap (the title character). |
| `WF_GLYPH_DIR`, `WF_GLYPH_PATTERNS`, `WF_GLYPH_CSV` | Where the vector glyph SVGs are and how they are named (`;`-separated patterns using `{font}`, `{index}`, `{sheet}`, `{x}`, `{y}`, `{csv}`). |

Options:

| | |
|---|---|
| `--size WxH` | Window size (default 1280x720, resizable; the arena is fitted and centred). |
| `--arena classic\|hd` | The arena of new games (else the last choice). |
| `--fps-cap off\|vsync\|N` | Frame cap (default off: uncapped, interpolated). |
| `--fullscreen`, `--borderless`, `--windowed` | Window mode. |
| `--render-scale K` | Render at K times the window's pixels, then stretch. |
| `--show-fps` | Show the frame rate. |
| `--bench SECONDS` | With `--headless`: measure the uncapped frame rate offscreen. |
| `--replay rec/<file>.rec` | Play a replay (relative to the home folder). |
| `--headless` | No window and no audio device: run frames as fast as possible. |
| `--frames N --screenshot out.png` | Stop after frame N and save it. |
| `--script FRAME[-LAST]:KEY,...` | Hold keys over frames (up, down, left, right, z, x, d, f, q, e, pageup, pagedown, esc, f12). |
| `--hash-log out.txt` | Write a hash of the game state after every game frame (desync hunting). |
| `--viewer` | The art and sound viewer instead of the game. |
| `WF_LOG=error\|warn\|info\|debug` | Log level (stage results, game over, file errors). |

## Controls

Input is action based: every device makes the same actions, and the game
reduces them each tick to the original's eight directions and eight
triggers, which is all a `.rec` stores. Devices work side by side, and the
last one used picks the prompts.

* Keyboard: arrows (or numpad) move; Z shoots, X moves slowly, D selects the
  weapon, F pauses (when enabled). Esc quits, F12 resets to the title.
* Mouse: the ship steers toward the pointer; left shoots, right moves
  slowly, middle selects the weapon (all rebindable).
* Gamepad (SDL_Gamepad, hot-plugged): left stick or d-pad, with a dead zone;
  buttons are triggers. A gamepad touchpad drags the ship.
* Touch: drag anywhere to move the ship relative to the finger; a finger
  down shoots.

The options screen is a tab strip (GAME, DISPLAY, AUDIO, CONTROLS,
RECORDS): Q/E, PageUp/PageDown, the shoulder buttons, a click or the wheel
over the strip, or a sideways swipe change tabs. Keys and pad buttons are
remapped separately there, along with the mouse buttons, mouse steering,
the pad dead zone and the touch speed; the display settings (window mode,
resolution, render scale, FPS cap, show FPS) apply at once.

## Architecture and seams

| Crate | |
|---|---|
| `crates/wf-art` | The cooked art format: a GPU half (texture images uploaded as they are) and a CPU half (the sprite table). |
| `crates/wf-cook` | The build-time cooker: SVG art to Slug curves and bands, vector or traced glyphs, the bitmap atlas, the baked feTurbulence lattice. |
| `crates/wf-engine` | The engine: app loop, time, renderer, layout, input, audio mixer, config, asset registry, platform (SDL3). |
| `crates/wf` | The game: deterministic state that emits a draw list each tick. |

The engine owns the loop. The game advances in fixed 17 ms ticks from an
accumulator (at most 8 per frame), exactly in the original's order and float
order; the display runs uncapped by default (Mailbox or Immediate present)
or capped (VSync on Fifo, or 60/120/144/240/custom by a sleep-then-spin
limiter), and draws between the last two ticks' snapshots with alpha
interpolation. Interpolation only reads the snapshots: draws are matched
by key, and snap across teleports, wraps, flips and sharp turns. A
`Layout` fits the arena into the window (DPI-aware, render scale by an
offscreen target), with background draws over the whole window and arena
draws scissored to the arena. Instances go through a persistent three-deep
ring buffer.

| Engine module | |
|---|---|
| `app.rs` | The `Game` trait, the windowed loop, headless ticks and the bench, the arena border. |
| `time.rs` | `FixedStep`, `FpsCap`, `Limiter`, `FpsCounter`. |
| `gfx/` | `DrawList` (the snapshot), `Layout`, interpolation, the wgpu Slug renderer (`gpu.rs`, `slug.wgsl`), the render-scale blit (`scale.rs`). |
| `input.rs` | Actions, bindings (`wf-plus.ini`), and the device providers. |
| `platform/` | **Seam**: `RawInput` (keys, gamepads, mouse, fingers, last device), events, window modes and icon; `sdl.rs` is the SDL3 backend. |
| `audio/` | The software mixer the audio callback pulls from. |
| `assets.rs` | The asset registry over mounted sources (data.bin, directories). |
| `config.rs` | `wf-plus.ini`, display settings. |
| `log.rs` | **Seam**: logging (`WF_LOG`). |

Inside `crates/wf/src`:

| Module | |
|---|---|
| `game/` | The port, file by file from the decompilation: `app.rs` (Application::Run), `task.rs` (task lists), the stages `fn_title.rs`, `fn_game.rs`, `fn_option.rs`, `hud.rs` (game HUD, game over, name entry, results), `actor/` (actor system and every control), `enm/` (boss generator), `replay.rs`, `records.rs`, `score_table.rs`, `profile.rs` (portable ini), `math.rs` (the f32/x87 maths), `debug.rs` (state hash); and the port's own `arena.rs` (arena modes), `settings.rs` (`wf-plus.ini` settings, menu input), `input.rs` (actions to the original's input bits). |
| `data/` | data.bin and the .pat pattern files. |
| `main.rs` | The command line, and `WfGame`, the game as the engine drives it. |

Icons are in `assets/icon` (the window icon, and the exe's on Windows).

### Bit-exact simulation

The simulation keeps the original's float behaviour: f32 maths in the
decompilation's operation order, no fused multiply-add, transcendentals as
`(x as f64).fn() as f32` with `fn` from the pure-Rust `libm` crate (the same
bits on every platform), `_ftol` truncation, and the C runtime's `rand()`.
The sine table is baked from the x87's FSIN and checked against the x87 (with
24-bit precision control) by `game/x87_tests.rs` on x86-64, along with
libm's `sin`, `cos` and `atan2` over sweeps, and `sqrt`.

### Rendering

Art is drawn with the Slug algorithm (Lengyel, JCGT 2017). The cooker turns
each SVG path into quadratic curves and builds horizontal and vertical bands
per layer; the fragment shader casts a ray along each axis and turns the
crossings into coverage. Layers are painted (solid, linear or radial ramp,
the sys0_008 combined ramps, and add0_027's SVG `feTurbulence` fractal noise,
evaluated on the spec's lattice, which the cooker bakes per seed) and
composed in the shader, then blended once, normal or additive, in gamma space
as on the original's Direct3D 7. Untextured shapes go through Slug too. The
constant point tables (`gfx/shape_table.rs`, addresses cited) are cooked as
sprites `shape/<name>` in local coordinates, transformed per instance on the
GPU like any sprite. Lines are stroke layers: each edge is stored as centre
points plus unit normals, and the shader offsets them by half a pixel under
the instance's transform, so lines stay one pixel wide at any scale, as
Direct3D 7's did. Shapes whose points come from game logic (lasers, the game
over chart, the options' bars) are rebuilt on the CPU into a small dynamic
region of the textures, only when their points change. Glyphs come from vector SVGs
when present, else from exact traces of the font sheets. The title character
stays the original pixels in a bitmap atlas, drawn with sharp-bilinear
sampling.

## Verification

* `cargo test --workspace` runs the unit tests (score table, profile,
  sequencer, WAV, turbulence lattice, and the x87 checks), and
  `cargo clippy --workspace --all-targets` is clean.
* `scripts/playtest.sh [recordings] [data.bin]` plays every `stageNN.rec` of
  every recording made with the original game, headless. It checks that each
  stage ends on the score the next stage's recording starts with, and that
  the last ends on the score in that run's `wf.sco`. All 24 stages of the six
  recordings match: normal, 3 ships, 5-minute attack and sudden death,
  including a 19,085-frame stage with pauses.
* `--hash-log` gives identical files across runs of the same replay.
* `scripts/hd-determinism.sh [data.bin]` records an HD game in
  4:3, 16:9, 21:9 and portrait windows and replays each at its own size and
  another: the hash logs match and each replay ends on the live result.
* Uncapped frame rate, headless and offscreen (`--bench 5`), on this
  project's CI container (lavapipe, a 4-core CPU rasterizer, so it measures
  pixel fill): 5 fps at 1280x720 and 71 fps at 320x180. A GPU is far faster.

## Known gaps

* Vector glyphs depend on the glyph SVG set; missing or unreadable ones are
  traced from the font sheets, with a build warning.
* As in 1.08, a game over whose score does not rank fades straight to the
  title, and `best.rec` is never written.
