//! Everything that differs between operating systems: monitors, the windows on screen, the pointer, power and
//! session state, autostart, and dressing up the overlay windows. Coordinates are global desktop units with y
//! pointing down: physical pixels on Windows and Linux, points on macOS.

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use self::windows::*;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use self::macos::*;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use self::linux::*;

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
mod other;
#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub use self::other::*;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub l: f32,
    pub t: f32,
    pub r: f32,
    pub b: f32,
}

impl Rect {
    pub fn offset(&self, dx: f32, dy: f32) -> Rect {
        Rect { l: self.l + dx, t: self.t + dy, r: self.r + dx, b: self.b + dy }
    }
    pub fn width(&self) -> f32 {
        self.r - self.l
    }
    pub fn height(&self) -> f32 {
        self.b - self.t
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MonitorInfo {
    pub rect: Rect, // the whole monitor
    pub work: Rect, // without the taskbar / menu bar and Dock
}

/// A top-level window, front to back.
#[derive(Clone, Copy, Debug)]
pub struct RawWindow {
    pub id: u64,
    pub rect: Rect,
    pub maximized: bool,
    pub foreground: bool,
}

/// What the tray menu calls starting with the system.
pub const AUTOSTART_LABEL: &str = if cfg!(target_os = "macos") {
    "Open at login"
} else if cfg!(windows) {
    "Start with Windows"
} else {
    "Start on login"
};
