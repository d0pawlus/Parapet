//! The particle world: leaves, rain, snow, splashes and ripples, the layers that pile up on window tops and on
//! the taskbar, leaf heaps swept by the pointer, and the gusts the pointer sheds.

use crate::platform::Rect;
use crate::tracker::{Tracker, Win, WinId};
use crate::weather::EffectMix;
use std::collections::VecDeque;
use std::f32::consts::{PI, TAU};

pub const MAX_PARTICLES: usize = 7000;
const MAX_RESTING: i32 = 400;
pub const LEAF_SHAPES: usize = 28;
pub const LEAF_COLORS: usize = 8;

// spawn rates per second for a 1920 px wide screen at intensity 1
const LEAF_RATE: f32 = 4.0;
const RAIN_RATE: f32 = 380.0;
const SNOW_RATE: f32 = 45.0;

// relative size of each leaf species (plane and magnolia are big, hawthorn and rowan small)
const SPECIES_SIZE: [f32; LEAF_SHAPES] = [
    1.0, 1.15, 0.9, 1.0, 1.15, 0.8, 1.0, 0.9, 0.85, 1.0, 1.15, 0.9, 1.25, 0.85, 0.85, 1.0, 1.0, 0.85, 0.85, 0.95, 0.7,
    1.1, 1.0, 1.15, 0.8, 1.25, 0.7, 1.05,
];
const GINKGO: u8 = 13;

pub const K_LEAF: u8 = 0;
pub const K_RAIN: u8 = 1;
pub const K_SNOW: u8 = 2;
pub const K_SPLASH: u8 = 3;
pub const K_RIPPLE: u8 = 4;

pub const FALLING: u8 = 0;
pub const RESTING: u8 = 1;
pub const FADING: u8 = 2;

const PENDULUM: u8 = 0;
const TUMBLE: u8 = 1;
const SPIRAL: u8 = 2;

pub const BUCKET: i32 = 4;
const B: f32 = BUCKET as f32;

fn resample(v: &mut Vec<f32>, n: usize) {
    if v.len() == n {
        return;
    }
    let old = std::mem::take(v);
    *v = (0..n).map(|i| if old.is_empty() { 0.0 } else { old[i * old.len() / n] }).collect();
}

// ---------------------------------------------------------------- surface layers

/// Everything that piles up on a surface (a window's top edge or the taskbar), per 4 px bucket:
/// snow, mud, standing water and a small 1D wave simulation on the water.
#[derive(Clone)]
pub struct SnowCap {
    pub h: WinId,
    pub height: Vec<f32>,
    pub water: Vec<f32>,
    pub mud: Vec<f32>,
    pub wave: Vec<f32>,
    pub wave_v: Vec<f32>,
    pub terrain: Vec<f32>,
    pub pile: Vec<f32>, // height of the leaf heap
    pub max: f32,
    pub water_max: f32,
    pub mud_max: f32,
    pub last_seen: f32,
    pub drains: bool, // windows spill water over their ends; the taskbar keeps it
    pub bead_l: f32,  // drops gathering at a window's top corners before they fall
    pub bead_r: f32,
    pub cornice_l: f32, // snow creeping over a window's corners, hanging there until it drops
    pub cornice_r: f32,
    pub stirred: f32, // the pointer swept this surface's leaf heap recently (seconds left, counts down)
    pub stir_l: f32,  // ...and where: only leaves in that stretch follow the moving heap
    pub stir_r: f32,
}

impl Default for SnowCap {
    fn default() -> Self {
        Self {
            h: 0,
            height: Vec::new(),
            water: Vec::new(),
            mud: Vec::new(),
            wave: Vec::new(),
            wave_v: Vec::new(),
            terrain: Vec::new(),
            pile: Vec::new(),
            max: 14.0,
            water_max: 6.0,
            mud_max: 0.0,
            last_seen: 0.0,
            drains: true,
            bead_l: 0.0,
            bead_r: 0.0,
            cornice_l: 0.0,
            cornice_r: 0.0,
            stirred: 0.0,
            stir_l: 0.0,
            stir_r: 0.0,
        }
    }
}

impl SnowCap {
    fn layers(&mut self) -> [&mut Vec<f32>; 7] {
        [&mut self.height, &mut self.water, &mut self.mud, &mut self.wave, &mut self.wave_v, &mut self.terrain, &mut self.pile]
    }

    pub fn init(&mut self, width: i32, max_height: f32, max_water: f32, max_mud: f32) {
        let n = (width / BUCKET).max(1) as usize;
        for v in self.layers() {
            *v = vec![0.0; n];
        }
        self.max = max_height;
        self.water_max = max_water;
        self.mud_max = max_mud;
    }

    pub fn resize(&mut self, width: i32) {
        let n = (width / BUCKET).max(1) as usize;
        for v in self.layers() {
            resample(v, n);
        }
    }

    pub fn any_snow(&self) -> bool {
        self.height.iter().any(|&h| h > 0.3)
    }
    pub fn any_water(&self) -> bool {
        self.water.iter().any(|&h| h > 0.2)
    }
    pub fn any_mud(&self) -> bool {
        self.mud.iter().any(|&h| h > 0.2)
    }
    pub fn any(&self) -> bool {
        self.any_snow() || self.any_water() || self.any_mud()
    }

    pub fn at(&self, rel: f32) -> f32 {
        let i = (rel / B) as i32;
        if i >= 0 && (i as usize) < self.height.len() {
            let i = i as usize;
            self.height[i] + self.water[i] + self.mud[i]
        } else {
            0.0
        }
    }

    pub fn add(&mut self, rel: f32, amount: f32) {
        let n = self.height.len() as i32;
        let c = (rel / B) as i32;
        for d in -1..=1 {
            let i = c + d;
            if i < 0 || i >= n {
                continue;
            }
            // snow slides off near the edges, so caps taper there
            let edge = i.min(n - 1 - i);
            let limit = self.max * ((edge + 1) as f32 / 14.0).min(1.0);
            let h = &mut self.height[i as usize];
            *h = limit.min(*h + if d == 0 { amount } else { amount * 0.5 });
        }
    }

    pub fn add_water(&mut self, rel: f32, amount: f32) {
        const SPREAD: [f32; 5] = [0.3, 0.6, 1.0, 0.6, 0.3];
        let n = self.water.len() as i32;
        let c = (rel / B) as i32;
        for d in -2..=2 {
            let i = c + d;
            if i >= 0 && i < n {
                let w = &mut self.water[i as usize];
                *w = self.water_max.min(*w + amount * SPREAD[(d + 2) as usize]);
            }
        }
    }

    pub fn add_mud(&mut self, rel: f32, amount: f32) {
        if self.mud_max <= 0.0 {
            return;
        }
        let n = self.mud.len() as i32;
        let c = (rel / B) as i32;
        for d in -1..=1 {
            let i = c + d;
            if i >= 0 && i < n {
                let m = &mut self.mud[i as usize];
                *m = self.mud_max.min(*m + if d == 0 { amount } else { amount * 0.5 });
            }
        }
    }

    pub fn relax(&mut self, dt: f32, melt: f32) {
        for i in 0..self.height.len().saturating_sub(1) {
            let d = self.height[i] - self.height[i + 1];
            if d > 2.0 {
                let t = (d - 2.0) * 0.25;
                self.height[i] -= t;
                self.height[i + 1] += t;
            } else if d < -2.0 {
                let t = (-d - 2.0) * 0.25;
                self.height[i] += t;
                self.height[i + 1] -= t;
            }
        }
        let m = melt * dt;
        for h in &mut self.height {
            *h = (*h - m).max(0.0);
        }
    }

    /// Returns water spilled over the left/right ends (windows drip, the taskbar does not).
    pub fn flow(&mut self, dt: f32, evaporate: f32, dry: f32) -> (f32, f32) {
        let (mut spill_l, mut spill_r) = (0.0, 0.0);
        let n = self.water.len();
        // water runs towards lower ground; sweep both ways so neither direction is favoured
        for pass in 0..2 {
            for k in 0..n.saturating_sub(1) {
                let i = if pass == 0 { k } else { n - 2 - k };
                let level = |s: &Self, i: usize| s.terrain[i] + s.mud[i] + s.height[i] + s.water[i];
                let d = (level(self, i) - level(self, i + 1)) * 0.45;
                if d > 0.0 {
                    let m = d.min(self.water[i]);
                    self.water[i] -= m;
                    self.water[i + 1] += m;
                } else {
                    let m = (-d).min(self.water[i + 1]);
                    self.water[i + 1] -= m;
                    self.water[i] += m;
                }
            }
        }
        if self.drains && n > 1 {
            let k = (dt * 3.0).min(1.0);
            spill_l = self.water[0] * k;
            spill_r = self.water[n - 1] * k;
            self.water[0] -= spill_l;
            self.water[n - 1] -= spill_r;
        }
        for i in 0..n {
            self.water[i] = (self.water[i] - evaporate * dt).max(0.0);
            self.mud[i] = (self.mud[i] - dry * dt).max(0.0);
        }

        // ripples travelling across the puddles: a 1D wave equation, sub-stepped to stay stable
        let steps = ((dt * 60.0).ceil() as i32).clamp(1, 4);
        let h = dt / steps as f32;
        for _ in 0..steps {
            for i in 0..n {
                if self.water[i] < 0.2 {
                    self.wave[i] = 0.0;
                    self.wave_v[i] = 0.0;
                    continue;
                }
                let l = self.wave[if i > 0 { i - 1 } else { i }];
                let r = self.wave[if i + 1 < n { i + 1 } else { i }];
                self.wave_v[i] += ((l + r - 2.0 * self.wave[i]) * 900.0 - self.wave_v[i] * 3.0) * h;
            }
            for i in 0..n {
                self.wave[i] += self.wave_v[i] * h;
                let lim = self.water[i] * 0.8;
                self.wave[i] = self.wave[i].clamp(-lim, lim);
            }
        }
        (spill_l, spill_r)
    }
}

// ---------------------------------------------------------------- particles

