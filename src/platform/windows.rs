//! Windows: Win32 window enumeration with DWM frame bounds, WinEvent hooks so windows are re-listed only when
//! something changed, power and session state, EcoQoS on battery and the Run key for autostart.

use super::{MonitorInfo, RawWindow, Rect};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Dwm::*;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::Power::*;
use windows_sys::Win32::System::Registry::*;
use windows_sys::Win32::System::StationsAndDesktops::*;
use windows_sys::Win32::System::SystemInformation::GetLocalTime;
use windows_sys::Win32::System::SystemServices::GUID_CONSOLE_DISPLAY_STATE;
use windows_sys::Win32::System::Threading::*;
use windows_sys::Win32::UI::Accessibility::*;
use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

static TRACK_DIRTY: AtomicBool = AtomicBool::new(true);
static MOVING: AtomicBool = AtomicBool::new(false);
static DISPLAY_OFF: AtomicBool = AtomicBool::new(false);

/// Process set-up; returns false when another instance is already running.
pub fn init() -> bool {
    unsafe {
        let name = wide("Local\\Parapet.SingleInstance");
        let m = CreateMutexW(std::ptr::null(), 1, name.as_ptr());
        if m.is_null() || GetLastError() == ERROR_ALREADY_EXISTS {
            return false;
        }
        // deliberately leaked: held for the life of the process
        // normal priority: the work per frame is tiny, but late wake-ups would make the animation stutter
        SetPriorityClass(GetCurrentProcess(), NORMAL_PRIORITY_CLASS);
    }
    register_display_notifications();
    true
}

pub fn settings_path() -> PathBuf {
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("parapet.exe"));
    exe.parent().map(|p| p.join("settings.ini")).unwrap_or_else(|| PathBuf::from("settings.ini"))
}

pub fn local_month() -> u32 {
    unsafe {
        let mut t = std::mem::zeroed();
        GetLocalTime(&mut t);
        t.wMonth as u32
    }
}

fn rect(r: &RECT) -> Rect {
    Rect { l: r.left as f32, t: r.top as f32, r: r.right as f32, b: r.bottom as f32 }
}

