//! The sprite atlas, painted once on the CPU at start-up.
//!
//! Every leaf species x colour is painted at 4 mip sizes (each level packed below the previous), so any
//! on-screen size samples a prefiltered cell instead of shimmering. To the right: a soft dot, a ripple ring,
//! a snowflake disc and a soft-topped bar for the surface layers, then a rain streak.

use crate::leaves::{build_leaf_shape, Pt};
use crate::sim::{LEAF_COLORS, LEAF_SHAPES};
use tiny_skia::{
    Color, FillRule, GradientStop, LineCap, LinearGradient, Paint, PathBuilder, Pixmap, Point, RadialGradient, Rect,
    SpreadMode, Stroke, Transform,
};

pub const BASE: u32 = 96;
pub const LEVELS: u32 = 4;
const LEAF_AREA_W: u32 = 16 * BASE;
const LEAF_CELLS: u32 = (LEAF_SHAPES * LEAF_COLORS) as u32;
pub const DOT_X: u32 = LEAF_AREA_W;
pub const DOT_SIZE: u32 = 64;
pub const RING_Y: u32 = DOT_SIZE;
pub const FLAKE_Y: u32 = 2 * DOT_SIZE;
pub const SOFT_Y: u32 = 3 * DOT_SIZE;
pub const SOFT_W: u32 = 16;
pub const SOFT_FADE: u32 = 8;
pub const SOFT_H: u32 = 32;
pub const STREAK_X: u32 = DOT_X + DOT_SIZE;
pub const STREAK_W: u32 = 16;
pub const STREAK_H: u32 = 128;
pub const ATLAS_W: u32 = STREAK_X + STREAK_W;
pub const ATLAS_H: u32 = if level_y(LEVELS) > STREAK_H { level_y(LEVELS) } else { STREAK_H };

const fn level_y(level: u32) -> u32 {
    let mut y = 0;
    let mut l = 0;
    while l < level {
        let c = BASE >> l;
        let cols = LEAF_AREA_W / c;
        y += LEAF_CELLS.div_ceil(cols) * c;
        l += 1;
    }
    y
}

/// The atlas rectangle (l, t, r, b) of a leaf species in a colour at a mip level.
pub fn leaf_cell(shape: u32, color: u32, level: u32) -> [u32; 4] {
    let c = BASE >> level;
    let cols = LEAF_AREA_W / c;
    let i = shape * LEAF_COLORS as u32 + color;
    let (x, y) = (i % cols * c, level_y(level) + i / cols * c);
    [x, y, x + c, y + c]
}

// flat, warm autumn colours: no gradients, it should feel cosy rather than glossy
pub const PALETTES: [[f32; 3]; LEAF_COLORS] = [
    [0.77, 0.28, 0.12],
    [0.87, 0.47, 0.12],
    [0.92, 0.67, 0.16],
    [0.63, 0.16, 0.12],
    [0.80, 0.59, 0.24],
    [0.55, 0.35, 0.17],
    [0.90, 0.75, 0.27],
    [0.71, 0.39, 0.16],
];

fn shade(c: [f32; 3], f: f32, a: f32) -> Color {
    Color::from_rgba((c[0] * f).min(1.0), (c[1] * f).min(1.0), (c[2] * f).min(1.0), a).unwrap()
}

fn solid(c: Color) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color(c);
    p.anti_alias = true;
    p
}

fn white(a: f32) -> Color {
    Color::from_rgba(1.0, 1.0, 1.0, a).unwrap()
}

fn line(pm: &mut Pixmap, a: Pt, b: Pt, width: f32, paint: &Paint, t: Transform) {
    let mut pb = PathBuilder::new();
    pb.move_to(a.x, a.y);
    pb.line_to(b.x, b.y);
    if let Some(path) = pb.finish() {
        let stroke = Stroke { width, line_cap: LineCap::Butt, ..Stroke::default() };
        pm.stroke_path(&path, paint, &stroke, t, None);
    }
}

