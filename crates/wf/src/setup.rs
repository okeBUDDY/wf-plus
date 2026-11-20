//! The first-run screen, shown when no game data is found: it links the
//! official download and takes the archive. A zip (or data.bin) dropped on
//! the window is checked and its data.bin saved where the game looks; a
//! download that turns up in the usual folders (see
//! [`data_import::search_dirs`]) is used where it is, as the start-up
//! search does, without saving anything.

use std::path::PathBuf;

use crate::art::Art;
use crate::data_import;
use crate::game::background::BackgroundGrid;
use crate::game::text::Fonts;
use crate::gfx::{Argb, Blend, DrawList, Primitive, SpriteId, Transform};
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
const DIM_AMBER: Argb = Argb(0xff90_7800);
const RED: Argb = Argb(0xffff_4040);
/// "WARNING FOREVER" as the title screen's subtitle draws it.
const TITLE_AMBER: Argb = Argb(0xffff_d000);
/// The "+": the arena border's violet ([`wf_engine::app::ARENA_BORDER`]),
/// opaque.
const PLUS_VIOLET: Argb = Argb(0xff00_0000 | (wf_engine::app::ARENA_BORDER & 0xff_ffff));


/// The screen's two buttons: the download page, and the file picker.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Button {
    Download,
    Choose,
}

impl Button {
    const ALL: [Button; 2] = [Button::Download, Button::Choose];

    fn label(self) -> &'static str {
        match self {
            Button::Download => "HIKWARE.COM",
            Button::Choose => "CHOOSE WF108.ZIP",
        }
    }

    /// The label's top (logical units); labels are 16 px text.
    fn y(self) -> f32 {
        match self {
            Button::Download => 200.0,
            Button::Choose => 296.0,
        }
    }

    /// The box clicks and taps hit.
    fn hit(self, p: [f32; 2]) -> bool {
        let w = self.label().len() as f32 * 16.0 + 32.0;
        let (x, y) = (320.0 - w / 2.0, self.y() - 10.0);
        (x..x + w).contains(&p[0]) && (y..y + 36.0).contains(&p[1])
    }
}

pub struct Setup {
    fonts: Fonts,
    draw: DrawList,
    /// The title screen's scrolling grid, and its line.
    grid: BackgroundGrid,
    grid_line: SpriteId,
    /// Where a chosen or dropped archive's data.bin is saved.
    dest: PathBuf,
    dirs: Vec<PathBuf>,
    pub found: Option<Found>,
    quit: bool,
    /// The last import's error, shown under the instructions.
    error: Option<String>,
    ticks: u32,
    /// The button Enter and the confirm button press.
    selected: Button,
    /// The file picker was asked for (the loop opens it).
    pick: bool,
    /// A phone or tablet: no drop, no Downloads folder to search, no keys;
    /// the text says so.
    mobile: bool,
    /// Last tick's mouse buttons, keys (Return, Up, Down), confirm and
    /// touch, for presses.
    held: (u32, [bool; 3], bool, bool),
    /// Files that did not hold the data, by size and time (not read again
    /// until they change).
    rejected: Vec<(PathBuf, u64, Option<std::time::SystemTime>)>,
}

