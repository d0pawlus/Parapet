//! Fallbacks for systems without a native layer yet (Linux, BSD): the overlay runs, but it knows only the
//! monitors and the pointer winit can tell it about, so nothing lands on windows.

use super::{MonitorInfo, RawWindow};
use std::path::PathBuf;

pub fn init() -> bool {
    true
}

pub fn settings_path() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("parapet/settings.ini")
}

pub fn local_month() -> u32 {
    10
}

pub fn monitors() -> Vec<MonitorInfo> {
    Vec::new()
}

pub fn cursor_pos() -> Option<(f32, f32)> {
    None
}

pub fn enum_windows() -> Vec<RawWindow> {
    Vec::new()
}

pub fn install_hooks() {}
pub fn take_track_dirty() -> bool {
    false
}
pub fn moving() -> bool {
    false
}
pub const TRACK_SAFETY_INTERVAL: f64 = 2.0;
pub fn display_off() -> bool {
    false
}
pub fn session_locked() -> bool {
    false
}
pub fn on_battery() -> bool {
    false
}
pub fn set_eco_qos(_on: bool) {}
pub fn autostart_enabled() -> bool {
    false
}
pub fn set_autostart(_on: bool) {}
pub fn open_in_editor(path: &std::path::Path) {
    let _ = std::process::Command::new("xdg-open").arg(path).spawn();
}
pub fn prepare_overlay(_window: &winit::window::Window) {}
pub fn raise_overlay(_window: &winit::window::Window) {}
pub const UNITS_ARE_PIXELS: bool = true;