/// Returns premultiplied RGBA8 pixels, `ATLAS_W` x `ATLAS_H`.
pub fn build_atlas() -> Vec<u8> {
    let mut pm = Pixmap::new(ATLAS_W, ATLAS_H).expect("atlas size");
    let shapes: Vec<_> = (0..LEAF_SHAPES).map(build_leaf_shape).collect();
    let outlines: Vec<_> = shapes
        .iter()
        .map(|s| {
            let mut pb = PathBuilder::new();
            pb.move_to(s.outline[0].x, s.outline[0].y);
            for p in &s.outline[1..] {
                pb.line_to(p.x, p.y);
            }
            pb.close();
            pb.finish().expect("leaf outline")
        })
        .collect();

    // every leaf is painted directly at each mip size, so small leaves stay crisp instead of being shrunk
    for level in 0..LEVELS {
        let cell = (BASE >> level) as f32;
        let unit = cell * 0.92;
        let px = 1.0 / unit;
        for (s, ls) in shapes.iter().enumerate() {
            for (c, body) in PALETTES.iter().enumerate() {
                let r = leaf_cell(s as u32, c as u32, level);
                let t = Transform::from_row(unit, 0.0, 0.0, unit, r[0] as f32 + cell / 2.0, r[1] as f32 + cell / 2.0);
                pm.fill_path(&outlines[s], &solid(shade(*body, 1.0, 1.0)), FillRule::Winding, t, None);

                // soft veins in a slightly deeper shade of the same colour (the first one is the midrib), then the stem
                let rib = (px * 1.1).max(0.02);
                let vein = solid(shade(*body, 0.72, 0.55));
                for v in 0..ls.vein_from.len() {
                    line(&mut pm, ls.vein_from[v], ls.vein_to[v], if v == 0 { rib } else { rib * 0.6 }, &vein, t);
                }
                let stem_end = Pt { x: ls.stem.x + 0.015, y: (ls.stem.y + 0.12).min(0.53) };
                line(&mut pm, ls.stem, stem_end, (px * 1.4).max(0.028), &solid(shade(*body, 0.6, 1.0)), t);
            }
        }
    }

    let id = Transform::identity();
    let half = DOT_SIZE as f32 / 2.0;
    let radial = |pm: &mut Pixmap, cx: f32, cy: f32, r: f32, stops: Vec<GradientStop>| {
        let shader = RadialGradient::new(Point::from_xy(cx, cy), Point::from_xy(cx, cy), r, stops, SpreadMode::Pad, id)
            .expect("radial gradient");
        let paint = Paint { shader, anti_alias: true, ..Paint::default() };
        let circle = PathBuilder::from_circle(cx, cy, r).unwrap();
        pm.fill_path(&circle, &paint, FillRule::Winding, id, None);
    };

    // soft dot for splashes and drops
    radial(
        &mut pm,
        DOT_X as f32 + half,
        half,
        half - 2.0,
        vec![GradientStop::new(0.0, white(1.0)), GradientStop::new(0.45, white(0.9)), GradientStop::new(1.0, white(0.0))],
    );

    // ripple ring: a bright line with a faint halo
    let ring = PathBuilder::from_circle(DOT_X as f32 + half, RING_Y as f32 + half, 25.0).unwrap();
    pm.stroke_path(&ring, &solid(white(0.35)), &Stroke { width: 7.0, ..Stroke::default() }, id, None);
    pm.stroke_path(&ring, &solid(white(1.0)), &Stroke { width: 2.5, ..Stroke::default() }, id, None);

    // snowflake: a plain flat disc with just a soft rim; cosy rather than glowing
    radial(
        &mut pm,
        DOT_X as f32 + half,
        FLAKE_Y as f32 + half,
        28.0,
        vec![GradientStop::new(0.0, white(1.0)), GradientStop::new(0.78, white(1.0)), GradientStop::new(1.0, white(0.0))],
    );

    // soft-topped bar for the surface layers: transparent at the top, ramping to solid over SOFT_FADE texels,
    // so their sloping edges come out smooth
    let soft = LinearGradient::new(
        Point::from_xy(0.0, SOFT_Y as f32),
        Point::from_xy(0.0, (SOFT_Y + SOFT_FADE) as f32),
        vec![GradientStop::new(0.0, white(0.0)), GradientStop::new(1.0, white(1.0))],
        SpreadMode::Pad,
        id,
    )
    .unwrap();
    let soft_rect = Rect::from_xywh(DOT_X as f32, SOFT_Y as f32, SOFT_W as f32, SOFT_FADE as f32).unwrap();
    pm.fill_rect(soft_rect, &Paint { shader: soft, ..Paint::default() }, id, None);
    let body = Rect::from_xywh(DOT_X as f32, (SOFT_Y + SOFT_FADE) as f32, SOFT_W as f32, (SOFT_H - SOFT_FADE) as f32).unwrap();
    pm.fill_rect(body, &solid(white(1.0)), id, None);

    // rain streak, fading towards its tail
    let streak = LinearGradient::new(
        Point::from_xy(0.0, 4.0),
        Point::from_xy(0.0, STREAK_H as f32 - 4.0),
        vec![GradientStop::new(0.0, white(0.0)), GradientStop::new(1.0, white(1.0))],
        SpreadMode::Pad,
        id,
    )
    .unwrap();
    let (l, t, r, b, rad) = (STREAK_X as f32 + 5.0, 4.0, (STREAK_X + STREAK_W) as f32 - 5.0, STREAK_H as f32 - 4.0, 3.0);
    let mut pb = PathBuilder::new();
    pb.move_to(l + rad, t);
    pb.line_to(r - rad, t);
    pb.quad_to(r, t, r, t + rad);
    pb.line_to(r, b - rad);
    pb.quad_to(r, b, r - rad, b);
    pb.line_to(l + rad, b);
    pb.quad_to(l, b, l, b - rad);
    pb.line_to(l, t + rad);
    pb.quad_to(l, t, l + rad, t);
    pb.close();
    pm.fill_path(&pb.finish().unwrap(), &Paint { shader: streak, anti_alias: true, ..Paint::default() }, FillRule::Winding, id, None);

    pm.take()
}

