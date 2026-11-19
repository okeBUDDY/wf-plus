//! Bitmap-font text (HLSTxtSys, hls_txt.cpp), drawn from the cooked font
//! glyphs. Strings are bytes in the game's encoding (Shift-JIS), so the
//! two-byte katakana font indexes exactly as the original did.

use super::Cx;
use crate::art::Art;
use crate::gfx::{Argb, Blend, DrawList, SpriteId, Transform};

/// HLSTxtSys::default_palette_ (0x45d280), for "@@<digit>".
pub const DEFAULT_PALETTE: [Argb; 10] = [
    Argb(0xff000000),
    Argb(0xff0000ff),
    Argb(0xffff0000),
    Argb(0xffff00ff),
    Argb(0xff00ff00),
    Argb(0xff00ffff),
    Argb(0xffff00ff),
    Argb(0xffffffff),
    Argb(0xff555555),
    Argb(0xffaaaaaa),
];

/// g_title_font_widths (0x45d100): the advance of each capital and '-'.
pub const TITLE_WIDTHS: [f32; 28] = [
    15.0, 13.0, 12.0, 13.0, 13.0, 13.0, 12.0, 14.0, 7.0, 13.0, 15.0, 13.0, 17.0, 14.0, 12.0, 13.0, 12.0, 13.0, 12.0, 14.0,
    14.0, 14.0, 19.0, 14.0, 14.0, 12.0, 7.0, 0.0,
];

/// Encodes `s` in the game's character set.
pub fn sjis(s: &str) -> Vec<u8> {
    encoding_rs::SHIFT_JIS.encode(s).0.into_owned()
}

pub struct Font {
    glyphs: Vec<Option<SpriteId>>,
    first_char: u16,
    char_width: f32,
    char_height: f32,
    space_width: f32,
    widths: Option<&'static [f32]>,
    mappings: Vec<(u16, u16)>,
}

impl Font {
    /// HLSTxtSys::Initialize: `count` glyphs from `first` (one or two bytes).
    fn new(art: &Art, name: &str, first: &str, count: usize, width: f32, height: f32, space_width: f32, widths: Option<&'static [f32]>) -> Self {
        let f = sjis(first);
        let first_char = if f.len() == 1 { f[0] as u16 } else { (f[0] as u16) << 8 | f[1] as u16 };
        let glyphs = (0..count).map(|i| art.find(&format!("font/{name}/{i:03}"))).collect();
        Font {
            glyphs,
            first_char,
            char_width: width,
            char_height: height,
            space_width: if space_width > 0.0 { space_width } else { width },
            widths,
            mappings: Vec::new(),
        }
    }

    fn two_byte(&self) -> bool {
        self.first_char > 0xff
    }

    /// HLSTxtSys::Map: draw `character` with glyph `index`.
    fn map(&mut self, character: &str, index: u16) {
        let c = sjis(character);
        let code = if self.two_byte() { (c[0] as u16) << 8 | c[1] as u16 } else { c[0] as u16 };
        self.mappings.push((code, self.first_char + index));
    }

    fn glyph_index(&self, code: u16) -> u16 {
        let code = self.mappings.iter().find(|m| m.0 == code).map_or(code, |m| m.1);
        code.wrapping_sub(self.first_char)
    }

    fn advance(&self, index: u16) -> f32 {
        self.widths.and_then(|w| w.get(index as usize).copied()).unwrap_or(self.char_width)
    }

    pub fn line_height(&self) -> f32 {
        self.char_height
    }

