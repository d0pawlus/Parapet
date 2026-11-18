//! Rare visitors: small hand-drawn creatures and plants that turn up now and then on the taskbar or on
//! window tops, only when the weather suits them, and that react to the mouse pointer.

use crate::sim::*;
use crate::tracker::Tracker;

// chance per second of each visitor appearing while its conditions hold (a few minutes apart on average)
const CHANCE: [f32; CRITTER_KINDS] = [
    1.0 / 240.0, // hedgehog
    1.0 / 200.0, // caterpillar
    1.0 / 150.0, // snow fox
    1.0 / 90.0,  // snowdrops
    1.0 / 200.0, // snail
    1.0 / 180.0, // frog
    1.0 / 300.0, // robin
    1.0 / 300.0, // squirrel
    1.0 / 150.0, // mushrooms
];

const BF: f32 = BUCKET as f32;

impl Sim {
    // ------------------------------------------------------------ queries

    /// Top of the taskbar ground (mud and snow, not water).
    pub fn surface_y(&self, x: f32) -> f32 {
        let n = self.ground_y.len() as i32;
        let i = ((x / BF) as i32).clamp(0, n - 1) as usize;
        self.ground_y[i] - self.ground.mud[i] - self.ground.height[i]
    }

    fn cursor_near(&self, x: f32, y: f32, radius: f32) -> bool {
        if !self.cursor.valid {
            return false;
        }
        let (dx, dy) = (self.cursor.x1 - x, self.cursor.y1 - y);
        dx * dx + dy * dy < radius * radius
    }

    fn nearest_ground_leaf(&self, x: f32, range: f32) -> Option<usize> {
        let mut best = None;
        let mut best_d = range;
        for (i, p) in self.parts.iter().enumerate() {
            if p.kind != K_LEAF || p.state != RESTING || !p.on_ground || p.hidden {
                continue;
            }
            let d = (p.x - x).abs();
            if d < best_d {
                best_d = d;
                best = Some(i);
            }
        }
        best
    }

    fn resting_ground_leaves(&self) -> usize {
        self.parts.iter().filter(|p| p.kind == K_LEAF && p.state == RESTING && p.on_ground && !p.hidden).count()
    }

    fn puddle_at(&self, x: f32) -> Option<f32> {
        let i = ((x / BF) as i32).clamp(0, self.ground.water.len() as i32 - 1) as usize;
        let depth = self.ground.water[i];
        (depth > 1.5 * self.s).then_some(depth)
    }

    fn find_puddle(&self, from: f32, dir: f32) -> f32 {
        let s = self.s;
        let n = self.ground.water.len() as i32;
        let start = (from / BF) as i32;
        let reach = (500.0 * s / BF) as i32;
        for k in (40.0 * s / BF) as i32..reach {
            let i = start + dir as i32 * k;
            if i < 0 || i >= n {
                break;
            }
            if self.ground.water[i as usize] > 2.0 * s {
                return (i as f32 + 0.5) * BF;
            }
        }
        -1.0
    }

    // ------------------------------------------------------------ spawning

