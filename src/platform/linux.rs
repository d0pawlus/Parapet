//! Linux on X11, or XWayland under a Wayland session: monitors from RandR, windows from the window manager's
//! EWMH stacking list, the pointer from QueryPointer, the screen from DPMS, power from sysfs, an XDG autostart
//! entry. Coordinates are physical pixels on the X root window.
//!
//! Under Wayland XWayland sees only X11 apps and loses the pointer over native windows, so there the overlay
//! only falls: no windows to land on, no pointer to dodge.

use super::{MonitorInfo, RawWindow, Rect};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use x11rb::connection::Connection;
use x11rb::protocol::randr::ConnectionExt as _;
use x11rb::protocol::xproto::{AtomEnum, ConfigureWindowAux, ConnectionExt as _, StackMode, Window};
use x11rb::rust_connection::RustConnection;

x11rb::atom_manager! {
    Atoms: AtomsCookie {
        _NET_CLIENT_LIST_STACKING,
        _NET_ACTIVE_WINDOW,
        _NET_CURRENT_DESKTOP,
        _NET_WORKAREA,
        _NET_WM_DESKTOP,
        _NET_WM_STATE,
        _NET_WM_STATE_HIDDEN,
        _NET_WM_STATE_MAXIMIZED_VERT,
        _NET_WM_STATE_MAXIMIZED_HORZ,
        _NET_WM_WINDOW_TYPE,
        _NET_WM_WINDOW_TYPE_NORMAL,
        _NET_WM_WINDOW_TYPE_DIALOG,
        _NET_FRAME_EXTENTS,
        _GTK_FRAME_EXTENTS,
    }
}

/// A connection of our own, separate from winit's.
struct X11 {
    conn: RustConnection,
    root: Window,
    screen: usize,
    atoms: Atoms,
}

fn x11() -> Option<&'static X11> {
    static X: OnceLock<Option<X11>> = OnceLock::new();
    X.get_or_init(|| {
        let (conn, screen) = x11rb::connect(None).map_err(|e| eprintln!("parapet: no X11 display: {e}")).ok()?;
        let root = conn.setup().roots.get(screen)?.root;
        let atoms = Atoms::new(&conn).ok()?.reply().ok()?;
        Some(X11 { conn, root, screen, atoms })
    })
    .as_ref()
}

/// A Wayland session: XWayland's view of windows and the pointer is incomplete there.
fn wayland_session() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_some()
        || std::env::var("XDG_SESSION_TYPE").is_ok_and(|t| t.eq_ignore_ascii_case("wayland"))
}

// ---------------------------------------------------------------- process

/// Process set-up; returns false when another instance is already running.
pub fn init() -> bool {
    let dir = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(config_dir);
    let _ = std::fs::create_dir_all(&dir);
    let Ok(file) = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(dir.join("parapet.lock"))
    else {
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

fn config_dir() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|| home().join(".config"))
}

pub fn settings_path() -> PathBuf {
    config_dir().join("parapet/settings.ini")
}

pub fn local_month() -> u32 {
    unsafe {
        let now = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&now, &mut tm);
        tm.tm_mon as u32 + 1
    }
}

// ---------------------------------------------------------------- the tray menu runs on GTK

/// Must be called on the main thread before the tray icon is made.
pub fn init_ui() {
    if let Err(e) = gtk::init() {
        eprintln!("parapet: GTK unavailable, no tray icon: {e}");
    }
}

/// GTK has no thread of its own here: run whatever it has queued (tray clicks, menu redraws).
pub fn pump_ui() {
    if !gtk::is_initialized_main_thread() {
        return;
    }
    while gtk::events_pending() {
        gtk::main_iteration_do(false);
    }
}

/// How often the main thread wakes to pump GTK, in seconds.
pub const UI_PUMP_INTERVAL: Option<f64> = Some(0.05);

// ---------------------------------------------------------------- monitors, pointer, windows

fn cardinals(x: &X11, window: Window, property: u32, len: u32) -> Option<Vec<u32>> {
    let reply = x.conn.get_property(false, window, property, AtomEnum::ANY, 0, len).ok()?.reply().ok()?;
    let values = reply.value32()?.collect();
    Some(values)
}

/// The work area is one rectangle over all monitors; each monitor gets its share of it.
fn work_area(monitor: Rect, workarea: Option<Rect>) -> Rect {
    let Some(w) = workarea else { return monitor };
    let r = Rect { l: monitor.l.max(w.l), t: monitor.t.max(w.t), r: monitor.r.min(w.r), b: monitor.b.min(w.b) };
    if r.width() > 0.0 && r.height() > 0.0 {
        r
    } else {
        monitor
    }
}

