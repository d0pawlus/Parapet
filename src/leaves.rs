//! Leaf outlines are generated from a handful of botanical parameters, then painted once into the atlas.
//! Three families: simple pinnate blades (profile along the midrib), palmate leaves (lobes around a centre)
//! and fans (ginkgo).

use std::f32::consts::PI;

#[derive(Clone, Copy, Debug, Default)]
pub struct Pt {
    pub x: f32,
    pub y: f32,
}

pub const fn pt(x: f32, y: f32) -> Pt {
    Pt { x, y }
}

#[derive(Default, Clone)]
pub struct LeafShape {
    pub outline: Vec<Pt>,
    pub vein_from: Vec<Pt>, // vein 0 is the midrib
    pub vein_to: Vec<Pt>,
    pub stem: Pt,
}

#[derive(Clone, Copy, PartialEq)]
enum Family {
    Simple,
    Palmate,
    Fan,
}

#[derive(Clone, Copy)]
struct Spec {
    family: Family,
    // simple blades
    width: f32,     // half width relative to length
    skew: f32,      // >1 moves the widest point towards the base, <1 towards the tip
    fullness: f32,  // <1 rounder, >1 slimmer shoulders
    cordate: f32,   // width kept at the base (heart shape), 0 = tapering base
    notch: f32,     // depth of the notch at a heart-shaped base
    acuminate: f32, // drawn-out drip tip
    retuse: f32,    // blunt notched tip
    asym: f32,      // left half wider towards the base (elm, linden)
    lobes: i32,
    lobe_depth: f32,
    lobe_sharp: f32,
    teeth: i32,
    tooth: f32,
    double_tooth: f32,
    // palmate / fan
    count: usize,
    spread: f32,
    rmin: f32,
    sigma: f32,
    lengths: &'static [f32],
    lobe_pow: f32, // palmate lobe profile: 1 = pointed tips, 2 = rounded lobes
}

const ZERO: Spec = Spec {
    family: Family::Simple,
    width: 0.0,
    skew: 0.0,
    fullness: 0.0,
    cordate: 0.0,
    notch: 0.0,
    acuminate: 0.0,
    retuse: 0.0,
    asym: 0.0,
    lobes: 0,
    lobe_depth: 0.0,
    lobe_sharp: 0.0,
    teeth: 0,
    tooth: 0.0,
    double_tooth: 0.0,
    count: 0,
    spread: 0.0,
    rmin: 0.0,
    sigma: 0.0,
    lengths: &[],
    lobe_pow: 0.0,
};

const MAPLE5: [f32; 5] = [0.62, 0.92, 1.0, 0.92, 0.62];
const JAPANESE7: [f32; 7] = [0.55, 0.8, 0.95, 1.0, 0.95, 0.8, 0.55];
const EVEN5: [f32; 5] = [0.8, 0.95, 1.0, 0.95, 0.8];
const GRAPE3: [f32; 3] = [0.8, 1.0, 0.8];
const MITTEN: [f32; 2] = [1.0, 0.75];
const HAWTHORN5: [f32; 5] = [0.7, 0.9, 1.0, 0.9, 0.7];

fn s(width: f32, skew: f32, fullness: f32) -> Spec {
    Spec { family: Family::Simple, width, skew, fullness, ..ZERO }
}
fn teeth(mut s: Spec, n: i32, amp: f32, dbl: f32) -> Spec {
    s.teeth = n;
    s.tooth = amp;
    s.double_tooth = dbl;
    s
}
fn lobes(mut s: Spec, n: i32, depth: f32, sharp: f32) -> Spec {
    s.lobes = n;
    s.lobe_depth = depth;
    s.lobe_sharp = sharp;
    s
}
fn heart(mut s: Spec, base: f32, notch: f32) -> Spec {
    s.cordate = base;
    s.notch = notch;
    s
}
fn tip(mut s: Spec, acuminate: f32, retuse: f32) -> Spec {
    s.acuminate = acuminate;
    s.retuse = retuse;
    s
}
fn asym(mut s: Spec, a: f32) -> Spec {
    s.asym = a;
    s
}
#[allow(clippy::too_many_arguments)]
fn p(count: usize, spread: f32, rmin: f32, sigma: f32, lengths: &'static [f32], teeth: i32, tooth: f32, lobe_pow: f32) -> Spec {
    Spec { family: Family::Palmate, count, spread, rmin, sigma, lengths, teeth, tooth, lobe_pow, ..ZERO }
}

