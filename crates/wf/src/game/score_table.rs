//! ScoreTable (score_table.cpp, g_score_table): the eight best scores of
//! the current rules, kept in wf.sco with one table per rule set. The file
//! format is the original's, so old score files keep working.

use super::game_state::{GameRules, ScoreEntry};
use super::profile::Profile;
use std::path::PathBuf;

pub const ENTRIES: usize = 8;
const FILE_VERSION: i32 = 0x67;
const RECORD_SIZE: usize = std::mem::size_of::<GameRules>() + ENTRIES * std::mem::size_of::<ScoreEntry>();

/// "1ST" .. "8TH" (0x45de80).
/// A name in streamer mode: as wide as the name field.
pub const MASK: &[u8; 3] = b"***";
pub const RANK_NAMES: [&str; ENTRIES] = ["1ST", "2ND", "3RD", "4TH", "5TH", "6TH", "7TH", "8TH"];

#[derive(Clone, Debug)]
pub struct ScoreTable {
    rules: GameRules,
    pub entries: [ScoreEntry; ENTRIES],
    rank: i32,
    path: Option<PathBuf>,
    /// The HD table (the port's own file): each entry's arena,
    /// stored after the entries of its record. None for Classic.
    pub arenas: Option<[[u16; 2]; ENTRIES]>,
    /// The arena of the entry `insert` adds.
    pub arena: [u16; 2],
}

impl Default for ScoreTable {
    fn default() -> Self {
        ScoreTable { rules: GameRules::default(), entries: [ScoreEntry::default(); ENTRIES], rank: -1, path: None, arenas: None, arena: [640, 480] }
    }
}

fn name(s: &str) -> [u8; 4] {
    let mut n = [0u8; 4];
    for (d, c) in n.iter_mut().zip(s.bytes().take(3)) {
        *d = c;
    }
    n
}

impl ScoreTable {
    pub fn rank(&self) -> i32 {
        self.rank
    }

    pub fn clear_rank(&mut self) {
        self.rank = -1;
    }

    /// SetRules (0x420f80).
    pub fn set_rules(&mut self, rules: &GameRules) {
        self.rules = *rules;
    }

    /// Reset (0x420fa0): the default scores.
    pub fn reset(&mut self) {
        let mut score = 80000;
        for e in &mut self.entries {
            *e = ScoreEntry { score, time: 0, stage: 1, deaths: 0, name: name("---") };
            score -= 10000;
        }
        if let Some(a) = &mut self.arenas {
            *a = [[0; 2]; ENTRIES];
        }
        self.rank = -1;
    }

    /// FindRank (0x420fe0).
    pub fn find_rank(&self, entry: &ScoreEntry) -> i32 {
        let mut rank = ENTRIES as i32 - 1;
        while rank >= 0 {
            if entry.score < self.entries[rank as usize].score {
                break;
            }
            rank -= 1;
        }
        rank += 1;
        if rank == ENTRIES as i32 { -1 } else { rank }
    }

    /// Insert (0x421010).
    pub fn insert(&mut self, entry: &ScoreEntry, rank: i32) {
        let rank = if rank < 0 { self.find_rank(entry) } else { rank };
        if rank >= 0 {
            let r = rank as usize;
            for i in (r + 1..ENTRIES).rev() {
                self.entries[i] = self.entries[i - 1];
            }
            self.entries[r] = *entry;
            if let Some(a) = &mut self.arenas {
                for i in (r + 1..ENTRIES).rev() {
                    a[i] = a[i - 1];
                }
                a[r] = self.arena;
            }
        }
        self.rank = rank;
    }

    /// Format (0x4210c0): the table as text, "@@1"/"@@0" around the last
    /// inserted rank; returns it and its visible length. `hide_names`
    /// (streamer mode) shows every entered name as "***" (display only).
    pub fn format(&self, hide_names: bool) -> (Vec<u8>, i32) {
        let mut text = if self.arenas.is_some() {
            b"   NAME SCORE      STG DEAD TIME     ARENA\n\n\n".to_vec()
        } else {
            b"   NAME SCORE      STG DEAD TIME\n\n\n".to_vec()
        };
        for (i, e) in self.entries.iter().enumerate() {
            let (begin, end) = if i as i32 == self.rank { ("@@1", "@@0") } else { ("", "") };
            let mut n: Vec<u8> = e.name.iter().copied().take_while(|&c| c != 0).take(3).collect();
            if hide_names && n != b"---" {
                n = MASK.to_vec();
            }
            text.extend_from_slice(format!("{begin}{} ", RANK_NAMES[i]).as_bytes());
            text.extend_from_slice(&n);
            text.extend(std::iter::repeat_n(b' ', 3 - n.len()));
            text.extend_from_slice(
                format!(
                    " {:010} {:3} {:3}  {:02}:{:02}:{:02}{}{end}\n\n\n",
                    e.score as i32,
                    e.stage,
                    e.deaths,
                    e.time / 3600,
                    e.time % 3600 / 60,
                    e.time % 60,
                    match self.arenas {
                        Some(a) if a[i][0] != 0 => format!("  {:>4}X{}", a[i][0], a[i][1]),
                        Some(_) => "      ---".to_string(),
                        None => String::new(),
                    }
                )
                .as_bytes(),
            );
        }
        let len = text.len() as i32 - if self.rank >= 0 { 6 } else { 0 };
        (text, len)
    }

