//! The platform seam. Everything the game needs from the operating system
//! comes through these types: a window to present into, the keys and
//! gamepad buttons held this frame, and an audio device that pulls from the
//! game's mixer. The SDL3 implementation is in `sdl`; game code never sees
//! SDL types.

#[cfg(not(target_arch = "wasm32"))]
pub mod sdl;
#[cfg(target_os = "android")]
pub mod android;
#[cfg(target_arch = "wasm32")]
pub mod web;

/// The display's refresh rate (Hz, f32 bits; 0 = unknown): on Android the
/// panel's highest, set by the activity (it picks that display mode); on
/// the desktop the window's display's desktop mode, at start-up and when
/// the window moves to another display. The options offer it as an FPS
/// cap (MAX).
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

/// The window's display's resolution in pixels (width << 32 | height; 0 =
/// unknown): set by the SDL platform at start-up and when the window moves
/// to another display. Borderless and fullscreen windows take this size.
static DISPLAY_SIZE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub fn set_display_size(width: u32, height: u32) {
    if width > 0 && height > 0 {
        DISPLAY_SIZE.store((width as u64) << 32 | height as u64, std::sync::atomic::Ordering::Relaxed);
    }
}

/// The window's display's resolution, when known.
pub fn display_size() -> Option<(u32, u32)> {
    let v = DISPLAY_SIZE.load(std::sync::atomic::Ordering::Relaxed);
    (v != 0).then_some(((v >> 32) as u32, v as u32))
}

/// The window's display's modes, as sizes in pixels, largest first and
/// each size once (SDL lists a size again for each refresh rate): set with
/// the display size. Empty when unknown (no window, or the platform has no
/// list).
static DISPLAY_MODES: std::sync::Mutex<Vec<(u32, u32)>> = std::sync::Mutex::new(Vec::new());

pub fn set_display_modes(mut modes: Vec<(u32, u32)>) {
    modes.retain(|&(w, h)| w > 0 && h > 0);
    modes.sort_by_key(|&(w, h)| std::cmp::Reverse((w * h, w)));
    modes.dedup();
    *DISPLAY_MODES.lock().unwrap_or_else(|e| e.into_inner()) = modes;
}

