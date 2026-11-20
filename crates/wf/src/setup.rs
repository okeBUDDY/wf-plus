//! The first-run screen, shown when no game data is found: it links the
//! official download and takes the archive. A zip (or data.bin) dropped on
//! the window is checked and its data.bin saved where the game looks; a
//! download that turns up in the usual folders (see
//! [`data_import::search_dirs`]) is used where it is, as the start-up
//! search does, without saving anything.

use std::path::PathBuf;

use crate::art::Art;
use crate::data_import;
use crate::game::text::Fonts;
use crate::gfx::{Argb, Blend, DrawList, Primitive, Transform};
use wf_engine::input::{Action, ActionState};
use wf_engine::platform::Key;

/// Where the game data turned up.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Found {
    /// A data.bin file (saved from a dropped archive, or found).
    File(PathBuf),
    /// The download, read in place.
    Zip(PathBuf),
}

/// The game data a start-up search finds in `dirs`: the first candidate
/// that holds version 1.08's data.bin.
pub fn search(dirs: &[PathBuf]) -> Option<Found> {
    data_import::candidates(dirs).into_iter().find_map(|p| check(&p))
}

/// `path` as a source of the game data, if it is one.
fn check(path: &std::path::Path) -> Option<Found> {
    let bytes = std::fs::read(path).ok()?;
    data_import::extract(&bytes).ok()?;
    let zip = bytes.starts_with(b"PK");
    Some(if zip { Found::Zip(path.to_path_buf()) } else { Found::File(path.to_path_buf()) })
}

/// Ticks between looks in the search folders (about a second).
const RESCAN_TICKS: u32 = 60;

const WHITE: Argb = Argb(0xffff_ffff);
const GREY: Argb = Argb(0xff80_8080);
const AMBER: Argb = Argb(0xffff_dd00);
const RED: Argb = Argb(0xffff_4040);

/// The link's line (logical units) and font size: `text` is 16 px.
const LINK_Y: f32 = 208.0;
const LINK_TEXT: &str = "HIKWARE.COM";

pub struct Setup {
    fonts: Fonts,
    draw: DrawList,
    /// Where a dropped archive's data.bin is saved.
    dest: PathBuf,
    dirs: Vec<PathBuf>,
    pub found: Option<Found>,
    quit: bool,
    /// The last import's error, shown under the instructions.
    error: Option<String>,
    ticks: u32,
    /// Last tick's mouse buttons, Return and confirm, for presses.
    held: (u32, bool, bool, bool),
    /// Files that did not hold the data, by size and time (not read again
    /// until they change).
    rejected: Vec<(PathBuf, u64, Option<std::time::SystemTime>)>,
}

impl Setup {
    pub fn new(art: &Art, dest: PathBuf, dirs: Vec<PathBuf>) -> Self {
        Setup {
            fonts: Fonts::load(art),
            draw: DrawList::new(),
            dest,
            dirs,
            found: None,
            quit: false,
            error: None,
            ticks: 0,
            held: (0, false, false, false),
            rejected: Vec::new(),
        }
    }