pub fn monitors() -> Vec<MonitorInfo> {
    unsafe extern "system" fn collect(m: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> windows_sys::core::BOOL {
        let list = &mut *(data as *mut Vec<MonitorInfo>);
        let mut mi: MONITORINFO = std::mem::zeroed();
        mi.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        if GetMonitorInfoW(m, &mut mi) != 0 {
            let r = rect(&mi.rcMonitor);
            let work = rect(&mi.rcWork);
            // the primary monitor first: its height sets the size of everything
            if mi.dwFlags & MONITORINFOF_PRIMARY != 0 {
                list.insert(0, MonitorInfo { rect: r, work });
            } else {
                list.push(MonitorInfo { rect: r, work });
            }
        }
        1
    }
    let mut list: Vec<MonitorInfo> = Vec::new();
    unsafe {
        EnumDisplayMonitors(std::ptr::null_mut(), std::ptr::null(), Some(collect), &mut list as *mut _ as LPARAM);
    }
    list
}

/// One GetCursorPos per frame: no mouse hook, so pointer movement never wakes the process by itself.
pub fn cursor_pos() -> Option<(f32, f32)> {
    unsafe {
        let mut p = POINT { x: 0, y: 0 };
        (GetCursorPos(&mut p) != 0).then_some((p.x as f32, p.y as f32))
    }
}

pub fn enum_windows() -> Vec<RawWindow> {
    unsafe extern "system" fn collect(h: HWND, data: LPARAM) -> windows_sys::core::BOOL {
        (*(data as *mut Vec<HWND>)).push(h);
        1
    }
    let mut handles: Vec<HWND> = Vec::with_capacity(256);
    let mut out = Vec::new();
    unsafe {
        EnumWindows(Some(collect), &mut handles as *mut _ as LPARAM);
        let fg = GetForegroundWindow();
        let me = GetCurrentProcessId();
        let mut buf = [0u16; 64];
        for h in handles {
            if IsWindowVisible(h) == 0 || IsIconic(h) != 0 {
                continue;
            }
            let mut pid = 0;
            GetWindowThreadProcessId(h, &mut pid);
            if pid == me {
                continue;
            }
            let ex = GetWindowLongW(h, GWL_EXSTYLE) as u32;
            if ex & (WS_EX_TOOLWINDOW | WS_EX_TRANSPARENT) != 0 {
                continue;
            }
            // InternalGetWindowText never sends messages, so a hung app cannot stall us
            if InternalGetWindowText(h, buf.as_mut_ptr(), 2) == 0 {
                continue;
            }
            let mut cloaked: u32 = 0;
            if DwmGetWindowAttribute(h, DWMWA_CLOAKED as u32, &mut cloaked as *mut _ as *mut _, 4) >= 0 && cloaked != 0 {
                continue;
            }
            let n = GetClassNameW(h, buf.as_mut_ptr(), 64);
            if n > 0 {
                let class = String::from_utf16_lossy(&buf[..n as usize]);
                if matches!(class.as_str(), "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd") {
                    continue;
                }
            }
            let mut r: RECT = std::mem::zeroed();
            if DwmGetWindowAttribute(
                h,
                DWMWA_EXTENDED_FRAME_BOUNDS as u32,
                &mut r as *mut _ as *mut _,
                std::mem::size_of::<RECT>() as u32,
            ) < 0
                && GetWindowRect(h, &mut r) == 0
            {
                continue;
            }
            out.push(RawWindow { id: h as usize as u64, rect: rect(&r), maximized: IsZoomed(h) != 0, foreground: h == fg });
        }
    }
    out
}

// ---------------------------------------------------------------- change notifications

unsafe extern "system" fn on_win_event(_: HWINEVENTHOOK, ev: u32, h: HWND, obj: i32, child: i32, _: u32, _: u32) {
    if obj != OBJID_WINDOW || child != CHILDID_SELF as i32 || h.is_null() {
        return;
    }
    if ev == EVENT_SYSTEM_MOVESIZESTART {
        MOVING.store(true, Ordering::Relaxed);
    } else if ev == EVENT_SYSTEM_MOVESIZEEND {
        MOVING.store(false, Ordering::Relaxed);
    }
    TRACK_DIRTY.store(true, Ordering::Relaxed);
}

/// Must be called on a thread that pumps messages (the main thread).
pub fn install_hooks() {
    // deliberately no EVENT_OBJECT_LOCATIONCHANGE: it fires on every mouse move system-wide
    let ranges = [
        (EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_FOREGROUND),
        (EVENT_SYSTEM_MOVESIZESTART, EVENT_SYSTEM_MOVESIZEEND),
        (EVENT_SYSTEM_MINIMIZESTART, EVENT_SYSTEM_MINIMIZEEND),
        (EVENT_OBJECT_DESTROY, EVENT_OBJECT_REORDER),
        (EVENT_OBJECT_CLOAKED, EVENT_OBJECT_UNCLOAKED),
    ];
    for (a, b) in ranges {
        unsafe {
            SetWinEventHook(a, b, std::ptr::null_mut(), Some(on_win_event), 0, 0, WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS);
        }
    }
}

/// Did a window appear, move, change order or disappear since the last call?
pub fn take_track_dirty() -> bool {
    TRACK_DIRTY.swap(false, Ordering::Relaxed)
}

/// Is a window being dragged right now (re-list every frame)?
pub fn moving() -> bool {
    MOVING.load(Ordering::Relaxed)
}

/// How often to re-list windows when nothing reported a change (a safety net).
pub const TRACK_SAFETY_INTERVAL: f64 = 0.5;

unsafe extern "system" fn on_power(_: *const core::ffi::c_void, kind: u32, setting: *const core::ffi::c_void) -> u32 {
    if kind == PBT_POWERSETTINGCHANGE && !setting.is_null() {
        let ps = &*(setting as *const POWERBROADCAST_SETTING);
        if ps.PowerSetting.data1 == GUID_CONSOLE_DISPLAY_STATE.data1 && ps.DataLength >= 1 {
            DISPLAY_OFF.store(ps.Data[0] == 0, Ordering::Relaxed);
        }
    }
    0
}

fn register_display_notifications() {
    // a callback rather than a window message, so the render thread doesn't depend on any window
    static mut PARAMS: DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS =
        DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS { Callback: Some(on_power), Context: std::ptr::null_mut() };
    unsafe {
        RegisterPowerSettingNotification(
            std::ptr::addr_of_mut!(PARAMS) as HANDLE,
            &GUID_CONSOLE_DISPLAY_STATE,
            DEVICE_NOTIFY_CALLBACK,
        );
    }
}

pub fn display_off() -> bool {
    DISPLAY_OFF.load(Ordering::Relaxed)
}

/// The lock screen runs on its own desktop: if the input desktop isn't ours, the session is locked.
pub fn session_locked() -> bool {
    unsafe {
        let d = OpenInputDesktop(0, 0, DESKTOP_READOBJECTS);
        if d.is_null() {
            return true;
        }
        let mut name = [0u16; 32];
        let mut needed = 0;
        let ok = GetUserObjectInformationW(d as HANDLE, UOI_NAME, name.as_mut_ptr() as *mut _, 64, &mut needed) != 0;
        CloseDesktop(d);
        if !ok {
            return false;
        }
        let len = name.iter().position(|&c| c == 0).unwrap_or(name.len());
        !String::from_utf16_lossy(&name[..len]).eq_ignore_ascii_case("Default")
    }
}

pub fn on_battery() -> bool {
    unsafe {
        let mut ps: SYSTEM_POWER_STATUS = std::mem::zeroed();
        GetSystemPowerStatus(&mut ps) != 0 && ps.ACLineStatus == 0
    }
}

/// EcoQoS: lets Windows run us on efficiency cores at low clocks.
pub fn set_eco_qos(on: bool) {
    unsafe {
        let s = PROCESS_POWER_THROTTLING_STATE {
            Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
            ControlMask: PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
            StateMask: if on { PROCESS_POWER_THROTTLING_EXECUTION_SPEED } else { 0 },
        };
        SetProcessInformation(
            GetCurrentProcess(),
            ProcessPowerThrottling,
            &s as *const _ as *const _,
            std::mem::size_of::<PROCESS_POWER_THROTTLING_STATE>() as u32,
        );
    }
}

// ---------------------------------------------------------------- autostart + settings

const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const RUN_VALUE: &str = "Parapet";

pub fn autostart_enabled() -> bool {
    let (key, value) = (wide(RUN_KEY), wide(RUN_VALUE));
    unsafe {
        RegGetValueW(HKEY_CURRENT_USER, key.as_ptr(), value.as_ptr(), RRF_RT_REG_SZ, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut())
            == ERROR_SUCCESS
    }
}

pub fn set_autostart(on: bool) {
    let (key_name, value) = (wide(RUN_KEY), wide(RUN_VALUE));
    unsafe {
        let mut key: HKEY = std::ptr::null_mut();
        if RegOpenKeyExW(HKEY_CURRENT_USER, key_name.as_ptr(), 0, KEY_SET_VALUE, &mut key) != ERROR_SUCCESS {
            return;
        }
        if on {
            let exe = std::env::current_exe().unwrap_or_default();
            let cmd = wide(&format!("\"{}\"", exe.display()));
            RegSetValueExW(key, value.as_ptr(), 0, REG_SZ, cmd.as_ptr() as *const u8, (cmd.len() * 2) as u32);
        } else {
            RegDeleteValueW(key, value.as_ptr());
        }
        RegCloseKey(key);
    }
}

pub fn open_in_editor(path: &std::path::Path) {
    let (verb, app, args) = (wide("open"), wide("notepad.exe"), wide(&format!("\"{}\"", path.display())));
    unsafe {
        ShellExecuteW(std::ptr::null_mut(), verb.as_ptr(), app.as_ptr(), args.as_ptr(), std::ptr::null(), SW_SHOWNORMAL);
    }
}

// ---------------------------------------------------------------- overlay windows

fn hwnd_of(window: &winit::window::Window) -> Option<HWND> {
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    match window.window_handle().ok()?.as_raw() {
        RawWindowHandle::Win32(h) => Some(h.hwnd.get() as HWND),
        _ => None,
    }
}

/// Click-through, never activated, not in the taskbar or Alt+Tab, above other windows.
pub fn prepare_overlay(window: &winit::window::Window) {
    let Some(h) = hwnd_of(window) else { return };
    unsafe {
        let ex = GetWindowLongW(h, GWL_EXSTYLE) as u32;
        SetWindowLongW(
            h,
            GWL_EXSTYLE,
            (ex | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE) as i32,
        );
        SetLayeredWindowAttributes(h, 0, 255, LWA_ALPHA);
    }
    raise_overlay(window);
}

/// Other topmost windows may have been raised above us.
pub fn raise_overlay(window: &winit::window::Window) {
    let Some(h) = hwnd_of(window) else { return };
    unsafe {
        SetWindowPos(h, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
    }
}

/// Windows sizes windows in physical pixels, which is what the overlay works in.
pub const UNITS_ARE_PIXELS: bool = true;