#[derive(Clone, Copy, Default, Debug)]
pub struct Particle {
    pub x: f32,
    pub y: f32,
    pub vx: f32,
    pub vy: f32,
    pub rot: f32,
    pub rotv: f32,
    pub phase: f32,
    pub freq: f32,
    pub size: f32,
    pub depth: f32,
    pub age: f32,
    pub life: f32,
    pub alpha: f32,
    pub offx: f32,
    pub rest: f32,
    // leaf flight: a second (flip) oscillator, swing amplitude, drag, tilt and the accumulated spin
    pub flip: f32,
    pub phase2: f32,
    pub freq2: f32,
    pub amp: f32,
    pub drag: f32,
    pub tilt: f32,
    pub spin: f32,
    pub mode_time: f32,
    pub noise: f32,
    pub fall: f32,
    pub landed: f32,   // when it came to rest
    pub land_seq: u32, // unique landing order, so leaves that land in the same frame stack the same way every frame
    // settling down after landing: progress 0..1 from the falling pose (start_rot, start_flip) to lying flat (rest_rot)
    pub settle: f32,
    pub start_rot: f32,
    pub rest_rot: f32,
    pub start_flip: f32,
    pub heap_vol: f32, // volume this leaf adds to a heap, and what it started with (it shrinks while fading)
    pub heap_vol0: f32,
    pub mass_frac: f32, // where in the heap's mass it sits (0 = left end, 1 = right end)
    pub host: WinId,
    pub kind: u8,
    pub state: u8,
    pub shape: u8,
    pub color: u8, // doubles as splash tint: 0 water, 1 mud, 2 leaf crumbs
    pub mode: u8,
    pub on_ground: bool,
    pub hidden: bool,
    pub heaped: bool, // part of a heap swept up by the pointer: sits on the heap's surface at depth vy (0..1)
}

/// A puff of air shed by the moving pointer: it keeps travelling ahead of the pointer, widens and dies away.
#[derive(Clone, Copy)]
struct Gust {
    x: f32,
    y: f32,
    vx: f32,
    vy: f32,
    r: f32,
    age: f32,
    life: f32,
    swirl: f32,
}

/// The mouse pointer's path during the last frame, in overlay coordinates; a moving pointer is a gust of wind.
#[derive(Clone, Copy, Default, Debug)]
pub struct CursorWind {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
    pub vx: f32,
    pub vy: f32,
    pub valid: bool,
}

/// Stretches of the taskbar drift where the snow is being blown away for a few seconds.
#[derive(Clone, Copy)]
struct SnowGust {
    x: f32,
    w: f32,
    strength: f32,
    life: f32,
    age: f32,
}

// Rare visitors and plants. Each appears only now and then, and only when the weather suits it.
pub const HEDGEHOG: u8 = 0;
pub const CATERPILLAR: u8 = 1;
pub const SNOW_FOX: u8 = 2;
pub const SNOWDROPS: u8 = 3;
pub const SNAIL: u8 = 4;
pub const FROG: u8 = 5;
pub const ROBIN: u8 = 6;
pub const SQUIRREL: u8 = 7;
pub const MUSHROOMS: u8 = 8;
pub const CRITTER_KINDS: usize = 9;

#[derive(Clone, Default)]
pub struct Critter {
    pub kind: u8,
    pub state: u8,
    pub x: f32,
    pub y: f32,
    pub vx: f32,
    pub vy: f32,
    pub dir: f32, // +1 facing right, -1 facing left
    pub t: f32,   // age
    pub timer: f32, // time left in the current state
    pub anim: f32,  // gait / flap / pulse phase
    pub grow: f32,  // plants: 0..1, others: generic progress
    pub alpha: f32,
    pub angle: f32,
    pub seed: f32,
    pub face: f32,   // drawn facing, eases towards dir so turning around is a smooth flip
    pub blink: f32,  // seconds to the next blink; negative while the eyes are shut
    pub look: f32,   // head turned towards the pointer, -1..1 in screen terms
    pub squash: f32, // landing / crouching squash, 0..1
    pub aux: f32,    // per-kind extras: tail flicks, tongue, wiggles
    pub aux2: f32,
    pub count: i32,       // leaves eaten or carried, hops made
    pub carried: [u8; 4], // leaf colours on the hedgehog's spines
    pub host: WinId,      // robin: the window it sits on
    pub offx: f32,
    pub hidden: bool,
    pub trail: VecDeque<(f32, f32)>, // snail slime
    pub trail_age: VecDeque<f32>,
}

/// Which surface a heap or layer operation applies to.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum CapRef {
    Ground,
    Cap(usize),
}

pub struct Sim {
    pub parts: Vec<Particle>,
    pub caps: Vec<SnowCap>,
    pub ground: SnowCap,
    pub ground_y: Vec<f32>,     // per SnowCap bucket
    pub ground_scale: Vec<f32>, // per bucket: pixel scale of the monitor there (1 = 1080p)
    pub rest_order: Vec<usize>, // resting leaves bottom to top, for drawing heaps in the right order
    pub critters: Vec<Critter>,
    pub rare_events: bool,
    pub flash: f32,
    pub s: f32,
    pub w: i32,
    pub h: i32,

    pub(crate) pending: Vec<Particle>,
    pub(crate) cursor: CursorWind,
    gusts: Vec<Gust>,
    gust_clock: f32,
    pub(crate) critter_clock: f32,
    step_dt: f32,
    land_counter: u32,
    snow_gusts: Vec<SnowGust>,
    pub(crate) mix: EffectMix,
    prev_mix: EffectMix,
    seed: u32,
    pub(crate) time: f32,
    leaf_acc: f32,
    rain_acc: f32,
    snow_acc: f32,
    resting: i32,
}

pub fn heap_at(c: &SnowCap, rel: f32) -> f32 {
    let n = c.pile.len() as i32;
    if n == 0 {
        return 0.0;
    }
    let u = rel / B - 0.5;
    let i = u.floor() as i32;
    let t = u - i as f32;
    let i0 = i.clamp(0, n - 1) as usize;
    let i1 = (i + 1).clamp(0, n - 1) as usize;
    c.pile[i0] + (c.pile[i1] - c.pile[i0]) * t
}

fn ground_at(ground_y: &[f32], x: f32) -> f32 {
    let i = ((x / B) as i32).clamp(0, ground_y.len() as i32 - 1);
    ground_y[i as usize]
}

impl Default for Sim {
    fn default() -> Self {
        Self {
            parts: Vec::new(),
            caps: Vec::new(),
            ground: SnowCap::default(),
            ground_y: Vec::new(),
            ground_scale: Vec::new(),
            rest_order: Vec::new(),
            critters: Vec::new(),
            rare_events: true,
            flash: 0.0,
            s: 1.0,
            w: 0,
            h: 0,
            pending: Vec::new(),
            cursor: CursorWind::default(),
            gusts: Vec::new(),
            gust_clock: 0.0,
            critter_clock: 0.0,
            step_dt: 1.0 / 60.0,
            land_counter: 0,
            snow_gusts: Vec::new(),
            mix: EffectMix::none(),
            prev_mix: EffectMix::none(),
            seed: 0x9E37_79B9,
            time: 0.0,
            leaf_acc: 0.0,
            rain_acc: 0.0,
            snow_acc: 0.0,
            resting: 0,
        }
    }
}

impl Sim {
    // ------------------------------------------------------------ random numbers

