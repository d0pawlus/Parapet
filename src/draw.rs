//! Turns the simulation into one list of triangles, in draw order: the surface layers (snow, mud, water), the
//! visitors, then every particle as a sprite from the atlas, then the lightning flash. Coordinates are overlay
//! desktop units; the GPU maps them to each monitor's surface.
//!
//! Vertices carry a mode for the fragment shader: textured atlas sprites, or flat shapes (ellipses, lines,
//! polygons) whose edges are antialiased analytically, so everything goes out in a single draw call.

use crate::atlas::*;
use crate::sim::*;
use crate::tracker::Tracker;
use bytemuck::{Pod, Zeroable};
use std::f32::consts::PI;

pub const MODE_TEXTURE: f32 = 0.0;
pub const MODE_ELLIPSE: f32 = 1.0;
pub const MODE_RECT: f32 = 2.0;
pub const MODE_EDGE: f32 = 3.0;
pub const MODE_SOLID: f32 = 4.0;
const NO_CLIP: f32 = 1e9;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Debug)]
pub struct Vertex {
    pub pos: [f32; 2],
    pub uv: [f32; 2],
    pub color: [f32; 4], // straight alpha; the shader premultiplies
    pub extra: [f32; 2], // mode, clip y (fragments below it are dropped)
}

pub type Color = [f32; 4];

/// A 2D affine transform with Direct2D's conventions: row vectors, `a.then(b)` applies `a` first, and
/// positive angles turn clockwise on screen (y points down).
#[derive(Clone, Copy, Debug)]
pub struct Mat {
    pub m11: f32,
    pub m12: f32,
    pub m21: f32,
    pub m22: f32,
    pub dx: f32,
    pub dy: f32,
}

impl Mat {
    pub const IDENTITY: Mat = Mat { m11: 1.0, m12: 0.0, m21: 0.0, m22: 1.0, dx: 0.0, dy: 0.0 };

    pub fn scale(sx: f32, sy: f32) -> Mat {
        Mat { m11: sx, m22: sy, ..Self::IDENTITY }
    }
    pub fn translation(x: f32, y: f32) -> Mat {
        Mat { dx: x, dy: y, ..Self::IDENTITY }
    }
    pub fn rotation(deg: f32) -> Mat {
        let (s, c) = deg.to_radians().sin_cos();
        Mat { m11: c, m12: s, m21: -s, m22: c, dx: 0.0, dy: 0.0 }
    }
    pub fn rotation_about(deg: f32, px: f32, py: f32) -> Mat {
        Mat::translation(-px, -py).then(&Mat::rotation(deg)).then(&Mat::translation(px, py))
    }
    /// Scale, rotate, then move: how a sprite is placed.
    pub fn place(sx: f32, sy: f32, deg: f32, x: f32, y: f32) -> Mat {
        let (s, c) = deg.to_radians().sin_cos();
        Mat { m11: sx * c, m12: sx * s, m21: -sy * s, m22: sy * c, dx: x, dy: y }
    }
    pub fn then(&self, b: &Mat) -> Mat {
        let a = self;
        Mat {
            m11: a.m11 * b.m11 + a.m12 * b.m21,
            m12: a.m11 * b.m12 + a.m12 * b.m22,
            m21: a.m21 * b.m11 + a.m22 * b.m21,
            m22: a.m21 * b.m12 + a.m22 * b.m22,
            dx: a.dx * b.m11 + a.dy * b.m21 + b.dx,
            dy: a.dx * b.m12 + a.dy * b.m22 + b.dy,
        }
    }
    pub fn apply(&self, x: f32, y: f32) -> [f32; 2] {
        [x * self.m11 + y * self.m21 + self.dx, x * self.m12 + y * self.m22 + self.dy]
    }
    fn unit_scale(&self) -> f32 {
        (self.m11 * self.m22 - self.m12 * self.m21).abs().sqrt().max(1e-6)
    }
}

pub struct Geo {
    pub verts: Vec<Vertex>,
    /// Size of one output pixel in desktop units (for antialiasing margins and choosing leaf mips).
    pub px: f32,
    clip: f32,
}

impl Geo {
    pub fn new() -> Self {
        Self { verts: Vec::with_capacity(1 << 16), px: 1.0, clip: NO_CLIP }
    }

    fn v(&mut self, pos: [f32; 2], uv: [f32; 2], color: Color, mode: f32) {
        self.verts.push(Vertex { pos, uv, color, extra: [mode, self.clip] });
    }

    fn quad(&mut self, p: [[f32; 2]; 4], uv: [[f32; 2]; 4], color: Color, mode: f32) {
        // p: top-left, top-right, bottom-right, bottom-left
        for i in [0, 1, 2, 0, 2, 3] {
            self.v(p[i], uv[i], color, mode);
        }
    }

    /// An atlas sprite: `dest` (l, t, r, b) in the sprite's local space, placed by `m`, tinted by `color`.
    pub fn sprite(&mut self, dest: [f32; 4], src: [u32; 4], color: Color, m: &Mat) {
        let (aw, ah) = (ATLAS_W as f32, ATLAS_H as f32);
        let (u0, v0, u1, v1) = (src[0] as f32 / aw, src[1] as f32 / ah, src[2] as f32 / aw, src[3] as f32 / ah);
        let [l, t, r, b] = dest;
        self.quad(
            [m.apply(l, t), m.apply(r, t), m.apply(r, b), m.apply(l, b)],
            [[u0, v0], [u1, v0], [u1, v1], [u0, v1]],
            color,
            MODE_TEXTURE,
        );
    }

    pub fn ellipse(&mut self, m: &Mat, cx: f32, cy: f32, rx: f32, ry: f32, color: Color) {
        if rx <= 0.0 || ry <= 0.0 || color[3] <= 0.0 {
            return;
        }
        let local_px = self.px / m.unit_scale();
        let (ex, ey) = (1.0 + 1.5 * local_px / rx, 1.0 + 1.5 * local_px / ry);
        let (x0, x1, y0, y1) = (cx - rx * ex, cx + rx * ex, cy - ry * ey, cy + ry * ey);
        self.quad(
            [m.apply(x0, y0), m.apply(x1, y0), m.apply(x1, y1), m.apply(x0, y1)],
            [[-ex, -ey], [ex, -ey], [ex, ey], [-ex, ey]],
            color,
            MODE_ELLIPSE,
        );
    }

    /// A straight line with flat ends, `w` wide (in local units, like Direct2D's DrawLine).
    #[allow(clippy::too_many_arguments)]
    pub fn line(&mut self, m: &Mat, x0: f32, y0: f32, x1: f32, y1: f32, w: f32, color: Color) {
        let (dx, dy) = (x1 - x0, y1 - y0);
        let len = (dx * dx + dy * dy).sqrt();
        if len < 1e-4 || w <= 0.0 || color[3] <= 0.0 {
            return;
        }
        let (ux, uy) = (dx / len, dy / len);
        let (nx, ny) = (-uy, ux);
        let e = 1.0 * self.px / m.unit_scale();
        let (hl, hw) = (len / 2.0, w / 2.0);
        let (mx, my) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
        let (a, b) = (hl + e, hw + e);
        let corner = |sa: f32, sb: f32| m.apply(mx + ux * a * sa + nx * b * sb, my + uy * a * sa + ny * b * sb);
        let (ua, vb) = (a / hl, b / hw);
        self.quad(
            [corner(-1.0, -1.0), corner(1.0, -1.0), corner(1.0, 1.0), corner(-1.0, 1.0)],
            [[-ua, -vb], [ua, -vb], [ua, vb], [-ua, vb]],
            color,
            MODE_RECT,
        );
    }

