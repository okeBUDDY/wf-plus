# Art from Warning Forever 1.08

The inputs `crates/wf/build.rs` cooks into the executable, laid out as in the
version 1.08 analysis folder they were taken from:

| Path | What |
|---|---|
| `sprites/vector/<sheet>/` | The sprites (add0, enm0, etc0, etc1, etc2, sys0) redrawn as SVG. |
| `sprites/vector/fonts/` | The fonts' glyphs redrawn as SVG (text, small text, digits, title, kana, the font2 bars). |
| `sprites/fonts/glyphs.csv` | Which glyph of which font sheet each SVG replaces. |
| `sprites/sheets/etc2/etc2_001_character.png` | The original title character, the one sprite still drawn as a bitmap. |

The vector art was redrawn from the original sprites; the PNG is the
original game's image. All of it is derived from Warning Forever
(© Hikoza T. Ohkubo, Hikware) and is under no licence from this repository.