    pub(crate) fn rand(&mut self) -> f32 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 17;
        self.seed ^= self.seed << 5;
        (self.seed & 0xFF_FFFF) as f32 / 16_777_216.0
    }

    pub(crate) fn range(&mut self, a: f32, b: f32) -> f32 {
        a + (b - a) * self.rand()
    }

    // ------------------------------------------------------------ setup

    pub fn reset(&mut self, width: i32, height: i32, scale: f32) {
        self.w = width;
        self.h = height;
        self.s = scale;
        self.parts.clear();
        self.parts.reserve(MAX_PARTICLES);
        self.caps.clear();
        self.critters.clear();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos() ^ d.as_secs() as u32)
            .unwrap_or(1);
        self.seed ^= nanos | 1;
        let s = self.s;
        self.ground = SnowCap::default();
        self.ground.init(width, 48.0 * s, 16.0 * s, 7.0 * s);
        self.ground.drains = false;
        // invisible bumps along the taskbar so rain gathers into separate puddles
        let a1 = 2.0 * PI * B / (self.range(260.0, 520.0) * s);
        let a2 = 2.0 * PI * B / (self.range(90.0, 180.0) * s);
        let p1 = self.range(0.0, TAU);
        let p2 = self.range(0.0, TAU);
        for (i, t) in self.ground.terrain.iter_mut().enumerate() {
            let i = i as f32;
            *t = 5.0 * s * (1.0 + 0.65 * (i * a1 + p1).sin() + 0.35 * (i * a2 + p2).sin());
        }
        self.ground_y = vec![height as f32; self.ground.height.len()];
        self.ground_scale = vec![s; self.ground_y.len()];
        self.prev_mix = EffectMix::none();
    }

    /// Work areas and monitors in overlay coordinates; `unit_scale` converts a monitor's height in desktop
    /// units to its 1080p-relative size (1 on Windows, where units are pixels).
    pub fn set_ground(&mut self, work_areas: &[Rect], monitors: &[Rect]) {
        self.ground_scale = vec![self.s; self.ground_y.len()];
        for i in 0..self.ground_y.len() {
            let x = (i as i32 * BUCKET + BUCKET / 2) as f32;
            let mut gy = self.h as f32 + 200.0; // gaps between monitors have no ground
            let mut found = false;
            for r in work_areas {
                if x >= r.l && x < r.r {
                    gy = if found { gy.max(r.b) } else { r.b };
                    found = true;
                }
            }
            self.ground_y[i] = gy;
            for m in monitors {
                if x >= m.l && x < m.r {
                    self.ground_scale[i] = ((m.b - m.t) / 1080.0).clamp(1.0, 3.0);
                }
            }
        }
    }

    /// Monitor-local pixel scale, so a 1080p screen next to a 4K one gets 1080p-sized things.
    pub fn scale_at(&self, x: f32) -> f32 {
        if self.ground_scale.is_empty() {
            return self.s;
        }
        let i = ((x / B) as i32).clamp(0, self.ground_scale.len() as i32 - 1);
        self.ground_scale[i as usize]
    }

    pub fn idle(&self) -> bool {
        if !self.parts.is_empty() || !self.critters.is_empty() || self.flash > 0.0 || self.ground.any() {
            return false;
        }
        !self.caps.iter().any(|c| c.any())
    }

    pub fn ground_at(&self, x: f32) -> f32 {
        ground_at(&self.ground_y, x)
    }

    pub fn cap_of(&self, h: WinId) -> Option<&SnowCap> {
        self.caps.iter().find(|c| c.h == h)
    }

    pub(crate) fn cap_at(&self, w: &Win, x: f32) -> f32 {
        self.cap_of(w.id).map(|c| c.at(x - w.l)).unwrap_or(0.0)
    }

    pub(crate) fn cap_mut(&mut self, r: CapRef) -> &mut SnowCap {
        match r {
            CapRef::Ground => &mut self.ground,
            CapRef::Cap(i) => &mut self.caps[i],
        }
    }

    pub(crate) fn cap_for(&mut self, w: &Win) -> usize {
        if let Some(i) = self.caps.iter().position(|c| c.h == w.id) {
            return i;
        }
        let s = self.s;
        let mut c = SnowCap { h: w.id, last_seen: self.time, ..Default::default() };
        c.init((w.r - w.l) as i32, 36.0 * s, 5.0 * s, 0.0);
        // a window's top edge works like a little roof: it falls off towards both corners, so rain runs to the gutters
        let n = c.terrain.len();
        for (j, t) in c.terrain.iter_mut().enumerate() {
            *t = 3.0 * s * (1.0 - (2.0 * j as f32 / (n.max(2) - 1) as f32 - 1.0).abs());
        }
        self.caps.push(c);
        self.caps.len() - 1
    }

    // ------------------------------------------------------------ spawning

    fn wind(&self) -> f32 {
        let t = self.time;
        let g = 0.5 * (t * 0.31).sin() + 0.3 * (t * 0.77 + 1.3).sin() + 0.2 * (t * 1.73 + 0.5).sin();
        (self.mix.wind_px * (0.75 + 0.45 * g) + 25.0 * g) * self.s
    }

    fn spawn_x(&mut self, wind: f32) -> f32 {
        let margin = wind.abs() * 3.0;
        let w = self.w as f32;
        if wind > 0.0 {
            self.range(-margin, w)
        } else {
            self.range(0.0, w + margin)
        }
    }

    fn pick_flight(&mut self, p: &mut Particle) {
        let s = self.scale_at(p.x); // sizes and speeds follow the monitor the leaf is on
        let sign = if self.rand() < 0.5 { -1.0 } else { 1.0 };
        let r = self.rand();
        p.mode_time = self.range(2.5, 9.0);
        if r < 0.55 {
            // the classic falling leaf: swinging side to side, stalling at each end of the swing
            p.mode = PENDULUM;
            p.freq = self.range(1.6, 3.4);
            p.amp = self.range(35.0, 110.0) * s * p.depth;
            p.tilt = self.range(20.0, 55.0);
            p.freq2 = self.range(0.2, 0.9) * sign;
            p.rotv = self.range(-25.0, 25.0);
        } else if r < 0.8 {
            // tumbling end over end
            p.mode = TUMBLE;
            p.freq = self.range(2.5, 5.0);
            p.amp = self.range(10.0, 35.0) * s * p.depth;
            p.tilt = self.range(5.0, 20.0);
            p.freq2 = self.range(3.0, 8.0) * sign;
            p.rotv = self.range(150.0, 480.0) * sign;
        } else {
            // spiralling down around a vertical axis
            p.mode = SPIRAL;
            p.freq = self.range(1.5, 3.0);
            p.amp = self.range(40.0, 90.0) * s * p.depth;
            p.tilt = self.range(10.0, 30.0);
            p.freq2 = p.freq;
            p.rotv = self.range(40.0, 140.0) * sign;
        }
    }

    fn new_leaf(&mut self, wind: f32) -> Particle {
        let mut p = Particle { kind: K_LEAF, ..Default::default() };
        p.x = self.spawn_x(wind);
        let s = self.scale_at(p.x);
        p.depth = self.range(0.6, 1.0);
        p.shape = ((self.rand() * LEAF_SHAPES as f32) as usize % LEAF_SHAPES) as u8;
        p.size = self.range(22.0, 40.0) * s * p.depth * SPECIES_SIZE[p.shape as usize];
        p.color = ((self.rand() * LEAF_COLORS as f32) as usize % LEAF_COLORS) as u8;
        if p.shape == GINKGO {
            p.color = if self.rand() < 0.5 { 2 } else { 6 }; // ginkgo turns butter yellow
        }
        p.y = -p.size;
        p.vx = wind;
        p.vy = 40.0 * s;
        p.phase = self.range(0.0, 2.0 * PI);
        p.phase2 = self.range(0.0, 2.0 * PI);
        p.spin = self.range(0.0, 360.0);
        p.drag = self.range(0.8, 2.2);
        p.fall = self.range(45.0, 85.0) * s * p.depth;
        p.noise = self.range(0.0, 50.0);
        p.life = self.range(0.0, 100.0); // per-leaf seed for its turbulence
        p.alpha = 1.0;
        p.flip = 1.0;
        self.pick_flight(&mut p);
        p
    }

    fn new_rain(&mut self, wind: f32) -> Particle {
        let mut p = Particle { kind: K_RAIN, ..Default::default() };
        p.x = self.spawn_x(wind * 0.4);
        let s = self.scale_at(p.x);
        p.depth = self.range(0.5, 1.0);
        p.y = -self.range(20.0, 120.0) * s;
        p.vx = wind * 1.3;
        p.vy = 1300.0 * s * p.depth;
        p.alpha = 1.0;
        p
    }

    pub(crate) fn new_snow(&mut self, wind: f32) -> Particle {
        let mut p = Particle { kind: K_SNOW, ..Default::default() };
        p.x = self.spawn_x(wind);
        let s = self.scale_at(p.x);
        // near flakes are bigger and quicker, far ones small, slow and faint; now and then a heavier clump
        p.depth = self.range(0.35, 1.0);
        let clump = self.rand() < 0.06;
        p.size = self.range(2.5, 5.5) * s * p.depth * if clump { 1.8 } else { 1.0 };
        p.fall = self.range(32.0, 60.0) * s * (0.5 + 0.5 * p.depth) * if clump { 1.3 } else { 1.0 };
        p.amp = self.range(12.0, 35.0) * s * p.depth;
        p.y = -10.0 * s;
        p.vx = wind * 0.75;
        p.vy = p.fall;
        p.phase = self.range(0.0, 2.0 * PI);
        p.phase2 = self.range(0.0, 2.0 * PI);
        p.freq = self.range(0.3, 0.9);
        p.freq2 = self.range(0.9, 1.8);
        p.life = self.range(0.0, 100.0);
        p.alpha = 1.0;
        p
    }

    fn seed_kind(&mut self, kind: u8, count: i32, wind: f32, tr: &Tracker) {
        for _ in 0..count {
            if self.parts.len() >= MAX_PARTICLES {
                break;
            }
            let mut p = match kind {
                K_LEAF => self.new_leaf(wind),
                K_RAIN => self.new_rain(wind),
                _ => self.new_snow(wind),
            };
            p.x = self.range(0.0, self.w as f32);
            p.y = self.range(0.0, self.h as f32);
            if p.y > self.ground_at(p.x) - 2.0 || self.swallowed(p.x, p.y, tr) {
                continue;
            }
            self.parts.push(p);
        }
    }

    fn room(&self) -> bool {
        self.parts.len() + self.pending.len() <= MAX_PARTICLES - 8
    }

    pub(crate) fn splash(&mut self, x: f32, y: f32, depth: f32, tint: u8, count: i32, power: f32) {
        if !self.room() {
            return;
        }
        let n = if count > 0 { count } else { 2 + (self.rand() * 3.0) as i32 };
        let s = self.s;
        for _ in 0..n {
            let mut p = Particle { kind: K_SPLASH, color: tint, x, y: y - 1.0, depth, alpha: 1.0, ..Default::default() };
            p.vx = self.range(-90.0, 90.0) * s * power;
            p.vy = -self.range(100.0, 300.0) * s * depth * power;
            p.life = self.range(0.2, 0.45);
            p.size = self.range(1.2, 2.5) * s * depth * if tint != 0 { 1.3 } else { 1.0 };
            self.pending.push(p);
        }
    }

    pub(crate) fn ripple(&mut self, x: f32, y: f32, depth: f32) {
        if !self.room() {
            return;
        }
        let life = self.range(0.5, 0.8);
        self.pending.push(Particle { kind: K_RIPPLE, x, y, depth, life, alpha: 1.0, ..Default::default() });
    }

    /// A drop leaving a window's top corner: either it runs down the window's side (mode 2) and lets go at the
    /// bottom, or it falls straight off the corner like from a gutter (mode 1, accelerating under gravity).
    fn drip(&mut self, w: &Win, side: i32) {
        if !self.room() {
            return;
        }
        let s = self.s;
        let mut p = Particle { kind: K_RAIN, host: w.id, offx: side as f32, alpha: 1.0, ..Default::default() };
        p.depth = self.range(0.6, 0.9);
        p.x = if side < 0 { w.l - 1.5 * s } else { w.r + 1.5 * s };
        p.y = w.t + 2.0;
        p.phase = self.range(0.0, TAU);
        if self.rand() < 0.5 {
            p.mode = 2;
            p.fall = self.range(90.0, 240.0) * s; // how fast it runs down the side
            p.rest = 0.0; // distance travelled down from the top corner
        } else {
            p.mode = 1;
            p.vy = self.range(20.0, 80.0) * s;
        }
        self.pending.push(p);
    }

    // ------------------------------------------------------------ stepping

    pub fn step(&mut self, dt: f32, tr: &Tracker, m: EffectMix, cursor: CursorWind) {
        self.time += dt;
        self.step_dt = dt;
        self.mix = m;
        let wind = self.wind();

        self.resting = self.parts.iter().filter(|p| p.state == RESTING).count() as i32;

        self.update_caps(dt, tr);

        let (w, h, s) = (self.w as f32, self.h as f32, self.s);
        let sx = w / 1920.0;
        if m.leaves > 0.0 && self.prev_mix.leaves <= 0.0 {
            self.seed_kind(K_LEAF, (m.leaves * LEAF_RATE * sx * h / (55.0 * s)) as i32, wind, tr);
        }
        if m.rain > 0.0 && self.prev_mix.rain <= 0.0 {
            self.seed_kind(K_RAIN, (m.rain * RAIN_RATE * sx * h / (1000.0 * s)) as i32, wind, tr);
        }
        if m.snow > 0.0 && self.prev_mix.snow <= 0.0 {
            self.seed_kind(K_SNOW, (m.snow * SNOW_RATE * sx * h / (45.0 * s)) as i32, wind, tr);
        }
        self.prev_mix = m;

        self.leaf_acc += m.leaves * LEAF_RATE * sx * dt;
        self.rain_acc += m.rain * RAIN_RATE * sx * dt;
        self.snow_acc += m.snow * SNOW_RATE * sx * dt;
        while self.leaf_acc >= 1.0 {
            self.leaf_acc -= 1.0;
            if self.parts.len() < MAX_PARTICLES {
                let p = self.new_leaf(wind);
                self.parts.push(p);
            }
        }
        while self.rain_acc >= 1.0 {
            self.rain_acc -= 1.0;
            if self.parts.len() < MAX_PARTICLES {
                let p = self.new_rain(wind);
                self.parts.push(p);
            }
        }
        while self.snow_acc >= 1.0 {
            self.snow_acc -= 1.0;
            if self.parts.len() < MAX_PARTICLES {
                let p = self.new_snow(wind);
                self.parts.push(p);
            }
        }

        if cursor.valid {
            self.cursor = cursor;
        }
        self.apply_cursor(&cursor, dt, tr);
        self.apply_gusts(&cursor, dt);

        let mut i = self.parts.len();
        while i > 0 {
            i -= 1;
            let mut p = self.parts[i];
            if self.update(&mut p, dt, wind, tr) {
                self.parts[i] = p;
            } else {
                self.parts.swap_remove(i);
            }
        }
        let pending = std::mem::take(&mut self.pending);
        self.parts.extend_from_slice(&pending);
        self.pending = pending;
        self.pending.clear();

        self.stack_leaves(tr);
        self.update_critters(dt, tr);

        self.flash = (self.flash - dt * 4.0).max(0.0);
        if m.thunder && self.rand() < dt * 0.05 {
            self.flash = 1.0;
        }
    }

    pub(crate) fn fast(&self, w: &Win, dt: f32) -> bool {
        w.dx.abs() + w.dy.abs() > 800.0 * self.s * dt
    }

    fn update_caps(&mut self, dt: f32, tr: &Tracker) {
        let s = self.s;
        let melt = if self.mix.snow > 0.0 {
            0.04 * s
        } else if self.mix.temp_c > 2.0 {
            1.2 * s
        } else {
            0.35 * s
        };
        let raining = self.mix.rain > 0.0;
        let evaporate = if raining { 0.002 * s } else { 0.06 * s };
        let dry = if raining { 0.0 } else { 0.012 * s };

        let mut i = self.caps.len();
        while i > 0 {
            i -= 1;
            let mut c = std::mem::take(&mut self.caps[i]);
            let keep = match tr.get(c.h).filter(|w| w.surface) {
                None => self.time - c.last_seen < 2.0,
                Some(w) => {
                    c.last_seen = self.time;
                    c.resize((w.r - w.l) as i32);
                    if self.fast(w, dt) {
                        // a shaken window throws its snow and water off
                        let mut j = 0;
                        while j < c.height.len() {
                            let (hgt, wa) = (c.height[j], c.water[j]);
                            let x = w.l + j as f32 * B + self.rand() * B;
                            let n = if hgt >= 1.5 { 1 + (hgt / (6.0 * s)) as i32 } else { 0 };
                            for _ in 0..n {
                                if self.parts.len() + self.pending.len() >= MAX_PARTICLES {
                                    break;
                                }
                                let mut p = self.new_snow(0.0);
                                p.x = x;
                                p.y = w.t - self.rand() * hgt - 2.0;
                                p.vx = w.dx / dt * 0.4 + self.range(-60.0, 60.0) * s;
                                p.vy = -self.range(0.0, 150.0) * s;
                                self.pending.push(p);
                            }
                            if wa > 0.5 * s && self.rand() < 0.3 {
                                self.splash(x, w.t - wa, 1.0, 0, 2, 1.5);
                            }
                            c.height[j] *= 0.25;
                            if j + 1 < c.height.len() {
                                c.height[j + 1] *= 0.25;
                            }
                            j += 2;
                        }
                        for wa in &mut c.water {
                            *wa *= 0.3;
                        }
                    }
                    c.relax(dt, melt);
                    self.settle_snow(&mut c, w.l, false, dt);
                    self.slide_snow_off_edges(&mut c, w, dt);
                    let (spill_l, spill_r) = c.flow(dt, evaporate, dry);
                    // water reaching a corner gathers into a bead that lets go once it is heavy enough;
                    // in a downpour that becomes a steady trickle like from a gutter
                    let drop = 0.5 * s;
                    c.bead_l += spill_l;
                    c.bead_r += spill_r;
                    for _ in 0..4 {
                        if c.bead_l <= drop {
                            break;
                        }
                        c.bead_l -= drop;
                        self.drip(w, -1);
                    }
                    for _ in 0..4 {
                        if c.bead_r <= drop {
                            break;
                        }
                        c.bead_r -= drop;
                        self.drip(w, 1);
                    }
                    c.bead_l = c.bead_l.min(drop);
                    c.bead_r = c.bead_r.min(drop);
                    let heap = c.pile.iter().any(|&h| h > 0.3);
                    c.any() || heap || self.mix.snow > 0.0 || raining
                }
            };
            self.caps[i] = c;
            if !keep {
                self.caps.swap_remove(i);
            }
        }
        let mut g = std::mem::take(&mut self.ground);
        g.relax(dt, melt);
        self.settle_snow(&mut g, 0.0, true, dt);
        g.flow(dt, evaporate, dry);
        self.ground = g;
    }

    fn find_landing<'a>(&self, x: f32, y0: f32, y1: f32, tr: &'a Tracker) -> Option<(&'a Win, f32)> {
        for w in &tr.all {
            if !w.surface || x < w.l || x > w.r {
                continue;
            }
            let top = w.t - self.cap_of(w.id).map(|c| c.at(x - w.l) + heap_at(c, x - w.l)).unwrap_or(0.0);
            if y0 <= top && y1 >= top && !tr.covered(x, top - 2.0, w.z, false) {
                return Some((w, top));
            }
        }
        None
    }

    fn swallowed(&self, x: f32, y: f32, tr: &Tracker) -> bool {
        tr.all
            .iter()
            .any(|w| w.surface && x > w.l && x < w.r && y > w.t + 4.0 && y < w.b && !tr.covered(x, y, w.z, false))
    }

    fn update_leaf_flight(&mut self, p: &mut Particle, dt: f32, wind: f32) {
        p.mode_time -= dt;
        if p.mode_time <= 0.0 {
            self.pick_flight(p); // leaves change their way of falling now and then
        }
        p.phase += p.freq * dt;
        p.phase2 += p.freq2 * dt;
        p.noise += dt;
        let s = self.s;

        // per-leaf turbulence so no two leaves follow the same curve
        let turb = 0.6 * (p.noise * 1.3 + p.life).sin() + 0.4 * (p.noise * 2.9 + p.life * 2.1).sin();
        let (sn, c) = (p.phase.sin(), p.phase.cos());
        let (tvx, tvy);
        p.spin += p.rotv * dt;
        match p.mode {
            PENDULUM => {
                tvx = wind * 0.9 + p.amp * c * (1.0 + 0.3 * turb);
                tvy = p.fall * (0.2 + 1.25 * c * c) + turb * 12.0 * s; // fastest mid-swing, stalls at the ends
                p.rot = p.spin + p.tilt * c;
                p.flip = p.phase2.cos() * (0.75 + 0.25 * c.abs());
            }
            TUMBLE => {
                tvx = wind + p.amp * sn + turb * 30.0 * s;
                tvy = p.fall * 1.15 + turb * 15.0 * s;
                p.rot = p.spin;
                p.flip = p.phase2.cos();
            }
            _ => {
                tvx = wind * 0.85 + p.amp * c;
                tvy = p.fall * 0.95 + turb * 10.0 * s;
                p.rot = p.spin + p.tilt * sn;
                p.flip = sn;
            }
        }
        // after a gust a leaf coasts on its own momentum for a while before its usual flight takes over again
        let coast = if p.rest > 0.0 { 0.2 } else { 1.0 };
        p.rest = (p.rest - dt).max(0.0);
        p.vx += (tvx - p.vx) * (dt * p.drag * 2.5 * coast).min(1.0);
        p.vy += (tvy - p.vy) * (dt * p.drag * 3.0 * coast).min(1.0);
    }

    fn update(&mut self, p: &mut Particle, dt: f32, wind: f32, tr: &Tracker) -> bool {
        let s = self.s;
        p.age += dt;
        if p.kind == K_RIPPLE {
            return p.age < p.life;
        }
        if p.state != FALLING {
            return self.update_resting(p, dt, tr);
        }

        match p.kind {
            K_LEAF => self.update_leaf_flight(p, dt, wind),
            K_RAIN => {
                if p.mode == 2 {
                    // running down a window's side edge, wobbling a little, until it reaches the bottom corner
                    match tr.get(p.host).filter(|w| w.surface) {
                        None => p.mode = 1,
                        Some(w) => {
                            p.rest += p.fall * (0.8 + 0.4 * (p.age * 3.0 + p.phase).sin()) * dt;
                            let h = w.b - w.t;
                            p.x = (if p.offx < 0.0 { w.l - 1.5 * s } else { w.r + 1.5 * s })
                                + (p.age * 5.0 + p.phase).sin() * 0.6 * s;
                            p.y = w.t + p.rest.min(h);
                            p.hidden = tr.covered(p.x, p.y, w.z, false);
                            if p.rest >= h {
                                p.mode = 1;
                                p.vx = 0.0;
                                p.vy = p.fall * 0.5;
                                p.hidden = false;
                            }
                            return true;
                        }
                    }
                } else if p.mode == 1 {
                    // dripping: accelerates from rest instead of arriving at full speed
                    p.vy = (1300.0 * s).min(p.vy + 2200.0 * s * dt);
                    p.vx += (wind * 0.3 - p.vx) * (dt * 2.0).min(1.0);
                } else {
                    p.vx += (wind * 1.3 - p.vx) * (dt * 4.0).min(1.0);
                }
            }
            K_SNOW => {
                // a lazy meander from two slow oscillators per flake; far flakes feel less wind
                p.phase += p.freq * dt;
                p.phase2 += p.freq2 * dt;
                let drift = p.phase.sin() * p.amp + (p.phase2 + p.life).sin() * p.amp * 0.35;
                let k = (dt * 1.5).min(1.0);
                p.vx += (wind * (0.55 + 0.35 * p.depth) + drift - p.vx) * k;
                p.vy += (p.fall * (1.0 + 0.15 * p.phase2.sin()) - p.vy) * k;
            }
            K_SPLASH => {
                p.vy += 1600.0 * s * dt;
                p.x += p.vx * dt;
                p.y += p.vy * dt;
                return p.age < p.life;
            }
            _ => {}
        }

        let py = p.y;
        p.x += p.vx * dt;
        p.y += p.vy * dt;
        let (w, h) = (self.w as f32, self.h as f32);
        if p.y > h + 60.0 * s || p.x < -400.0 * s || p.x > w + 400.0 * s {
            return false;
        }
        if p.vy <= 0.0 {
            return true;
        }

        // a leaf touches down with its lower edge, not its middle
        let reach = if p.kind == K_LEAF { p.size * 0.22 } else { 0.0 };
        if let Some((win, hy)) = self.find_landing(p.x, py + reach, p.y + reach, tr) {
            return self.land(p, Some(win), hy);
        }
        if p.x >= 0.0 && p.x < w {
            let gy = self.ground_at(p.x) - self.ground.at(p.x) - heap_at(&self.ground, p.x);
            if py + reach <= gy && p.y + reach >= gy {
                return self.land(p, None, gy);
            }
            if p.y > gy + 20.0 * s {
                return false;
            }
        }
        !self.swallowed(p.x, p.y, tr)
    }

    fn land(&mut self, p: &mut Particle, w: Option<&Win>, y: f32) -> bool {
        let s = self.s;
        match p.kind {
            K_RAIN => {
                let cr = match w {
                    Some(w) => CapRef::Cap(self.cap_for(w)),
                    None => CapRef::Ground,
                };
                let rel = w.map(|w| p.x - w.l).unwrap_or(p.x);
                let (wet, muddy) = {
                    let c = self.cap_mut(cr);
                    let i = ((rel / B) as i32).clamp(0, c.water.len() as i32 - 1) as usize;
                    let wet = c.water[i] > 0.4 * s;
                    if wet {
                        // a drop into a puddle: a ring, a little crown and a dent that travels as a wave
                        c.wave_v[i] -= 25.0 * s * p.depth;
                    }
                    (wet, c.mud[i] > 1.5 * s)
                };
                if wet {
                    if self.rand() < 0.35 {
                        self.ripple(p.x, y, p.depth);
                    }
                    let extra = if self.rand() < 0.5 { 1 } else { 0 };
                    self.splash(p.x, y, p.depth, 0, 1 + extra, 0.6);
                } else {
                    let tint = if w.is_none() && muddy && self.rand() < 0.3 { 1 } else { 0 };
                    self.splash(p.x, y, p.depth, tint, 0, 1.0);
                }
                let c = self.cap_mut(cr);
                c.add_water(rel, 0.025 * s * p.depth * if w.is_some() { 1.0 } else { 2.0 });
                if w.is_none() {
                    c.add_mud(rel, 0.006 * s * if wet { 2.0 } else { 1.0 });
                }
                false
            }
            K_SNOW => {
                match w {
                    Some(w) => {
                        let i = self.cap_for(w);
                        self.caps[i].add(p.x - w.l, p.size * 0.55);
                    }
                    None => self.ground.add(p.x, p.size * 0.55),
                }
                false
            }
            _ => {
                // keep a little of the sideways drift so it skids to a stop, and settle from the falling pose
                let ls = self.scale_at(p.x);
                p.vx = (p.vx * 0.1).clamp(-20.0 * ls, 20.0 * ls);
                p.vy = 0.0;
                p.y = y;
                p.on_ground = w.is_none();
                p.host = w.map(|w| w.id).unwrap_or(0);
                p.offx = w.map(|w| p.x - w.l).unwrap_or(0.0);
                p.start_rot = p.rot;
                // mirrored or not makes no difference to a leaf; never squash through zero while settling
                p.start_flip = p.flip.abs().max(0.2);
                // lying with its length along the ground: whichever way round is nearer to how it came down
                let mut r = p.rot % 360.0;
                if r < 0.0 {
                    r += 360.0;
                }
                let to90 = ((r - 90.0 + 540.0) % 360.0 - 180.0).abs();
                let to270 = ((r - 270.0 + 540.0) % 360.0 - 180.0).abs();
                p.rest_rot = (if to90 < to270 { 90.0 } else { 270.0 }) + self.range(-12.0, 12.0);
                p.rot = p.rest_rot;
                p.settle = 0.0;
                p.heaped = false;
                // landing on a heap the user swept up: it joins it, on top
                let (cr, left) = match w {
                    Some(w) => (CapRef::Cap(self.cap_for(w)), w.l),
                    None => (CapRef::Ground, 0.0),
                };
                if heap_at(self.cap_mut(cr), p.x - left) > 0.5 * ls {
                    join_heap(p, self.cap_mut(cr), left, 1.0, false);
                }
                p.alpha = 1.0;
                if self.resting >= MAX_RESTING {
                    p.state = FADING;
                } else {
                    p.state = RESTING;
                    p.rest = self.range(40.0, 120.0);
                    self.resting += 1;
                }
                p.landed = self.time;
                self.land_counter += 1;
                p.land_seq = self.land_counter;
                true
            }
        }
    }

    fn update_resting(&mut self, p: &mut Particle, dt: f32, tr: &Tracker) -> bool {
        let s = self.s;
        p.settle = (p.settle + dt / 0.35).min(1.0);
        if p.on_ground {
            // a nudge (from a hedgehog, say) slides it along smoothly instead of teleporting it
            p.x = (p.x + p.vx * dt).clamp(0.0, self.w as f32 - 1.0);
            p.vx *= (-dt * 8.0).exp();
            if !p.heaped {
                p.y = self.ground_at(p.x) - self.ground.at(p.x); // heaped leaves are placed by stack_leaves
            }
            p.hidden = tr.covered(p.x, p.y - 2.0, i32::MAX, true);
        } else {
            let w = tr.get(p.host);
            let fall = match w {
                None => true,
                Some(w) => !w.surface || p.offx < 0.0 || p.offx > w.r - w.l,
            };
            if let Some(w) = w {
                if !fall && self.fast(w, dt) {
                    // flung off a window that is being dragged around
                    p.state = FALLING;
                    p.vx = (w.dx / dt * 0.5).clamp(-1500.0 * s, 1500.0 * s);
                    p.vy = (w.dy / dt * 0.5).min(-80.0 * s) - self.range(0.0, 150.0) * s;
                    p.rotv = self.range(-400.0, 400.0);
                    p.mode_time = self.range(0.6, 1.5);
                    p.spin = p.rot;
                    p.heaped = false;
                    p.y -= 3.0;
                    p.hidden = false;
                    return true;
                }
            }
            if fall {
                if p.state == FADING {
                    return false;
                }
                p.state = FALLING;
                p.vx = 0.0;
                p.vy = 0.0;
                p.spin = p.rot;
                p.heaped = false;
                p.hidden = false;
                return true;
            }
            let w = w.unwrap();
            p.offx += p.vx * dt;
            p.vx *= (-dt * 8.0).exp();
            p.x = w.l + p.offx;
            if !p.heaped {
                p.y = w.t - self.cap_at(w, p.x);
            }
            p.hidden = tr.covered(p.x, p.y - 2.0, w.z, false);
        }

        if p.state == RESTING {
            p.rest -= dt;
            if p.rest <= 0.0 {
                p.state = FADING;
            }
            return true;
        }
        p.alpha -= dt * 0.5;
        p.alpha > 0.0
    }

    // ------------------------------------------------------------ mouse wind

    pub(crate) fn cursor_falloff(c: &CursorWind, x: f32, y: f32, radius: f32) -> f32 {
        // distance to the segment the pointer travelled this frame, so fast swipes act along their whole path
        if x < c.x0.min(c.x1) - radius
            || x > c.x0.max(c.x1) + radius
            || y < c.y0.min(c.y1) - radius
            || y > c.y0.max(c.y1) + radius
        {
            return 0.0;
        }
        let (dx, dy) = (c.x1 - c.x0, c.y1 - c.y0);
        let len2 = dx * dx + dy * dy;
        let t = if len2 > 0.0 { (((x - c.x0) * dx + (y - c.y0) * dy) / len2).clamp(0.0, 1.0) } else { 0.0 };
        let (px, py) = (c.x0 + t * dx - x, c.y0 + t * dy - y);
        let d2 = px * px + py * py;
        if d2 >= radius * radius {
            return 0.0;
        }
        let f = 1.0 - d2.sqrt() / radius;
        f * f
    }

    fn blow_cap(&mut self, c: &mut SnowCap, left: f32, w: Option<&Win>, cursor: &CursorWind, dt: f32) {
        let s = self.s;
        let r = 150.0 * s;
        for j in 0..c.height.len() {
            let h = c.height[j];
            if h < 0.5 {
                continue;
            }
            let x = left + j as f32 * B + B / 2.0;
            let base = w.map(|w| w.t).unwrap_or_else(|| self.ground_y[j.min(self.ground_y.len() - 1)]) - c.mud[j];
            let f = Self::cursor_falloff(cursor, x, base - h * 0.5, r);
            if f <= 0.0 {
                continue;
            }
            let take = h * (f * dt * 4.0).min(1.0);
            c.height[j] -= take;
            let n = take / (2.0 * s);
            let count = n as i32 + if self.rand() < n - (n as i32) as f32 { 1 } else { 0 };
            for _ in 0..count {
                if self.parts.len() >= MAX_PARTICLES {
                    break;
                }
                let mut p = self.new_snow(0.0);
                p.x = x + self.range(-2.0, 2.0) * s;
                p.y = base - self.rand() * h - 1.0;
                p.vx = cursor.vx * 0.6 * f + self.range(-60.0, 60.0) * s;
                p.vy = cursor.vy * 0.6 * f - self.range(40.0, 200.0) * s;
                self.parts.push(p);
            }
        }
    }

    fn stir_water(&mut self, c: &mut SnowCap, left: f32, w: Option<&Win>, cursor: &CursorWind, dt: f32) {
        let s = self.s;
        let r = 160.0 * s;
        let speed = (cursor.vx * cursor.vx + cursor.vy * cursor.vy).sqrt();
        let n = c.water.len() as i32;
        let dir = if cursor.vx > 0.0 { 1 } else { -1 };
        for j in 0..n {
            let ju = j as usize;
            if c.water[ju] < 0.3 * s {
                continue;
            }
            let x = left + j as f32 * B + B / 2.0;
            let base = w.map(|w| w.t).unwrap_or_else(|| self.ground_y[ju.min(self.ground_y.len() - 1)]);
            let y = base - c.mud[ju] - c.height[ju] - c.water[ju];
            let f = Self::cursor_falloff(cursor, x, y, r);
            if f <= 0.0 {
                continue;
            }

            // the pointer drags a bow wave along and sloshes water in its direction
            c.wave_v[ju] += (cursor.vy * 0.025 - cursor.vx.abs() * 0.01) * f;
            let to = j + dir;
            if to >= 0 && to < n {
                let mv = c.water[ju] * (f * dt * cursor.vx.abs() / (200.0 * s)).min(1.0) * 0.5;
                c.water[ju] -= mv;
                c.water[to as usize] = c.water_max.min(c.water[to as usize] + mv);
            }
            // fast swipes throw droplets, muddy ones where the puddle sits on mud
            if speed > 500.0 * s && self.rand() < f * dt * speed / (120.0 * s) && self.parts.len() < MAX_PARTICLES {
                let mut p = Particle { kind: K_SPLASH, x, y: y - 1.0, depth: 1.0, alpha: 1.0, ..Default::default() };
                p.color = if c.mud[ju] > 1.0 * s && self.rand() < 0.5 { 1 } else { 0 };
                p.vx = cursor.vx * 0.35 * f + self.range(-60.0, 60.0) * s;
                p.vy = -self.range(150.0, 400.0) * s * f - cursor.vy.abs() * 0.2;
                p.life = self.range(0.35, 0.7);
                p.size = self.range(1.5, 3.0) * s;
                self.parts.push(p);
            }
        }
    }

    fn apply_cursor(&mut self, c: &CursorWind, dt: f32, tr: &Tracker) {
        if !c.valid {
            return;
        }
        let s = self.s;
        let speed = (c.vx * c.vx + c.vy * c.vy).sqrt();
        if speed < 40.0 * s {
            return;
        }
        let r = 190.0 * s;
        let strong = speed > 450.0 * s;

        for i in 0..self.parts.len() {
            let mut p = self.parts[i];
            if p.kind == K_SPLASH || p.kind == K_RIPPLE {
                continue;
            }
            let f = Self::cursor_falloff(c, p.x, p.y, r);
            if f <= 0.0 || p.state == FALLING {
                // falling things are moved by the gusts the pointer sheds (apply_gusts), not dragged along with it
                continue;
            }
            let lift = strong && f > 0.55 && self.rand() < 0.2;
            if p.kind == K_LEAF && !lift {
                // the pointer sweeps resting leaves along the edge: a swept leaf becomes part of a heap,
                // whose volume moves with the sweep (transport_heap) and slumps into a mound
                if !p.heaped {
                    if p.on_ground {
                        let d = self.range(0.05, 1.0);
                        join_heap(&mut p, &mut self.ground, 0.0, d, true);
                    } else if let Some(w) = tr.get(p.host) {
                        let ci = self.cap_for(w);
                        let d = self.range(0.05, 1.0);
                        join_heap(&mut p, &mut self.caps[ci], w.l, d, true);
                    }
                }
                // its position now follows the heap's mass as transport_heap moves it
                p.rest = p.rest.max(60.0);
                self.parts[i] = p;
                continue;
            }
            if strong && f > 0.1 {
                // a hard swipe right over them lifts leaves back into the air
                p.state = FALLING;
                p.vx = c.vx * 0.35 * f;
                p.vy = c.vy * 0.35 * f - self.range(60.0, 150.0) * s;
                p.rotv = self.range(-500.0, 500.0);
                p.mode_time = self.range(0.6, 1.5);
                p.rest = 1.0; // coasts on the gust for a moment
                p.spin = p.rot;
                p.heaped = false;
                p.y -= 3.0;
                p.hidden = false;
                self.parts[i] = p;
            }
        }

        for i in 0..self.caps.len() {
            let Some(w) = tr.get(self.caps[i].h).filter(|w| w.surface) else { continue };
            let mut cap = std::mem::take(&mut self.caps[i]);
            self.transport_heap(&mut cap, w.l, Some(w), c, dt);
            if strong {
                self.blow_cap(&mut cap, w.l, Some(w), c, dt);
            }
            self.stir_water(&mut cap, w.l, Some(w), c, dt);
            self.caps[i] = cap;
        }
        let mut g = std::mem::take(&mut self.ground);
        self.transport_heap(&mut g, 0.0, None, c, dt);
        if strong {
            self.blow_cap(&mut g, 0.0, None, c, dt);
        }
        self.stir_water(&mut g, 0.0, None, c, dt);
        self.ground = g;
    }

    // ------------------------------------------------------------ leaf heaps

    // A heap is a persistent height profile of leaf volume along a surface, like a snow drift. Sweeping with the
    // pointer moves that volume along (transport_heap); every frame the slopes slump back to a natural angle. The
    // leaves in the heap sit on its surface at a fixed depth each, so they follow the heap smoothly instead of
    // restacking themselves frame by frame.

    fn transport_heap(&self, c: &mut SnowCap, left: f32, w: Option<&Win>, cursor: &CursorWind, dt: f32) {
        let n = c.pile.len() as i32;
        if n < 2 || cursor.vx.abs() < 1.0 {
            return;
        }
        let s = self.s;
        let r = 190.0 * s;
        let dir = if cursor.vx > 0.0 { 1 } else { -1 };
        let mut tmp = c.pile.clone();
        let mut moved = false;
        let (mut lo, mut hi) = (1e9f32, -1e9f32);
        for j in 0..n {
            let ju = j as usize;
            if c.pile[ju] < 0.05 {
                continue;
            }
            let base = w.map(|w| w.t).unwrap_or_else(|| self.ground_y[ju.min(self.ground_y.len() - 1)]);
            let f = Self::cursor_falloff(cursor, left + (j as f32 + 0.5) * B, base - c.pile[ju] * 0.5, r);
            if f <= 0.0 {
                continue;
            }
            let to = j + dir;
            if to < 0 || to >= n {
                continue;
            }
            // the same push the swept leaves get, as a fraction of a bucket's volume per frame
            let shift = (12.0 * s).min(cursor.vx.abs() * f * dt * 0.35);
            let m = c.pile[ju] * (shift / B).min(0.8);
            tmp[ju] -= m;
            tmp[to as usize] += m;
            moved = true;
            lo = lo.min(j as f32 * B);
            hi = hi.max((to + 1) as f32 * B);
        }
        if moved {
            c.pile = tmp;
            // leaves in the swept stretch follow the moving heap for a moment
            if c.stirred > 0.0 {
                c.stir_l = c.stir_l.min(lo);
                c.stir_r = c.stir_r.max(hi);
            } else {
                c.stir_l = lo;
                c.stir_r = hi;
            }
            c.stirred = 0.6;
        }
    }

    fn stack_leaves(&mut self, tr: &Tracker) {
        let dt = self.step_dt;

        // how much volume each surface's heaped leaves account for; fading leaves give theirs up gradually
        let mut need_ground = 0.0f32;
        let mut need = vec![0.0f32; self.caps.len()];
        for p in &mut self.parts {
            if p.kind != K_LEAF || p.state == FALLING || !p.heaped {
                continue;
            }
            if p.state == FADING {
                p.heap_vol = (p.heap_vol - p.heap_vol0 * 0.5 * dt).max(0.0);
            }
            if p.on_ground {
                need_ground += p.heap_vol;
            } else if let Some(i) = self.caps.iter().position(|c| c.h == p.host) {
                need[i] += p.heap_vol;
            }
        }

        let settle = |c: &mut SnowCap, required: f32| {
            let n = c.pile.len();
            let actual: f32 = c.pile.iter().map(|h| h * B).sum();
            if actual <= 0.01 {
                return;
            }
            // leaves that left the heap (lifted, eaten, faded) take their share with them, smoothly
            if actual > required + 1.0 {
                let k = 1.0 - (dt * 3.0).min(1.0) * (1.0 - required / actual);
                for h in &mut c.pile {
                    *h *= k;
                }
            }
            // slopes slump until they are no steeper than about 35 degrees
            let lim = 0.7 * B;
            for _ in 0..2 {
                for j in 0..n.saturating_sub(1) {
                    let d = c.pile[j] - c.pile[j + 1];
                    if d > lim {
                        let t = (d - lim) * 0.3;
                        c.pile[j] -= t;
                        c.pile[j + 1] += t;
                    } else if d < -lim {
                        let t = (-d - lim) * 0.3;
                        c.pile[j] += t;
                        c.pile[j + 1] -= t;
                    }
                }
            }
            for h in &mut c.pile {
                if *h < 0.02 {
                    *h = 0.0;
                }
            }
        };
        settle(&mut self.ground, need_ground);
        for (i, c) in self.caps.iter_mut().enumerate() {
            settle(c, need[i]);
        }

        // While the pointer is sweeping a heap, its leaves keep their place in the heap's mass and glide to
        // where that mass is now (inverse of the cumulative volume), so they travel with the sweep. The rest of
        // the time they stay put and their place in the mass is just re-read from where they lie.
        fn cumulative(c: &SnowCap, cum: &mut Vec<f32>) -> f32 {
            cum.clear();
            cum.push(0.0);
            let mut acc = 0.0;
            for &h in &c.pile {
                acc += h;
                cum.push(acc);
            }
            acc
        }
        fn mass_to_rel(cum: &[f32], frac: f32) -> f32 {
            let total = *cum.last().unwrap();
            let target = frac * total;
            // first index in cum[1..] with a value greater than target
            let j = 1 + cum[1..].partition_point(|&v| v <= target);
            if j >= cum.len() {
                return (cum.len() - 1) as f32 * B;
            }
            let span = cum[j] - cum[j - 1];
            ((j - 1) as f32 + if span > 0.0 { (target - cum[j - 1]) / span } else { 0.5 }) * B
        }
        fn rel_to_mass(cum: &[f32], rel: f32) -> f32 {
            let u = (rel / B).clamp(0.0, (cum.len() - 1) as f32);
            let j = (cum.len() - 2).min(u as usize);
            let v = cum[j] + (cum[j + 1] - cum[j]) * (u - j as f32);
            let total = *cum.last().unwrap();
            if total > 0.0 {
                v / total
            } else {
                0.5
            }
        }
        self.ground.stirred = (self.ground.stirred - dt).max(0.0);
        for c in &mut self.caps {
            c.stirred = (c.stirred - dt).max(0.0);
        }
        let mut cum_ground = Vec::new();
        let ground_heap = cumulative(&self.ground, &mut cum_ground) > 0.0;
        let mut cum_cap = Vec::new();
        let glide = (dt * 8.0).min(1.0);
        let ease = (dt * 10.0).min(1.0);
        let Sim { parts, caps, ground, ground_y, ground_scale, s, .. } = self;
        let scale_at = |x: f32| -> f32 {
            if ground_scale.is_empty() {
                return *s;
            }
            ground_scale[((x / B) as i32).clamp(0, ground_scale.len() as i32 - 1) as usize]
        };
        for p in parts.iter_mut() {
            if p.kind != K_LEAF || p.state == FALLING || !p.heaped {
                continue;
            }
            let (c, cum, left, base): (&SnowCap, &Vec<f32>, f32, f32);
            if p.on_ground {
                if !ground_heap {
                    continue;
                }
                c = ground;
                cum = &cum_ground;
                left = 0.0;
                base = ground_at(ground_y, p.x) - ground.at(p.x);
            } else {
                let Some(w) = tr.get(p.host) else { continue };
                let Some(cc) = caps.iter().find(|c| c.h == w.id) else { continue };
                if cumulative(cc, &mut cum_cap) <= 0.0 {
                    continue;
                }
                c = cc;
                cum = &cum_cap;
                left = w.l;
                base = w.t - cc.at(p.x - left);
            }
            let mut rel = p.x - left;
            let margin = 40.0 * scale_at(p.x);
            let goal = if c.stirred > 0.0 && rel > c.stir_l - margin && rel < c.stir_r + margin {
                mass_to_rel(cum, p.mass_frac)
            } else {
                rel
            };
            // only short moves along with the swept mass; never jump to some other heap further away
            if (goal - rel).abs() < 2.0 * margin && goal != rel {
                rel += (goal - rel) * glide;
                if p.on_ground {
                    p.x = rel;
                } else {
                    p.offx = rel;
                    p.x = left + rel;
                }
            } else {
                p.mass_frac = rel_to_mass(cum, rel);
            }
            // ride on the heap surface at its own depth, easing so small reshapes don't make it bob
            let target = base - heap_at(c, rel) * p.vy;
            p.y = if p.settle < 1.0 { target } else { p.y + (target - p.y) * ease };
        }

        self.rest_order.clear();
        for (i, p) in self.parts.iter().enumerate() {
            if p.kind == K_LEAF && p.state != FALLING {
                self.rest_order.push(i);
            }
        }
        // draw flat leaves first in landing order, then heaped ones from the inside out
        let parts = &self.parts;
        self.rest_order.sort_by(|&a, &b| {
            let (pa, pb) = (&parts[a], &parts[b]);
            if pa.heaped != pb.heaped {
                return pa.heaped.cmp(&pb.heaped);
            }
            if pa.heaped && pa.vy != pb.vy {
                return pa.vy.partial_cmp(&pb.vy).unwrap_or(std::cmp::Ordering::Equal);
            }
            pa.land_seq.cmp(&pb.land_seq)
        });
    }

    // ------------------------------------------------------------ pointer gusts

    // A moving pointer sheds puffs of air. Each puff carries on in the pointer's direction, slows, widens and
    // fades within about half a second. Whatever it passes through gets a push along it, a nudge sideways out of
    // its path and a little curl, then keeps its own momentum: like waving a hand through falling leaves.
    fn apply_gusts(&mut self, c: &CursorWind, dt: f32) {
        let s = self.s;
        self.gust_clock += dt;
        if c.valid {
            let speed = (c.vx * c.vx + c.vy * c.vy).sqrt();
            if speed > 150.0 * s && self.gust_clock >= 1.0 / 40.0 && self.gusts.len() < 24 {
                self.gust_clock = 0.0;
                let k = (2000.0 * s / speed).min(1.0) * 0.4;
                let swirl = if self.rand() < 0.5 { -1.0 } else { 1.0 };
                self.gusts.push(Gust {
                    x: c.x1,
                    y: c.y1,
                    vx: c.vx * k,
                    vy: c.vy * k,
                    r: (40.0 * s + speed * 0.025).clamp(50.0 * s, 140.0 * s),
                    age: 0.0,
                    life: 0.55 + (speed / (4000.0 * s)).min(0.5),
                    swirl,
                });
            }
        }
        if self.gusts.is_empty() {
            return;
        }

        let drag = (-dt * 2.2).exp();
        let mut i = self.gusts.len();
        while i > 0 {
            i -= 1;
            let g = &mut self.gusts[i];
            g.age += dt;
            if g.age >= g.life {
                self.gusts.swap_remove(i);
                continue;
            }
            g.x += g.vx * dt;
            g.y += g.vy * dt;
            g.vx *= drag;
            g.vy *= drag;
            g.r += 90.0 * s * dt;
        }

        let gusts = std::mem::take(&mut self.gusts);
        for i in 0..self.parts.len() {
            let mut p = self.parts[i];
            if p.state != FALLING || p.kind == K_SPLASH || p.kind == K_RIPPLE || (p.kind == K_RAIN && p.mode == 2) {
                continue;
            }
            let mut touched = false;
            for g in &gusts {
                let (dx, dy) = (p.x - g.x, p.y - g.y);
                if dx.abs() > g.r || dy.abs() > g.r {
                    continue;
                }
                let d2 = dx * dx + dy * dy;
                if d2 >= g.r * g.r {
                    continue;
                }
                touched = true;
                let d = d2.sqrt() + 1e-3;
                let fade = 1.0 - g.age / g.life;
                let f = (1.0 - d / g.r) * (1.0 - d / g.r) * fade * fade;
                let weight = match p.kind {
                    K_RAIN => 0.15,
                    K_SNOW => 1.3,
                    _ => 1.0,
                };
                let a = f * weight * 3.0 * dt;
                let gs = (g.vx * g.vx + g.vy * g.vy).sqrt();
                let (nx, ny) = (dx / d, dy / d);
                let ax = (g.vx - p.vx * 0.3) + nx * gs * 0.2 - ny * gs * 0.12 * g.swirl;
                let ay = (g.vy - p.vy * 0.3) + ny * gs * 0.2 + nx * gs * 0.12 * g.swirl;
                p.vx += ax * a;
                if p.kind != K_RAIN {
                    p.vy += ay * a;
                }
                if p.kind == K_LEAF {
                    p.rotv = (p.rotv + g.swirl * gs / s * f * dt * 0.7).clamp(-500.0, 500.0);
                    p.rest = p.rest.max(0.6);
                    if f > 0.5 && p.mode != TUMBLE && self.rand() < dt * 1.5 {
                        // caught properly by the gust: it tumbles for a moment
                        p.mode = TUMBLE;
                        p.freq2 = self.range(4.0, 9.0) * g.swirl;
                        p.mode_time = self.range(0.8, 2.0);
                    }
                }
            }
            if touched {
                self.parts[i] = p;
            }
        }
        self.gusts = gusts;
    }

    // ------------------------------------------------------------ snow settling

    // Lying snow never just fills the space up: the deeper it gets, the faster it packs down and sublimates,
    // at a rate that wanders slowly along the surface so drifts grow in some places and shrink in others.
    // Now and then a stretch of the taskbar drift is blown away over a few seconds, lifting a few flakes.
    fn settle_snow(&mut self, c: &mut SnowCap, left: f32, is_ground: bool, dt: f32) {
        let n = c.height.len();
        if n == 0 || c.max <= 0.0 {
            return;
        }

        if is_ground {
            // start a new blow-away now and then, somewhere there is a fair bit of snow
            if self.snow_gusts.len() < 3 && self.rand() < dt * 0.25 {
                let j = (self.rand() * n as f32) as usize % n;
                if c.height[j] > c.max * 0.25 {
                    let x = (j as f32 + 0.5) * B;
                    let w = self.range(120.0, 320.0) * self.scale_at(x);
                    let strength = self.range(1.5, 3.0);
                    let life = self.range(3.0, 6.0);
                    self.snow_gusts.push(SnowGust { x, w, strength, life, age: 0.0 });
                }
            }
            let mut i = self.snow_gusts.len();
            while i > 0 {
                i -= 1;
                self.snow_gusts[i].age += dt;
                if self.snow_gusts[i].age >= self.snow_gusts[i].life {
                    self.snow_gusts.swap_remove(i);
                }
            }
        }

        for j in 0..n {
            let h = c.height[j];
            if h <= 0.0 {
                continue;
            }
            let x = left + (j as f32 + 0.5) * B;
            let s = self.scale_at(x);
            let r = h / c.max;
            let time = self.time;
            // slowly drifting unevenness, 0.3..1.5
            let wander = 0.9 + 0.6 * (x * 0.0040 / s + time * 0.07).sin() * (x * 0.0013 / s - time * 0.05 + 1.7).sin();
            let mut loss = 2.0 * s * wander * r * r;
            if is_ground {
                for gi in 0..self.snow_gusts.len() {
                    let g = self.snow_gusts[gi];
                    let d = (x - g.x).abs() / (g.w * 0.5);
                    if d >= 1.0 {
                        continue;
                    }
                    let env = (PI * g.age / g.life).sin(); // builds up and dies down
                    let shape = 0.5 + 0.5 * (PI * d).cos();
                    loss += g.strength * s * shape * env * (0.3 + r);
                    // a few flakes lifted off the top
                    if self.rand() < dt * 2.0 * shape * env && self.parts.len() + self.pending.len() < MAX_PARTICLES {
                        let mut p = self.new_snow(0.0);
                        p.x = x;
                        p.y = self.ground_at(x) - c.mud[j] - h - 1.0;
                        p.vx = self.range(-40.0, 40.0) * s + self.mix.wind_px * s * 0.5;
                        p.vy = -self.range(30.0, 110.0) * s;
                        self.pending.push(p);
                    }
                }
            }
            c.height[j] = (h - loss * dt).max(0.0);
        }
    }

    // Snow on a window's top edge creeps slowly towards the corners, faster the deeper it lies. At each corner
    // it builds a small overhang; once that gets heavy enough it breaks off and drops as a clump of flakes that
    // spreads out as it falls.
    fn slide_snow_off_edges(&mut self, c: &mut SnowCap, w: &Win, dt: f32) {
        let n = c.height.len() as i32;
        if n < 4 {
            return;
        }
        let reach = (n / 2).min(12); // buckets from each end that slide outwards
        for side in 0..2 {
            let mut cornice = if side == 1 { c.cornice_r } else { c.cornice_l };
            for k in (0..reach).rev() {
                let j = if side == 1 { n - 1 - k } else { k };
                let out = if side == 1 { j + 1 } else { j - 1 };
                let h = c.height[j as usize];
                if h <= 0.0 {
                    continue;
                }
                let m = h * (dt * 0.35 * (0.2 + h / c.max)).min(1.0);
                c.height[j as usize] -= m;
                if out < 0 || out >= n {
                    cornice += m; // over the edge: onto the overhang
                } else {
                    c.height[out as usize] += m;
                }
            }

            let s = self.scale_at(if side == 1 { w.r } else { w.l });
            if cornice > 6.0 * s && self.rand() < dt * (cornice / (6.0 * s) - 0.8) {
                // the overhang breaks off: a clump that falls together and slowly comes apart
                let count = 30.min(3 + (cornice / (1.2 * s)) as i32);
                let dir = if side == 1 { 1.0 } else { -1.0 };
                let cx = (if side == 1 { w.r } else { w.l }) + dir * 3.0 * s;
                let cy = w.t - 2.0 * s;
                let vx = dir * self.range(15.0, 50.0) * s;
                let vy = self.range(10.0, 50.0) * s;
                for _ in 0..count {
                    if self.parts.len() + self.pending.len() >= MAX_PARTICLES {
                        break;
                    }
                    let mut p = self.new_snow(0.0);
                    p.x = cx + self.range(-4.0, 4.0) * s;
                    p.y = cy + self.range(-3.0, 3.0) * s;
                    p.vx = vx + self.range(-12.0, 12.0) * s;
                    p.vy = vy + self.range(-10.0, 10.0) * s;
                    p.size *= self.range(1.0, 1.5);
                    self.pending.push(p);
                }
                cornice = 0.0;
            }
            cornice = cornice.min(14.0 * s);
            if side == 1 {
                c.cornice_r = cornice;
            } else {
                c.cornice_l = cornice;
            }
        }
    }
}

