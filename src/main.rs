//! Parapet: a cozy, transparent, click-through overlay for the whole desktop with autumn leaves, rain, snow
//! and storms that react to your windows, your mouse pointer and the real weather outside.
//!
//! The main thread runs the event loop: one overlay window per monitor, the tray menu, settings.ini and the
//! weather. The frames are made on a render thread (render_thread.rs).

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod atlas;
mod critters;
mod draw;
mod gpu;
mod leaves;
mod platform;
mod render_thread;
mod settings;
mod sim;
mod tracker;
mod weather;

use platform::{MonitorInfo, Rect};
use render_thread::{Command, Feedback, Layout};
use settings::Settings;
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use tray_icon::{TrayIcon, TrayIconBuilder};
use weather::{WeatherService, WeatherState};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalPosition, LogicalSize, PhysicalPosition, PhysicalSize};
use winit::event::{StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::window::{Window, WindowId, WindowLevel};

const APP_NAME: &str = concat!("Parapet ", env!("CARGO_PKG_VERSION"));

const MODES: [(&str, &str); 6] = [
    ("Auto", "Automatic (follows the weather)"),
    ("Leaves", "Autumn leaves"),
    ("Rain", "Rain"),
    ("Snow", "Snow"),
    ("Storm", "Autumn storm"),
    ("Off", "Off"),
];
const INTENSITIES: [(f32, &str); 4] = [(0.5, "Low"), (1.0, "Medium"), (1.75, "High"), (3.0, "Extreme")];
const FPS_CHOICES: [i32; 3] = [20, 30, 60];
const QUALITY_LABELS: [&str; 4] = [
    "Quality: low (½ resolution, lightest)",
    "Quality: medium (¾ resolution)",
    "Quality: high (full resolution)",
    "Quality: ultra (full + leaf shadows)",
];
const CRITTER_LABELS: [&str; sim::CRITTER_KINDS] = [
    "Hedgehog (autumn, gathers leaves)",
    "Caterpillar (eats fallen leaves)",
    "Arctic fox (leaps out of snowdrifts)",
    "Snowdrops (when it gets warmer)",
    "Snail (after rain)",
    "Frog (hops between puddles)",
    "Robin (perches on windows)",
    "Squirrel (with an acorn)",
    "Mushrooms (sprout from the mud)",
];

enum UserEvent {
    Menu(MenuEvent),
    Weather,
    Feedback(Feedback),
}

struct TrayMenu {
    weather: MenuItem,
    modes: Vec<CheckMenuItem>,
    intensities: Vec<CheckMenuItem>,
    fps: Vec<CheckMenuItem>,
    quality: Vec<CheckMenuItem>,
    battery: CheckMenuItem,
    pause_fullscreen: CheckMenuItem,
    rare: CheckMenuItem,
    ignore_max: CheckMenuItem,
    autostart: CheckMenuItem,
}

struct Overlay {
    window: Arc<Window>,
    size: PhysicalSize<u32>,
}

struct App {
    proxy: EventLoopProxy<UserEvent>,
    instance: wgpu::Instance,
    settings: Settings,
    ini_time: Option<SystemTime>,
    weather: WeatherService,
    weather_now: WeatherState,
    tx: Sender<Command>,
    overlays: Vec<Overlay>,
    retiring: Vec<Arc<Window>>,
    monitors: Vec<MonitorInfo>,
    tray: Option<TrayIcon>,
    menu: Option<TrayMenu>,
    started: bool,
    next_housekeeping: Instant,
    /// Why the platform cannot show a transparent overlay right now; shown at the top of the tray menu.
    overlay_problem: Option<String>,
}

fn load_icon(png_bytes: &[u8]) -> Option<tray_icon::Icon> {
    let decoder = png::Decoder::new(std::io::Cursor::new(png_bytes));
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).ok()?;
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf[..info.buffer_size()].to_vec(),
        png::ColorType::Rgb => buf[..info.buffer_size()].chunks(3).flat_map(|c| [c[0], c[1], c[2], 255]).collect(),
        _ => return None,
    };
    tray_icon::Icon::from_rgba(rgba, info.width, info.height).ok()
}

impl App {
    fn send(&self, cmd: Command) {
        let _ = self.tx.send(cmd);
    }

    fn save_settings(&mut self) {
        self.settings.save();
        self.ini_time = settings::modified();
    }