    /// A filled polygon (convex, or close to it), fanned out from its centre.
    pub fn poly(&mut self, m: &Mat, pts: &[(f32, f32)], color: Color) {
        if pts.len() < 3 || color[3] <= 0.0 {
            return;
        }
        let n = pts.len() as f32;
        let cx = pts.iter().map(|p| p.0).sum::<f32>() / n;
        let cy = pts.iter().map(|p| p.1).sum::<f32>() / n;
        let c = m.apply(cx, cy);
        for i in 0..pts.len() {
            let a = pts[i];
            let b = pts[(i + 1) % pts.len()];
            self.v(c, [1.0, 0.0], color, MODE_EDGE);
            self.v(m.apply(a.0, a.1), [0.0, 0.0], color, MODE_EDGE);
            self.v(m.apply(b.0, b.1), [0.0, 0.0], color, MODE_EDGE);
        }
    }

    pub fn rect(&mut self, l: f32, t: f32, r: f32, b: f32, color: Color) {
        self.quad([[l, t], [r, t], [r, b], [l, b]], [[0.0; 2]; 4], color, MODE_SOLID);
    }
}

fn rgba(r: f32, g: f32, b: f32, a: f32) -> Color {
    [r, g, b, a]
}

// ---------------------------------------------------------------- the frame

pub fn build_frame(geo: &mut Geo, sim: &Sim, tr: &Tracker, shadows: bool) {
    geo.verts.clear();
    draw_caps(geo, sim, tr);
    draw_critters(geo, sim, tr);
    build_sprites(geo, sim, tr, shadows);
    if sim.flash > 0.0 {
        geo.rect(0.0, 0.0, sim.w as f32, sim.h as f32, rgba(0.85, 0.9, 1.0, sim.flash * 0.28));
    }
}

fn build_sprites(geo: &mut Geo, sim: &Sim, tr: &Tracker, shadows: bool) {
    let s_glob = sim.s;
    let dot = [DOT_X, 0, DOT_X + DOT_SIZE, DOT_SIZE];
    let flake = [DOT_X, FLAKE_Y, DOT_X + DOT_SIZE, FLAKE_Y + DOT_SIZE];
    let ring = [DOT_X, RING_Y, DOT_X + DOT_SIZE, RING_Y + DOT_SIZE];
    let streak = [STREAK_X, 0, STREAK_X + STREAK_W, STREAK_H];

    // snow overhanging window corners, about to break off
    for c in &sim.caps {
        let Some(w) = tr.get(c.h).filter(|w| w.surface) else { continue };
        for side in 0..2 {
            let mass = if side == 1 { c.cornice_r } else { c.cornice_l };
            let edge = if side == 1 { w.r } else { w.l };
            let s = sim.scale_at(edge);
            if mass < 0.5 * s {
                continue;
            }
            let dir = if side == 1 { 1.0 } else { -1.0 };
            let r = (7.0 * s).min(1.5 * s + mass * 0.45);
            let (x, y) = (edge + dir * r * 0.35, w.t - r * 0.35);
            if tr.covered(x, y, w.z, false) {
                continue;
            }
            let d = r * 2.2;
            geo.sprite([-d / 2.0, -d / 2.0, d / 2.0, d / 2.0], flake, rgba(0.99, 0.98, 0.96, 0.97), &Mat::place(1.3, 0.8, 0.0, x, y));
        }
    }

    // drops swelling at window corners, about to fall off like from a gutter
    for c in &sim.caps {
        let Some(w) = tr.get(c.h).filter(|w| w.surface) else { continue };
        for side in 0..2 {
            let bead = if side == 1 { c.bead_r } else { c.bead_l };
            if bead < 0.08 * s_glob {
                continue;
            }
            let x = if side == 1 { w.r + 1.5 * s_glob } else { w.l - 1.5 * s_glob };
            if tr.covered(x, w.t + 2.0, w.z, false) {
                continue;
            }
            let d = (2.5 + 4.5 * bead / (0.5 * s_glob)) * s_glob;
            geo.sprite([-d / 2.0, -d / 2.0, d / 2.0, d / 2.0], dot, rgba(0.80, 0.88, 0.98, 0.85), &Mat::place(0.8, 1.1, 0.0, x, w.t + d / 2.0));
        }
    }

    // a leaf's pose: size, squash, y and turn
    let leaf_pose = |p: &Particle| -> (f32, f32, f32, f32, f32) {
        let s = p.size;
        let mut sx = 1.0;
        let sy;
        let mut y = p.y;
        let mut rot = p.rot;
        if p.state == FALLING {
            // turning in 3D: squash along the leaf axis, never quite edge-on
            sy = if p.flip.abs() < 0.2 { if p.flip < 0.0 { -0.2 } else { 0.2 } } else { p.flip };
        } else {
            // settling down: it touched with its edge, now flops flat with a soft ease-out, the turn
            // overshooting a hair and swinging back
            let t = p.settle;
            let e = 1.0 - (1.0 - t) * (1.0 - t) * (1.0 - t);
            let back = 1.0 + 2.2 * (t - 1.0).powi(3) + 1.2 * (t - 1.0).powi(2);
            let d = (p.rest_rot - p.start_rot + 540.0) % 360.0 - 180.0;
            rot = p.start_rot + d * back;
            sx = 1.0 + (0.45 - 1.0) * e;
            sy = p.start_flip + (1.0 - p.start_flip) * e;
            y -= p.size * (0.22 + (0.05 - 0.22) * e);
        }
        (s, sx, sy, y, rot)
    };
    // smallest mip cell that is still at least as large as the leaf on screen
    let px = geo.px;
    let level = |s: f32| -> u32 {
        let on_screen = s / px;
        let mut l = 0;
        while l + 1 < LEVELS && (BASE >> (l + 1)) as f32 >= on_screen {
            l += 1;
        }
        l
    };

    if shadows {
        // soft drop shadows first, from a coarser mip so they come out blurred for free
        for p in &sim.parts {
            if p.kind != K_LEAF || p.hidden || p.alpha <= 0.0 {
                continue;
            }
            let (s, sx, sy, y, rot) = leaf_pose(p);
            let lift = (if p.state == FALLING { 9.0 } else { 2.5 }) * s_glob * p.depth;
            let h = s * 0.54;
            geo.sprite(
                [-h, -h, h, h],
                leaf_cell(p.shape as u32, p.color as u32, (level(s) + 2).min(LEVELS - 1)),
                rgba(0.0, 0.0, 0.0, 0.22 * p.alpha),
                &Mat::place(sx, sy, rot, p.x + lift * 0.5, y + lift),
            );
        }
    }

    let add_leaf = |geo: &mut Geo, p: &Particle| {
        let (s, sx, sy, y, rot) = leaf_pose(p);
        geo.sprite(
            [-s / 2.0, -s / 2.0, s / 2.0, s / 2.0],
            leaf_cell(p.shape as u32, p.color as u32, level(s)),
            rgba(1.0, 1.0, 1.0, p.alpha),
            &Mat::place(sx, sy, rot, p.x, y),
        );
    };
    // resting leaves bottom to top, so heaps overlap the right way round
    for &i in &sim.rest_order {
        let p = &sim.parts[i];
        if !p.hidden && p.alpha > 0.0 {
            add_leaf(geo, p);
        }
    }

    for p in &sim.parts {
        match p.kind {
            K_LEAF => {
                if p.state == FALLING && !p.hidden && p.alpha > 0.0 {
                    add_leaf(geo, p);
                }
            }
            K_RAIN => {
                if p.hidden {
                    continue;
                }
                if p.mode == 2 {
                    // a drop running down a window's side: a short glossy bead elongated along the edge
                    let (len, w) = (9.0 * s_glob, 2.6 * s_glob);
                    geo.sprite(
                        [-w / 2.0, -len / 2.0, w / 2.0, len / 2.0],
                        streak,
                        rgba(0.80, 0.88, 0.98, 0.85),
                        &Mat::place(1.0, 1.0, 0.0, p.x, p.y - 4.5 * s_glob),
                    );
                    continue;
                }
                let v = (p.vx * p.vx + p.vy * p.vy).sqrt().max(1.0);
                let len = (4.0 * s_glob).max(v * 0.022);
                let w = (1.2 + p.depth) * 1.6 * s_glob;
                let (cx, cy) = (p.x - p.vx * 0.011, p.y - p.vy * 0.011); // centre: half a streak behind the head
                geo.sprite(
                    [-w / 2.0, -len / 2.0, w / 2.0, len / 2.0],
                    streak,
                    rgba(0.70, 0.78, 0.90, 0.30 + 0.40 * p.depth),
                    &Mat::place(1.0, 1.0, -p.vx.atan2(p.vy).to_degrees(), cx, cy),
                );
            }
            K_SNOW | K_SPLASH => {
                let snow = p.kind == K_SNOW;
                let d = p.size * 2.0;
                let fade = if snow { 1.0 } else { 1.0 - p.age / p.life };
                let color = if snow {
                    rgba(1.0, 0.985, 0.96, 0.45 + 0.5 * p.depth)
                } else {
                    match p.color {
                        1 => rgba(0.40, 0.28, 0.16, 0.95 * fade),
                        2 => rgba(0.82, 0.52, 0.20, 0.95 * fade),
                        _ => rgba(0.75, 0.84, 0.95, 0.8 * fade),
                    }
                };
                geo.sprite([-d / 2.0, -d / 2.0, d / 2.0, d / 2.0], if snow { flake } else { dot }, color, &Mat::translation(p.x, p.y));
            }
            K_RIPPLE => {
                let t = p.age / p.life;
                let d = (3.0 + 22.0 * t) * s_glob * p.depth;
                // seen from the side: a flat ellipse
                geo.sprite([-d / 2.0, -d / 2.0, d / 2.0, d / 2.0], ring, rgba(0.85, 0.92, 1.0, 0.7 * (1.0 - t)), &Mat::place(1.0, 0.3, 0.0, p.x, p.y));
            }
            _ => {}
        }
    }
}