pub fn monitors() -> Vec<MonitorInfo> {
    let Some(x) = x11() else { return Vec::new() };
    let Some(reply) = x.conn.randr_get_monitors(x.root, true).ok().and_then(|c| c.reply().ok()) else {
        return Vec::new();
    };
    let workarea = cardinals(x, x.root, x.atoms._NET_WORKAREA, 4)
        .filter(|v| v.len() == 4)
        .map(|v| Rect { l: v[0] as f32, t: v[1] as f32, r: (v[0] + v[2]) as f32, b: (v[1] + v[3]) as f32 });
    let mut out = Vec::new();
    for m in reply.monitors {
        let rect = Rect { l: m.x as f32, t: m.y as f32, r: (m.x as i32 + m.width as i32) as f32, b: (m.y as i32 + m.height as i32) as f32 };
        let info = MonitorInfo { rect, work: work_area(rect, workarea) };
        // the primary monitor first: its height sets the size of everything
        if m.primary {
            out.insert(0, info);
        } else {
            out.push(info);
        }
    }
    out
}

pub fn cursor_pos() -> Option<(f32, f32)> {
    if wayland_session() {
        return None;
    }
    let x = x11()?;
    let p = x.conn.query_pointer(x.root).ok()?.reply().ok()?;
    p.same_screen.then_some((p.root_x as f32, p.root_y as f32))
}

/// Edges as EWMH stores them: left, right, top, bottom.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Extents {
    l: f32,
    r: f32,
    t: f32,
    b: f32,
}

impl Extents {
    fn from(v: Option<Vec<u32>>) -> Extents {
        match v.as_deref() {
            Some([l, r, t, b, ..]) => Extents { l: *l as f32, r: *r as f32, t: *t as f32, b: *b as f32 },
            _ => Extents::default(),
        }
    }
}

/// What the user sees of a window: the client area grown by the window manager's frame, shrunk by the
/// invisible shadow a client-side-decorated (GTK) window draws around itself.
fn visible_rect(client: Rect, frame: Extents, csd_shadow: Extents) -> Rect {
    Rect {
        l: client.l - frame.l + csd_shadow.l,
        t: client.t - frame.t + csd_shadow.t,
        r: client.r + frame.r - csd_shadow.r,
        b: client.b + frame.b - csd_shadow.b,
    }
}

const ALL_DESKTOPS: u32 = 0xFFFF_FFFF;

pub fn enum_windows() -> Vec<RawWindow> {
    if wayland_session() {
        return Vec::new();
    }
    let Some(x) = x11() else { return Vec::new() };
    let a = &x.atoms;
    // bottom to top; we want front to back
    let Some(mut stack) = cardinals(x, x.root, a._NET_CLIENT_LIST_STACKING, 4096) else { return Vec::new() };
    stack.reverse();
    let active = cardinals(x, x.root, a._NET_ACTIVE_WINDOW, 1).and_then(|v| v.first().copied());
    let desktop = cardinals(x, x.root, a._NET_CURRENT_DESKTOP, 1).and_then(|v| v.first().copied());

    // every request first, then every reply: one round trip for the whole list
    let prop = |w, p, len| x.conn.get_property(false, w, p, AtomEnum::ANY, 0, len).ok();
    let cookies: Vec<_> = stack
        .iter()
        .map(|&w| {
            (
                w,
                prop(w, a._NET_WM_STATE, 32),
                prop(w, a._NET_WM_WINDOW_TYPE, 8),
                prop(w, a._NET_WM_DESKTOP, 1),
                prop(w, a._NET_FRAME_EXTENTS, 4),
                prop(w, a._GTK_FRAME_EXTENTS, 4),
                x.conn.get_geometry(w).ok(),
                x.conn.translate_coordinates(w, x.root, 0, 0).ok(),
            )
        })
        .collect();

    let values = |c: Option<x11rb::cookie::Cookie<'_, RustConnection, x11rb::protocol::xproto::GetPropertyReply>>| {
        c.and_then(|c| c.reply().ok()).and_then(|r| r.value32().map(|v| v.collect::<Vec<u32>>()))
    };
    let mut out = Vec::new();
    for (w, state, kind, on_desktop, frame, csd, geometry, origin) in cookies {
        let state = values(state).unwrap_or_default();
        let kind = values(kind).unwrap_or_default();
        let on_desktop = values(on_desktop).and_then(|v| v.first().copied());
        let (frame, csd) = (Extents::from(values(frame)), Extents::from(values(csd)));
        let geometry = geometry.and_then(|c| c.reply().ok());
        let origin = origin.and_then(|c| c.reply().ok());
        let (Some(g), Some(o)) = (geometry, origin) else { continue };
        if state.contains(&a._NET_WM_STATE_HIDDEN) {
            continue;
        }
        // panels, docks, desktops, menus and tooltips are not windows to land on
        if !kind.is_empty() && !kind.iter().any(|&k| k == a._NET_WM_WINDOW_TYPE_NORMAL || k == a._NET_WM_WINDOW_TYPE_DIALOG) {
            continue;
        }
        if let (Some(d), Some(cur)) = (on_desktop, desktop) {
            if d != cur && d != ALL_DESKTOPS {
                continue;
            }
        }
        let client = Rect {
            l: o.dst_x as f32,
            t: o.dst_y as f32,
            r: (o.dst_x as i32 + g.width as i32) as f32,
            b: (o.dst_y as i32 + g.height as i32) as f32,
        };
        let rect = visible_rect(client, frame, csd);
        if rect.width() < 1.0 || rect.height() < 1.0 {
            continue;
        }
        let maximized = state.contains(&a._NET_WM_STATE_MAXIMIZED_VERT) && state.contains(&a._NET_WM_STATE_MAXIMIZED_HORZ);
        out.push(RawWindow { id: w as u64, rect, maximized, foreground: Some(w) == active });
    }
    out
}