    /// A file handed over by the player: its data.bin is saved.
    pub fn import(&mut self, path: &std::path::Path) {
        match data_import::install_file(path, &self.dest) {
            Ok(()) => self.found = Some(Found::File(self.dest.clone())),
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    /// The bytes of a file handed over by the player (a picker that gives
    /// no path): its data.bin is saved.
    pub fn import_bytes(&mut self, bytes: &[u8]) {
        match data_import::install(bytes, &self.dest) {
            Ok(()) => self.found = Some(Found::File(self.dest.clone())),
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    fn open_download(&mut self) {
        if let Err(e) = wf_engine::platform::sdl::open_url(data_import::DOWNLOAD_URL) {
            self.error = Some(format!("CANNOT OPEN THE BROWSER ({})", e.to_ascii_uppercase()));
        }
    }

    /// Looks in the search folders for a download that appeared.
    fn rescan(&mut self) {
        for path in data_import::candidates(&self.dirs) {
            let meta = std::fs::metadata(&path).ok();
            let key = (meta.as_ref().map_or(0, |m| m.len()), meta.and_then(|m| m.modified().ok()));
            if self.rejected.iter().any(|(p, len, at)| *p == path && (*len, *at) == key) {
                continue;
            }
            if let Some(found) = check(&path) {
                self.found = Some(found);
                return;
            }
            self.rejected.retain(|(p, ..)| *p != path);
            self.rejected.push((path, key.0, key.1));
        }
    }

    /// The link's box (logical units).
    fn link_box() -> [f32; 4] {
        let w = LINK_TEXT.len() as f32 * 16.0;
        [320.0 - w / 2.0 - 8.0, LINK_Y - 6.0, w + 16.0, 28.0]
    }

    fn over_link(p: [f32; 2]) -> bool {
        let [x, y, w, h] = Self::link_box();
        (x..x + w).contains(&p[0]) && (y..y + h).contains(&p[1])
    }

    fn centred(&mut self, big: bool, y: f32, text: &str, color: Argb) {
        let font = if big { &self.fonts.text } else { &self.fonts.small_text };
        let w = text.len() as f32 * if big { 16.0 } else { 8.0 };
        font.print(&mut self.draw, (320.0 - w / 2.0).round(), y, text.as_bytes(), None, color, -1, Blend::Normal);
    }

    fn render(&mut self, hover: bool) {
        self.draw.reset();
        self.draw.menu = true;
        self.centred(true, 112.0, "WARNING FOREVER+", WHITE);
        self.centred(false, 160.0, "THIS NEEDS THE GAME DATA OF WARNING FOREVER 1.08,", WHITE);
        self.centred(false, 176.0, "A FREE DOWNLOAD FROM THE OFFICIAL WEBSITE:", WHITE);
        let link = if hover || (self.ticks / 30).is_multiple_of(2) { AMBER } else { Argb(0xffc0_a000) };
        self.centred(true, LINK_Y, LINK_TEXT, link);
        let w = LINK_TEXT.len() as f32 * 16.0;
        let y = LINK_Y + 18.0;
        let line = [[320.0 - w / 2.0, y], [320.0 + w / 2.0, y]];
        self.draw.shape(Primitive::LineList, &Transform::IDENTITY, &line, link, Blend::Normal);
        self.centred(false, 256.0, "PRESS ENTER OR CLICK THE LINK TO OPEN IT, DOWNLOAD", WHITE);
        self.centred(false, 272.0, "WF108.ZIP, THEN DROP IT ON THIS WINDOW.", WHITE);
        self.centred(false, 300.0, "A WF108.ZIP NEXT TO THE GAME OR IN DOWNLOADS", GREY);
        self.centred(false, 316.0, "IS FOUND BY ITSELF.", GREY);
        if let Some(e) = self.error.clone() {
            self.centred(false, 352.0, &e, RED);
        }
        self.centred(false, 440.0, "ESC: QUIT", GREY);
    }
}

impl wf_engine::app::Game for Setup {
    fn tick(&mut self, actions: &ActionState) -> bool {
        if actions.held(Action::Quit) {
            self.quit = true;
        }
        let raw = &actions.raw;
        let buttons = raw.mouse.buttons;
        let (ret, confirm, touching) = (raw.key(Key::Return), actions.held(Action::CONFIRM), !actions.touches.is_empty());
        let clicked = buttons & 1 != 0 && self.held.0 & 1 == 0 && actions.cursor.is_some_and(Self::over_link);
        let tapped = touching && !self.held.3 && actions.touches.iter().any(|&p| Self::over_link(p));
        if clicked || tapped || (ret && !self.held.1) || (confirm && !self.held.2) {
            self.open_download();
        }
        self.held = (buttons, ret, confirm, touching);
        if self.ticks.is_multiple_of(RESCAN_TICKS) {
            self.rescan();
        }
        self.ticks += 1;
        self.render(actions.cursor.is_some_and(Self::over_link));
        true
    }
    fn snapshot(&self) -> &DrawList {
        &self.draw
    }
    fn running(&self) -> bool {
        self.found.is_none() && !self.quit
    }
    fn quit(&mut self) {
        self.quit = true;
    }
    fn focus(&mut self, _active: bool) {}
    fn active(&self) -> bool {
        true
    }
    fn shader_time(&self) -> f32 {
        self.ticks as f32 / 60.0
    }
    fn file_dropped(&mut self, path: PathBuf) {
        self.import(&path);
    }
}
