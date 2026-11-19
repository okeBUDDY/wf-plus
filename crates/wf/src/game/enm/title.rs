//! The title of the planned boss (enm_title.cpp): chosen from the modes of
//! its sides and its weapons, and written in Japanese or English. The word
//! tables are wf.exe's (addresses below).

use super::{EnmParam, EnmTitle};
use crate::game::math::Rand;
use crate::game::text::{Font, sjis};

/// g_title_words_ja (0x45d5e0): (word, makes the title alone).
const TITLE_WORDS_JA: [(&str, bool); 26] = [
    ("ピュアハート", false), ("アタック", false), ("ワイド", false), ("アーマード", false), ("アタッカー", false),
    ("ワイダー", false), ("アーマー", false), ("ダブル", false), ("バスタード", false), ("ウイング", false),
    ("トルネード", false), ("ストライカー", false), ("アームズ", false), ("ディフェンダー", false), ("ムラマサ", true),
    ("フェニックス", false), ("バーサーカー", false), ("スコルピオン", false), ("アスラ", true), ("ヘカトンケイル", false),
    ("ビッグマグナム", false), ("ハンマーヘッド", false), ("イセワン", true), ("カタストロフィ", true), ("アルマゲドン", true),
    ("インフィニティ", true),
];
/// g_weapon_words_ja (0x45d6b0).
const WEAPON_WORDS_JA: [&str; 8] = ["バルカン", "キャノン", "ビーム", "ポッド", "スマッシュ", "ボマー", "ニードル", "フォース"];
/// g_level_words_ja (0x45d6d0).
const LEVEL_WORDS_JA: [&str; 21] = [
    "", "", "ツイン", "トライ", "テトラ", "ペンタ", "ヘキサ", "セヴン", "オクタ", "ナイン", "デカ", "ウンデカ", "ドデカ", "", "ヘクト", "", "メガ", "",
    "ギガ", "", "テラ",
];
/// g_variant_words_ja (0x45d728).
const VARIANT_WORDS_JA: [&str; 21] = [
    "カスタム", "ツヴァイ", "オメガ", "グレート", "スーパー", "マキシマム", "パーフェクト", "フルサイズ", "デス", "エックス", "ファイナル",
    "リターンズ", "ヘル", "ファンタジー", "エキサイト", "クレイジー", "ブラボー", "エクセレント", "スペシャル", "ゴンザレス", "エターナル",
];
/// g_title_words_en (0x45d780).
const TITLE_WORDS_EN: [(&str, bool); 26] = [
    ("PURE HEART", false), ("ATTACK ", false), ("WIDE ", false), ("ARMORED ", false), ("ATTACKER", false), ("WIDER", false),
    ("ARMOR", false), ("DOUBLE ", false), ("BUSTERD ", false), ("WING ", false), ("TORNADO ", false), ("STRIKER", false),
    ("ARMS", false), ("DEFENDER", false), ("MURAMASA", true), ("PHOENIX", false), ("BERSERKER", false), ("SCORPION", false),
    ("ASURA", true), ("HEKATONCHEIR", false), ("BIG MAGNUM", false), ("HAMMER HEAD", false), ("ISE-ONE", true),
    ("CATASTROPHE", true), ("ARMAGEDDON", true), ("INFINITY", true),
];
/// g_weapon_words_en (0x45d850).
const WEAPON_WORDS_EN: [&str; 8] = ["VULCAN", "CANNON", "BEAM", "POD", "SMASH", "BOMBER", "NEEDLE", "FORCE"];
/// g_level_words_en (0x45d870).
const LEVEL_WORDS_EN: [&str; 21] = [
    "", "", "TWIN-", "TRI-", "TETRA-", "PENTA-", "HEXA-", "SEVEN-", "OCTA-", "NINE-", "DECA-", "UNDECA-", "DODECA-", "", "HECTO-", "",
    "MEGA-", "", "GIGA-", "", "TERA-",
];
/// g_variant_words_en (0x45d8c8).
const VARIANT_WORDS_EN: [&str; 21] = [
    " CUSTOM", " ZWEI", " OMEGA", " GREAT", " SUPER", " MAXIMUM", " PERFECT", " FULL SIZE", " DEATH", " X", " FINAL", " RETURNS",
    " HELL", " FANTASY", " EXCITE", " CRAZY", " BRAVO", " EXCELLENT", " SPECIAL", " GONZALEZ", " ETERNAL",
];

