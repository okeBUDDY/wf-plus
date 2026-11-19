//! Application::Run (0x406c20): one frame of the main loop. Window
//! messages, pacing and presenting belong to the platform and the frontend
//! (main.rs); this is the game-facing part.

use super::Cx;
use super::task::{self, ListId};

/// A stage of the program (UserFunc): the title, the game, ...
pub trait Stage {
    /// Returns false when the stage cannot start.
    fn initialize(&mut self, _cx: &mut Cx) -> bool {
        true
    }
    fn finalize(&mut self, _cx: &mut Cx) {}
    /// Called every frame (unless the time is stopped), between the task
    /// sweeps.
    fn update(&mut self, _cx: &mut Cx) {}
    /// Called every frame after the tasks ran, before the overlays.
    fn draw(&mut self, _cx: &mut Cx) {}
    /// A menu screen (the port's): laid out to the whole window rather
    /// than the 640x480 arena (see `Cx::screen_view`).
    fn is_menu(&self) -> bool {
        false
    }
}

/// Application flags (AppSettings::Flags).
pub struct AppFlags;
impl AppFlags {
    pub const SKIP_FRAME: u32 = 0x200;
    pub const PAUSED: u32 = 0x400;
    pub const TIME_STOPPED: u32 = 0x800;
    pub const STOP_TIME: u32 = 0x1000;
    pub const START_TIME: u32 = 0x2000;
    pub const RESUMED: u32 = 0x4000;
    pub const INACTIVE: u32 = 0x8000;
    pub const RESET: u32 = 0x10000;
}

/// The application state stages and tasks may change.
pub struct AppState {
    pub flags: u32,
    /// Frames the time moved.
    pub frame_count: u32,
    /// False while the time is stopped (g_time_moves).
    pub time_moves: bool,
    /// Frames the current stage has run.
    pub stage_frames: u32,
    stage_done: bool,
    stage_next: Option<Box<dyn Stage>>,
}

impl Default for AppState {
    fn default() -> Self {
        AppState { flags: 0, frame_count: 0, time_moves: true, stage_frames: 0, stage_done: false, stage_next: None }
    }
}

impl AppState {
    /// UserFunc::Finish: ends the current stage; `next` follows (None ends
    /// the program).
    pub fn finish(&mut self, next: Option<Box<dyn Stage>>) {
        self.stage_next = next;
        self.stage_done = true;
    }

    /// Application::SetTimeStop: stops (or restarts) the time after this
    /// frame.
    pub fn set_time_stop(&mut self, stop: bool) {
        self.flags |= if stop { AppFlags::STOP_TIME } else { AppFlags::START_TIME };
    }

    /// Application::RequestReset: go back to the reset stage after this
    /// frame.
    pub fn request_reset(&mut self) {
        self.flags |= AppFlags::RESET;
    }
}

pub struct App {
    stage: Option<Box<dyn Stage>>,
    /// Makes the stage that F12 (and RequestReset) returns to.
    reset: Option<fn() -> Box<dyn Stage>>,
}

impl App {
    pub fn new(reset: Option<fn() -> Box<dyn Stage>>) -> Self {
        App { stage: None, reset }
    }

    /// Starts `stage` (WinMain's `Finish(&g_fn_title)` before Run()).
    pub fn start(&mut self, cx: &mut Cx, mut stage: Box<dyn Stage>) -> bool {
        if !stage.initialize(cx) {
            stage.finalize(cx);
            return false;
        }
        self.stage = Some(stage);
        cx.app.stage_frames = 0;
        cx.app.stage_done = false;
        true
    }

    pub fn running(&self) -> bool {
        self.stage.is_some()
    }

    /// Application::Quit while running: finish without a successor.
    pub fn quit(&mut self, cx: &mut Cx) {
        cx.app.finish(None);
        self.pause(cx, false);
        cx.app.flags &= !AppFlags::SKIP_FRAME;
    }