    /// `kind` < 0 picks one at random. `tr` lets the robin pick a window to perch on.
    pub fn spawn_critter(&mut self, kind: i32, forced: bool, tr: Option<&Tracker>) {
        let kind = if kind < 0 { (self.rand() * CRITTER_KINDS as f32) as usize % CRITTER_KINDS } else { kind as usize } as u8;
        if (!forced && self.critters.len() >= 6) || self.ground_y.is_empty() {
            return;
        }
        let s = self.s;
        let (w, h) = (self.w as f32, self.h as f32);

        let mut c = Critter { kind, alpha: 1.0, ..Default::default() };
        c.seed = self.range(0.0, 100.0);
        c.dir = if self.rand() < 0.5 { -1.0 } else { 1.0 };
        c.face = c.dir;
        c.blink = self.range(1.0, 4.0);
        c.x = if c.dir > 0.0 { -60.0 * s } else { w + 60.0 * s };

        match kind {
            CATERPILLAR => {
                // start close to the leaves it is going to eat
                match self.nearest_ground_leaf(w * 0.5, w) {
                    Some(leaf) => {
                        let off = self.range(120.0, 260.0);
                        c.x = (self.parts[leaf].x - c.dir * off * s).clamp(10.0 * s, w - 10.0 * s);
                    }
                    None => c.x = self.range(0.1, 0.9) * w,
                }
                c.alpha = 0.0;
            }
            SNOW_FOX => {
                let mut best = 0;
                for i in 0..self.ground.height.len() {
                    if self.ground.height[i] > self.ground.height[best] {
                        best = i;
                    }
                }
                if self.ground.height[best] < 12.0 * s {
                    if !forced {
                        return;
                    }
                    // summoned without a drift: heap one up
                    best = (self.range(0.2, 0.8) * self.ground.height.len() as f32) as usize;
                    let cx = (best as f32 + 0.5) * BF;
                    for k in -30..=30 {
                        self.ground.add(cx + k as f32 * 2.0 * s, 1.6 * s * (1.0 - (k as f32 / 31.0).abs()));
                    }
                    for _ in 0..10 {
                        let dx = self.range(-50.0, 50.0) * s;
                        self.ground.add(cx + dx, 2.0 * s);
                    }
                }
                c.x = (best as f32 + 0.5) * BF;
                c.grow = 0.0;
            }
            SNOWDROPS | MUSHROOMS => {
                c.x = self.range(0.1, 0.9) * w;
                if kind == MUSHROOMS {
                    // where the ground is muddiest
                    let mut best = (c.x / BF) as usize;
                    for i in 0..self.ground.mud.len() {
                        if self.ground.mud[i] > self.ground.mud[best] + 0.5 * s {
                            best = i;
                        }
                    }
                    c.x = (best as f32 + 0.5) * BF;
                }
                c.count = if kind == SNOWDROPS { 3 + (self.rand() * 3.0) as i32 } else { 2 + (self.rand() * 2.0) as i32 };
                c.timer = if kind == SNOWDROPS { 150.0 } else { 180.0 };
                c.angle = 0.0;
                c.vx = self.rand(); // mushrooms: which kind (fly agaric or bolete)
            }
            SNAIL => {
                let mut px = -1.0;
                for _ in 0..8 {
                    if px >= 0.0 {
                        break;
                    }
                    let from = self.range(0.1, 0.9) * w;
                    px = self.find_puddle(from, c.dir);
                }
                c.x = if px > 0.0 { px - c.dir * 60.0 * s } else { self.range(0.1, 0.9) * w };
                c.alpha = 0.0;
                c.grow = 1.0;
            }
            FROG => {
                let mut px = -1.0;
                for _ in 0..8 {
                    if px >= 0.0 {
                        break;
                    }
                    let from = self.range(0.1, 0.9) * w;
                    px = self.find_puddle(from, c.dir);
                }
                c.x = if px > 0.0 { px + c.dir * 30.0 * s } else { self.range(0.15, 0.85) * w };
                c.timer = self.range(2.0, 4.0);
                c.alpha = 0.0;
            }
            ROBIN => {
                // fly in to a visible window top, or to the taskbar if there is none
                c.host = 0;
                if let Some(tr) = tr {
                    let roofs: Vec<_> = tr
                        .all
                        .iter()
                        .filter(|w| {
                            w.surface
                                && w.r - w.l > 200.0 * s
                                && w.t > 60.0 * s
                                && !tr.covered((w.l + w.r) / 2.0, w.t - 4.0, w.z, false)
                        })
                        .copied()
                        .collect();
                    if !roofs.is_empty() {
                        let win = roofs[(self.rand() * roofs.len() as f32) as usize % roofs.len()];
                        c.host = win.id;
                        c.offx = self.range(40.0 * s, win.r - win.l - 40.0 * s);
                    }
                }
                if c.host == 0 {
                    c.offx = self.range(0.2, 0.8) * w;
                }
                c.y = self.range(0.1, 0.4) * h;
            }
            _ => {}
        }
        if c.kind != ROBIN {
            c.y = self.surface_y(c.x);
        }
        self.critters.push(c);
    }