    // ------------------------------------------------------------ overlay windows

    fn layout_for(monitors: &[MonitorInfo]) -> Layout {
        let mut v = monitors[0].rect;
        for m in monitors {
            v.l = v.l.min(m.rect.l);
            v.t = v.t.min(m.rect.t);
            v.r = v.r.max(m.rect.r);
            v.b = v.b.max(m.rect.b);
        }
        let (ox, oy) = (v.l, v.t);
        Layout {
            origin: (ox, oy),
            size: (v.width().round() as i32, v.height().round() as i32),
            monitors: monitors.iter().map(|m| m.rect).collect(),
            monitors_local: monitors.iter().map(|m| m.rect.offset(-ox, -oy)).collect(),
            work_local: monitors.iter().map(|m| m.work.offset(-ox, -oy)).collect(),
            pixel_scale: (monitors[0].rect.height() / 1080.0).clamp(1.0, 3.0),
        }
    }

    fn current_monitors(event_loop: &ActiveEventLoop) -> Vec<MonitorInfo> {
        let mut mons = platform::monitors();
        if mons.is_empty() {
            // no native monitor list on this system: winit's (no work areas)
            let primary = event_loop.primary_monitor();
            for m in event_loop.available_monitors() {
                let (p, s) = (m.position(), m.size());
                let r = Rect { l: p.x as f32, t: p.y as f32, r: (p.x + s.width as i32) as f32, b: (p.y + s.height as i32) as f32 };
                let info = MonitorInfo { rect: r, work: r };
                if Some(&m) == primary.as_ref() {
                    mons.insert(0, info);
                } else {
                    mons.push(info);
                }
            }
        }
        mons
    }