// ---------------------------------------------------------------- surface layers

#[derive(Clone, Copy)]
struct Pt3 {
    x: f32,
    t: f32,
    b: f32,
}

fn draw_caps(geo: &mut Geo, sim: &Sim, tr: &Tracker) {
    let bk = BUCKET as f32;
    // an 8x8 solid white patch from the middle of the snowflake disc, and the soft dot for rims
    let solid = [DOT_X + 28, FLAKE_Y + 28, DOT_X + 36, FLAKE_Y + 36];
    let dot = [DOT_X, 0, DOT_X + DOT_SIZE, DOT_SIZE];
    let fade = 1.3 * geo.px.max(1.0); // width of the soft edge
    let mut run: Vec<Pt3> = Vec::new();

    // One layer of a surface, between bottom(j) and top(j) per bucket, drawn per visible run as a polyline
    // surface: between neighbouring points a solid body plus a skewed cap whose top follows the slope and
    // fades out over a pixel or so, then a soft rim line along the top.
    let mut layer = |geo: &mut Geo,
                     n: usize,
                     w: Option<&crate::tracker::Win>,
                     bottom: &dyn Fn(usize) -> f32,
                     top: &dyn Fn(usize) -> f32,
                     fill: Color,
                     edge: Color,
                     edge_width: f32| {
        let left = w.map(|w| w.l).unwrap_or(0.0);
        let z = w.map(|w| w.z).unwrap_or(i32::MAX);
        let visible = |j: usize| {
            let (b, t) = (bottom(j), top(j));
            b - t > 0.3 && !tr.covered(left + j as f32 * bk + bk / 2.0, (b + t) * 0.5 - 1.0, z, w.is_none())
        };
        let mut i = 0;
        while i < n {
            if !visible(i) {
                i += 1;
                continue;
            }
            let a = i;
            while i < n && visible(i) {
                i += 1;
            }
            // the run tapers to nothing at both ends
            run.clear();
            run.push(Pt3 { x: left + a as f32 * bk, t: bottom(a), b: bottom(a) });
            for j in a..i {
                run.push(Pt3 { x: left + j as f32 * bk + bk / 2.0, t: top(j), b: bottom(j) });
            }
            run.push(Pt3 { x: left + i as f32 * bk, t: bottom(i - 1), b: bottom(i - 1) });

            for s in 0..run.len() - 1 {
                let (p, q) = (run[s], run[s + 1]);
                let lower = p.t.max(q.t);
                let floor = p.b.max(q.b);
                if floor - lower - 1.0 > 0.01 {
                    // starts where the cap ends, so translucent water isn't doubled up along the seam
                    geo.sprite([p.x, lower + 1.0, q.x, floor], solid, fill, &Mat::IDENTITY);
                }
                // the cap: a parallelogram from the sloping top line down past the body's top edge
                let cap_h = (p.t - q.t).abs() + fade + 1.0;
                let rows = (SOFT_FADE as f32 * cap_h / fade).clamp(SOFT_FADE as f32, SOFT_H as f32) as u32;
                let m = Mat { m11: q.x - p.x, m12: q.t - p.t, m21: 0.0, m22: cap_h, dx: p.x, dy: p.t - fade };
                geo.sprite([0.0, 0.0, 1.0, 1.0], [DOT_X, SOFT_Y, DOT_X + SOFT_W, SOFT_Y + rows], fill, &m);
                // rim along the top, a soft-ended line
                let (dx, dy) = (q.x - p.x, q.t - p.t);
                let len = (dx * dx + dy * dy).sqrt();
                if len > 0.01 && s > 0 && s + 2 < run.len() {
                    let ew = edge_width * 2.2;
                    geo.sprite(
                        [-ew * 0.3, -ew / 2.0, len + ew * 0.3, ew / 2.0],
                        dot,
                        edge,
                        &Mat::place(1.0, 1.0, dy.atan2(dx).to_degrees(), p.x, p.t - fade * 0.5),
                    );
                }
            }
        }
    };

    let mut surface = |geo: &mut Geo, c: &SnowCap, w: Option<&crate::tracker::Win>| {
        let (snow, water, mud) = (c.any_snow(), c.any_water(), c.any_mud());
        if !snow && !water && !mud {
            return;
        }
        let n = c.height.len();
        let gy = &sim.ground_y;
        let base = |j: usize| w.map(|w| w.t).unwrap_or_else(|| gy[j.min(gy.len() - 1)]);
        let mud_top = |j: usize| base(j) - c.mud[j];
        let snow_top = |j: usize| mud_top(j) - c.height[j];
        let water_top = |j: usize| {
            let wv = c.wave[j] * (c.water[j] / (2.0 * sim.s)).min(1.0);
            snow_top(j) - c.water[j] - wv
        };
        if mud {
            layer(geo, n, w, &base, &mud_top, rgba(0.36, 0.25, 0.15, 0.94), rgba(0.24, 0.16, 0.09, 0.8), 1.0);
        }
        if snow {
            layer(geo, n, w, &mud_top, &snow_top, rgba(0.99, 0.98, 0.96, 0.97), rgba(0.80, 0.80, 0.84, 0.45), 0.9);
        }
        if water {
            layer(geo, n, w, &snow_top, &water_top, rgba(0.40, 0.55, 0.70, 0.50), rgba(0.92, 0.96, 1.0, 0.65), 1.0);
        }
    };

    for c in &sim.caps {
        if let Some(w) = tr.get(c.h).filter(|w| w.surface) {
            surface(geo, c, Some(w));
        }
    }
    surface(geo, &sim.ground, None);
}