    pub(crate) fn update_critters(&mut self, dt: f32, tr: &Tracker) {
        let s = self.s;
        self.critter_clock += dt;
        if self.critter_clock >= 1.0 {
            self.critter_clock -= 1.0;
            if self.rare_events && self.critters.len() < 2 {
                let max_of = |v: &[f32]| v.iter().copied().fold(0.0f32, f32::max);
                let max_snow = max_of(&self.ground.height);
                let max_water = max_of(&self.ground.water);
                let max_mud = max_of(&self.ground.mud);
                let month = crate::platform::local_month();
                let autumn = (9..=11).contains(&month);
                let late_winter = month == 2 || month == 3;
                let snowdrop_season = (1..=4).contains(&month);
                let mix = self.mix;
                let snowing = mix.snow > 0.0;
                let t = mix.temp_c;
                let leaves = self.resting_ground_leaves();
                let ok = [
                    !snowing && max_snow < 3.0 * s && t > 3.0 && (mix.leaves > 0.0 || leaves > 3), // hedgehog
                    leaves >= 3 && !snowing && t > 2.0,                                             // caterpillar
                    max_snow > 18.0 * s,                                                            // snow fox
                    // only in their real season: a thaw from January to April, or any mild day in February-March
                    snowdrop_season && !snowing && t > 2.0 && (max_snow > 2.0 * s || late_winter), // snowdrops
                    max_water > 1.0 * s && mix.rain < 0.6 && t > 4.0,                               // snail
                    max_water > 4.0 * s && t > 5.0,                                                 // frog
                    mix.rain < 0.6,                                                                 // robin
                    !snowing && (autumn || mix.leaves > 0.0),                                       // squirrel
                    max_mud > 2.5 * s && t > 3.0,                                                   // mushrooms
                ];
                for k in 0..CRITTER_KINDS {
                    let present = self.critters.iter().any(|c| c.kind as usize == k);
                    if ok[k] && !present && self.rand() < CHANCE[k] {
                        self.spawn_critter(k as i32, false, Some(tr));
                        break;
                    }
                }
            }
        }
        let mut critters = std::mem::take(&mut self.critters);
        let mut i = critters.len();
        while i > 0 {
            i -= 1;
            if !self.update_critter(&mut critters[i], dt, tr) {
                critters.remove(i);
            }
        }
        self.critters = critters;
    }

    // ------------------------------------------------------------ behaviour

    fn critter_on_ground(&self, c: &mut Critter, tr: &Tracker) {
        let s = self.scale_at(c.x);
        c.y = self.surface_y(c.x);
        c.hidden = tr.covered(c.x, c.y - 6.0 * s, i32::MAX, true);
    }