    fn build_overlays(&mut self, event_loop: &ActiveEventLoop) {
        let monitors = Self::current_monitors(event_loop);
        if monitors.is_empty() {
            return;
        }
        let problem = platform::overlay_supported().err();
        if problem != self.overlay_problem {
            if let Some(why) = &problem {
                eprintln!("parapet: no overlay: {why}");
            }
            self.overlay_problem = problem;
            self.update_weather_text();
        }
        if self.overlay_problem.is_some() {
            return;
        }
        let layout = Self::layout_for(&monitors);
        let old: Vec<_> = self.overlays.drain(..).map(|o| o.window).collect();
        self.retiring.extend(old);
        let mut targets = Vec::new();
        for m in &monitors {
            let r = m.rect;
            let mut attrs = Window::default_attributes()
                .with_title("Parapet")
                .with_decorations(false)
                .with_transparent(true)
                .with_resizable(false)
                .with_active(false)
                .with_window_level(WindowLevel::AlwaysOnTop);
            attrs = if platform::UNITS_ARE_PIXELS {
                attrs
                    .with_position(PhysicalPosition::new(r.l as i32, r.t as i32))
                    .with_inner_size(PhysicalSize::new(r.width() as u32, r.height() as u32))
            } else {
                attrs
                    .with_position(LogicalPosition::new(r.l as f64, r.t as f64))
                    .with_inner_size(LogicalSize::new(r.width() as f64, r.height() as f64))
            };
            #[cfg(windows)]
            {
                use winit::platform::windows::WindowAttributesExtWindows;
                attrs = attrs.with_skip_taskbar(true).with_no_redirection_bitmap(true);
            }
            #[cfg(target_os = "macos")]
            {
                use winit::platform::macos::WindowAttributesExtMacOS;
                attrs = attrs.with_has_shadow(false);
            }
            #[cfg(target_os = "linux")]
            {
                // unmanaged by the window manager: no frame, no taskbar entry, never focused, on every workspace
                use winit::platform::x11::WindowAttributesExtX11;
                attrs = attrs.with_override_redirect(true);
            }
            let window = match event_loop.create_window(attrs) {
                Ok(w) => Arc::new(w),
                Err(e) => {
                    eprintln!("parapet: cannot create overlay window: {e}");
                    continue;
                }
            };
            let _ = window.set_cursor_hittest(false);
            platform::prepare_overlay(&window);
            let size = window.inner_size();
            let surface = match self.instance.create_surface(window.clone()) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("parapet: cannot create a GPU surface: {e}");
                    continue;
                }
            };
            targets.push(gpu::TargetDesc {
                surface,
                rect: r.offset(-layout.origin.0, -layout.origin.1),
                size: (size.width, size.height),
            });
            self.overlays.push(Overlay { window, size });
        }
        self.monitors = monitors;
        self.send(Command::Targets(targets, layout));
    }

    // ------------------------------------------------------------ tray

    fn build_tray(&mut self) {
        let s = &self.settings;
        let check = |id: String, label: &str, on: bool| CheckMenuItem::with_id(id, label, true, on, None);
        let weather = MenuItem::with_id("weather", self.status_text(), false, None);
        let modes: Vec<_> = MODES.iter().map(|(k, l)| check(format!("mode:{k}"), l, s.mode == *k)).collect();
        let intensities: Vec<_> = INTENSITIES
            .iter()
            .enumerate()
            .map(|(i, (v, l))| check(format!("intensity:{i}"), l, (s.intensity - v).abs() < 0.01))
            .collect();
        let fps: Vec<_> = FPS_CHOICES.iter().map(|f| check(format!("fps:{f}"), &format!("{f} fps"), s.fps == *f)).collect();
        let quality: Vec<_> = QUALITY_LABELS
            .iter()
            .enumerate()
            .map(|(i, l)| check(format!("quality:{i}"), l, s.quality == i as i32))
            .collect();
        let battery = check("battery".into(), "Save battery (20 fps, EcoQoS)", s.battery_saver);
        let pause_fullscreen = check("pausefs".into(), "Pause for full-screen apps", s.pause_on_fullscreen);
        let rare = check("rare".into(), "Appear on their own (rarely, depending on the weather)", s.rare_events);
        let ignore_max = check("ignoremax".into(), "Fall through maximized windows", s.ignore_maximized);
        let autostart = check("autostart".into(), platform::AUTOSTART_LABEL, platform::autostart_enabled());

        let menu = Menu::new();
        let effect = Submenu::new("Effect", true);
        for m in &modes {
            let _ = effect.append(m);
        }
        let intensity = Submenu::new("Intensity", true);
        for m in &intensities {
            let _ = intensity.append(m);
        }
        let perf = Submenu::new("Performance", true);
        for m in &fps {
            let _ = perf.append(m);
        }
        let _ = perf.append(&PredefinedMenuItem::separator());
        for m in &quality {
            let _ = perf.append(m);
        }
        let _ = perf.append(&PredefinedMenuItem::separator());
        let _ = perf.append_items(&[&battery, &pause_fullscreen]);
        let visitors = Submenu::new("Rare visitors", true);
        let _ = visitors.append(&rare);
        let _ = visitors.append(&PredefinedMenuItem::separator());
        let _ = visitors.append(&MenuItem::with_id("critter:random", "Summon a random one now", true, None));
        for (i, l) in CRITTER_LABELS.iter().enumerate() {
            let _ = visitors.append(&MenuItem::with_id(format!("critter:{i}"), l, true, None));
        }
        let _ = menu.append_items(&[
            &weather,
            &PredefinedMenuItem::separator(),
            &effect,
            &intensity,
            &perf,
            &visitors,
            &ignore_max,
            &MenuItem::with_id("refresh", "Refresh weather", true, None),
            &MenuItem::with_id("edit", "Settings (city, coordinates)…", true, None),
            &autostart,
            &PredefinedMenuItem::separator(),
            &MenuItem::with_id("exit", if cfg!(target_os = "macos") { "Quit" } else { "Exit" }, true, None),
        ]);

        #[cfg(target_os = "macos")]
        let icon = load_icon(include_bytes!("../assets/logo-64.png"));
        #[cfg(not(target_os = "macos"))]
        let icon = load_icon(include_bytes!("../assets/logo-32.png"));
        let mut builder = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_menu_on_left_click(true)
            .with_tooltip(format!("{APP_NAME}\n{}", self.status_text()));
        if let Some(icon) = icon {
            builder = builder.with_icon(icon);
        }
        match builder.build() {
            Ok(t) => self.tray = Some(t),
            Err(e) => eprintln!("parapet: no tray icon: {e}"),
        }
        self.menu = Some(TrayMenu {
            weather,
            modes,
            intensities,
            fps,
            quality,
            battery,
            pause_fullscreen,
            rare,
            ignore_max,
            autostart,
        });
    }

    /// Check marks follow the settings (menu items toggle themselves when clicked, radio groups don't exist).
    fn sync_menu(&self) {
        let Some(m) = &self.menu else { return };
        let s = &self.settings;
        for (item, (k, _)) in m.modes.iter().zip(MODES) {
            item.set_checked(s.mode == k);
        }
        for (item, (v, _)) in m.intensities.iter().zip(INTENSITIES) {
            item.set_checked((s.intensity - v).abs() < 0.01);
        }
        for (item, f) in m.fps.iter().zip(FPS_CHOICES) {
            item.set_checked(s.fps == f);
        }
        for (i, item) in m.quality.iter().enumerate() {
            item.set_checked(s.quality == i as i32);
        }
        m.battery.set_checked(s.battery_saver);
        m.pause_fullscreen.set_checked(s.pause_on_fullscreen);
        m.rare.set_checked(s.rare_events);
        m.ignore_max.set_checked(s.ignore_maximized);
        m.autostart.set_checked(platform::autostart_enabled());
    }

    /// The first, disabled line of the tray menu: the weather, or why nothing is shown.
    fn status_text(&self) -> String {
        match &self.overlay_problem {
            Some(why) => format!("Overlay off: {why}"),
            None => self.weather_now.describe(),
        }
    }

    fn update_weather_text(&self) {
        let text = self.status_text();
        if let Some(m) = &self.menu {
            m.weather.set_text(&text);
        }
        if let Some(t) = &self.tray {
            let _ = t.set_tooltip(Some(format!("{APP_NAME}\n{text}")));
        }
    }

    fn command(&mut self, event_loop: &ActiveEventLoop, id: &str) {
        let (kind, arg) = id.split_once(':').unwrap_or((id, ""));
        match kind {
            "mode" => self.settings.mode = arg.to_string(),
            "intensity" => {
                if let Some((v, _)) = arg.parse::<usize>().ok().and_then(|i| INTENSITIES.get(i)) {
                    self.settings.intensity = *v;
                }
            }
            "fps" => self.settings.fps = arg.parse().unwrap_or(30),
            "quality" => self.settings.quality = arg.parse::<i32>().unwrap_or(2).clamp(0, 3),
            "battery" => self.settings.battery_saver = !self.settings.battery_saver,
            "pausefs" => self.settings.pause_on_fullscreen = !self.settings.pause_on_fullscreen,
            "rare" => self.settings.rare_events = !self.settings.rare_events,
            "ignoremax" => self.settings.ignore_maximized = !self.settings.ignore_maximized,
            "refresh" => {
                self.weather.refresh(&self.settings, false);
                return;
            }
            "edit" => {
                self.save_settings();
                platform::open_in_editor(&settings::path());
                return;
            }
            "autostart" => {
                platform::set_autostart(!platform::autostart_enabled());
                self.sync_menu();
                return;
            }
            "critter" => {
                self.send(Command::Spawn(arg.parse::<i32>().unwrap_or(-1)));
                return;
            }
            "exit" => {
                self.send(Command::Quit);
                self.weather.stop();
                self.tray = None;
                event_loop.exit();
                return;
            }
            _ => return,
        }
        self.save_settings();
        self.sync_menu();
        self.send(Command::Settings(self.settings.clone()));
    }

    /// Housekeeping every 2 s, and more often where the tray toolkit needs pumping.
    fn next_wake(&self) -> Instant {
        match platform::UI_PUMP_INTERVAL {
            Some(s) => self.next_housekeeping.min(Instant::now() + Duration::from_secs_f64(s)),
            None => self.next_housekeeping,
        }
    }

    fn housekeeping(&mut self, event_loop: &ActiveEventLoop) {
        // settings.ini edited by hand: pick the changes up
        let t = settings::modified();
        if t != self.ini_time {
            self.ini_time = t;
            let old = std::mem::replace(&mut self.settings, Settings::load());
            let s = &self.settings;
            if s.city != old.city || s.latitude != old.latitude || s.longitude != old.longitude {
                self.weather.refresh(s, true);
            }
            self.sync_menu();
            self.send(Command::Settings(self.settings.clone()));
        }
        if Self::current_monitors(event_loop) != self.monitors || self.overlay_problem.is_some() {
            self.build_overlays(event_loop);
        }
        for o in &self.overlays {
            platform::raise_overlay(&o.window);
        }
    }
}

