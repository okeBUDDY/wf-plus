//! The platform seam. Everything the game needs from the operating system
//! comes through these types: a window to present into, the keys and
//! gamepad buttons held this frame, and an audio device that pulls from the
//! game's mixer. The SDL3 implementation is in `sdl`; game code never sees
//! SDL types.

pub mod sdl;

/// The panel's highest refresh rate (Hz, f32 bits; 0 = unknown): set by
/// the Android activity (it picks that display mode) or from SDL's display
/// modes at start-up. The options offer it as an FPS cap.
static MAX_REFRESH: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

pub fn set_max_refresh(hz: f32) {
    if hz.is_finite() && hz > 0.0 {
        MAX_REFRESH.store(hz.to_bits(), std::sync::atomic::Ordering::Relaxed);
    }
}

/// The panel's highest refresh rate, when known.
pub fn max_refresh() -> Option<f32> {
    let hz = f32::from_bits(MAX_REFRESH.load(std::sync::atomic::Ordering::Relaxed));
    (hz > 0.0).then_some(hz)
}

/// What the platform lets the game change about its display. The options
/// screen offers only these (the rest of its rows are omitted).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Capabilities {
    /// Windowed, borderless and fullscreen can be switched.
    pub window_mode: bool,
    /// The window can be given a size (the options' RESOLUTION).
    pub window_size: bool,
    /// The display's own mode (its resolution or rate) can be switched.
    pub display_modes: bool,
}

impl Capabilities {
    /// A desktop: a window in any mode and size.
    pub const DESKTOP: Self = Capabilities { window_mode: true, window_size: true, display_modes: true };
    /// A phone or tablet: the game has the whole panel, in the panel's mode.
    pub const MOBILE: Self = Capabilities { window_mode: false, window_size: false, display_modes: false };

    /// By name: "desktop", or "android" / "mobile".
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "desktop" => Some(Self::DESKTOP),
            "android" | "mobile" | "ios" => Some(Self::MOBILE),
            _ => None,
        }
    }

    /// Resets what these capabilities do not let change to the defaults,
    /// so stale wf-plus.ini values (a window mode or size written on another
    /// platform) are not used.
    pub fn mask(&self, d: &mut crate::config::DisplayConfig) {
        let default = crate::config::DisplayConfig::default();
        if !self.window_mode {
            d.mode = default.mode;
        }
        if !self.window_size {
            (d.width, d.height) = (default.width, default.height);
        }
    }
}

/// This platform's display capabilities: Android and iOS give the game the
/// whole panel; desktops allow everything.
pub const fn capabilities() -> Capabilities {
    if cfg!(any(target_os = "android", target_os = "ios")) { Capabilities::MOBILE } else { Capabilities::DESKTOP }
}

impl Default for Capabilities {
    fn default() -> Self {
        capabilities()
    }
}

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
    Q,
    PageUp,
    PageDown,
}

impl Key {
    pub const COUNT: usize = Key::PageDown as usize + 1;

    /// A key by its name in bindings ("z", "up", "kp8", "escape").
    pub fn parse(s: &str) -> Option<Key> {
        Some(match s {
            "up" => Key::Up,
            "down" => Key::Down,
            "left" => Key::Left,
            "right" => Key::Right,
            "kp1" => Key::Numpad1,
            "kp2" => Key::Numpad2,
            "kp3" => Key::Numpad3,
            "kp4" => Key::Numpad4,
            "kp6" => Key::Numpad6,
            "kp7" => Key::Numpad7,
            "kp8" => Key::Numpad8,
            "kp9" => Key::Numpad9,
            "z" => Key::Z,
            "x" => Key::X,
            "s" => Key::S,
            "d" => Key::D,
            "a" => Key::A,
            "w" => Key::W,
            "e" => Key::E,
            "f" => Key::F,
            "esc" | "escape" => Key::Escape,
            "f12" => Key::F12,
            "return" | "enter" => Key::Return,
            "space" => Key::Space,
            "q" => Key::Q,
            "pageup" => Key::PageUp,
            "pagedown" => Key::PageDown,
            _ => return None,
        })
    }
}

/// Gamepad buttons, in the SDL standard (Xbox-style) layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
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

    /// A button by its name in bindings ("south", "start", "dpup").
    pub fn parse(s: &str) -> Option<PadButton> {
        Some(match s {
            "south" | "a" => PadButton::South,
            "east" | "b" => PadButton::East,
            "west" | "x" => PadButton::West,
            "north" | "y" => PadButton::North,
            "lb" | "leftshoulder" => PadButton::LeftShoulder,
            "rb" | "rightshoulder" => PadButton::RightShoulder,
            "back" | "select" => PadButton::Back,
            "start" => PadButton::Start,
            "ls" | "leftstick" => PadButton::LeftStick,
            "rs" | "rightstick" => PadButton::RightStick,
            "dpup" => PadButton::DpadUp,
            "dpdown" => PadButton::DpadDown,
            "dpleft" => PadButton::DpadLeft,
            "dpright" => PadButton::DpadRight,
            _ => return None,
        })
    }
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
    /// The first finger on the first touchpad (DualSense and the like):
    /// 0..1, y down, when touching.
    pub touchpad: Option<[f32; 2]>,
}

/// The mouse: where it is (window pixels) and the buttons held (bit 0
/// left, 1 middle, 2 right, 3 back (X1), 4 forward (X2)).
#[derive(Clone, Copy, Debug, Default)]
pub struct MouseState {
    pub pos: [f32; 2],
    pub buttons: u32,
    /// Inside the window.
    pub inside: bool,
    /// The wheel's travel since the start, in notches (up is positive):
    /// a running total, so any tick can tell how far it moved.
    pub wheel: f32,
}

/// A finger on a touch screen: 0..1 of the window, y down.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Finger {
    pub id: u64,
    pub pos: [f32; 2],
}

/// A kind of input device, for the last-used switch.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Device {
    #[default]
    Keyboard,
    Mouse,
    Gamepad,
    Touch,
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
    pub mouse: MouseState,
    pub fingers: Vec<Finger>,
    /// The device the player used last (a key or button pressed, the
    /// mouse moved or clicked, a finger down, a stick pushed).
    pub last_device: Device,
    /// The window's size in pixels, for pointer positions.
    pub window: (u32, u32),
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