fn species(i: usize) -> Spec {
    match i {
        0 => teeth(s(0.36, 1.1, 0.8), 18, 0.012, 0.0),                                     // beech
        1 => p(5, 1.40 * PI, 0.44, 0.30, &MAPLE5, 26, 0.11, 1.1),                          // Norway maple
        2 => teeth(tip(s(0.12, 1.0, 0.9), 0.5, 0.0), 60, 0.006, 0.0),                      // willow
        3 => lobes(s(0.34, 1.2, 0.7), 4, 0.5, 0.6),                                        // English oak
        4 => teeth(lobes(s(0.40, 1.1, 0.6), 3, 0.66, 0.35), 10, 0.02, 0.0),                // red oak
        5 => teeth(tip(s(0.36, 1.6, 0.65), 0.4, 0.0), 34, 0.02, 0.5),                      // birch
        6 => asym(teeth(tip(heart(s(0.44, 1.35, 0.6), 0.45, 0.08), 0.5, 0.0), 40, 0.014, 0.0), 0.18), // linden
        7 => asym(teeth(tip(s(0.34, 1.2, 0.75), 0.3, 0.0), 30, 0.02, 0.6), 0.22),          // elm
        8 => teeth(heart(s(0.48, 1.25, 0.45), 0.3, 0.03), 16, 0.035, 0.0),                 // aspen
        9 => p(5, 1.50 * PI, 0.40, 0.25, &EVEN5, 0, 0.0, 1.0),                             // sweetgum
        10 => p(5, 1.38 * PI, 0.46, 0.32, &MAPLE5, 14, 0.14, 1.1),                         // sugar maple
        11 => p(7, 1.60 * PI, 0.28, 0.17, &JAPANESE7, 70, 0.035, 1.1),                     // Japanese maple
        12 => p(5, 1.25 * PI, 0.55, 0.34, &EVEN5, 14, 0.07, 1.2),                          // plane tree
        13 => Spec { family: Family::Fan, spread: 0.78 * PI, ..ZERO },                     // ginkgo
        14 => teeth(tip(s(0.30, 1.15, 0.8), 0.45, 0.0), 46, 0.016, 0.7),                   // hornbeam
        15 => teeth(tip(s(0.18, 1.0, 0.85), 0.4, 0.0), 22, 0.05, 0.0),                     // sweet chestnut
        16 => teeth(s(0.30, 0.62, 0.9), 34, 0.018, 0.0),                                   // horse chestnut leaflet
        17 => teeth(tip(s(0.26, 1.0, 0.75), 0.7, 0.0), 50, 0.01, 0.0),                     // cherry
        18 => teeth(tip(s(0.42, 0.75, 0.6), 0.0, 0.12), 26, 0.022, 0.5),                   // alder
        19 => teeth(tip(heart(s(0.48, 1.1, 0.5), 0.4, 0.06), 0.55, 0.0), 30, 0.03, 0.8),   // hazel
        20 => teeth(s(0.15, 1.0, 0.5), 34, 0.018, 0.0),                                    // rowan leaflet
        21 => p(5, 1.30 * PI, 0.55, 0.36, &MAPLE5, 0, 0.0, 2.0),                           // field maple
        22 => p(2, 0.55 * PI, 0.50, 0.45, &MITTEN, 0, 0.0, 1.3),                           // sassafras mitten
        23 => p(3, 0.75 * PI, 0.70, 0.50, &GRAPE3, 30, 0.07, 1.5),                         // grape vine
        24 => teeth(tip(s(0.17, 1.0, 0.8), 0.55, 0.0), 28, 0.012, 0.0),                    // ash leaflet
        25 => s(0.30, 0.9, 0.7),                                                           // magnolia
        26 => p(5, 1.20 * PI, 0.34, 0.22, &HAWTHORN5, 18, 0.06, 1.2),                      // hawthorn
        _ => teeth(tip(heart(s(0.46, 1.2, 0.5), 0.5, 0.07), 0.35, 0.0), 42, 0.025, 0.0),   // mulberry
    }
}

fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn frac(x: f32) -> f32 {
    x - x.floor()
}

/// half width of a simple blade at t (0 = tip, 1 = base)
fn profile(s: &Spec, t: f32) -> f32 {
    let mut prof = (PI * t.powf(s.skew)).sin().max(0.0).powf(s.fullness);
    if s.cordate > 0.0 {
        prof = prof.max(s.cordate * smoothstep(0.45, 1.0, t));
    }
    if s.acuminate > 0.0 {
        prof *= 1.0 - s.acuminate + s.acuminate * smoothstep(0.0, 0.32, t);
    }
    if s.retuse > 0.0 {
        prof += s.retuse * (1.0 - t / 0.14).max(0.0) * smoothstep(0.0, 0.03, t);
    }
    if s.lobes > 0 {
        let u = (t - 0.06) / 0.86;
        if u > 0.0 && u < 1.0 {
            // lobe tips at u = k / lobes; lobeSharp < 1 gives round lobes, > 1 narrow pointed ones
            let bulge = (PI * s.lobes as f32 * u).cos().abs().powf(s.lobe_sharp);
            let fade = smoothstep(0.0, 0.1, u) * (1.0 - smoothstep(0.85, 1.0, u));
            prof *= 1.0 - s.lobe_depth * (1.0 - bulge) * fade;
        }
    }
    prof
}

fn tooth_offset(s: &Spec, phase: f32) -> f32 {
    if s.teeth <= 0 {
        return 0.0;
    }
    // saw teeth leaning towards the tip, optionally with a smaller tooth on each flank
    let saw = 1.0 - frac(phase);
    let mut off = s.tooth * saw * saw;
    if s.double_tooth > 0.0 {
        off += s.tooth * s.double_tooth * 0.5 * (1.0 - frac(phase * 2.0));
    }
    off
}

fn build_simple(s: &Spec, out: &mut LeafShape) {
    const N: usize = 110;
    let mut right = Vec::with_capacity(N + 1);
    let mut left = Vec::with_capacity(N + 1);
    for i in 0..=N {
        let t = i as f32 / N as f32;
        let y = -0.5 + t;
        let w = s.width * profile(s, t);
        let fade = (w / (s.width * 0.25 + 1e-4)).min(1.0);
        let edge = tooth_offset(s, t * s.teeth as f32) * fade;
        let x = w + edge;
        right.push(pt(x, y));
        left.push(pt(-x * (1.0 + s.asym * t * t), y + s.asym * 0.04 * t));
    }
    out.outline = right;
    if s.cordate > 0.0 {
        out.outline.push(pt(0.0, 0.5 - s.notch));
    }
    out.outline.extend(left.iter().rev());

    let base = if s.cordate > 0.0 { 0.5 - s.notch } else { 0.5 };
    out.vein_from.push(pt(0.0, base));
    out.vein_to.push(pt(0.0, -0.47));
    out.stem = pt(0.0, base);

    // side veins: to the lobe tips for lobed blades, evenly spaced otherwise
    let pairs = if s.lobes > 0 { s.lobes } else { ((s.width * 22.0) as i32).clamp(4, 9) };
    for k in 0..pairs {
        let tt = if s.lobes > 0 {
            0.06 + 0.86 * (k as f32 + 0.02) / s.lobes as f32 + 0.86 / s.lobes as f32
        } else {
            0.2 + 0.68 * (k as f32 + 0.5) / pairs as f32
        };
        if tt >= 0.97 {
            continue;
        }
        let t_end = (tt - 0.12).max(0.03);
        let x_end = 0.82 * s.width * profile(s, t_end);
        out.vein_from.push(pt(0.0, -0.5 + tt));
        out.vein_to.push(pt(x_end, -0.5 + t_end));
        out.vein_from.push(pt(0.0, -0.5 + tt));
        out.vein_to.push(pt(-x_end * (1.0 + s.asym * t_end * t_end), -0.5 + t_end));
    }
}

