//! The platform seam. Everything the game needs from the operating system
//! comes through these types: a window to present into, the keys and
//! gamepad buttons held this frame, and an audio device that pulls from the
//! game's mixer. The SDL3 implementation is in `sdl`; game code never sees
//! SDL types.

pub mod sdl;

/// The keys the game reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Key {
    Up,
    Down,
    Left,
    Right,
    Numpad1,
    Numpad2,
    Numpad3,
    Numpad4,
    Numpad6,
    Numpad7,
    Numpad8,
    Numpad9,
    Z,
    X,
    S,
    D,
    A,
    W,
    E,
    F,
    Escape,
    F12,
    Return,
    Space,
}

impl Key {
    pub const COUNT: usize = Key::Space as usize + 1;
}

/// Gamepad buttons, in the SDL standard (Xbox-style) layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum PadButton {
    South,
    East,
    West,
    North,
    LeftShoulder,
    RightShoulder,
    Back,
    Start,
    LeftStick,
    RightStick,
    DpadUp,
    DpadDown,
    DpadLeft,
    DpadRight,
}

impl PadButton {
    pub const COUNT: usize = PadButton::DpadRight as usize + 1;
    /// The buttons a trigger map indexes (physical button n), in order.
    pub const TRIGGERS: [PadButton; 10] = [
        PadButton::South,
        PadButton::East,
        PadButton::West,
        PadButton::North,
        PadButton::LeftShoulder,
        PadButton::RightShoulder,
        PadButton::Back,
        PadButton::Start,
        PadButton::LeftStick,
        PadButton::RightStick,
    ];
}

/// One gamepad's state.
#[derive(Clone, Copy, Debug, Default)]
pub struct GamepadState {
    pub buttons: u32,
    /// Left stick, -1..1, y down.
    pub stick: [f32; 2],
}

impl GamepadState {
    pub fn held(&self, b: PadButton) -> bool {
        self.buttons & (1 << b as u32) != 0
    }
}

/// What the player holds this frame.
#[derive(Clone, Debug, Default)]
pub struct RawInput {
    pub keys: [bool; Key::COUNT],
    pub gamepads: Vec<GamepadState>,
}

impl RawInput {
    pub fn key(&self, k: Key) -> bool {
        self.keys[k as usize]
    }
}

/// Window and application events.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    Quit,
    /// The window lost or got the focus.
    Focus(bool),
    /// The drawable size changed.
    Resized(u32, u32),
}
