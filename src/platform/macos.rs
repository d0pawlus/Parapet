//! macOS: screens from NSScreen, windows from the window server (CGWindowList), the pointer from a CGEvent,
//! power from IOKit, a LaunchAgent for autostart. Coordinates are points with the origin at the top-left of
//! the main display, as Core Graphics uses them.

use super::{MonitorInfo, RawWindow, Rect};
use core_foundation::base::{CFType, TCFType};
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::{CFDictionary, CFDictionaryRef};
use core_foundation::number::{CFNumber, CFNumberRef};
use core_foundation::string::{CFString, CFStringRef};
use core_graphics::event::CGEvent;
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
use core_graphics::geometry::CGRect;
use core_graphics::window::*;
use objc2::MainThreadMarker;
use objc2_app_kit::{NSScreen, NSView, NSWindowCollectionBehavior};
use std::ffi::c_void;
use std::path::PathBuf;

/// Process set-up; returns false when another instance is already running.
pub fn init() -> bool {
    let dir = support_dir();
    let _ = std::fs::create_dir_all(&dir);
    let Ok(file) = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(dir.join(".lock")) else {
        return true;
    };
    use std::os::fd::AsRawFd;
    let ok = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0;
    // held for the life of the process
    std::mem::forget(file);
    ok
}

fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/tmp"))
}

fn support_dir() -> PathBuf {
    home().join("Library/Application Support/Parapet")
}

pub fn settings_path() -> PathBuf {
    support_dir().join("settings.ini")
}

pub fn local_month() -> u32 {
    unsafe {
        let now = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&now, &mut tm);
        tm.tm_mon as u32 + 1
    }
}

/// Must be called on the main thread.
pub fn monitors() -> Vec<MonitorInfo> {
    let Some(mtm) = MainThreadMarker::new() else { return Vec::new() };
    let screens = NSScreen::screens(mtm);
    let mut out = Vec::new();
    // AppKit's origin is the bottom-left of the main (first) screen, y up; flip to top-left, y down
    let main_h = screens.iter().next().map(|s| s.frame().size.height).unwrap_or(0.0);
    for s in screens.iter() {
        let flip = |r: objc2_foundation::NSRect| Rect {
            l: r.origin.x as f32,
            t: (main_h - r.origin.y - r.size.height) as f32,
            r: (r.origin.x + r.size.width) as f32,
            b: (main_h - r.origin.y) as f32,
        };
        out.push(MonitorInfo { rect: flip(s.frame()), work: flip(s.visibleFrame()) });
    }
    out
}

pub fn cursor_pos() -> Option<(f32, f32)> {
    let src = CGEventSource::new(CGEventSourceStateID::CombinedSessionState).ok()?;
    let ev = CGEvent::new(src).ok()?;
    let p = ev.location();
    Some((p.x as f32, p.y as f32))
}

fn number(dict: &CFDictionary, key: CFStringRef) -> Option<f64> {
    let v = dict.find(key as *const c_void)?;
    let n = unsafe { CFNumber::wrap_under_get_rule(*v as CFNumberRef) };
    n.to_f64()
}

pub fn enum_windows() -> Vec<RawWindow> {
    let mut out = Vec::new();
    let Some(list) = copy_window_info(kCGWindowListOptionOnScreenOnly | kCGWindowListExcludeDesktopElements, kCGNullWindowID)
    else {
        return out;
    };
    let me = std::process::id() as f64;
    let mut first = true;
    for item in list.iter() {
        let dict = unsafe { CFDictionary::wrap_under_get_rule(*item as CFDictionaryRef) };
        unsafe {
            // layer 0 holds ordinary app windows; the Dock, menu bar and overlays sit higher
            if number(&dict, kCGWindowLayer) != Some(0.0) || number(&dict, kCGWindowOwnerPID) == Some(me) {
                continue;
            }
            if number(&dict, kCGWindowAlpha).is_some_and(|a| a <= 0.01) {
                continue;
            }
            let Some(id) = number(&dict, kCGWindowNumber) else { continue };
            let Some(b) = dict.find(kCGWindowBounds as *const c_void) else { continue };
            let bounds = CFDictionary::wrap_under_get_rule(*b as CFDictionaryRef);
            let Some(r) = CGRect::from_dict_representation(&bounds) else { continue };
            out.push(RawWindow {
                id: id as u64,
                rect: Rect {
                    l: r.origin.x as f32,
                    t: r.origin.y as f32,
                    r: (r.origin.x + r.size.width) as f32,
                    b: (r.origin.y + r.size.height) as f32,
                },
                maximized: false,
                foreground: first,
            });
            first = false;
        }
    }
    out
}