fn mode_word(mode: i16, words: [i8; 3]) -> Option<i8> {
    match mode {
        1 => Some(words[0]),
        2 => Some(words[1]),
        4 => Some(words[2]),
        _ => None,
    }
}

impl EnmParam {
    /// CEnmParam::ChooseTitle (0x417c10).
    pub fn choose_title(&mut self, rng: &mut Rand) {
        let mut title = EnmTitle { words: [-1, -1], weapon: -1, level: -1, variant: -1 };
        let s = self.state.sides;
        if s[0].mode == 0 && s[1].mode == 0 {
            title.words[0] = 0;
        } else if s[0].mode == 3 || s[1].mode == 3 {
            title.words[0] = self.state.title.words[0];
            if s[0].mode == 4 {
                if s[1].progress >= 100 {
                    title.words[0] = 0x17;
                }
            } else if s[1].mode == 4 {
                if s[0].progress >= 100 {
                    title.words[0] = 0x18;
                }
            } else if s[0].progress >= 100 && s[1].progress >= 100 {
                title.words[0] = 0x19;
            }
        } else if s[0].progress >= 100 && s[1].progress >= 100 {
            let row = match s[0].mode {
                1 => Some([0xe, 0xf, 0x10]),
                2 => Some([0x11, 0x12, 0x13]),
                4 => Some([0x14, 0x15, 0x16]),
                _ => None,
            };
            if let Some(w) = row.and_then(|r| mode_word(s[1].mode, r)) {
                title.words[0] = w;
            }
        } else {
            if s[0].progress >= 0x41 {
                if let Some(w) = mode_word(s[0].mode, [0xb, 0xc, 0xd]) {
                    title.words[1] = w;
                }
            } else if s[0].mode != 0 {
                if let Some(w) = mode_word(s[0].mode, [4, 5, 6]) {
                    title.words[1] = w;
                }
            }
            if s[1].progress >= 0x41 {
                if let Some(w) = mode_word(s[1].mode, [8, 9, 0xa]) {
                    title.words[0] = w;
                }
            } else if s[1].mode != 0 {
                if s[0].mode == 0 {
                    if let Some(w) = mode_word(s[1].mode, [4, 5, 6]) {
                        title.words[0] = w;
                    }
                } else if s[1].mode == s[0].mode && s[0].progress < 0x41 {
                    title.words[0] = 7;
                } else if let Some(w) = mode_word(s[1].mode, [1, 2, 3]) {
                    title.words[0] = w;
                }
            }
        }

        let count = self.title_count as usize;
        let mut i = (0..count).find(|&i| self.titles[i].words == title.words).unwrap_or(count);
        if i != count {
            let parts = &self.u10c[0..7];
            if self.new_weapon >= 0 && parts[self.new_weapon as usize] != 0 {
                title.weapon = self.new_weapon as i16;
                title.level = (parts[self.new_weapon as usize] * 2) as i16;
            } else {
                let mut most = -1;
                let mut main = -1i32;
                let mut kinds = 0;
                for (j, &n) in parts.iter().enumerate() {
                    if n > 0 {
                        if n > most {
                            most = n;
                            main = j as i32;
                        }
                        kinds += 1;
                    }
                }
                let close = (0..7).any(|j| j as i32 != main && most - parts[j] < 3);
                if !close {
                    title.weapon = main as i16;
                    title.level = (most * 2) as i16;
                } else {
                    title.weapon = 7;
                    title.level = kinds as i16;
                }
            }
            if title.level > 20 {
                title.level = 20;
            }
            i = (0..count)
                .find(|&i| {
                    let t = &self.titles[i];
                    t.words == title.words && t.weapon == title.weapon && t.level == title.level
                })
                .unwrap_or(count);
        }
        if i == count {
            if count >= 32 {
                title.variant = 20;
            } else {
                self.titles[count] = title;
                self.title_count += 1;
            }
        } else {
            let seen = &mut self.titles[i];
            if seen.variant >= 0 {
                seen.variant += 1;
                if seen.variant >= 20 {
                    seen.variant = 0;
                }
            } else {
                seen.variant = (rng.rand() % 20) as i16;
            }
            title.variant = seen.variant;
        }
        self.state.title = title;
    }

