//! The frame loop runs on its own thread, so it keeps going while the main thread sits in a tray menu or
//! any other modal loop. It owns the simulation, the window tracker and the GPU surfaces.

use crate::draw::{build_frame, Geo};
use crate::gpu::{FrameError, Gpu, TargetDesc};
use crate::platform::{self, Rect};
use crate::settings::Settings;
use crate::sim::{CursorWind, Sim};
use crate::tracker::Tracker;
use crate::weather::{compute_mix, WeatherState};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

// render scale per quality level; ultra also draws soft leaf shadows (twice the leaf sprites)
pub const QUALITY_SCALES: [f32; 4] = [0.5, 0.75, 1.0, 1.0];

#[derive(Clone, Debug, Default)]
pub struct Layout {
    pub origin: (f32, f32), // top-left of the virtual screen in global units
    pub size: (i32, i32),
    pub monitors: Vec<Rect>, // global units
    pub monitors_local: Vec<Rect>,
    pub work_local: Vec<Rect>,
    pub pixel_scale: f32, // primary monitor height / 1080, clamped to 1..3
}

pub enum Command {
    Targets(Vec<TargetDesc>, Layout),
    Resize(usize, (u32, u32)),
    Settings(Settings),
    Weather(WeatherState),
    Spawn(i32),
    Quit,
}

