//! The game's data files: the data.bin archive (sounds, sprite patterns,
//! the boss planning data) and the formats inside it. The art itself is not
//! read from here: it is cooked at build time (see `crate::art`).

pub mod pack;
pub mod pat;