impl Setup {
    pub fn new(art: &Art, dest: PathBuf, dirs: Vec<PathBuf>, mobile: bool) -> Self {
        Setup {
            fonts: Fonts::load(art),
            draw: DrawList::new(),
            // As the title's (colour, speed); a new angle each run.
            grid: BackgroundGrid::started(0xff00_4030, 8.0, {
                let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.subsec_nanos());
                (nanos >> 10 & 0x3ff) as i16
            }),
            grid_line: art.shapes.grid_line,
            dest,
            dirs,
            found: None,
            quit: false,
            error: None,
            ticks: 0,
            selected: Button::Download,
            pick: false,
            mobile,
            held: (0, [false; 3], false, false),
            rejected: Vec::new(),
        }
    }

    /// A file handed over by the player (dropped or chosen): its data.bin
    /// is saved.
    pub fn import(&mut self, path: &std::path::Path) {
        match data_import::install_file(path, &self.dest) {
            Ok(()) => self.found = Some(Found::File(self.dest.clone())),
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    fn press(&mut self, button: Button) {
        self.selected = button;
        self.error = None;
        match button {
            Button::Download => {
                if let Err(e) = wf_engine::platform::sdl::open_url(data_import::DOWNLOAD_URL) {
                    self.error = Some(format!("CANNOT OPEN THE BROWSER ({})", e.to_ascii_uppercase()));
                }
            }
            Button::Choose => self.pick = true,
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

    fn centred(&mut self, big: bool, y: f32, text: &str, color: Argb) {
        let font = if big { &self.fonts.text } else { &self.fonts.small_text };
        let w = text.len() as f32 * if big { 16.0 } else { 8.0 };
        font.print(&mut self.draw, (320.0 - w / 2.0).round(), y, text.as_bytes(), None, color, -1, Blend::Normal);
    }

    fn button(&mut self, button: Button, hover: bool) {
        let lit = hover || (self.selected == button && (self.ticks / 30).is_multiple_of(2)) || self.mobile;
        let color = if lit { AMBER } else { DIM_AMBER };
        self.centred(true, button.y(), button.label(), color);
        let w = button.label().len() as f32 * 16.0;
        let y = button.y() + 18.0;
        let line = [[320.0 - w / 2.0, y], [320.0 + w / 2.0, y]];
        self.draw.shape(Primitive::LineList, &Transform::IDENTITY, &line, color, Blend::Normal);
    }

    fn render(&mut self, hover: Option<Button>) {
        self.draw.reset();
        self.draw.menu = true;
        self.grid.draw(&mut self.draw, self.grid_line, 320.0);
        // "WARNING FOREVER" in the title's amber, the "+" in the border's
        // violet, centred as one.
        let x = (320.0 - "WARNING FOREVER+".len() as f32 * 8.0).round();
        self.fonts.text.print(&mut self.draw, x, 112.0, b"WARNING FOREVER", None, TITLE_AMBER, -1, Blend::Normal);
        self.fonts.text.print(&mut self.draw, x + 15.0 * 16.0, 112.0, b"+", None, PLUS_VIOLET, -1, Blend::Normal);
        self.centred(false, 152.0, "THIS NEEDS THE GAME DATA OF WARNING FOREVER 1.08,", WHITE);
        self.centred(false, 168.0, "A FREE DOWNLOAD FROM THE OFFICIAL WEBSITE:", WHITE);
        self.button(Button::Download, hover == Some(Button::Download));
        if self.mobile {
            self.centred(false, 248.0, "TAP IT TO DOWNLOAD WF108.ZIP THERE,", WHITE);
            self.centred(false, 264.0, "THEN CHOOSE THE DOWNLOADED FILE:", WHITE);
        } else {
            self.centred(false, 248.0, "DOWNLOAD WF108.ZIP THERE, THEN DROP IT ON", WHITE);
            self.centred(false, 264.0, "THIS WINDOW OR CHOOSE IT:", WHITE);
        }
        self.button(Button::Choose, hover == Some(Button::Choose));
        if !self.mobile {
            self.centred(false, 344.0, "A WF108.ZIP NEXT TO THE GAME OR IN DOWNLOADS", GREY);
            self.centred(false, 360.0, "IS FOUND BY ITSELF.", GREY);
        }
        if let Some(e) = self.error.clone() {
            self.centred(false, 392.0, &e, RED);
        } else if wf_engine::platform::sdl::Platform::file_dialog_open() {
            self.centred(false, 392.0, "WAITING FOR THE FILE DIALOG", GREY);
        }
        if !self.mobile {
            self.centred(false, 440.0, "ENTER: OPEN   UP/DOWN: SELECT   ESC: QUIT", GREY);
        }
    }
}

impl wf_engine::app::Game for Setup {
    fn tick(&mut self, actions: &ActionState) -> bool {
        if actions.held(Action::Quit) {
            self.quit = true;
        }
        let raw = &actions.raw;
        let buttons = raw.mouse.buttons;
        let keys = [raw.key(Key::Return), actions.held(Action::Up), actions.held(Action::Down)];
        let (confirm, touching) = (actions.held(Action::CONFIRM), !actions.touches.is_empty());
        let hover = actions.cursor.and_then(|p| Button::ALL.into_iter().find(|b| b.hit(p)));
        let (was_buttons, was_keys, was_confirm, was_touching) = self.held;
        self.held = (buttons, keys, confirm, touching);
        let tapped = if touching && !was_touching { actions.touches.iter().find_map(|&p| Button::ALL.into_iter().find(|b| b.hit(p))) } else { None };
        if let Some(b) = tapped {
            self.press(b);
        } else if buttons & 1 != 0 && was_buttons & 1 == 0 && let Some(b) = hover {
            self.press(b);
        } else if (keys[0] && !was_keys[0]) || (confirm && !was_confirm) {
            self.press(self.selected);
        } else if (keys[1] && !was_keys[1]) || (keys[2] && !was_keys[2]) {
            self.selected = if self.selected == Button::Download { Button::Choose } else { Button::Download };
        }
        self.grid.scroll();
        if self.ticks.is_multiple_of(RESCAN_TICKS) {
            self.rescan();
        }
        self.ticks += 1;
        self.render(hover);
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
    fn file_dialog(&mut self) -> bool {
        std::mem::take(&mut self.pick)
    }
}