// ---------------------------------------------------------------- visitors

const LEAF_FLAT: [Color; LEAF_COLORS] = [
    [0.77, 0.28, 0.12, 1.0],
    [0.87, 0.47, 0.12, 1.0],
    [0.92, 0.67, 0.16, 1.0],
    [0.63, 0.16, 0.12, 1.0],
    [0.80, 0.59, 0.24, 1.0],
    [0.55, 0.35, 0.17, 1.0],
    [0.90, 0.75, 0.27, 1.0],
    [0.71, 0.39, 0.16, 1.0],
];

/// Draws with a current transform, like a Direct2D device context.
struct Pen<'a> {
    geo: &'a mut Geo,
    m: Mat,
}

impl Pen<'_> {
    fn fill(&mut self, cx: f32, cy: f32, rx: f32, ry: f32, c: Color) {
        let m = self.m;
        self.geo.ellipse(&m, cx, cy, rx, ry, c);
    }
    fn stroke(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, w: f32, c: Color) {
        let m = self.m;
        self.geo.line(&m, x0, y0, x1, y1, w, c);
    }
    fn poly(&mut self, pts: &[(f32, f32)], c: Color) {
        let m = self.m;
        self.geo.poly(&m, pts, c);
    }
}

// Each visitor is drawn in its own local space: 1 unit = 1 px at 1080p, origin at its feet, facing +x, up is -y.
// Flat warm colours, a soft shadow underneath, eyes with a highlight that blink.
fn draw_critters(geo: &mut Geo, sim: &Sim, _tr: &Tracker) {
    const CS: f32 = 1.6; // visitors are drawn a bit larger than life so they read on the screen
    for c in &sim.critters {
        if c.hidden || c.alpha <= 0.01 {
            continue;
        }
        let s = sim.scale_at(c.x);
        let sc = s * CS;
        let a = c.alpha;
        let col = |r: f32, g: f32, b: f32, al: f32| -> Color { [r, g, b, al * a] };
        let cc = |r: f32, g: f32, b: f32| -> Color { [r, g, b, a] };
        // facing eases through zero when turning round, which reads as the creature turning
        let face = if c.face.abs() < 0.15 { if c.face < 0.0 { -0.15 } else { 0.15 } } else { c.face };
        let look_fwd = c.look * if c.face < 0.0 { -1.0 } else { 1.0 }; // pointer in front (+) or behind (-)
        let place = |dy: f32, angle: f32, sy: f32, dx: f32| -> Mat {
            Mat::scale(face * sc, sc * sy)
                .then(&Mat::rotation(angle * if face < 0.0 { -1.0 } else { 1.0 }))
                .then(&Mat::translation(c.x + dx * s * CS, c.y + dy * s * CS))
        };
        let mut t = place(0.0, c.angle, 1.0, 0.0);
        let mut pen = Pen { geo: &mut *geo, m: t };
        let eye = |pen: &mut Pen, x: f32, y: f32, r: f32, iris: Color| {
            if c.blink < 0.0 {
                pen.stroke(x - r, y + r * 0.2, x + r, y + r * 0.2, r * 0.55, iris);
                return;
            }
            pen.fill(x, y, r, r, iris);
            pen.fill(x - r * 0.35 + look_fwd * r * 0.15, y - r * 0.35, r * 0.38, r * 0.38, col(1.0, 1.0, 1.0, 0.9));
        };
        let shadow = |pen: &mut Pen, rx: f32| pen.fill(0.0, 0.6, rx, 2.2, col(0.1, 0.07, 0.05, 0.16));
        let breathe = 1.0 + 0.025 * (c.t * 3.1 + c.seed).sin();

        match c.kind {
            HEDGEHOG => {
                let spine = cc(0.42, 0.30, 0.20);
                let dark = cc(0.26, 0.18, 0.12);
                let tip = cc(0.74, 0.60, 0.42);
                let face_c = cc(0.88, 0.76, 0.60);
                let nose = cc(0.14, 0.09, 0.07);
                let blush = col(0.93, 0.58, 0.52, 0.5);
                let curl = c.grow;
                shadow(&mut pen, if curl > 0.5 { 13.0 } else { 17.0 });
                if curl > 0.5 {
                    // a spiky ball; trembles while the pointer hovers, peeks out with one eye when it is gone
                    let shake = (c.t * 60.0).sin() * c.aux2 * 0.8;
                    t = place(-c.squash * 1.5, 0.0, 1.0, shake);
                    pen.m = t;
                    pen.fill(0.0, -11.5, 12.5, 11.5, spine);
                    for i in 0..28 {
                        let ang = i as f32 * 2.0 * PI / 28.0 + c.seed;
                        let (sn, cs) = ang.sin_cos();
                        pen.stroke(cs * 9.5, -11.5 + sn * 9.5, cs * 15.5, -11.5 + sn * 15.5, 1.5, if i % 3 != 0 { dark } else { tip });
                    }
                    if curl < 0.9 {
                        pen.fill(9.0, -6.0, 4.5, 3.4, face_c);
                        pen.fill(13.0, -6.2, 1.4, 1.2, nose);
                        eye(&mut pen, 9.5, -8.2, 1.2, nose);
                    }
                } else {
                    let waddle = c.anim.sin() * 2.0;
                    t = place(0.0, waddle, breathe, 0.0);
                    pen.m = t;
                    for i in 0..4 {
                        let fi = i as f32;
                        let lx = -9.0 + fi * 6.0 + (c.anim + fi * 1.7).sin() * 1.5;
                        let lift = if c.state == 0 { (c.anim + fi * PI).sin().max(0.0) * 1.4 } else { 0.0 };
                        pen.fill(lx, -1.6 - lift, 2.3, 1.8, dark);
                    }
                    pen.fill(-1.0, -9.5, 16.5, 9.5, spine);
                    // three rows of spines sweeping backwards, darker at the root, pale at the tips
                    for row in 0..3 {
                        let fr = row as f32;
                        let (rx, ry) = (16.5 - fr * 3.8, 9.5 - fr * 2.6);
                        for i in 0..=13 {
                            let ang = PI * (1.02 + 0.86 * i as f32 / 13.0) + fr * 0.07;
                            let (bx, by) = (-1.0 + ang.cos() * rx, -9.5 + ang.sin() * ry);
                            let (ox, oy) = (ang.cos() - 0.7, ang.sin());
                            let l = (ox * ox + oy * oy).sqrt();
                            let len = 5.0 - fr * 0.8;
                            pen.stroke(bx, by, bx + ox / l * len, by + oy / l * len, 1.4, if (i + row) % 3 == 0 { tip } else { dark });
                        }
                    }
                    let sniff = if c.state == 1 { c.aux.sin() * 0.7 } else { 0.0 };
                    let head_low = if c.state == 1 { 1.2 } else { 0.0 };
                    pen.fill(13.0, -6.3 + head_low, 7.5, 5.2, face_c);
                    pen.fill(18.8 + sniff * 0.3, -5.8 + head_low, 3.4, 2.6, face_c);
                    pen.fill(21.8 + sniff, -6.1 + head_low, 1.9, 1.6, nose);
                    pen.fill(21.4 + sniff, -6.6 + head_low, 0.6, 0.5, col(1.0, 1.0, 1.0, 0.6));
                    eye(&mut pen, 15.3 + look_fwd * 0.4, -8.4 + head_low, 1.35, nose);
                    pen.fill(16.5, -5.0 + head_low, 1.7, 1.0, blush);
                    pen.fill(11.5, -11.2 + head_low, 2.1, 1.9, face_c);
                    pen.fill(11.6, -11.0 + head_low, 0.9, 0.8, cc(0.6, 0.45, 0.35));
                }
                // leaves speared on the spines
                const SPOTS: [[f32; 3]; 4] = [[-7.0, -17.0, 35.0], [0.0, -19.5, -20.0], [7.0, -16.5, 60.0], [-2.0, -14.0, 100.0]];
                for i in 0..(c.count.min(4) as usize) {
                    let lx = SPOTS[i][0] * if curl > 0.5 { 0.7 } else { 1.0 };
                    let ly = if curl > 0.5 { SPOTS[i][1] - 3.0 } else { SPOTS[i][1] };
                    pen.m = Mat::rotation_about(SPOTS[i][2], lx, ly).then(&t);
                    let mut lc = LEAF_FLAT[c.carried[i] as usize % LEAF_COLORS];
                    lc[3] = a;
                    let leaf = [(lx - 7.0, ly), (lx - 3.0, ly - 2.6), (lx + 3.0, ly - 2.4), (lx + 7.0, ly), (lx + 3.0, ly + 2.4), (lx - 3.0, ly + 2.6)];
                    pen.poly(&leaf, lc);
                    pen.stroke(lx - 6.0, ly, lx + 6.0, ly, 0.6, col(lc[0] * 0.7, lc[1] * 0.7, lc[2] * 0.7, 0.7));
                    pen.m = t;
                }
            }
            CATERPILLAR => {
                let body = cc(0.50, 0.68, 0.26);
                let light = cc(0.72, 0.85, 0.42);
                let spot = cc(0.95, 0.82, 0.34);
                let head = cc(0.40, 0.56, 0.18);
                let legs = cc(0.30, 0.42, 0.14);
                let dark = cc(0.08, 0.08, 0.05);
                const N: i32 = 9;
                if c.state == 2 {
                    // curled into a wobbling ring
                    shadow(&mut pen, 8.0);
                    let wob = c.aux.sin() * 0.8;
                    for i in (0..N).rev() {
                        let ang = -PI / 2.0 + i as f32 * 2.0 * PI / (N + 1) as f32;
                        let (px, py) = (ang.cos() * 6.5 + wob, -7.5 + ang.sin() * 6.5);
                        pen.fill(px, py, 3.3, 3.3, if i == 0 { head } else { body });
                        if i != 0 {
                            pen.fill(px - 0.6, py - 1.2, 1.4, 0.9, light);
                        }
                    }
                } else {
                    let arch = if c.state == 1 { 0.0 } else { (-c.anim.sin()).max(0.0) };
                    let len = 32.0 * (1.0 - 0.35 * arch);
                    let munch = if c.state == 1 { c.anim.sin() * 1.2 } else { 0.0 };
                    shadow(&mut pen, len * 0.55);
                    for i in (0..N).rev() {
                        let u = i as f32 / (N - 1) as f32;
                        let px = -u * len;
                        let mut py = -3.3 - arch * 12.0 * (PI * u).sin();
                        if i == 0 {
                            py += munch;
                        }
                        if py > -6.0 && i > 0 {
                            pen.stroke(px, py + 2.4, px + (c.anim * 3.0 + i as f32).sin() * 0.9, 0.0, 0.9, legs);
                        }
                        pen.fill(px, py, 3.5, 3.3, if i == 0 { head } else { body });
                        if i > 0 {
                            pen.fill(px - 0.5, py - 1.4, 1.6, 0.9, light);
                            if i % 2 == 1 {
                                pen.fill(px + 0.4, py + 0.6, 0.8, 0.8, spot);
                            }
                        }
                    }
                    let bob = (c.t * 4.0).sin() * 0.8;
                    pen.stroke(0.4, -6.0 + munch, 1.6, -9.5 + bob + munch, 0.7, head);
                    pen.stroke(-0.8, -6.0 + munch, -0.6, -9.8 - bob + munch, 0.7, head);
                    pen.fill(1.6, -9.5 + bob + munch, 0.8, 0.8, head);
                    pen.fill(-0.6, -9.8 - bob + munch, 0.8, 0.8, head);
                    eye(&mut pen, 1.6, -4.6 + munch, 0.9, dark);
                    pen.stroke(2.2, -2.4 + munch, 3.3, -2.9 + munch, 0.5, dark);
                }
            }
            SNOW_FOX => {
                let fur = cc(0.98, 0.97, 0.94);
                let shade = cc(0.76, 0.79, 0.86);
                let cream = cc(0.94, 0.91, 0.86);
                let dark = cc(0.17, 0.14, 0.14);
                let pink = cc(0.94, 0.76, 0.73);
                let partly = c.state == 0 || c.state == 3;
                let flying = c.state == 2;
                let crouch = if c.state == 4 { c.squash } else { 0.0 };
                let wiggle = crouch * (c.t * 28.0).sin() * 1.6;
                if !partly && !flying {
                    shadow(&mut pen, 20.0);
                }
                if partly {
                    // only the part above the snow shows while emerging or diving in
                    pen.geo.clip = sim.surface_y(c.x);
                    t = place((1.0 - c.grow) * 42.0, if c.state == 3 { 70.0 } else { 0.0 }, 1.0, 0.0);
                } else {
                    t = place(crouch * 3.0, c.angle, breathe - crouch * 0.12, 0.0);
                }
                pen.m = t;
                // every part over a slightly larger grey-blue silhouette, so white fur reads against snow
                let both = |pen: &mut Pen, cx: f32, cy: f32, rx: f32, ry: f32, color: Color| {
                    pen.fill(cx, cy, rx + 1.0, ry + 1.0, shade);
                    pen.fill(cx, cy, rx, ry, color);
                };
                let tail_lift = if flying { 4.0 } else { c.aux2 * 0.4 };
                both(&mut pen, -20.0 + wiggle, -16.0 - tail_lift * 0.3, 9.0, 6.5, fur);
                both(&mut pen, -28.0 + wiggle, -19.0 - tail_lift * 0.6, 7.0, 5.5, fur);
                both(&mut pen, -33.0 + wiggle, -21.0 - tail_lift, 4.5, 4.0, cream);
                for i in 0..4 {
                    let lx = -10.0 + i as f32 * 6.5;
                    let stretch = if flying { if i < 2 { -7.0 } else { 7.0 } } else { 0.0 };
                    let ly = if flying { -3.0 } else { -1.0 };
                    pen.stroke(lx, -10.0, lx + stretch, ly, 4.4, shade);
                    pen.stroke(lx, -10.0, lx + stretch, ly, 3.2, if i < 2 { cream } else { fur });
                    pen.fill(lx + stretch, ly + 0.4, 2.2, 1.3, fur);
                }
                both(&mut pen, -3.0 + wiggle * 0.5, -14.0, 15.0, 8.5, fur);
                both(&mut pen, 8.5, -13.0, 6.0, 6.5, cream); // chest fluff
                // the head, cocked while listening
                pen.m = Mat::rotation_about(if c.state == 1 { c.aux } else { 0.0 }, 11.0, -18.0).then(&t);
                let lx = look_fwd * 0.8;
                both(&mut pen, 10.0 + lx, -19.0, 5.0, 4.5, cream); // cheek ruff
                both(&mut pen, 13.0 + lx, -22.0, 7.0, 6.0, fur);
                let twitch = if c.aux2 > 0.5 { 1.5 } else { 0.0 };
                pen.poly(&[(9.0 + lx, -26.0), (10.5 + lx - twitch, -34.5), (14.0 + lx, -27.0)], shade);
                pen.poly(&[(14.0 + lx, -27.0), (17.0 + lx, -33.5), (18.5 + lx, -25.5)], shade);
                pen.poly(&[(10.6 + lx, -27.0), (11.2 + lx - twitch, -32.0), (13.0 + lx, -27.4)], pink);
                pen.poly(&[(15.0 + lx, -27.0), (16.8 + lx, -31.5), (17.4 + lx, -26.4)], pink);
                both(&mut pen, 19.5 + lx, -20.6, 5.0, 3.0, fur); // muzzle
                pen.fill(24.3 + lx, -21.0, 1.6, 1.4, dark);
                pen.fill(24.0 + lx, -21.5, 0.5, 0.4, col(1.0, 1.0, 1.0, 0.6));
                eye(&mut pen, 16.0 + lx * 1.2, -23.5, 1.4, dark);
                pen.stroke(20.0 + lx, -18.6, 23.0 + lx, -18.9, 0.6, cc(0.6, 0.6, 0.65));
                pen.m = t;
                pen.geo.clip = NO_CLIP;
            }
            SNOWDROPS => {
                let stem = cc(0.38, 0.56, 0.26);
                let leaf = cc(0.46, 0.63, 0.47);
                let fold = cc(0.36, 0.52, 0.38);
                let petal = cc(0.99, 0.98, 0.95);
                let petal_shade = cc(0.88, 0.90, 0.92);
                let tip = cc(0.50, 0.70, 0.30);
                for i in 0..c.count {
                    let fi = i as f32;
                    let alt = (i % 2) as f32;
                    let bx = (fi - (c.count - 1) as f32 / 2.0) * 7.0 + (c.seed + fi).sin() * 2.0;
                    let h = (24.0 + 6.0 * (c.seed * 3.0 + fi).sin()) * c.grow;
                    let sway = (c.t * (1.1 + fi * 0.17) + fi).sin() * 2.0;
                    let bend = (c.angle + sway + fi * 2.5 - (c.count - 1) as f32 * 1.2) * PI / 180.0;
                    let (mx, my) = (bx + (bend * 0.5).sin() * h * 0.5, -h * 0.5);
                    let (tx, ty) = (bx + bend.sin() * h, -bend.cos() * h);
                    // strap leaves with a fold down the middle
                    let lh = h * (0.55 + 0.1 * alt);
                    pen.poly(&[(bx - 2.0, 0.0), (bx - 4.0 - alt * 2.0, -lh * 0.6), (bx - 3.0 - alt * 2.0, -lh), (bx + 1.0, 0.0)], leaf);
                    pen.stroke(bx - 0.5, 0.0, bx - 3.0 - alt * 2.0, -lh * 0.95, 0.5, fold);
                    pen.stroke(bx, 0.0, mx, my, 1.4, stem);
                    pen.stroke(mx, my, tx, ty, 1.3, stem);
                    if c.grow > 0.5 {
                        // a closed bud first, then the nodding bell opens its three outer petals
                        let f = ((c.grow - 0.5) * 2.0).min(1.0);
                        let open = ((c.grow - 0.8) * 5.0).max(0.0);
                        let (hx, hy) = (tx + 3.0, ty + 1.0);
                        pen.stroke(tx, ty, hx, hy, 1.1, stem);
                        pen.fill(hx, hy + 0.5, 1.4, 1.4, tip);
                        let spread = 1.2 + open * 1.6;
                        pen.fill(hx - spread, hy + 4.6 * f, 1.7 * f, 4.2 * f, petal_shade);
                        pen.fill(hx + spread, hy + 4.6 * f, 1.7 * f, 4.2 * f, petal_shade);
                        pen.fill(hx, hy + 5.0 * f, 1.9 * f, 4.6 * f, petal);
                        if open > 0.3 {
                            pen.fill(hx, hy + 8.2, 1.3 * open, 1.0 * open, petal);
                            pen.fill(hx, hy + 8.6, 0.8 * open, 0.4 * open, tip);
                        }
                    }
                }
            }
            MUSHROOMS => {
                let agaric = c.vx < 0.6;
                let stalk = cc(0.95, 0.91, 0.82);
                let stalk_shade = cc(0.85, 0.80, 0.70);
                let cap = if agaric { cc(0.82, 0.21, 0.13) } else { cc(0.54, 0.33, 0.17) };
                let cap_light = if agaric { cc(0.92, 0.36, 0.22) } else { cc(0.66, 0.44, 0.24) };
                let gills = cc(0.88, 0.82, 0.70);
                let grass = cc(0.42, 0.58, 0.26);
                for g in 0..5 {
                    let fg = g as f32;
                    let gx = (fg - 2.0) * 7.0 + (c.seed + fg).sin() * 2.0;
                    pen.stroke(gx, 0.0, gx + (c.t + fg).sin() * 1.5 - 1.0, -4.0 - (g % 3) as f32 * 1.5, 0.8, grass);
                }
                for i in 0..c.count {
                    let fi = i as f32;
                    let bx = (fi - (c.count - 1) as f32 / 2.0) * 11.0 + (c.seed + fi * 2.0).sin() * 2.0;
                    let h = (9.0 + fi * 3.5 + (c.seed + fi).sin() * 2.0) * c.grow * (1.0 - c.squash * 0.15);
                    let w = (if agaric { 8.0 } else { 9.5 }) * (0.7 + 0.3 * c.grow) * (1.0 - fi * 0.12) * (1.0 + c.squash * 0.1);
                    pen.m = Mat::rotation_about(c.angle * (1.0 + fi * 0.3), bx, 0.0).then(&t);
                    let sw = if agaric { 2.0 } else { 3.0 };
                    let foot = if agaric { 0.8 } else { 1.5 };
                    pen.poly(
                        &[(bx - sw - foot, 0.0), (bx - sw, -h * 0.4), (bx - sw * 0.75, -h), (bx + sw * 0.75, -h), (bx + sw, -h * 0.4), (bx + sw + foot, 0.0)],
                        stalk,
                    );
                    pen.stroke(bx + sw * 0.4, -h * 0.1, bx + sw * 0.3, -h * 0.9, 0.8, stalk_shade);
                    if agaric && c.grow > 0.5 {
                        pen.fill(bx, -h * 0.72, sw + 1.4, 1.1, stalk); // the little skirt
                    }
                    let dome: Vec<(f32, f32)> = (0..=12)
                        .map(|j| {
                            let ang = PI + PI * j as f32 / 12.0;
                            (bx + ang.cos() * w, -h + 1.0 + ang.sin() * w * 0.65 * c.grow)
                        })
                        .collect();
                    pen.poly(&dome, cap);
                    pen.fill(bx - w * 0.25, -h - w * 0.32 * c.grow, w * 0.45, w * 0.18 * c.grow, cap_light);
                    pen.stroke(bx - w * 0.9, -h + 1.2, bx + w * 0.9, -h + 1.2, 1.0, gills);
                    if agaric && c.grow > 0.4 {
                        const DOTS: [[f32; 3]; 5] = [[-0.55, 0.25, 1.1], [-0.15, 0.45, 0.8], [0.25, 0.3, 1.2], [0.55, 0.15, 0.7], [0.05, 0.12, 0.6]];
                        for d in DOTS {
                            pen.fill(bx + d[0] * w, -h - d[1] * w * 0.65 * c.grow * 1.3, d[2], d[2] * 0.85, cc(1.0, 0.98, 0.94));
                        }
                    }
                    pen.m = t;
                }
            }
            SNAIL => {
                // the slime trail lives in screen space and glints as it dries
                pen.m = Mat::IDENTITY;
                let trail: Vec<_> = c.trail.iter().copied().collect();
                let ages: Vec<_> = c.trail_age.iter().copied().collect();
                for i in 1..trail.len() {
                    let fade = 1.0 - ages[i] / 40.0;
                    let glint = 0.25 + 0.2 * (trail[i].0 * 0.05 + c.t * 2.0).sin();
                    pen.stroke(trail[i - 1].0, trail[i - 1].1, trail[i].0, trail[i].1, 1.6 * s * CS * 0.7, col(0.88, 0.92, 0.97, (glint + 0.15) * fade));
                }
                pen.m = t;
                let body = cc(0.78, 0.70, 0.58);
                let body_dark = cc(0.64, 0.56, 0.45);
                let shell = cc(0.63, 0.43, 0.22);
                let band = cc(0.80, 0.60, 0.34);
                let line = cc(0.42, 0.27, 0.13);
                let eye_col = cc(0.18, 0.14, 0.1);
                let out = c.grow;
                shadow(&mut pen, 10.0);
                if out > 0.05 {
                    // the foot, rippling as it glides
                    for i in 0..6 {
                        let u = i as f32 / 5.0;
                        let px = -4.0 + u * (9.0 + 8.0 * out);
                        pen.fill(px, -2.2 - (c.aux - i as f32).sin().max(0.0) * 0.4, 3.2, 2.3, body);
                    }
                    for i in 0..3 {
                        pen.fill(-1.0 + i as f32 * 4.0 * out, -1.4, 0.6, 0.4, body_dark);
                    }
                    let hx = 6.0 + 8.0 * out;
                    let wob = (c.t * 2.3).sin() * 0.8;
                    pen.fill(hx, -3.4, 3.0, 2.6, body);
                    pen.stroke(hx + 1.0, -5.0, hx + 3.0 + wob, -5.0 - 7.0 * out, 0.9, body);
                    pen.stroke(hx - 0.5, -5.0, hx + 0.5 - wob * 0.5, -5.0 - 6.0 * out, 0.9, body);
                    pen.fill(hx + 3.0 + wob, -5.0 - 7.0 * out, 1.1, 1.1, eye_col);
                    pen.fill(hx + 0.5 - wob * 0.5, -5.0 - 6.0 * out, 1.0, 1.0, eye_col);
                    pen.stroke(hx + 2.4, -2.6, hx + 4.0, -1.8, 0.6, body_dark);
                }
                // a spiral shell: banded rings drawn inwards with a growth line spiral on top
                pen.fill(0.0, -8.0, 8.0, 7.5, shell);
                pen.fill(0.6, -8.3, 5.6, 5.2, band);
                pen.fill(1.0, -8.5, 3.6, 3.4, shell);
                pen.fill(1.3, -8.6, 1.8, 1.7, band);
                for i in 0..14 {
                    let fi = i as f32;
                    let (r0, r1) = (7.2 * (1.0 - fi / 15.0), 7.2 * (1.0 - (fi + 1.0) / 15.0));
                    let (a0, a1) = (fi * 0.85, (fi + 1.0) * 0.85);
                    pen.stroke(1.0 + a0.cos() * r0, -8.4 + a0.sin() * r0, 1.0 + a1.cos() * r1, -8.4 + a1.sin() * r1, 0.8, line);
                }
                pen.fill(-2.5, -12.0, 2.2, 1.2, col(1.0, 0.95, 0.85, 0.35)); // shine
            }
            FROG => {
                let skin = cc(0.40, 0.62, 0.30);
                let skin_dark = cc(0.30, 0.48, 0.22);
                let belly = cc(0.84, 0.87, 0.60);
                let iris = cc(0.96, 0.78, 0.26);
                let dark = cc(0.10, 0.10, 0.06);
                let tongue = cc(0.92, 0.45, 0.50);
                let jump = c.state == 1;
                if !jump {
                    shadow(&mut pen, 11.0);
                }
                let sq = c.squash;
                t = place(0.0, c.angle, (if jump { 1.05 } else { breathe }) * (1.0 - sq * 0.2), 0.0);
                pen.m = t;
                if jump {
                    pen.stroke(-5.0, -4.0, -16.0, 1.0, 3.2, skin);
                    pen.stroke(-15.0, 1.0, -19.0, 2.2, 1.6, skin);
                    pen.stroke(-4.0, -5.0, -15.0, -1.0, 2.3, skin_dark);
                } else {
                    pen.fill(-5.0, -3.5, 6.0, 4.0, skin);
                    pen.fill(-8.5, -0.8, 4.0, 1.4, skin_dark); // folded foot
                }
                pen.fill(0.0, -6.5, 9.5, 6.5, skin);
                pen.fill(-3.0, -9.0, 1.6, 1.2, skin_dark);
                pen.fill(-6.0, -6.5, 1.3, 1.0, skin_dark);
                pen.fill(1.0, -10.0, 1.1, 0.9, skin_dark);
                pen.fill(2.0, -4.0, 6.5, 3.2, belly);
                let throat = if jump { 0.0 } else { c.anim.sin().max(0.0) * 1.3 };
                pen.fill(7.0, -3.4, 2.0 + throat, 1.8 + throat * 0.6, belly);
                // front leg with a little webbed hand
                let hand = if jump { 10.0 } else { 8.0 };
                pen.stroke(6.0, -3.0, hand, 0.0, 1.7, skin);
                for f in -1..=1 {
                    let ff = f as f32;
                    pen.stroke(hand, 0.0, hand + 1.4 + ff * 0.4, 0.2 + ff * 0.9, 0.6, skin);
                }
                // bulging golden eyes with a horizontal pupil
                pen.fill(4.0, -12.0, 3.3, 3.3, skin);
                if c.blink < 0.0 {
                    pen.stroke(2.2, -12.2, 6.0, -12.2, 1.2, skin_dark);
                } else {
                    pen.fill(4.3 + look_fwd * 0.3, -12.3, 2.3, 2.3, iris);
                    pen.fill(4.5 + look_fwd * 0.6, -12.3, 1.6, 0.8, dark);
                    pen.fill(3.6, -13.1, 0.6, 0.6, col(1.0, 1.0, 1.0, 0.85));
                }
                pen.stroke(6.5, -6.8, 10.5, -6.2, 0.6, skin_dark);
                pen.fill(9.5, -8.6, 0.4, 0.3, dark);
                if c.aux > 0.0 {
                    // the tongue shoots out to the snack and back
                    let e = (c.aux / 0.25 * PI).sin();
                    let tx = 10.0 + (c.vx / CS - 10.0) * e;
                    let ty = -6.5 + (c.vy / CS + 6.5) * e;
                    pen.stroke(10.0, -6.5, tx, ty, 1.1, tongue);
                    pen.fill(tx, ty, 1.3, 1.3, tongue);
                }
            }
            ROBIN => {
                let back = cc(0.50, 0.42, 0.31);
                let wing = cc(0.42, 0.35, 0.26);
                let feather = cc(0.32, 0.26, 0.19);
                let breast = cc(0.92, 0.48, 0.22);
                let belly = cc(0.95, 0.92, 0.85);
                let dark = cc(0.10, 0.08, 0.06);
                let legs = cc(0.40, 0.28, 0.22);
                let flying = c.state != 1;
                let hop = (PI * c.grow).sin() * 6.0;
                if !flying {
                    shadow(&mut pen, 7.0);
                }
                t = place(-hop, 0.0, (1.0 - c.squash * 0.15) * if flying { 1.0 } else { breathe }, 0.0);
                pen.m = t;
                let peck = if !flying && c.anim > 0.0 { (c.anim * PI).sin() * 4.0 } else { 0.0 };
                let flick = c.aux2 * 14.0;
                pen.m = Mat::rotation_about(-flick, -5.0, -8.0).then(&t);
                pen.poly(&[(-5.0, -8.5), (-14.0, -12.0), (-13.5, -9.5), (-12.0, -7.0)], wing);
                pen.m = t;
                if !flying {
                    pen.stroke(-0.5, -2.0, -1.0, 0.0, 0.8, legs);
                    pen.stroke(1.5, -2.0, 2.0, 0.0, 0.8, legs);
                    pen.stroke(-1.0, 0.0, 1.0, 0.0, 0.6, legs);
                    pen.stroke(2.0, 0.0, 4.0, 0.0, 0.6, legs);
                }
                pen.fill(0.0, -7.0, 7.2, 6.2, back);
                pen.fill(2.0, -3.6, 4.8, 2.8, belly);
                pen.fill(3.6, -7.4, 4.6, 4.4, breast);
                // the head, turned towards the pointer when it is around, ducking to peck
                let (hx, hy) = (5.5 + look_fwd * 0.6, -12.5 + peck);
                pen.fill(hx, hy, 4.4, 4.1, back);
                pen.fill(hx + 1.6, hy + 1.6, 3.0, 2.7, breast);
                eye(&mut pen, hx + 1.2, hy - 0.4, 0.95, dark);
                pen.poly(&[(hx + 3.2, hy - 0.6), (hx + 6.4, hy + 0.3), (hx + 3.2, hy + 1.1)], dark);
                if flying {
                    let sn = c.anim.sin();
                    pen.poly(&[(-3.0, -9.0), (3.0, -9.0), (-1.0, -9.0 - 9.0 * sn)], feather);
                    pen.poly(&[(-4.0, -8.0), (3.0, -8.0), (-3.0, -8.0 - 10.0 * sn)], wing);
                } else {
                    pen.fill(-1.8, -8.0, 4.8, 3.2, wing);
                    for f in 0..3 {
                        let ff = f as f32;
                        pen.stroke(-5.0 + ff * 1.6, -7.5, -2.0 + ff * 1.6, -6.0, 0.5, feather);
                    }
                }
            }
            SQUIRREL => {
                let fur = cc(0.76, 0.42, 0.18);
                let fur_dark = cc(0.62, 0.33, 0.14);
                let tail_light = cc(0.86, 0.56, 0.28);
                let belly = cc(0.96, 0.88, 0.74);
                let dark = cc(0.13, 0.09, 0.06);
                let acorn = cc(0.60, 0.40, 0.20);
                let cup = cc(0.38, 0.26, 0.14);
                let sitting = c.state == 1 || c.state == 2;
                let bounce = if sitting { 0.0 } else { -c.anim.sin().abs() * 10.0 };
                let stretch = if sitting { 1.0 } else { 1.0 + (c.anim * 2.0).sin() * 0.08 };
                shadow(&mut pen, if sitting { 9.0 } else { 12.0 });
                t = place(bounce, if sitting { -25.0 } else { c.anim.sin() * 8.0 }, if sitting { breathe } else { 1.0 }, 0.0);
                pen.m = t;
                // a big S-shaped bushy tail, pale at the rim
                let flick = c.aux2 * 6.0 + if sitting { 0.0 } else { c.anim.sin() * 3.0 };
                pen.m = Mat::rotation_about(flick, -9.0, -10.0).then(&t);
                const TAIL: [[f32; 4]; 5] = [[-10.0, -12.0, 5.5, 7.0], [-13.0, -19.0, 6.5, 6.0], [-12.0, -26.0, 6.0, 5.0], [-8.0, -30.0, 4.5, 3.6], [-5.0, -31.0, 2.6, 2.2]];
                for tl in TAIL {
                    pen.fill(tl[0], tl[1], tl[2] + 1.2, tl[3] + 1.2, tail_light);
                }
                for tl in TAIL {
                    pen.fill(tl[0], tl[1], tl[2], tl[3], fur);
                }
                pen.m = t;
                let sn = c.anim.sin();
                let moving = if sitting { 0.0 } else { 1.0 };
                pen.stroke(-5.0, -6.0, -8.0 - sn * 5.0 * moving, 0.0, 2.8, fur_dark);
                pen.stroke(5.0, -7.0, 8.0 + sn * 5.0 * moving, 0.0, 2.2, fur_dark);
                pen.fill(0.0, -9.0, 9.5 * stretch, 6.0, fur);
                pen.fill(2.5, -6.8, 5.0, 3.0, belly);
                let nibble = if c.state == 1 { c.aux.sin() * 0.8 } else { 0.0 };
                let (hx, hy) = (8.0 + look_fwd * 0.6, -13.0 + nibble * 0.4);
                pen.fill(hx, hy, 4.8, 4.2, fur);
                pen.fill(hx + 2.4, hy + 1.6, 2.4, 1.8, belly);
                pen.poly(&[(hx - 1.0, hy - 3.5), (hx + 0.2, hy - 8.5), (hx + 2.6, hy - 3.6)], fur);
                pen.stroke(hx + 0.2, hy - 8.5, hx - 0.6, hy - 10.0, 0.8, fur_dark); // ear tuft
                eye(&mut pen, hx + 1.4, hy - 0.8, 1.1, dark);
                pen.fill(hx + 4.6, hy + 0.8, 0.7, 0.6, dark);
                // the acorn, held in the paws when sitting up
                let (ax, ay) = if sitting { (hx + 3.8, hy + 4.5 + nibble) } else { (hx + 5.0, hy + 2.0) };
                if sitting {
                    pen.stroke(4.0, -9.0, ax - 0.5, ay + 1.0, 1.6, fur_dark);
                }
                pen.fill(ax, ay, 2.0, 2.4, acorn);
                pen.fill(ax, ay - 1.8, 2.3, 1.2, cup);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matrix_order_matches_direct2d() {
        // scale then move
        let m = Mat::scale(2.0, 2.0).then(&Mat::translation(10.0, 0.0));
        assert_eq!(m.apply(1.0, 1.0), [12.0, 2.0]);
        // +90 degrees turns +x towards +y (clockwise on a y-down screen)
        let r = Mat::rotation(90.0).apply(1.0, 0.0);
        assert!((r[0]).abs() < 1e-6 && (r[1] - 1.0).abs() < 1e-6);
        let p = Mat::rotation_about(180.0, 5.0, 5.0).apply(6.0, 5.0);
        assert!((p[0] - 4.0).abs() < 1e-5 && (p[1] - 5.0).abs() < 1e-5);
    }

    #[test]
    fn a_busy_frame_builds() {
        let mut sim = Sim::default();
        sim.reset(1920, 1080, 1.0);
        let r = crate::platform::Rect { l: 0.0, t: 0.0, r: 1920.0, b: 1080.0 };
        sim.set_ground(&[crate::platform::Rect { b: 1040.0, ..r }], &[r]);
        let tr = Tracker::default();
        for k in 0..CRITTER_KINDS as i32 {
            sim.spawn_critter(k, true, Some(&tr));
        }
        let mix = crate::weather::EffectMix { leaves: 2.0, rain: 1.0, snow: 1.0, thunder: true, ..crate::weather::EffectMix::none() };
        for _ in 0..300 {
            sim.step(1.0 / 30.0, &tr, mix, CursorWind::default());
        }
        let mut geo = Geo::new();
        build_frame(&mut geo, &sim, &tr, true);
        assert!(geo.verts.len() > 1000);
        assert_eq!(geo.verts.len() % 3, 0);
        assert!(geo.verts.iter().all(|v| v.pos[0].is_finite() && v.pos[1].is_finite()));
    }
}