    /// Application::Pause.
    pub fn pause(&mut self, cx: &mut Cx, pause: bool) {
        if pause {
            if cx.app.flags & AppFlags::PAUSED == 0 {
                cx.sound.set_paused(true);
                cx.app.flags |= AppFlags::PAUSED;
            }
        } else if cx.app.flags & AppFlags::PAUSED != 0 {
            cx.app.flags = (cx.app.flags & !AppFlags::PAUSED) | AppFlags::RESUMED;
        }
    }

    /// One pass of the main loop. Returns false when a frame was not made
    /// (paused); the caller then presents nothing.
    pub fn frame(&mut self, cx: &mut Cx) -> bool {
        if cx.app.flags & AppFlags::RESUMED != 0 {
            cx.sound.set_paused(false);
            cx.app.flags &= !AppFlags::RESUMED;
        }
        if cx.app.flags & (AppFlags::SKIP_FRAME | AppFlags::PAUSED) != 0 {
            return false;
        }
        let Some(mut stage) = self.stage.take() else { return false };
        if cx.input.quit_pressed {
            cx.app.finish(None);
        }
        if cx.input.reset_pressed {
            cx.app.request_reset();
        }
        // InputTask::IsProtected (0x405f30): the pause button of any pad,
        // checked only when the pads were polled (the window is active).
        if cx.app.flags & AppFlags::INACTIVE == 0 {
            for i in 0..cx.input.pads.len() {
                if cx.input.pad(i).pressed & cx.input.time_stop_mask != 0 {
                    let stopped = cx.app.flags & AppFlags::TIME_STOPPED != 0;
                    cx.app.set_time_stop(!stopped);
                }
            }
        }

        if cx.app.flags & AppFlags::TIME_STOPPED != 0 {
            task::remove_up_to(cx, ListId::Tasks, 0);
            task::remove_up_to(cx, ListId::Overlays, 0);
        } else {
            task::remove_up_to(cx, ListId::Tasks, 2);
            stage.update(cx);
            task::remove_up_to(cx, ListId::Overlays, 2);
        }
        cx.draw.reset();
        cx.draw.menu = stage.is_menu();
        task::run_all(cx, ListId::Tasks);
        stage.draw(cx);
        task::run_all(cx, ListId::Overlays);

        if cx.app.flags & AppFlags::RESET != 0 {
            cx.app.flags |= AppFlags::START_TIME;
            task::remove_from(cx, ListId::Tasks, 2);
            task::remove_from(cx, ListId::Overlays, 2);
            if let Some(reset) = self.reset {
                cx.app.finish(Some(reset()));
            }
            cx.app.flags &= !AppFlags::RESET;
        }
        let mut current = Some(stage);
        if !cx.app.stage_done {
            cx.app.stage_frames += 1;
        } else {
            let mut done = current.take().unwrap();
            done.finalize(cx);
            cx.app.stage_done = false;
            cx.app.stage_frames = 0;
            if let Some(mut next) = cx.app.stage_next.take() {
                if next.initialize(cx) {
                    current = Some(next);
                } else {
                    next.finalize(cx);
                }
            }
        }
        self.stage = current;
        if self.stage.is_none() {
            // Shutdown: drop every task.
            task::remove_from(cx, ListId::Tasks, 0);
            task::remove_from(cx, ListId::Overlays, 0);
        }
        if cx.app.flags & AppFlags::TIME_STOPPED == 0 {
            cx.app.frame_count += 1;
        }
        match cx.app.flags & (AppFlags::STOP_TIME | AppFlags::START_TIME) {
            AppFlags::STOP_TIME => {
                cx.app.flags |= AppFlags::TIME_STOPPED;
                cx.app.time_moves = false;
            }
            AppFlags::START_TIME => {
                cx.app.flags &= !AppFlags::TIME_STOPPED;
                cx.app.time_moves = true;
            }
            _ => {}
        }
        cx.app.flags &= !(AppFlags::STOP_TIME | AppFlags::START_TIME);
        true
    }
}