    fn record(&self) -> Vec<u8> {
        let mut r = bytemuck::bytes_of(&self.rules).to_vec();
        r.extend_from_slice(bytemuck::cast_slice(&self.entries));
        if let Some(a) = &self.arenas {
            r.extend_from_slice(bytemuck::cast_slice(a));
        }
        r
    }

    /// Save (0x4211e0): rewrites the file with this table only.
    fn save(&self) -> bool {
        let Some(path) = &self.path else { return false };
        let mut d = FILE_VERSION.to_le_bytes().to_vec();
        d.extend_from_slice(&1i32.to_le_bytes());
        let rec = self.record();
        d.extend_from_slice(&(rec.len() as i32).to_le_bytes());
        d.extend_from_slice(&rec);
        wf_engine::fs::write(path, d).is_ok()
    }

    /// Recover (0x421330).
    fn recover(&mut self) {
        crate::log::warn("Score data was broken.");
        self.reset();
        self.save();
    }

    /// The records of the file: (offset, count, size); None when broken.
    fn header(data: &[u8]) -> Option<(usize, i32, usize)> {
        let i = |o: usize| data.get(o..o + 4).map(|b| i32::from_le_bytes(b.try_into().unwrap()));
        if i(0)? != FILE_VERSION {
            return None;
        }
        Some((12, i(4)?, i(8)? as usize))
    }

    /// Load (0x421360): the table of the current rules.
    pub fn load(&mut self) -> bool {
        let Some(path) = &self.path else { return false };
        let Ok(data) = wf_engine::fs::read(path) else { return false };
        if data.len() < 4 {
            self.recover();
            return false;
        }
        let Some((mut pos, count, size)) = Self::header(&data) else {
            self.recover();
            return false;
        };
        let rules = bytemuck::bytes_of(&self.rules).to_vec();
        for _ in 0..count {
            let Some(r) = data.get(pos..pos + rules.len()) else {
                self.recover();
                return false;
            };
            if r == rules.as_slice() {
                let at = pos + rules.len();
                let Some(e) = data.get(at..at + ENTRIES * std::mem::size_of::<ScoreEntry>()) else {
                    self.recover();
                    return false;
                };
                self.entries = bytemuck::pod_read_unaligned(e);
                if let Some(a) = &mut self.arenas {
                    let at = at + ENTRIES * std::mem::size_of::<ScoreEntry>();
                    *a = match data.get(at..at + ENTRIES * 4) {
                        Some(b) => bytemuck::pod_read_unaligned(b),
                        None => [[0; 2]; ENTRIES],
                    };
                }
                return true;
            }
            pos += size;
        }
        false
    }

    /// Store (0x421540): writes the table into its record of the file,
    /// adding a record if there is none.
    pub fn store(&mut self) -> bool {
        let Some(path) = &self.path else { return false };
        let Ok(mut data) = wf_engine::fs::read(path) else { return false };
        let Some((mut pos, count, size)) = Self::header(&data) else { return false };
        let rules = bytemuck::bytes_of(&self.rules).to_vec();
        let mut i = 0;
        while i < count {
            let Some(r) = data.get(pos..pos + rules.len()) else { return false };
            if r == rules.as_slice() {
                break;
            }
            i += 1;
            pos += size;
        }
        let rec = self.record();
        if data.len() < pos + rec.len() {
            data.resize(pos + rec.len(), 0);
        }
        data[pos..pos + rec.len()].copy_from_slice(&rec);
        if i == count {
            data[4..8].copy_from_slice(&(count + 1).to_le_bytes());
        }
        wf_engine::fs::write(path, data).is_ok()
    }