    /// CEnmParam::FormatTitleJa (0x418030), in Shift-JIS.
    pub fn format_title_ja(&self) -> Vec<u8> {
        let t = self.state.title;
        let mut name = Vec::new();
        if t.words[0] >= 0 {
            name.extend(sjis(TITLE_WORDS_JA[t.words[0] as usize].0));
        }
        if t.words[1] >= 0 {
            name.extend(sjis(TITLE_WORDS_JA[t.words[1] as usize].0));
        }
        let mut weapon = Vec::new();
        if t.level >= 0 {
            weapon.extend(sjis(LEVEL_WORDS_JA[t.level.min(20) as usize]));
        }
        if t.weapon >= 0 {
            weapon.extend(sjis(WEAPON_WORDS_JA[t.weapon.min(7) as usize]));
        }
        let the = |w: &[u8]| sjis(if w.len() >= 2 && w[0] == 0x83 && (0x40..=0x49).contains(&w[1]) { "ジ" } else { "ザ" });
        let space = sjis("　");
        let mut out = Vec::new();
        let length;
        if t.level < 0 {
            out.extend(the(&name));
            out.extend(&space);
            out.extend(&name);
            length = name.len() + 4;
        } else if TITLE_WORDS_JA[t.words[0].max(0) as usize].1 {
            let l = name.len() + weapon.len() + 6;
            out.extend(&name);
            out.extend(&space);
            out.extend(the(&weapon));
            if l > 30 {
                out.push(b'\n');
                length = weapon.len();
            } else {
                out.extend(&space);
                length = l;
            }
            out.extend(&weapon);
        } else {
            let of = if t.weapon == 7 { sjis("ド") } else { Vec::new() };
            let l = weapon.len() + of.len() + name.len();
            out.extend(&weapon);
            out.extend(&of);
            if l > 30 {
                out.push(b'\n');
                length = name.len();
            } else {
                length = l;
            }
            out.extend(&name);
        }
        if t.variant >= 0 {
            let variant = sjis(VARIANT_WORDS_JA[t.variant.min(20) as usize]);
            if variant.len() + length + 2 > 30 {
                out.push(b'\n');
            } else {
                out.extend(&space);
            }
            out.extend(variant);
        }
        out
    }

    /// CEnmParam::FormatTitleEn (0x418400): broken where it is wider than
    /// 240 pixels in the title font.
    pub fn format_title_en(&self, font: &Font) -> Vec<u8> {
        let t = self.state.title;
        let first = t.words[0];
        let mut name = String::new();
        if first >= 0 {
            name += TITLE_WORDS_EN[first as usize].0;
        }
        if t.words[1] >= 0 {
            name += TITLE_WORDS_EN[t.words[1] as usize].0;
        }
        let mut weapon = String::new();
        if t.level >= 0 {
            weapon += LEVEL_WORDS_EN[t.level.min(20) as usize];
        }
        if t.weapon >= 0 {
            weapon += WEAPON_WORDS_EN[t.weapon.min(7) as usize];
        }
        let mut out: Vec<u8>;
        let wide = |b: &[u8]| font.measure(b).0 >= 240.0;
        if t.level < 0 {
            out = format!("THE {name}").into_bytes();
        } else if TITLE_WORDS_EN[first.max(0) as usize].1 {
            out = format!("{name} WITH {weapon}S").into_bytes();
            if wide(&out) {
                out[name.len()] = b'\n';
            }
        } else {
            let ed = if weapon.ends_with('E') { "D" } else { "ED" };
            out = format!("{weapon}{ed} {name}").into_bytes();
            if wide(&out) {
                let at = out.len() - name.len() - 1;
                out[at] = b'\n';
            }
        }
        if t.variant >= 0 {
            let variant = VARIANT_WORDS_EN[t.variant.min(20) as usize];
            out.extend(variant.as_bytes());
            if wide(&out) {
                let at = out.len() - variant.len();
                out[at] = b'\n';
            }
        }
        out
    }
}