    /// HLSTxtSys::Print (0x40b180): at most `count` characters (all when
    /// negative) of `text` at (`x`, `y`). "@@<digit>" switches to that colour
    /// of `palette`, "@@" and anything else back to `color`.
    #[allow(clippy::too_many_arguments)]
    pub fn print(&self, draw: &mut DrawList, x: f32, y: f32, text: &[u8], palette: Option<&[Argb; 10]>, color: Argb, count: i32, blend: Blend) {
        if count == 0 {
            return;
        }
        let mut count = if count < 0 { i32::MAX } else { count };
        let palette = palette.unwrap_or(&DEFAULT_PALETTE);
        let mut current = color;
        let mut left = x;
        let mut y = y;
        let mut i = 0;
        while i < text.len() {
            match text[i] {
                0 => return,
                b'\n' => {
                    y += self.char_height;
                    left = x;
                }
                b'@' if text.get(i + 1) == Some(&b'@') => {
                    current = match text.get(i + 2) {
                        Some(d) if d.is_ascii_digit() => palette[(d - b'0') as usize],
                        _ => color,
                    };
                    i += 2;
                }
                b' ' => {
                    count -= 1;
                    if count <= 0 {
                        return;
                    }
                    left += self.space_width;
                }
                c => {
                    let code = if self.two_byte() {
                        let c2 = text.get(i + 1).copied().unwrap_or(0);
                        i += 1;
                        (c as u16) << 8 | c2 as u16
                    } else {
                        c as u16
                    };
                    let index = self.glyph_index(code);
                    if let Some(Some(glyph)) = self.glyphs.get(index as usize) {
                        draw.sprite(*glyph, &Transform::translation(left, y), current, blend);
                    }
                    count -= 1;
                    if count <= 0 {
                        return;
                    }
                    left += self.advance(index);
                }
            }
            i += 1;
        }
    }

    /// HLSTxtSys::Measure: the size of `text` in pixels.
    pub fn measure(&self, text: &[u8]) -> (f32, f32) {
        let (mut width, mut height, mut line) = (0.0f32, self.char_height, 0.0f32);
        let mut i = 0;
        while i < text.len() && text[i] != 0 {
            match text[i] {
                b'\n' => {
                    height += self.char_height;
                    width = width.max(line);
                    line = 0.0;
                }
                b'@' if text.get(i + 1) == Some(&b'@') => i += 2,
                b' ' => line += self.space_width,
                c => {
                    let code = if self.two_byte() {
                        i += 1;
                        (c as u16) << 8 | text.get(i).copied().unwrap_or(0) as u16
                    } else {
                        c as u16
                    };
                    line += if self.widths.is_some() { self.advance(self.glyph_index(code)) } else { self.char_width };
                }
            }
            i += 1;
        }
        width = width.max(line);
        if width == 0.0 { (0.0, 0.0) } else { (width, height) }
    }
}

/// The game's fonts (cGlobal::LoadFonts 0x401280).
pub struct Fonts {
    pub text: Font,
    pub small_text: Font,
    pub kana: Font,
    pub title: Font,
    pub large_digit: Font,
    pub digit: Font,
}

impl Fonts {
    pub fn load(art: &Art) -> Self {
        let mut f = Fonts {
            text: Font::new(art, "text", " ", 64, 16.0, 16.0, 0.0, None),
            small_text: Font::new(art, "small_text", " ", 64, 8.0, 8.0, 0.0, None),
            kana: Font::new(art, "kana", "ァ", 96, 16.0, 32.0, 0.0, None),
            title: Font::new(art, "title", "A", 28, 20.0, 32.0, 9.0, Some(&TITLE_WIDTHS)),
            large_digit: Font::new(art, "large_digit", "0", 10, 48.0, 80.0, 0.0, None),
            digit: Font::new(art, "digit", "0", 10, 24.0, 40.0, 0.0, None),
        };
        f.kana.map("　", 87);
        f.kana.map("ー", 88);
        f.kana.map("～", 89);
        f.kana.map("★", 90);
        f.kana.map("☆", 91);
        f.title.map("-", 26);
        f
    }
}

/// Prints with the default palette in normal blending.
pub fn print(cx: &mut Cx, font: fn(&Fonts) -> &Font, x: f32, y: f32, text: &str, color: Argb) {
    let bytes = sjis(text);
    let font = font(&cx.fonts);
    font.print(&mut cx.draw, x, y, &bytes, None, color, -1, Blend::Normal);
}