fn build_palmate(s: &Spec, out: &mut LeafShape) {
    const N: usize = 260;
    let c = pt(0.0, 0.12);
    let mut centres: Vec<f32> = (0..s.count)
        .map(|k| if s.count == 1 { 0.0 } else { -s.spread / 2.0 + s.spread * k as f32 / (s.count - 1) as f32 })
        .collect();
    if s.count == 2 {
        // mitten: main lobe and a thumb
        centres[0] = -0.08 * PI;
        centres[1] = 0.47 * PI;
    }

    let radius = |phi: f32| -> (f32, f32) {
        let mut lobe = 0.0f32;
        for (k, centre) in centres.iter().enumerate() {
            let d = phi - centre;
            lobe = lobe.max(s.lengths[k] * (-(d.abs() / s.sigma).powf(s.lobe_pow)).exp());
        }
        let mut r = s.rmin + (1.0 - s.rmin) * lobe;
        // around the petiole the blade pulls in to a small notch
        let back = (phi.abs() - PI).abs();
        r *= 0.55 + 0.45 * smoothstep(0.0, 0.5, back);
        r += tooth_offset(s, (phi + PI) / (2.0 * PI) * s.teeth as f32) * smoothstep(0.05, 0.4, lobe);
        (r, lobe)
    };

    for i in 0..N {
        let phi = -PI + 2.0 * PI * (i as f32 + 0.5) / N as f32;
        let r = radius(phi).0 * 0.5;
        out.outline.push(pt(c.x + r * phi.sin(), c.y - r * phi.cos()));
    }
    for &a in &centres {
        let r = radius(a).0 * 0.5 * 0.88;
        out.vein_from.push(c);
        out.vein_to.push(pt(c.x + r * a.sin(), c.y - r * a.cos()));
    }
    out.stem = pt(c.x, c.y + 0.5 * s.rmin * 0.55);
}

fn build_fan(s: &Spec, out: &mut LeafShape) {
    const N: usize = 90;
    let apex = pt(0.0, 0.42);
    out.outline.push(apex);
    for i in 0..=N {
        let phi = -s.spread / 2.0 + s.spread * i as f32 / N as f32;
        let r = 0.86 * (1.0 + 0.035 * (phi * 23.0).sin()) * (1.0 - 0.32 * (-(phi * phi) / 0.006).exp());
        out.outline.push(pt(apex.x + r * phi.sin(), apex.y - r * phi.cos()));
    }
    for k in 0..11 {
        let phi = -s.spread * 0.42 + s.spread * 0.84 * k as f32 / 10.0;
        out.vein_from.push(apex);
        out.vein_to.push(pt(apex.x + 0.78 * phi.sin(), apex.y - 0.78 * phi.cos()));
    }
    out.stem = apex;
}

pub fn build_leaf_shape(species_index: usize) -> LeafShape {
    let mut out = LeafShape::default();
    let s = species(species_index);
    match s.family {
        Family::Simple => build_simple(&s, &mut out),
        Family::Palmate => build_palmate(&s, &mut out),
        Family::Fan => build_fan(&s, &mut out),
    }

    // normalise: centre the outline and make its larger extent exactly 1 (fits [-0.5, 0.5], stem below)
    let (mut x0, mut y0, mut x1, mut y1) = (1e9f32, 1e9f32, -1e9f32, -1e9f32);
    for p in &out.outline {
        x0 = x0.min(p.x);
        x1 = x1.max(p.x);
        y0 = y0.min(p.y);
        y1 = y1.max(p.y);
    }
    let scale = 0.92 / (x1 - x0).max(y1 - y0 + 0.08);
    let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0 - 0.04);
    let fit = |p: &mut Pt| {
        p.x = (p.x - cx) * scale;
        p.y = (p.y - cy) * scale;
    };
    out.outline.iter_mut().for_each(fit);
    out.vein_from.iter_mut().for_each(fit);
    out.vein_to.iter_mut().for_each(fit);
    fit(&mut out.stem);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_species_fits_the_unit_cell() {
        for i in 0..28 {
            let s = build_leaf_shape(i);
            assert!(s.outline.len() > 50, "species {i}");
            for p in &s.outline {
                assert!(p.x.abs() <= 0.5 && p.y.abs() <= 0.5, "species {i} point {:?}", (p.x, p.y));
            }
        }
    }
}
