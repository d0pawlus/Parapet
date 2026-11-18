//! The windows on screen, front to back, in overlay coordinates (desktop units relative to the top-left of
//! the virtual screen). The platform layer lists them; this turns that list into surfaces things land on.

use crate::platform::{RawWindow, Rect};

/// Platform window identifier (HWND on Windows, CGWindowID on macOS); 0 = none.
pub type WinId = u64;

#[derive(Clone, Copy, Debug)]
pub struct Win {
    pub id: WinId,
    pub l: f32,
    pub t: f32,
    pub r: f32,
    pub b: f32,
    pub dx: f32, // movement since the previous update
    pub dy: f32,
    pub z: i32,        // index in Tracker::all, 0 = topmost
    pub surface: bool, // false for ignored maximized/fullscreen windows: they hide what is below, nothing lands on them
}

#[derive(Default)]
pub struct Tracker {
    pub all: Vec<Win>,
    pub fullscreen_foreground: bool,
    prev: Vec<Win>,
}

impl Tracker {
    /// `origin` is the top-left of the virtual screen; `monitors` are in the same global units as the windows.
    pub fn update(&mut self, raw: &[RawWindow], origin: (f32, f32), ignore_maximized: bool, monitors: &[Rect]) {
        std::mem::swap(&mut self.prev, &mut self.all);
        self.all.clear();
        self.fullscreen_foreground = false;
        for w in raw {
            let r = w.rect;
            if r.r - r.l < 60.0 || r.b - r.t < 40.0 {
                continue;
            }
            let fullscreen = monitors.iter().any(|m| r.l <= m.l && r.t <= m.t && r.r >= m.r && r.b >= m.b);
            if fullscreen && w.foreground {
                self.fullscreen_foreground = true;
            }
            let mut win = Win {
                id: w.id,
                l: r.l - origin.0,
                t: r.t - origin.1,
                r: r.r - origin.0,
                b: r.b - origin.1,
                dx: 0.0,
                dy: 0.0,
                z: self.all.len() as i32,
                surface: !(ignore_maximized && (fullscreen || w.maximized)),
            };
            if let Some(o) = self.prev.iter().find(|o| o.id == w.id) {
                win.dx = win.l - o.l;
                win.dy = win.t - o.t;
            }
            self.all.push(win);
        }
    }

    pub fn still(&mut self) {
        for w in &mut self.all {
            w.dx = 0.0;
            w.dy = 0.0;
        }
    }

    pub fn get(&self, id: WinId) -> Option<&Win> {
        if id == 0 {
            return None;
        }
        self.all.iter().find(|w| w.id == id)
    }

    /// Is (x, y) hidden by a window in front of z? `surfaces_only` ignores maximized/fullscreen windows that
    /// particles fly over (used for the taskbar ground).
    pub fn covered(&self, x: f32, y: f32, z: i32, surfaces_only: bool) -> bool {
        let n = (z.max(0) as usize).min(self.all.len());
        self.all[..n]
            .iter()
            .any(|w| !(surfaces_only && !w.surface) && x >= w.l && x <= w.r && y >= w.t && y <= w.b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(id: u64, l: f32, t: f32, r: f32, b: f32) -> RawWindow {
        RawWindow { id, rect: Rect { l, t, r, b }, maximized: false, foreground: false }
    }

    #[test]
    fn tracks_movement_and_cover() {
        let mut tr = Tracker::default();
        let mons = [Rect { l: 0.0, t: 0.0, r: 1920.0, b: 1080.0 }];
        tr.update(&[raw(1, 100.0, 100.0, 500.0, 400.0), raw(2, 300.0, 50.0, 900.0, 600.0)], (0.0, 0.0), true, &mons);
        tr.update(&[raw(1, 110.0, 100.0, 510.0, 400.0), raw(2, 300.0, 50.0, 900.0, 600.0)], (0.0, 0.0), true, &mons);
        assert_eq!(tr.get(1).unwrap().dx, 10.0);
        // window 2 is behind window 1 (z = 1): a point inside 1 is covered for 2, not for 1
        assert!(tr.covered(400.0, 200.0, 1, false));
        assert!(!tr.covered(400.0, 200.0, 0, false));
    }

    #[test]
    fn fullscreen_window_is_not_a_surface() {
        let mut tr = Tracker::default();
        let mons = [Rect { l: 0.0, t: 0.0, r: 1920.0, b: 1080.0 }];
        let mut w = raw(1, 0.0, 0.0, 1920.0, 1080.0);
        w.foreground = true;
        tr.update(&[w], (0.0, 0.0), true, &mons);
        assert!(tr.fullscreen_foreground);
        assert!(!tr.all[0].surface);
    }
}