/// The window server has no cheap change notifications for other apps' windows: re-list every frame.
pub fn install_hooks() {}
pub fn take_track_dirty() -> bool {
    true
}
pub fn moving() -> bool {
    false
}
pub const TRACK_SAFETY_INTERVAL: f64 = 0.0;

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGSessionCopyCurrentDictionary() -> CFDictionaryRef;
    fn CGMainDisplayID() -> u32;
    fn CGDisplayIsAsleep(display: u32) -> u32;
}

#[link(name = "IOKit", kind = "framework")]
extern "C" {
    fn IOPSCopyPowerSourcesInfo() -> *const c_void;
    fn IOPSGetProvidingPowerSourceType(snapshot: *const c_void) -> CFStringRef;
}

pub fn display_off() -> bool {
    unsafe { CGDisplayIsAsleep(CGMainDisplayID()) != 0 }
}

pub fn session_locked() -> bool {
    unsafe {
        let d = CGSessionCopyCurrentDictionary();
        if d.is_null() {
            return false;
        }
        let dict: CFDictionary = CFDictionary::wrap_under_create_rule(d);
        let key = CFString::from_static_string("CGSSessionScreenIsLocked");
        match dict.find(key.as_concrete_TypeRef() as *const c_void) {
            Some(v) => {
                let v = CFType::wrap_under_get_rule(*v as _);
                v.downcast::<CFBoolean>().map(bool::from).unwrap_or(false)
            }
            None => false,
        }
    }
}

pub fn on_battery() -> bool {
    unsafe {
        let info = IOPSCopyPowerSourcesInfo();
        if info.is_null() {
            return false;
        }
        let kind = IOPSGetProvidingPowerSourceType(info);
        let battery = !kind.is_null() && CFString::wrap_under_get_rule(kind) == "Battery Power";
        core_foundation::base::CFRelease(info);
        battery
    }
}

/// macOS schedules a background-ish app well on its own; nothing to switch.
pub fn set_eco_qos(_on: bool) {}

// ---------------------------------------------------------------- autostart + settings

fn agent_path() -> PathBuf {
    home().join("Library/LaunchAgents/com.d0pawlus.parapet.plist")
}

pub fn autostart_enabled() -> bool {
    agent_path().exists()
}

pub fn set_autostart(on: bool) {
    let path = agent_path();
    if !on {
        let _ = std::fs::remove_file(path);
        return;
    }
    let exe = std::env::current_exe().unwrap_or_default();
    let exe = exe.display().to_string().replace('&', "&amp;").replace('<', "&lt;");
    let plist = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <plist version=\"1.0\">\n<dict>\n\
         \t<key>Label</key>\n\t<string>com.d0pawlus.parapet</string>\n\
         \t<key>ProgramArguments</key>\n\t<array>\n\t\t<string>{exe}</string>\n\t</array>\n\
         \t<key>RunAtLoad</key>\n\t<true/>\n\
         \t<key>ProcessType</key>\n\t<string>Interactive</string>\n\
         </dict>\n</plist>\n"
    );
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(path, plist);
}

pub fn open_in_editor(path: &std::path::Path) {
    let _ = std::process::Command::new("open").arg("-t").arg(path).spawn();
}

// ---------------------------------------------------------------- overlay windows

/// Above every app window and the menu bar, on every Space and next to full-screen apps, never focused.
pub fn prepare_overlay(window: &winit::window::Window) {
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let Ok(handle) = window.window_handle() else { return };
    let RawWindowHandle::AppKit(h) = handle.as_raw() else { return };
    let view: &NSView = unsafe { h.ns_view.cast().as_ref() };
    let Some(ns_window) = view.window() else { return };
    // NSStatusWindowLevel: above the menu bar, below menus and alerts
    ns_window.setLevel(25);
    ns_window.setHasShadow(false);
    ns_window.setIgnoresMouseEvents(true);
    ns_window.setCollectionBehavior(
        NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::Stationary
            | NSWindowCollectionBehavior::IgnoresCycle
            | NSWindowCollectionBehavior::FullScreenAuxiliary,
    );
}

pub fn raise_overlay(_window: &winit::window::Window) {}

/// macOS sizes windows in points; surfaces are larger on Retina screens.
pub const UNITS_ARE_PIXELS: bool = false;
