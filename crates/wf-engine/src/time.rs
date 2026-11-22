//! Time: the fixed simulation step, decoupled from the display rate, and
//! the frame limiter.
//!
//! The game ticks at a fixed 17 ms (the original's HLSync timer, 58.8 Hz)
//! out of an accumulator of real time; the display renders as often as
//! the cap allows and interpolates between the last two ticks' snapshots
//! by [`FixedStep::alpha`].

use std::time::Duration;
use web_time::Instant;

/// HLSync::Initialize (0x40c9e0): (10000 / 60 + 5) / 10 = 17 ms.
pub const TICK: Duration = Duration::from_millis(17);

/// At most this many ticks run per display frame; beyond that the game
/// slows down instead of spiralling (a stall, a debugger).
pub const MAX_TICKS_PER_FRAME: u32 = 8;

/// The fixed-timestep accumulator.
pub struct FixedStep {
    tick: Duration,
    acc: Duration,
    last: Option<Instant>,
}

impl FixedStep {
    pub fn new(tick: Duration) -> Self {
        FixedStep { tick, acc: Duration::ZERO, last: None }
    }

    /// Adds the real time since the last call; returns how many ticks are
    /// due now.
    pub fn advance(&mut self, now: Instant) -> u32 {
        let dt = self.last.map_or(self.tick, |l| now.saturating_duration_since(l));
        self.last = Some(now);
        self.acc += dt;
        let mut n = 0;
        while self.acc >= self.tick && n < MAX_TICKS_PER_FRAME {
            self.acc -= self.tick;
            n += 1;
        }
        if n == MAX_TICKS_PER_FRAME && self.acc >= self.tick {
            self.acc = Duration::ZERO;
        }
        n
    }

    /// How far (0..1) the display is between the last tick and the next.
    pub fn alpha(&self) -> f32 {
        (self.acc.as_secs_f64() / self.tick.as_secs_f64()).clamp(0.0, 1.0) as f32
    }

    /// Forgets the time since the last call (after a pause or a stall).
    pub fn reset(&mut self) {
        self.acc = Duration::ZERO;
        self.last = None;
    }
}

/// The display rate cap.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FpsCap {
    /// As fast as the GPU presents (Mailbox or Immediate).
    Off,
    /// Present on the display's refresh (Fifo).
    VSync,
    /// At most this many frames a second, by the limiter.
    Fps(f64),
}

impl FpsCap {
    /// "off", "vsync" or a rate ("60", "144", "75.5").
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "off" | "unlimited" | "0" => Some(FpsCap::Off),
            "vsync" => Some(FpsCap::VSync),
            v => v.parse::<f64>().ok().filter(|f| *f >= 1.0).map(FpsCap::Fps),
        }
    }

    /// The presets the options cycle through.
    pub const PRESETS: [FpsCap; 6] =
        [FpsCap::Off, FpsCap::VSync, FpsCap::Fps(60.0), FpsCap::Fps(120.0), FpsCap::Fps(144.0), FpsCap::Fps(240.0)];

    pub fn label(&self) -> String {
        match self {
            FpsCap::Off => "OFF".into(),
            FpsCap::VSync => "VSYNC".into(),
            FpsCap::Fps(f) => format!("{f}"),
        }
    }
}

/// A precise frame limiter: sleeps most of the way, then spins.
pub struct Limiter {
    next: Option<Instant>,
}

/// The sleep granularity margin: the rest is spun.
const SPIN: Duration = Duration::from_micros(1500);

impl Limiter {
    pub fn new() -> Self {
        Limiter { next: None }
    }

    /// Waits until the next frame of `cap` is due.
    pub fn wait(&mut self, cap: FpsCap) {
        let FpsCap::Fps(fps) = cap else {
            self.next = None;
            return;
        };
        let period = Duration::from_secs_f64(1.0 / fps);
        let now = Instant::now();
        let target = self.next.unwrap_or(now);
        if target > now {
            let left = target - now;
            if left > SPIN {
                std::thread::sleep(left - SPIN);
            }
            while Instant::now() < target {
                std::hint::spin_loop();
            }
        }
        // Keep the cadence, but don't try to catch up after a stall.
        let after = Instant::now();
        let next = target + period;
        self.next = Some(if next < after { after } else { next });
    }
}

impl Default for Limiter {
    fn default() -> Self {
        Self::new()
    }
}

/// Counts frames over a window for a frames-per-second readout.
#[derive(Default)]
pub struct FpsCounter {
    start: Option<Instant>,
    frames: u32,
    pub fps: f64,
}

impl FpsCounter {
    pub fn frame(&mut self, now: Instant) {
        let start = *self.start.get_or_insert(now);
        self.frames += 1;
        let dt = now.saturating_duration_since(start).as_secs_f64();
        if dt >= 0.5 {
            self.fps = self.frames as f64 / dt;
            self.frames = 0;
            self.start = Some(now);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ticks_come_from_the_accumulator() {
        let mut s = FixedStep::new(TICK);
        let t0 = Instant::now();
        assert_eq!(s.advance(t0), 1);
        assert_eq!(s.advance(t0 + Duration::from_millis(8)), 0);
        assert!((s.alpha() - 8.0 / 17.0).abs() < 1e-3);
        assert_eq!(s.advance(t0 + Duration::from_millis(8 + 26)), 2);
        assert!(s.alpha() < 1e-3);
        // A long stall runs at most MAX_TICKS_PER_FRAME and drops the rest.
        assert_eq!(s.advance(t0 + Duration::from_secs(5)), MAX_TICKS_PER_FRAME);
        assert_eq!(s.alpha(), 0.0);
    }

    #[test]
    fn caps_parse() {
        assert_eq!(FpsCap::parse("off"), Some(FpsCap::Off));
        assert_eq!(FpsCap::parse("VSync"), Some(FpsCap::VSync));
        assert_eq!(FpsCap::parse("144"), Some(FpsCap::Fps(144.0)));
        assert_eq!(FpsCap::parse("x"), None);
    }
}