impl ApplicationHandler<UserEvent> for App {
    fn new_events(&mut self, event_loop: &ActiveEventLoop, _cause: StartCause) {
        if !self.started {
            return;
        }
        if Instant::now() >= self.next_housekeeping {
            self.next_housekeeping = Instant::now() + Duration::from_secs(2);
            self.housekeeping(event_loop);
        }
        platform::pump_ui();
        event_loop.set_control_flow(ControlFlow::WaitUntil(self.next_wake()));
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.started {
            return;
        }
        self.started = true;
        self.build_overlays(event_loop);
        self.build_tray();
        platform::install_hooks();
        self.next_housekeeping = Instant::now() + Duration::from_secs(2);
        event_loop.set_control_flow(ControlFlow::WaitUntil(self.next_wake()));
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::Menu(e) => self.command(event_loop, e.id().0.as_str()),
            UserEvent::Weather => {
                self.weather_now = self.weather.snapshot();
                self.update_weather_text();
                self.send(Command::Weather(self.weather_now.clone()));
            }
            UserEvent::Feedback(Feedback::Retire(surfaces)) => {
                // the render thread let go of the old surfaces: now their windows can close
                drop(surfaces);
                self.retiring.clear();
            }
            UserEvent::Feedback(Feedback::NeedTargets) => self.build_overlays(event_loop),
        }
    }

    fn window_event(&mut self, _event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let Some(index) = self.overlays.iter().position(|o| o.window.id() == id) else { return };
        match event {
            WindowEvent::ScaleFactorChanged { mut inner_size_writer, .. } => {
                // keep covering exactly the monitor, whatever its DPI
                if platform::UNITS_ARE_PIXELS {
                    let _ = inner_size_writer.request_inner_size(self.overlays[index].size);
                }
            }
            WindowEvent::Resized(size) => {
                if platform::UNITS_ARE_PIXELS && size != self.overlays[index].size {
                    let _ = self.overlays[index].window.request_inner_size(self.overlays[index].size);
                } else {
                    self.overlays[index].size = size;
                    self.send(Command::Resize(index, (size.width, size.height)));
                }
            }
            _ => {}
        }
    }
}