/// Debug aid: the full-size leaf cells over a dark desktop-ish background, as a PNG.
#[cfg(feature = "debug-overlay")]
pub fn dump_png(rgba: &[u8], path: &std::path::Path) {
    let (w, h) = (LEAF_AREA_W, level_y(1));
    let mut out = Vec::with_capacity((w * h * 3) as usize);
    for y in 0..h {
        for x in 0..w {
            let i = ((y * ATLAS_W + x) * 4) as usize;
            let a = rgba[i + 3] as u32;
            for ch in 0..3 {
                out.push((rgba[i + ch] as u32 + 40 * (255 - a) / 255).min(255) as u8);
            }
        }
    }
    if let Ok(f) = std::fs::File::create(path) {
        let mut enc = png::Encoder::new(std::io::BufWriter::new(f), w, h);
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        if let Ok(mut wr) = enc.write_header() {
            let _ = wr.write_image_data(&out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_fits() {
        assert_eq!(ATLAS_W, 1616);
        assert_eq!(ATLAS_H, 1800);
        let last = leaf_cell(LEAF_SHAPES as u32 - 1, LEAF_COLORS as u32 - 1, LEVELS - 1);
        assert!(last[2] <= LEAF_AREA_W && last[3] <= ATLAS_H);
    }

    #[test]
    fn paints_leaves_and_dots() {
        let px = build_atlas();
        assert_eq!(px.len(), (ATLAS_W * ATLAS_H * 4) as usize);
        let alpha = |x: u32, y: u32| px[((y * ATLAS_W + x) * 4 + 3) as usize];
        let c = leaf_cell(0, 0, 0);
        assert!(alpha((c[0] + c[2]) / 2, (c[1] + c[3]) / 2) > 200, "leaf centre is opaque");
        assert!(alpha(c[0] + 1, c[1] + 1) < 10, "leaf corner is clear");
        assert!(alpha(DOT_X + DOT_SIZE / 2, FLAKE_Y + DOT_SIZE / 2) == 255, "flake centre is opaque");
    }
}