/// Puts a resting leaf into the heap on `c` (whose left edge is at `left`).
pub(crate) fn join_heap(p: &mut Particle, c: &mut SnowCap, left: f32, depth: f32, add_volume: bool) {
    let n = c.pile.len() as i32;
    if n == 0 {
        return;
    }
    // swept leaves build the heap; a leaf that merely lands on one lies on top without reshaping it
    let vol = if add_volume { p.size * p.size * 0.06 } else { 0.0 };
    let rel = p.x - left;
    let half = p.size * 0.4;
    let a = (((rel - half) / B) as i32).max(0);
    let b = (((rel + half) / B) as i32).min(n - 1).max(a);
    let h = vol / ((b - a + 1) as f32 * B);
    for j in a..=b {
        c.pile[j as usize] += h;
    }
    p.heaped = true;
    p.vy = depth;
    p.heap_vol = vol;
    p.heap_vol0 = vol;
    // remember its place in the heap's mass, so it moves along when the heap is swept or slumps
    let (mut total, mut before) = (0.0, 0.0);
    for j in 0..n as usize {
        total += c.pile[j];
        let jf = j as f32;
        if (jf + 1.0) * B <= rel {
            before += c.pile[j];
        } else if jf * B < rel {
            before += c.pile[j] * (rel - jf * B) / B;
        }
    }
    p.mass_frac = if total > 0.0 { before / total } else { 0.5 };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::{RawWindow, Rect};

    fn world() -> (Sim, Tracker) {
        let mut sim = Sim::default();
        sim.reset(1920, 1080, 1.0);
        let r = Rect { l: 0.0, t: 0.0, r: 1920.0, b: 1080.0 };
        sim.set_ground(&[Rect { b: 1040.0, ..r }], &[r]);
        let mut tr = Tracker::default();
        let win = RawWindow { id: 7, rect: Rect { l: 400.0, t: 300.0, r: 1200.0, b: 800.0 }, maximized: false, foreground: true };
        tr.update(&[win], (0.0, 0.0), true, &[r]);
        (sim, tr)
    }

    #[test]
    fn rain_makes_puddles_on_the_ground_and_drips_off_windows() {
        let (mut sim, tr) = world();
        let mix = EffectMix { rain: 1.5, temp_c: 10.0, ..EffectMix::none() };
        for _ in 0..600 {
            sim.step(1.0 / 30.0, &tr, mix, CursorWind::default());
        }
        assert!(sim.ground.any_water(), "the taskbar should collect water");
        assert!(sim.caps.iter().any(|c| c.h == 7), "the window top should get a layer");
        assert!(sim.parts.len() <= MAX_PARTICLES);
    }

    #[test]
    fn leaves_land_and_rest() {
        let (mut sim, tr) = world();
        let mix = EffectMix { leaves: 3.0, temp_c: 10.0, ..EffectMix::none() };
        for _ in 0..900 {
            sim.step(1.0 / 30.0, &tr, mix, CursorWind::default());
        }
        assert!(sim.parts.iter().any(|p| p.kind == K_LEAF && p.state == RESTING && p.on_ground));
        assert!(sim.parts.iter().any(|p| p.kind == K_LEAF && p.state == RESTING && p.host == 7));
        assert_eq!(sim.rest_order.len(), sim.parts.iter().filter(|p| p.kind == K_LEAF && p.state != FALLING).count());
    }

    #[test]
    fn snow_builds_drifts_and_the_pointer_sweeps_heaps() {
        let (mut sim, tr) = world();
        let mix = EffectMix { snow: 1.6, leaves: 2.0, temp_c: -3.0, ..EffectMix::none() };
        for _ in 0..900 {
            sim.step(1.0 / 30.0, &tr, mix, CursorWind::default());
        }
        assert!(sim.ground.any_snow());
        // sweep along the ground
        let mut x = 100.0;
        for _ in 0..60 {
            let c = CursorWind { x0: x, y0: 1030.0, x1: x + 40.0, y1: 1030.0, vx: 1200.0, vy: 0.0, valid: true };
            sim.step(1.0 / 30.0, &tr, mix, c);
            x += 40.0;
        }
        assert!(!sim.idle());
    }

    #[test]
    fn every_visitor_can_be_summoned_and_leaves_eventually() {
        let (mut sim, tr) = world();
        for k in 0..CRITTER_KINDS as i32 {
            sim.spawn_critter(k, true, Some(&tr));
        }
        assert_eq!(sim.critters.len(), CRITTER_KINDS);
        let mix = EffectMix { temp_c: 10.0, ..EffectMix::none() };
        for _ in 0..(30 * 400) {
            sim.step(1.0 / 30.0, &tr, mix, CursorWind::default());
        }
        sim.rare_events = false;
        assert!(sim.critters.len() < CRITTER_KINDS, "visitors should wander off or wilt");
    }
}