/// Moving windows changes no root property, so there is nothing cheap to watch: re-list every frame, which
/// costs one round trip to the X server.
pub fn install_hooks() {}
pub fn take_track_dirty() -> bool {
    true
}
pub fn moving() -> bool {
    false
}
pub const TRACK_SAFETY_INTERVAL: f64 = 0.0;

// ---------------------------------------------------------------- power and session

pub fn display_off() -> bool {
    use x11rb::protocol::dpms::{ConnectionExt as _, DPMSMode};
    let Some(x) = x11() else { return false };
    let Some(info) = x.conn.dpms_info().ok().and_then(|c| c.reply().ok()) else { return false };
    info.state && info.power_level != DPMSMode::ON
}

/// Lock screens on X11 blank the display (caught by `display_off`); there is no portable cheap query beyond that.
pub fn session_locked() -> bool {
    false
}

/// One power supply from /sys/class/power_supply: its `type` and `status` files.
fn discharging(supplies: &[(String, String)]) -> bool {
    supplies.iter().any(|(kind, status)| kind.trim() == "Battery" && status.trim() == "Discharging")
}

pub fn on_battery() -> bool {
    let Ok(dir) = std::fs::read_dir("/sys/class/power_supply") else { return false };
    let supplies: Vec<_> = dir
        .flatten()
        .map(|e| {
            let read = |f: &str| std::fs::read_to_string(e.path().join(f)).unwrap_or_default();
            (read("type"), read("status"))
        })
        .collect();
    discharging(&supplies)
}

/// The kernel schedules a mostly sleeping process well on its own; nothing to switch.
pub fn set_eco_qos(_on: bool) {}

// ---------------------------------------------------------------- autostart + settings

fn autostart_path() -> PathBuf {
    config_dir().join("autostart/parapet.desktop")
}

pub fn autostart_enabled() -> bool {
    autostart_path().exists()
}

/// The program to start at login: an AppImage runs from a temporary mount, so it must be the AppImage itself.
fn launcher() -> PathBuf {
    std::env::var_os("APPIMAGE").map(PathBuf::from).unwrap_or_else(|| std::env::current_exe().unwrap_or_default())
}

/// An XDG autostart entry; `Exec` quotes the path as the Desktop Entry spec asks.
fn desktop_entry(exe: &Path) -> String {
    let quoted: String = exe
        .display()
        .to_string()
        .chars()
        .flat_map(|c| match c {
            '"' | '`' | '$' | '\\' => vec!['\\', c],
            '%' => vec!['%', '%'],
            c => vec![c],
        })
        .collect();
    format!(
        "[Desktop Entry]\nType=Application\nName=Parapet\nComment=Falling leaves, rain and snow over your desktop\n\
         Exec=\"{quoted}\"\nIcon=parapet\nTerminal=false\nX-GNOME-Autostart-enabled=true\n"
    )
}

pub fn set_autostart(on: bool) {
    let path = autostart_path();
    if !on {
        let _ = std::fs::remove_file(path);
        return;
    }
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Err(e) = std::fs::write(&path, desktop_entry(&launcher())) {
        eprintln!("parapet: cannot write {}: {e}", path.display());
    }
}