/// The window's display's modes (see `set_display_modes`).
pub fn display_modes() -> Vec<(u32, u32)> {
    DISPLAY_MODES.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// Reads a whole file. On Android a relative path reads from the APK's
/// assets (through SDL's IO), so the bundled data.bin needs no unpacking.
pub fn read_file(path: &std::path::Path) -> std::io::Result<Vec<u8>> {
    #[cfg(target_os = "android")]
    return android::read_file(path);
    #[cfg(target_arch = "wasm32")]
    return web::read_file(path);
    #[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
    std::fs::read(path)
}

/// Opens `url` in the system's browser (a new tab in the browser build).
pub fn open_url(url: &str) -> Result<(), String> {
    #[cfg(target_arch = "wasm32")]
    return web::open_url(url);
    #[cfg(not(target_arch = "wasm32"))]
    sdl::open_url(url)
}

/// The platform's file dialog is showing (none in the browser build: the
/// page has its own file input).
pub fn file_dialog_open() -> bool {
    #[cfg(target_arch = "wasm32")]
    return false;
    #[cfg(not(target_arch = "wasm32"))]
    sdl::Platform::file_dialog_open()
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
/// whole panel, and so does the browser (the page's canvas, its Escape a
/// phone's Back); desktops allow everything.
pub const fn capabilities() -> Capabilities {
    if cfg!(any(target_os = "android", target_os = "ios", target_arch = "wasm32")) { Capabilities::MOBILE } else { Capabilities::DESKTOP }
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
    /// The analog triggers, held past half way.
    LeftTrigger,
    RightTrigger,
}

impl PadButton {
    pub const COUNT: usize = PadButton::RightTrigger as usize + 1;

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
            "lt" | "lefttrigger" => PadButton::LeftTrigger,
            "rt" | "righttrigger" => PadButton::RightTrigger,
            _ => return None,
        })
    }
    /// The button's name in bindings and on screen (upper-cased): the
    /// Xbox layout's.
    pub fn name(self) -> &'static str {
        match self {
            PadButton::South => "a",
            PadButton::East => "b",
            PadButton::West => "x",
            PadButton::North => "y",
            PadButton::LeftShoulder => "lb",
            PadButton::RightShoulder => "rb",
            PadButton::Back => "back",
            PadButton::Start => "start",
            PadButton::LeftStick => "ls",
            PadButton::RightStick => "rs",
            PadButton::DpadUp => "dpup",
            PadButton::DpadDown => "dpdown",
            PadButton::DpadLeft => "dpleft",
            PadButton::DpadRight => "dpright",
            PadButton::LeftTrigger => "lt",
            PadButton::RightTrigger => "rt",
        }
    }

    /// The button's name on a `style` gamepad, as the options and the
    /// prompts show it.
    pub fn label(self, style: PadStyle) -> &'static str {
        use PadButton::*;
        match (style, self) {
            (PadStyle::PlayStation, South) => "CROSS",
            (PadStyle::PlayStation, East) => "CIRCLE",
            (PadStyle::PlayStation, West) => "SQUARE",
            (PadStyle::PlayStation, North) => "TRIANGLE",
            (PadStyle::PlayStation, LeftShoulder) => "L1",
            (PadStyle::PlayStation, RightShoulder) => "R1",
            (PadStyle::PlayStation, LeftTrigger) => "L2",
            (PadStyle::PlayStation, RightTrigger) => "R2",
            (PadStyle::PlayStation, LeftStick) => "L3",
            (PadStyle::PlayStation, RightStick) => "R3",
            (PadStyle::PlayStation, Back) => "SHARE",
            (PadStyle::PlayStation, Start) => "OPTIONS",
            (PadStyle::Nintendo, South) => "B",
            (PadStyle::Nintendo, East) => "A",
            (PadStyle::Nintendo, West) => "Y",
            (PadStyle::Nintendo, North) => "X",
            (PadStyle::Nintendo, LeftShoulder) => "L",
            (PadStyle::Nintendo, RightShoulder) => "R",
            (PadStyle::Nintendo, LeftTrigger) => "ZL",
            (PadStyle::Nintendo, RightTrigger) => "ZR",
            (PadStyle::Nintendo, Back) => "-",
            (PadStyle::Nintendo, Start) => "+",
            (PadStyle::SteamDeck, LeftShoulder) => "L1",
            (PadStyle::SteamDeck, RightShoulder) => "R1",
            (PadStyle::SteamDeck, LeftTrigger) => "L2",
            (PadStyle::SteamDeck, RightTrigger) => "R2",
            (PadStyle::SteamDeck, LeftStick) => "L3",
            (PadStyle::SteamDeck, RightStick) => "R3",
            (PadStyle::SteamDeck, Back) => "VIEW",
            (PadStyle::SteamDeck, Start) => "MENU",
            (PadStyle::Generic, South) => "BOTTOM",
            (PadStyle::Generic, East) => "RIGHT",
            (PadStyle::Generic, West) => "LEFT",
            (PadStyle::Generic, North) => "TOP",
            (PadStyle::Generic, LeftShoulder) => "L1",
            (PadStyle::Generic, RightShoulder) => "R1",
            (PadStyle::Generic, LeftTrigger) => "L2",
            (PadStyle::Generic, RightTrigger) => "R2",
            (PadStyle::Generic, LeftStick) => "L3",
            (PadStyle::Generic, RightStick) => "R3",
            (PadStyle::Generic, Back) => "SELECT",
            (_, South) => "A",
            (_, East) => "B",
            (_, West) => "X",
            (_, North) => "Y",
            (_, LeftShoulder) => "LB",
            (_, RightShoulder) => "RB",
            (_, LeftTrigger) => "LT",
            (_, RightTrigger) => "RT",
            (_, LeftStick) => "LS",
            (_, RightStick) => "RS",
            (_, Back) => "BACK",
            (_, Start) => "START",
            (_, DpadUp) => "UP",
            (_, DpadDown) => "DOWN",
            (_, DpadLeft) => "LEFT",
            (_, DpadRight) => "RIGHT",
        }
    }

    /// The buttons a game action can be bound to (not the menus' D-pad,
    /// START or BACK).
    pub const BINDABLE: [PadButton; 10] = [
        PadButton::South,
        PadButton::East,
        PadButton::West,
        PadButton::North,
        PadButton::LeftShoulder,
        PadButton::RightShoulder,
        PadButton::LeftTrigger,
        PadButton::RightTrigger,
        PadButton::LeftStick,
        PadButton::RightStick,
    ];

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
    /// The kind of the gamepad used last, for its buttons' names.
    pub pad_style: PadStyle,
    /// The window's size in pixels, for pointer positions.
    pub window: (u32, u32),
}

/// A family of gamepads, whose buttons have their own names.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PadStyle {
    /// A gamepad of no known family: the buttons by place.
    Generic,
    /// Xbox (and the many that pass for one): A B X Y, LB LT. Also the
    /// names before any gamepad is seen.
    #[default]
    Xbox,
    /// The Steam Deck: A B X Y, L1 L2, VIEW and MENU.
    SteamDeck,
    /// DualShock and DualSense: CROSS CIRCLE SQUARE TRIANGLE, L1 L2.
    PlayStation,
    /// Switch: B A Y X at the south, east, west and north, L ZL.
    Nintendo,
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
    /// The app is going to the background (mobile): its window surface is
    /// about to go away. Rendering must stop until [`Event::Foreground`].
    Background,
    /// The app is back in the foreground with a new window surface.
    Foreground,
}