    fn update_critter(&mut self, c: &mut Critter, dt: f32, tr: &Tracker) -> bool {
        let s = self.scale_at(c.x); // sizes and speeds follow the monitor the visitor is on
        let (w, h) = (self.w as f32, self.h as f32);
        c.t += dt;
        let offscreen = c.x < -100.0 * s || c.x > w + 100.0 * s;
        let cursor = self.cursor;
        let cursor_speed = if cursor.valid { (cursor.vx * cursor.vx + cursor.vy * cursor.vy).sqrt() } else { 0.0 };

        // shared liveliness: smooth turning, blinking, glancing at the pointer, settling after a squash
        c.face += (c.dir - c.face) * (dt * 7.0).min(1.0);
        c.blink -= dt;
        if c.blink < -0.14 {
            c.blink = self.range(1.5, 5.0);
        }
        c.squash = (c.squash - dt * 4.0).max(0.0);
        let mut look_target = 0.0;
        if cursor.valid {
            let (dx, dy) = (cursor.x1 - c.x, cursor.y1 - c.y);
            if dx * dx + dy * dy < 420.0 * s * 420.0 * s {
                look_target = (dx / (250.0 * s)).clamp(-1.0, 1.0);
            }
        }
        c.look += (look_target - c.look) * (dt * 4.0).min(1.0);

        match c.kind {
            HEDGEHOG => {
                // waddles along, snuffles, ploughs through leaf heaps picking leaves up on its spines;
                // poked, it rolls into a trembling ball, then peeks out before carrying on
                let poked = self.cursor_near(c.x, c.y - 10.0 * s, 150.0 * s);
                if poked && c.state != 2 {
                    c.state = 2;
                    c.timer = self.range(3.0, 5.0);
                    c.squash = 1.0;
                }
                match c.state {
                    0 => {
                        c.anim += dt * 9.0;
                        c.x += c.dir * 30.0 * s * dt;
                        if self.rand() < dt * 0.18 {
                            c.state = 1;
                            c.timer = self.range(1.0, 2.5);
                        }
                        // curious about a slow pointer nearby
                        if cursor.valid
                            && cursor_speed < 100.0 * s
                            && self.cursor_near(c.x, c.y, 350.0 * s)
                            && self.rand() < dt * 0.5
                        {
                            c.state = 1;
                            c.timer = 1.5;
                        }
                        for i in 0..self.parts.len() {
                            let p = self.parts[i];
                            if p.kind != K_LEAF || p.state != RESTING || !p.on_ground || p.heaped {
                                continue;
                            }
                            let ahead = (p.x - c.x) * c.dir;
                            // a nudge with the snout: the leaf slides a little way ahead and comes to rest again
                            if ahead > 4.0 * s && ahead < 14.0 * s && p.vx.abs() < 10.0 * s {
                                let v = self.range(70.0, 130.0);
                                let p = &mut self.parts[i];
                                p.vx = c.dir * v * s;
                                p.rest = p.rest.max(20.0);
                            }
                        }
                        let leaf = if c.count < 4 { self.nearest_ground_leaf(c.x + c.dir * 8.0 * s, 10.0 * s) } else { None };
                        if let Some(leaf) = leaf {
                            if self.rand() < dt * 1.5 {
                                c.carried[c.count as usize] = self.parts[leaf].color;
                                c.count += 1;
                                self.parts[leaf].state = FADING;
                                self.parts[leaf].alpha = 0.001;
                            }
                        }
                    }
                    1 => {
                        c.aux += dt * 25.0; // nose wiggle
                        c.timer -= dt;
                        if c.timer <= 0.0 {
                            c.state = 0;
                        }
                    }
                    2 => {
                        c.timer -= dt;
                        c.aux2 = if poked { 1.0 } else { (c.aux2 - dt * 2.0).max(0.0) }; // trembling while the pointer hovers
                        if poked {
                            c.timer = c.timer.max(1.0);
                        }
                        if c.timer <= 0.0 {
                            c.state = 3;
                            c.timer = 0.9;
                        }
                    }
                    _ => {
                        c.timer -= dt;
                        if c.timer <= 0.0 {
                            c.state = 0;
                        }
                    }
                }
                let curl = match c.state {
                    2 => 1.0,
                    3 => 0.55,
                    _ => 0.0,
                };
                c.grow += (curl - c.grow) * (dt * 7.0).min(1.0);
                self.critter_on_ground(c, tr);
                !(c.t > 2.0 && offscreen)
            }
            CATERPILLAR => {
                // inches towards the nearest fallen leaf, munches it away crumb by crumb, wanders off after a few
                c.alpha = (c.alpha + dt).min(1.0);
                let poked = self.cursor_near(c.x, c.y - 6.0 * s, 120.0 * s);
                if poked && c.state != 2 && c.state != 3 {
                    c.state = 2;
                    c.timer = 2.5;
                }
                if c.t > 240.0 && c.state != 3 {
                    c.state = 3;
                }
                if c.state == 0 || c.state == 3 {
                    if c.state == 0 {
                        match self.nearest_ground_leaf(c.x, w) {
                            None => c.state = 3,
                            Some(leaf) => {
                                let d = self.parts[leaf].x - c.x;
                                c.dir = if d > 0.0 { 1.0 } else { -1.0 };
                                if d.abs() < 7.0 * s {
                                    c.state = 1;
                                    c.timer = 5.0;
                                }
                            }
                        }
                    }
                    if c.state == 3 && c.t < 3.0 {
                        c.dir = if c.x < w / 2.0 { -1.0 } else { 1.0 };
                    }
                    c.anim += dt * 2.4;
                    c.x += c.dir * 26.0 * s * c.anim.sin().max(0.0) * dt; // moves only while stretching
                } else if c.state == 1 {
                    c.anim += dt * 10.0;
                    c.timer -= dt;
                    let leaf = self.nearest_ground_leaf(c.x, 16.0 * s);
                    if let Some(l) = leaf {
                        let p = &mut self.parts[l];
                        p.size *= 1.0 - dt * 0.12;
                        p.rest = p.rest.max(3.0);
                        if self.rand() < dt * 5.0 {
                            self.splash(c.x + c.dir * 7.0 * s, c.y - 3.0 * s, 0.5, 2, 1, 0.35); // crumbs
                        }
                    }
                    if c.timer <= 0.0 || leaf.is_none() {
                        if let Some(l) = leaf {
                            self.parts[l].state = FADING;
                            self.parts[l].alpha = 0.001;
                        }
                        c.count += 1;
                        c.state = if c.count >= 3 { 3 } else { 0 };
                    }
                } else {
                    c.aux += dt * 14.0; // wobble while curled up
                    c.timer -= dt;
                    if c.timer <= 0.0 {
                        c.state = 0;
                    }
                }
                self.critter_on_ground(c, tr);
                !(c.state == 3 && offscreen)
            }
            SNOW_FOX => {
                // pops out of a drift in a puff of snow, listens with its head cocked, wiggles, pounces in an arc
                // and dives head first back into the snow
                let puff = |sim: &mut Sim, c: &Critter, n: i32, up: f32| {
                    for _ in 0..n {
                        if sim.parts.len() >= MAX_PARTICLES {
                            break;
                        }
                        let mut p = sim.new_snow(0.0);
                        p.x = c.x + sim.range(-14.0, 14.0) * s;
                        p.y = c.y - 2.0;
                        p.vx = c.vx * 0.2 + sim.range(-120.0, 120.0) * s;
                        p.vy = -sim.range(up * 0.4, up) * s;
                        sim.parts.push(p);
                    }
                };
                match c.state {
                    0 => {
                        if c.grow == 0.0 {
                            self.critter_on_ground(c, tr);
                            puff(self, c, 14, 260.0);
                        }
                        c.grow = (c.grow + dt / 0.7).min(1.0);
                        if c.grow >= 1.0 {
                            c.state = 1;
                            c.timer = self.range(1.5, 3.0);
                        }
                        self.critter_on_ground(c, tr);
                    }
                    1 => {
                        c.anim += dt;
                        c.aux = (c.t * 1.7).sin() * 12.0; // head cocked, listening
                        c.aux2 = if (c.t * 9.0).sin() > 0.8 { 1.0 } else { 0.0 }; // ear twitch
                        c.timer -= dt;
                        if self.cursor_near(c.x, c.y - 20.0 * s, 220.0 * s) {
                            c.timer = 0.0;
                            c.dir = if cursor.x1 > c.x { -1.0 } else { 1.0 };
                        }
                        if c.timer <= 0.0 {
                            c.state = 4;
                            c.timer = if self.cursor_near(c.x, c.y - 20.0 * s, 220.0 * s) { 0.2 } else { 0.6 };
                        }
                        self.critter_on_ground(c, tr);
                    }
                    4 => {
                        // crouch and wiggle before the pounce
                        c.squash = 1.0;
                        c.aux = 0.0;
                        c.timer -= dt;
                        if c.timer <= 0.0 {
                            c.state = 2;
                            c.vx = c.dir * self.range(180.0, 280.0) * s;
                            c.vy = -self.range(480.0, 600.0) * s;
                        }
                        self.critter_on_ground(c, tr);
                    }
                    2 => {
                        c.vy += 1400.0 * s * dt;
                        c.x += c.vx * dt;
                        c.y += c.vy * dt;
                        c.angle = c.vy.atan2(c.vx.abs()).to_degrees();
                        let gy = self.surface_y(c.x);
                        if c.vy > 0.0 && c.y >= gy {
                            c.y = gy;
                            c.state = 3;
                            puff(self, c, 30, 380.0);
                        }
                        c.hidden = tr.covered(c.x, c.y - 10.0 * s, i32::MAX, true);
                    }
                    _ => {
                        c.grow -= dt / 0.45;
                        c.angle = 70.0;
                        c.aux2 = (c.t * 30.0).sin() * 8.0; // a last flick of the tail
                        if c.grow <= 0.0 {
                            if c.count == 0 && self.rand() < 0.5 {
                                // and once more, from a little further along
                                c.count = 1;
                                let off = self.range(120.0, 300.0);
                                c.x = (c.x + c.dir * off * s).clamp(30.0 * s, w - 30.0 * s);
                                c.state = 0;
                                c.grow = 0.0;
                                c.angle = 0.0;
                                c.vx = 0.0;
                            } else {
                                return false;
                            }
                        }
                    }
                }
                !offscreen
            }
            SNOWDROPS | MUSHROOMS => {
                // grow up, sway in the wind, spring back when brushed by the pointer, then wilt
                c.timer -= dt;
                let (wilt, rise) = if c.kind == SNOWDROPS { (20.0, 15.0) } else { (15.0, 10.0) };
                if c.timer > wilt {
                    c.grow = (c.grow + dt / rise).min(1.0);
                } else {
                    c.alpha = (c.timer / wilt).max(0.0);
                }
                let mut target = (c.t * 1.3 + c.seed).sin() * 4.0 + self.mix.wind_px * 0.02;
                if self.cursor_near(c.x, c.y - 20.0 * s, 130.0 * s) {
                    target += (cursor.vx / s * 0.03).clamp(-30.0, 30.0);
                    if c.kind == MUSHROOMS && cursor_speed > 200.0 * s {
                        c.squash = 1.0; // boing
                    }
                }
                c.vy += ((target - c.angle) * 30.0 - c.vy * 4.0) * dt; // a springy stem
                c.angle += c.vy * dt;
                self.critter_on_ground(c, tr);
                c.timer > 0.0
            }
            SNAIL => {
                // crawls very slowly after the rain leaving a glistening trail; hides in its shell when disturbed
                if c.t > 300.0 {
                    c.alpha -= dt / 3.0;
                } else {
                    c.alpha = (c.alpha + dt).min(1.0);
                }
                if self.cursor_near(c.x, c.y - 6.0 * s, 130.0 * s) {
                    c.state = 1;
                    c.timer = 4.0;
                }
                if c.state == 1 {
                    c.timer -= dt;
                    if c.timer <= 0.0 {
                        c.state = 0;
                    }
                }
                let (target, speed) = if c.state == 0 { (1.0, 1.5) } else { (0.0, 9.0) };
                c.grow += (target - c.grow) * (dt * speed).min(1.0);
                if c.state == 0 && c.grow > 0.9 {
                    let step = c.dir * 7.0 * s * (0.7 + 0.3 * (c.t * 2.0).sin()) * dt;
                    c.x += step;
                    c.anim += step.abs();
                    c.aux += dt * 3.0; // the foot ripples as it glides
                    if c.anim > 5.0 * s {
                        c.anim = 0.0;
                        c.trail.push_back((c.x - c.dir * 6.0 * s, c.y - 0.6 * s));
                        c.trail_age.push_back(0.0);
                    }
                }
                for a in c.trail_age.iter_mut() {
                    *a += dt;
                }
                while c.trail_age.front().is_some_and(|&a| a > 40.0) || c.trail.len() > 300 {
                    c.trail.pop_front();
                    c.trail_age.pop_front();
                }
                self.critter_on_ground(c, tr);
                c.alpha > 0.0 && !offscreen
            }
            FROG => {
                // sits by the puddles with a pulsing throat, snaps passing drops and flakes with its tongue,
                // hops from puddle to puddle (crouch, leap, plop) and leaps away when poked
                c.alpha = (c.alpha + dt).min(1.0);
                let leaving = c.count > 6 || c.t > 90.0;
                c.aux = (c.aux - dt).max(0.0); // tongue out
                c.aux2 = (c.aux2 - dt).max(0.0); // tongue cooldown
                if c.state == 0 {
                    c.anim += dt * 6.0;
                    c.timer -= dt;
                    let poked = self.cursor_near(c.x, c.y - 8.0 * s, 160.0 * s);
                    if poked {
                        c.dir = if cursor.x1 > c.x { -1.0 } else { 1.0 };
                        c.timer = 0.0;
                    }
                    if c.aux2 <= 0.0 && !poked {
                        for i in 0..self.parts.len() {
                            let p = self.parts[i];
                            if p.state != FALLING || (p.kind != K_SNOW && p.kind != K_RAIN && p.kind != K_LEAF) {
                                continue;
                            }
                            let (dx, dy) = ((p.x - c.x) * c.dir, p.y - c.y);
                            if dx > 6.0 * s && dx < 60.0 * s && dy > -70.0 * s && dy < -6.0 * s {
                                c.vx = dx / s;
                                c.vy = dy / s;
                                c.aux = 0.25;
                                c.aux2 = self.range(1.5, 4.0);
                                self.parts[i].y = h + 5000.0; // gulp
                                break;
                            }
                        }
                    }
                    if c.timer <= 0.0 {
                        c.state = 3;
                        c.timer = if poked { 0.08 } else { 0.2 };
                    }
                    self.critter_on_ground(c, tr);
                } else if c.state == 3 {
                    c.squash = 1.0; // crouch
                    c.timer -= dt;
                    if c.timer <= 0.0 {
                        let poked = self.cursor_near(c.x, c.y - 8.0 * s, 160.0 * s);
                        let mut target = if leaving { -1.0 } else { self.find_puddle(c.x, c.dir) };
                        if target < 0.0 && !leaving && self.rand() < 0.5 {
                            c.dir = -c.dir;
                            target = self.find_puddle(c.x, c.dir);
                        }
                        let dx = if target > 0.0 {
                            target - c.x
                        } else {
                            c.dir * self.range(60.0, 160.0) * s * if poked || leaving { 2.0 } else { 1.0 }
                        };
                        if target > 0.0 {
                            c.dir = if dx > 0.0 { 1.0 } else { -1.0 };
                        }
                        let t = 0.55;
                        c.vx = dx / t;
                        c.vy = -0.5 * 1600.0 * s * t;
                        c.state = 1;
                        c.count += 1;
                    }
                    self.critter_on_ground(c, tr);
                } else {
                    c.vy += 1600.0 * s * dt;
                    c.x += c.vx * dt;
                    c.y += c.vy * dt;
                    c.angle = c.vy.atan2(c.vx.abs() + 1.0).to_degrees() * 0.5;
                    let gy = self.surface_y(c.x);
                    if c.vy > 0.0 && c.y >= gy {
                        c.y = gy;
                        c.state = 0;
                        c.angle = 0.0;
                        c.squash = 1.0;
                        c.vx = 0.0;
                        c.vy = 0.0;
                        c.timer = if leaving { 0.1 } else { self.range(1.5, 5.0) };
                        if let Some(depth) = self.puddle_at(c.x) {
                            // plop!
                            self.splash(c.x, gy - depth, 1.0, 0, 6, 1.4);
                            self.ripple(c.x, gy - depth, 1.4);
                            let b = ((c.x / BF) as i32).clamp(0, self.ground.wave_v.len() as i32 - 1) as usize;
                            self.ground.wave_v[b] -= 60.0 * s;
                        }
                    }
                    c.hidden = tr.covered(c.x, c.y - 8.0 * s, i32::MAX, true);
                }
                !offscreen
            }
            ROBIN => {
                // flies in, lands with a little bob, hops and pecks along a window's top edge, flicks its tail,
                // cocks its head at the pointer, and flies off when it comes too close or the window moves
                let win = if c.host != 0 { tr.get(c.host).copied() } else { None };
                if c.host != 0 && !win.is_some_and(|w| w.surface) && c.state != 2 {
                    c.state = 2;
                    c.vx = c.dir * 320.0 * s;
                    c.vy = -200.0 * s;
                }
                let perch = |sim: &Sim, c: &Critter| -> (f32, f32) {
                    match &win {
                        Some(w) => {
                            let tx = w.l + c.offx;
                            (tx, w.t - sim.cap_at(w, tx))
                        }
                        None => (c.offx, sim.surface_y(c.offx)),
                    }
                };
                if c.state == 0 {
                    let (tx, ty) = perch(self, c);
                    let (dx, dy) = (tx - c.x, ty - c.y);
                    let d = (dx * dx + dy * dy).sqrt();
                    c.anim += dt * 18.0;
                    c.dir = if dx > 0.0 { 1.0 } else { -1.0 };
                    let v = (d / dt).min(420.0 * s);
                    if d < 6.0 * s {
                        c.state = 1;
                        c.timer = self.range(15.0, 35.0);
                        c.x = tx;
                        c.y = ty;
                        c.squash = 1.0;
                    } else {
                        c.x += dx / d * v * dt;
                        c.y += dy / d * v * dt + (c.t * 9.0).sin() * 20.0 * s * dt;
                    }
                    c.hidden = false;
                } else if c.state == 1 {
                    c.timer -= dt;
                    c.grow = (c.grow - dt * 4.0).max(0.0); // hop arc
                    c.anim = (c.anim - dt * 5.0).max(0.0); // peck
                    c.aux2 = (c.aux2 - dt * 4.0).max(0.0); // tail flick
                    if c.grow <= 0.0 && c.anim <= 0.0 {
                        let r = self.rand();
                        if r < dt * 0.7 {
                            let span = win.map(|w| w.r - w.l).unwrap_or(w);
                            let hop = self.range(-20.0, 20.0);
                            c.offx = (c.offx + hop * s).clamp(20.0 * s, span - 20.0 * s);
                            if self.rand() < 0.3 {
                                c.dir = -c.dir;
                            }
                            c.grow = 1.0;
                        } else if r < dt * 1.6 {
                            c.anim = 1.0;
                        } else if r < dt * 2.2 {
                            c.aux2 = 1.0;
                        }
                    }
                    let (tx, ty) = perch(self, c);
                    c.x = tx;
                    c.y = ty;
                    c.hidden = match &win {
                        Some(w) => tr.covered(c.x, c.y - 4.0 * s, w.z, false),
                        None => tr.covered(c.x, c.y - 4.0 * s, i32::MAX, true),
                    };
                    let scared = self.cursor_near(c.x, c.y - 8.0 * s, 200.0 * s) || win.is_some_and(|w| self.fast(&w, dt));
                    if scared || c.timer <= 0.0 || c.hidden {
                        c.state = 2;
                        c.dir = if cursor.valid && cursor.x1 > c.x {
                            -1.0
                        } else if scared {
                            c.dir
                        } else if self.rand() < 0.5 {
                            -1.0
                        } else {
                            1.0
                        };
                        c.face = c.dir;
                        c.vx = c.dir * self.range(250.0, 400.0) * s;
                        c.vy = -self.range(150.0, 300.0) * s;
                        c.hidden = false;
                    }
                } else {
                    c.anim += dt * 22.0;
                    c.vy -= 200.0 * s * dt;
                    c.x += c.vx * dt;
                    c.y += c.vy * dt;
                    if c.y < -60.0 * s {
                        return false;
                    }
                }
                !(c.state == 2 && offscreen)
            }
            SQUIRREL => {
                // bounds along with an acorn, sits up to nibble it with its tail twitching; a nearby pointer makes it
                // freeze, stare, and then dash off the other way
                let poked = self.cursor_near(c.x, c.y - 10.0 * s, 170.0 * s);
                c.aux2 = (c.aux2 - dt * 3.0).max(0.0);
                if poked && c.state == 0 && c.timer > -1.0 {
                    c.state = 2;
                    c.timer = 0.35;
                }
                if c.state == 0 {
                    let dashing = c.timer < -0.5;
                    c.anim += dt * if dashing { 22.0 } else { 14.0 };
                    c.x += c.dir * if dashing { 520.0 } else { 240.0 } * s * dt;
                    c.timer = (c.timer + dt).min(0.0);
                    if !poked && !dashing && self.rand() < dt * 0.25 {
                        c.state = 1;
                        c.timer = self.range(1.5, 3.0);
                    }
                } else if c.state == 1 {
                    c.aux += dt * 12.0; // nibbling
                    if self.rand() < dt * 1.2 {
                        c.aux2 = 1.0;
                    }
                    c.timer -= dt;
                    if poked {
                        c.state = 2;
                        c.timer = 0.35;
                    } else if c.timer <= 0.0 {
                        c.state = 0;
                    }
                } else {
                    // frozen, staring at the pointer
                    c.timer -= dt;
                    if c.timer <= 0.0 {
                        c.state = 0;
                        if cursor.valid {
                            c.dir = if cursor.x1 > c.x { -1.0 } else { 1.0 };
                        }
                        c.timer = -2.5; // dash for a while
                    }
                }
                self.critter_on_ground(c, tr);
                !(c.t > 2.0 && offscreen)
            }
            _ => false,
        }
    }
}