pub fn open_in_editor(path: &Path) {
    let _ = std::process::Command::new("xdg-open").arg(path).spawn();
}

// ---------------------------------------------------------------- overlay windows

/// Without a compositor an X11 window cannot be see-through: the overlay would be a black sheet.
pub fn overlay_supported() -> Result<(), String> {
    let x = x11().ok_or("no X11 display")?;
    let name = format!("_NET_WM_CM_S{}", x.screen);
    let atom = x.conn.intern_atom(false, name.as_bytes()).ok().and_then(|c| c.reply().ok()).map(|r| r.atom);
    let owner = atom.and_then(|a| x.conn.get_selection_owner(a).ok()).and_then(|c| c.reply().ok()).map(|r| r.owner);
    match owner {
        Some(w) if w != x11rb::NONE => Ok(()),
        _ => Err("no compositing window manager is running, so the overlay cannot be transparent".into()),
    }
}

fn xid(window: &winit::window::Window) -> Option<Window> {
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    match window.window_handle().ok()?.as_raw() {
        RawWindowHandle::Xlib(h) => Some(h.window as Window),
        RawWindowHandle::Xcb(h) => Some(h.window.get()),
        _ => None,
    }
}

/// The window is override-redirect (set when it is made), so the window manager leaves it alone.
pub fn prepare_overlay(window: &winit::window::Window) {
    raise_overlay(window);
}

/// Windows raised later may have covered us.
pub fn raise_overlay(window: &winit::window::Window) {
    let (Some(x), Some(w)) = (x11(), xid(window)) else { return };
    let _ = x.conn.configure_window(w, &ConfigureWindowAux::new().stack_mode(StackMode::ABOVE));
    let _ = x.conn.flush();
}

/// X11 sizes windows in physical pixels, which is what the overlay works in.
pub const UNITS_ARE_PIXELS: bool = true;

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(l: f32, t: f32, r: f32, b: f32) -> Rect {
        Rect { l, t, r, b }
    }

    #[test]
    fn work_area_is_the_monitor_share_of_the_desktop_work_area() {
        let workarea = Some(rect(0.0, 32.0, 3840.0, 1080.0));
        assert_eq!(work_area(rect(1920.0, 0.0, 3840.0, 1080.0), workarea), rect(1920.0, 32.0, 3840.0, 1080.0));
    }

    #[test]
    fn work_area_falls_back_to_the_monitor_when_it_misses_it() {
        let m = rect(1920.0, 0.0, 3840.0, 1080.0);
        assert_eq!(work_area(m, Some(rect(0.0, 0.0, 1920.0, 1080.0))), m);
        assert_eq!(work_area(m, None), m);
    }

    #[test]
    fn visible_rect_adds_the_window_manager_frame() {
        let frame = Extents { l: 2.0, r: 2.0, t: 30.0, b: 2.0 };
        assert_eq!(visible_rect(rect(100.0, 100.0, 500.0, 400.0), frame, Extents::default()), rect(98.0, 70.0, 502.0, 402.0));
    }

    #[test]
    fn visible_rect_drops_the_shadow_of_a_gtk_window() {
        let shadow = Extents { l: 26.0, r: 26.0, t: 23.0, b: 29.0 };
        assert_eq!(visible_rect(rect(0.0, 0.0, 852.0, 652.0), Extents::default(), shadow), rect(26.0, 23.0, 826.0, 623.0));
    }

    #[test]
    fn extents_need_all_four_edges() {
        assert_eq!(Extents::from(Some(vec![1, 2, 3, 4])), Extents { l: 1.0, r: 2.0, t: 3.0, b: 4.0 });
        assert_eq!(Extents::from(Some(vec![1, 2])), Extents::default());
        assert_eq!(Extents::from(None), Extents::default());
    }

    #[test]
    fn on_battery_only_when_a_battery_discharges() {
        let s = |k: &str, st: &str| (k.to_string(), st.to_string());
        assert!(discharging(&[s("Mains\n", ""), s("Battery\n", "Discharging\n")]));
        assert!(!discharging(&[s("Mains\n", ""), s("Battery\n", "Charging\n")]));
        assert!(!discharging(&[s("Battery", "Full")]));
        assert!(!discharging(&[]));
    }

    #[test]
    fn desktop_entry_quotes_the_program_path() {
        let entry = desktop_entry(Path::new("/home/a b/Parapet$1%.AppImage"));
        assert!(entry.contains("Exec=\"/home/a b/Parapet\\$1%%.AppImage\"\n"), "{entry}");
        assert!(entry.starts_with("[Desktop Entry]\n"));
    }
}