fn main() {
    if !platform::init() {
        return; // already running
    }

    let mut settings = Settings::load();
    if !settings::path().exists() {
        settings.save();
    }
    settings = Settings::load();

    let mut builder = EventLoop::<UserEvent>::with_user_event();
    #[cfg(target_os = "macos")]
    {
        use winit::platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS};
        // a menu bar extra: no Dock icon, no app menu
        builder.with_activation_policy(ActivationPolicy::Accessory).with_default_menu(false);
    }
    #[cfg(target_os = "linux")]
    {
        // X11 (XWayland under Wayland): Wayland lets no app place itself above the others
        use winit::platform::x11::EventLoopBuilderExtX11;
        builder.with_x11();
    }
    let event_loop = match builder.build() {
        Ok(el) => el,
        Err(e) => {
            eprintln!("parapet: cannot start: {e}");
            return;
        }
    };
    let proxy = event_loop.create_proxy();

    let menu_proxy = proxy.clone();
    MenuEvent::set_event_handler(Some(move |e| {
        let _ = menu_proxy.send_event(UserEvent::Menu(e));
    }));

    let weather_proxy = proxy.clone();
    let weather = WeatherService::start(settings.clone(), move || {
        let _ = weather_proxy.send_event(UserEvent::Weather);
    });

    platform::init_ui();
    let instance = gpu::new_instance();
    let (tx, rx) = std::sync::mpsc::channel();
    let render_instance = instance.clone();
    let render_settings = settings.clone();
    let feedback_proxy = proxy.clone();
    let _ = std::thread::Builder::new().name("render".into()).spawn(move || {
        render_thread::run(rx, render_instance, render_settings, move |f| {
            let _ = feedback_proxy.send_event(UserEvent::Feedback(f));
        })
    });

    let mut app = App {
        proxy,
        instance,
        ini_time: settings::modified(),
        settings,
        weather,
        weather_now: WeatherState::default(),
        tx,
        overlays: Vec::new(),
        retiring: Vec::new(),
        monitors: Vec::new(),
        tray: None,
        menu: None,
        started: false,
        next_housekeeping: Instant::now(),
        overlay_problem: None,
    };
    let _ = event_loop.run_app(&mut app);
    drop(app.proxy);
}