pub enum Feedback {
    /// Old surfaces, to be dropped on the main thread next to their windows.
    Retire(Vec<wgpu::Surface<'static>>),
    /// The surfaces were lost (GPU reset): please make new windows and surfaces.
    NeedTargets,
}

struct Loop {
    gpu: Gpu,
    sim: Sim,
    tracker: Tracker,
    geo: Geo,
    layout: Layout,
    settings: Settings,
    weather: WeatherState,
    feedback: Box<dyn Fn(Feedback) + Send>,
    start: Instant,
    last_frame: f64,
    last_track: f64,
    last_housekeeping: f64,
    cleared: bool,
    have_cursor: bool,
    cursor: (f32, f32),
    cursor_v: (f32, f32),
    on_battery: bool,
    eco_qos: bool,
    locked: bool,
    #[cfg(feature = "debug-overlay")]
    debug: Debug,
}

#[cfg(feature = "debug-overlay")]
#[derive(Default)]
struct Debug {
    summoned: bool,
    frames: u32,
    render_ms: f64,
    log: Option<std::fs::File>,
}

pub fn run(rx: Receiver<Command>, instance: wgpu::Instance, settings: Settings, feedback: impl Fn(Feedback) + Send + 'static) {
    let mut l = Loop {
        gpu: Gpu::new(instance),
        sim: Sim::default(),
        tracker: Tracker::default(),
        geo: Geo::new(),
        layout: Layout::default(),
        settings,
        weather: WeatherState::default(),
        feedback: Box::new(feedback),
        start: Instant::now(),
        last_frame: 0.0,
        last_track: -1.0,
        last_housekeeping: -10.0,
        cleared: true,
        have_cursor: false,
        cursor: (0.0, 0.0),
        cursor_v: (0.0, 0.0),
        on_battery: false,
        eco_qos: false,
        locked: false,
        #[cfg(feature = "debug-overlay")]
        debug: Debug::default(),
    };
    let mut next = Instant::now();
    loop {
        let wait = next.saturating_duration_since(Instant::now());
        if wait > Duration::from_millis(200) {
            // paused or idle: sleep until a command arrives or it is time to look again
            match rx.recv_timeout(wait) {
                Ok(cmd) => {
                    if !l.handle(cmd) {
                        return;
                    }
                    continue;
                }
                Err(RecvTimeoutError::Disconnected) => return,
                Err(RecvTimeoutError::Timeout) => {}
            }
        } else if !wait.is_zero() {
            // between frames: thread::sleep uses a high-resolution timer, channel timeouts round up to the
            // system tick (15.6 ms on Windows), which would make 30 fps come out at 21
            std::thread::sleep(wait);
        }
        loop {
            match rx.try_recv() {
                Ok(cmd) => {
                    if !l.handle(cmd) {
                        return;
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => return,
            }
        }
        let frame_start = Instant::now();
        let delay = l.frame();
        next = frame_start + Duration::from_secs_f64(delay);
    }
}

impl Loop {
    fn now(&self) -> f64 {
        self.start.elapsed().as_secs_f64()
    }

    fn handle(&mut self, cmd: Command) -> bool {
        match cmd {
            Command::Targets(targets, layout) => {
                let scale = QUALITY_SCALES[self.settings.quality.clamp(0, 3) as usize];
                match self.gpu.set_targets(targets, scale) {
                    Ok(old) => (self.feedback)(Feedback::Retire(old)),
                    Err(e) => eprintln!("parapet: {e}"),
                }
                if layout.size != self.layout.size || self.sim.ground_y.is_empty() {
                    self.sim.reset(layout.size.0, layout.size.1, layout.pixel_scale);
                }
                self.sim.set_ground(&layout.work_local, &layout.monitors_local);
                self.layout = layout;
                self.have_cursor = false;
                self.cleared = true;
            }
            Command::Resize(i, size) => self.gpu.resize(i, size),
            Command::Settings(s) => {
                self.gpu.set_quality(QUALITY_SCALES[s.quality.clamp(0, 3) as usize]);
                self.settings = s;
                self.last_track = -1.0;
                self.last_housekeeping = -10.0;
            }
            Command::Weather(w) => self.weather = w,
            Command::Spawn(kind) => {
                let tr = &self.tracker;
                self.sim.spawn_critter(kind, true, Some(tr));
            }
            Command::Quit => return false,
        }
        true
    }

    fn fps(&self) -> f64 {
        let mut fps = self.settings.fps;
        if self.on_battery && self.settings.battery_saver {
            fps = fps.min(20);
        }
        fps.max(1) as f64
    }

    fn housekeeping(&mut self) {
        self.on_battery = platform::on_battery();
        let eco = self.on_battery && self.settings.battery_saver;
        if eco != self.eco_qos {
            platform::set_eco_qos(eco);
            self.eco_qos = eco;
        }
        self.locked = platform::session_locked();
        #[cfg(feature = "debug-overlay")]
        self.debug_log();
    }

    #[cfg(feature = "debug-overlay")]
    fn debug_log(&mut self) {
        use std::io::Write;
        if self.debug.log.is_none() {
            let path = crate::settings::path().with_extension("ini.log");
            self.debug.log = std::fs::OpenOptions::new().create(true).append(true).open(path).ok();
        }
        let line = format!(
            "gfx={} locked={} off={} fsfg={} wins={} parts={} critters={} screen={:?} frames={} render={:.2}ms | {}\n",
            self.gpu.ready(),
            self.locked,
            platform::display_off(),
            self.tracker.fullscreen_foreground,
            self.tracker.all.len(),
            self.sim.parts.len(),
            self.sim.critters.len(),
            self.layout.size,
            self.debug.frames,
            if self.debug.frames > 0 { self.debug.render_ms / self.debug.frames as f64 } else { 0.0 },
            self.weather.describe(),
        );
        self.debug.frames = 0;
        self.debug.render_ms = 0.0;
        if let Some(f) = self.debug.log.as_mut() {
            let _ = f.write_all(line.as_bytes());
        }
    }

    /// The pointer is sampled once a frame; its movement since the last frame is a gust of wind.
    fn sample_cursor(&mut self, dt: f32) -> CursorWind {
        let mut c = CursorWind::default();
        let Some((gx, gy)) = platform::cursor_pos() else {
            self.have_cursor = false;
            return c;
        };
        let (x, y) = (gx - self.layout.origin.0, gy - self.layout.origin.1);
        if self.have_cursor && dt > 0.0 {
            c.x0 = self.cursor.0;
            c.y0 = self.cursor.1;
            c.x1 = x;
            c.y1 = y;
            let (dx, dy) = (x - c.x0, y - c.y0);
            // ignore jumps (pointer warps, monitor switches) that are not real motion
            if dx.abs() + dy.abs() < 600.0 * self.sim.s {
                // the pointer is sampled once a frame, so its speed is a bit ragged: smooth it a little
                self.cursor_v.0 += (dx / dt - self.cursor_v.0) * 0.5;
                self.cursor_v.1 += (dy / dt - self.cursor_v.1) * 0.5;
                c.vx = self.cursor_v.0;
                c.vy = self.cursor_v.1;
                c.valid = true;
            }
        }
        self.cursor = (x, y);
        self.have_cursor = true;
        c
    }

    fn clear_once(&mut self) {
        self.have_cursor = false;
        if !self.cleared && self.gpu.ready() {
            if let Err(FrameError::Lost) = self.gpu.render(&[]) {
                (self.feedback)(Feedback::NeedTargets);
            }
        }
        self.cleared = true;
    }

    /// Runs one frame; returns how long to wait before the next one, in seconds.
    fn frame(&mut self) -> f64 {
        let now = self.now();
        let dt = (now - self.last_frame).min(0.05) as f32;
        self.last_frame = now;

        if now - self.last_housekeeping > 2.0 {
            self.last_housekeeping = now;
            self.housekeeping();
        }

        if !self.gpu.ready() || self.locked || platform::display_off() || (self.settings.mode == "Off" && self.sim.idle()) {
            self.clear_once();
            return 1.0;
        }

        // windows are re-listed when something changed, every frame while one is dragged, and now and then
        // as a safety net
        if platform::take_track_dirty() || platform::moving() || now - self.last_track > platform::TRACK_SAFETY_INTERVAL {
            let raw = platform::enum_windows();
            self.tracker.update(&raw, self.layout.origin, self.settings.ignore_maximized, &self.layout.monitors);
            self.last_track = now;
        } else {
            self.tracker.still();
        }

        if self.settings.pause_on_fullscreen && self.tracker.fullscreen_foreground {
            self.clear_once();
            return 0.5;
        }

        self.sim.rare_events = self.settings.rare_events;
        #[cfg(feature = "debug-overlay")]
        if !self.debug.summoned && now > 3.0 {
            self.debug.summoned = true;
            for kind in 0..crate::sim::CRITTER_KINDS as i32 {
                self.sim.spawn_critter(kind, true, Some(&self.tracker));
            }
        }
        let pointer = self.sample_cursor(dt);
        let mix = compute_mix(&self.settings, &self.weather);
        self.sim.step(dt, &self.tracker, mix, pointer);
        if self.sim.idle() {
            self.clear_once();
            return 0.5;
        }

        #[cfg(feature = "debug-overlay")]
        let t0 = Instant::now();
        self.geo.px = 1.0 / self.gpu.pixels_per_unit();
        build_frame(&mut self.geo, &self.sim, &self.tracker, self.settings.quality >= 3);
        if let Err(FrameError::Lost) = self.gpu.render(&self.geo.verts) {
            (self.feedback)(Feedback::NeedTargets);
        }
        #[cfg(feature = "debug-overlay")]
        {
            self.debug.frames += 1;
            self.debug.render_ms += t0.elapsed().as_secs_f64() * 1000.0;
        }
        self.cleared = false;
        1.0 / self.fps()
    }
}