    /// Import (0x4217b0): without a score file, makes one from the scores
    /// of the old .ini file.
    fn import(&mut self, profile: &mut Profile) {
        let Some(path) = &self.path else { return };
        if path.exists() {
            return;
        }
        profile.set_section("Score");
        for i in 0..ENTRIES {
            let v = profile.get_string(&format!("{}", i + 1), "");
            if v.is_empty() {
                self.reset();
                break;
            }
            let mut it = v.split_whitespace().map(|s| s.parse::<i64>().unwrap_or(0));
            let e = &mut self.entries[i];
            e.score = it.next().unwrap_or(0) as u32;
            e.stage = it.next().unwrap_or(0) as i16;
            e.deaths = it.next().unwrap_or(0) as i16;
            e.name = name(&profile.get_string(&format!("{}name", i + 1), "---"));
            e.time = 0;
        }
        if !self.save() {
            self.path = None;
        }
    }

    /// The HD table's file (no import from wf.ini).
    pub fn init_hd(&mut self, path: PathBuf) {
        self.rules = GameRules::default();
        self.arenas = Some([[0; 2]; ENTRIES]);
        self.path = Some(path.clone());
        self.reset();
        if !path.exists() && !self.save() {
            self.path = None;
        }
    }

    pub fn is_hd(&self) -> bool {
        self.arenas.is_some()
    }

    /// Init (0x421990).
    pub fn init(&mut self, path: PathBuf, profile: &mut Profile) {
        self.rules = GameRules::default();
        self.path = Some(path);
        self.import(profile);
        self.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranks_insert_and_format() {
        let mut t = ScoreTable::default();
        t.reset();
        let e = ScoreEntry { score: 55000, time: 3725, stage: 4, deaths: 2, name: name("ABC") };
        assert_eq!(t.find_rank(&e), 3);
        t.insert(&e, -1);
        assert_eq!(t.rank(), 3);
        assert_eq!(t.entries[3].score, 55000);
        assert_eq!(t.entries[4].score, 50000);
        let (text, len) = t.format(false);
        let s = String::from_utf8(text.clone()).unwrap();
        assert!(s.contains("@@14TH ABC 0000055000   4   2  01:02:05@@0"));
        assert_eq!(len as usize, text.len() - 6);
        assert_eq!(t.find_rank(&ScoreEntry { score: 1, ..e }), -1);
    }

    #[test]
    fn hidden_names_show_no_initials() {
        for hd in [false, true] {
            let mut t = ScoreTable { arenas: hd.then_some([[0; 2]; ENTRIES]), ..Default::default() };
            t.reset();
            for (i, n) in ["ABC", "XYZ", "Q", "K9!"].iter().enumerate() {
                t.insert(&ScoreEntry { score: 900_000 - i as u32 * 1000, time: 60, stage: 3, deaths: 0, name: name(n) }, -1);
            }
            let shown = String::from_utf8(t.format(false).0).unwrap();
            assert!(shown.contains("1ST ABC 0000900000"), "{shown}");
            let (text, len) = t.format(true);
            let hidden = String::from_utf8(text.clone()).unwrap();
            for n in ["ABC", "XYZ", "Q ", "K9!"] {
                assert!(!hidden.contains(n), "{n} in {hidden}");
            }
            // The same layout: only the names differ.
            assert_eq!(text.len(), shown.len());
            assert_eq!(len, t.format(false).1);
            assert!(hidden.contains("1ST *** 0000900000"), "{hidden}");
            assert!(hidden.contains("4TH *** 0000897000"), "{hidden}");
            assert!(hidden.contains("5TH --- "), "placeholders stay: {hidden}");
        }
    }

    #[test]
    fn stores_one_record_per_rule_set() {
        let dir = std::env::temp_dir().join(format!("wf-sco-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("wf.sco");
        let mut p = Profile::default();
        let mut t = ScoreTable::default();
        t.init(path.clone(), &mut p);
        assert_eq!(std::fs::metadata(&path).unwrap().len() as usize, 12 + RECORD_SIZE);
        t.set_rules(&GameRules { stock: 3, ..Default::default() });
        assert!(!t.load());
        t.reset();
        t.entries[0].score = 123;
        assert!(t.store());
        assert_eq!(std::fs::metadata(&path).unwrap().len() as usize, 12 + 2 * RECORD_SIZE);
        let mut u = ScoreTable { path: Some(path), ..Default::default() };
        u.set_rules(&GameRules { stock: 3, ..Default::default() });
        assert!(u.load());
        assert_eq!(u.entries[0].score, 123);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
