//! CPU side of the garden: time-of-day grade, weather blend, swaying, leaves,
//! fireflies, rain, cursor ripples and the water basin. Produces one constant
//! buffer and a small particle list per frame; all heavy lifting is in shaders.

use crate::scene::{FrameCb, Particle, MAX_RIPPLES, P_DISC, P_LEAF, P_RING, P_STREAK};

// ---------- tiny deterministic RNG (no dependency) ----------
pub struct Rng(u64);
impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407) | 1)
    }
    pub fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        ((self.0 >> 40) as f32) / ((1u64 << 24) as f32)
    }
    pub fn range(&mut self, a: f32, b: f32) -> f32 {
        a + self.next() * (b - a)
    }
}

// ---------- time of day (scene.json: time_of_day_grade.keys) ----------
struct Key {
    h: f32,
    mul: [f32; 3],
    screen: [f32; 4],
    night: f32,
}
const KEYS: [Key; 9] = [
    Key { h: 0.0, mul: [70.0, 86.0, 140.0], screen: [0.0, 0.0, 0.0, 0.0], night: 1.0 },
    Key { h: 5.0, mul: [90.0, 105.0, 150.0], screen: [0.0, 0.0, 0.0, 0.0], night: 0.8 },
    Key { h: 6.5, mul: [238.0, 252.0, 246.0], screen: [255.0, 255.0, 255.0, 0.03], night: 0.0 },
    Key { h: 10.0, mul: [252.0, 255.0, 248.0], screen: [255.0, 255.0, 240.0, 0.07], night: 0.0 },
    Key { h: 15.0, mul: [255.0, 250.0, 236.0], screen: [255.0, 250.0, 230.0, 0.05], night: 0.0 },
    Key { h: 16.8, mul: [255.0, 196.0, 140.0], screen: [255.0, 150.0, 70.0, 0.09], night: 0.0 },
    Key { h: 18.3, mul: [230.0, 160.0, 140.0], screen: [255.0, 120.0, 60.0, 0.05], night: 0.1 },
    Key { h: 19.6, mul: [80.0, 96.0, 150.0], screen: [0.0, 0.0, 0.0, 0.0], night: 0.9 },
    Key { h: 24.0, mul: [70.0, 86.0, 140.0], screen: [0.0, 0.0, 0.0, 0.0], night: 1.0 },
];

/// Grade at a given hour: multiply rgb (0..1), screen rgb (0..1) + alpha, night 0..1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Grade {
    pub mul: [f32; 3],
    pub screen: [f32; 4],
    pub night: f32,
}
impl Grade {
    pub fn at(hour: f32) -> Grade {
        let h = hour.rem_euclid(24.0);
        for w in KEYS.windows(2) {
            let (a, b) = (&w[0], &w[1]);
            if h >= a.h && h <= b.h {
                let u = (h - a.h) / (b.h - a.h);
                let m = |x: f32, y: f32| x + (y - x) * u;
                return Grade {
                    mul: [0, 1, 2].map(|i| m(a.mul[i], b.mul[i]) / 255.0),
                    screen: [m(a.screen[0], b.screen[0]) / 255.0, m(a.screen[1], b.screen[1]) / 255.0, m(a.screen[2], b.screen[2]) / 255.0, m(a.screen[3], b.screen[3])],
                    night: m(a.night, b.night),
                };
            }
        }
        Grade::at(0.0)
    }
    fn lerp(&self, o: &Grade, u: f32) -> Grade {
        let l = |a: f32, b: f32| a + (b - a) * u;
        Grade {
            mul: [0, 1, 2].map(|i| l(self.mul[i], o.mul[i])),
            screen: [0, 1, 2, 3].map(|i| l(self.screen[i], o.screen[i])),
            night: l(self.night, o.night),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Sky {
    Clear,
    Cloudy,
    Rain,
}

struct Leaf {
    x: f32,
    y: f32,
    born: f32,
    r: f32,
    color: [f32; 3],
}

struct Ripple {
    x: f32,
    y: f32,
    born: f32,
}

pub struct Garden {
    rng: Rng,
    pub t: f32,
    // time of day: the shown grade eases toward the target grade
    shown: Grade,
    // weather blend weights (eased): cloudy, rain
    cloud: f32,
    rain: f32,
    pub sky: Sky,
    pub hour: f32,
    leaves: Vec<Leaf>,
    last_leaf: f32,
    flies: Vec<(f32, f32, f32)>,
    ripples: Vec<Ripple>,
    last_ripple: f32,
    // water basin
    pub basin_enabled: bool,
    pub water: f32,
    spout_phase: f32,
    splash: f32,
    /// TODO(v0.2 break reminder): set to 0 to stop the wind when a break is due.
    pub wind: f32,
    particles: Vec<Particle>,
}

const LEAF_COLORS: [[f32; 3]; 3] = [[0x8E as f32, 0x2F as f32, 0x24 as f32], [0xB0 as f32, 0x4A as f32, 0x2E as f32], [0xA0 as f32, 0x40 as f32, 0x2C as f32]];

impl Garden {
    pub fn new(hour: f32) -> Garden {
        let mut rng = Rng::new(7);
        let flies = (0..40).map(|_| (rng.range(0.0, 1920.0), rng.range(0.0, 1080.0), rng.range(0.0, 6.0))).collect();
        Garden {
            rng,
            t: 0.0,
            shown: Grade::at(hour),
            cloud: 0.0,
            rain: 0.0,
            sky: Sky::Clear,
            hour,
            leaves: Vec::new(),
            last_leaf: -10.0,
            flies,
            ripples: Vec::new(),
            last_ripple: -1.0,
            basin_enabled: false,
            water: 1.0,
            spout_phase: 0.0,
            splash: 0.0,
            wind: 1.0,
            particles: Vec::with_capacity(crate::scene::MAX_PARTICLES),
        }
    }

    /// Jump straight to the current target (used at startup so there is no fade-in).
    pub fn settle(&mut self) {
        self.shown = Grade::at(self.hour);
        self.cloud = (self.sky == Sky::Cloudy) as u8 as f32;
        self.rain = (self.sky == Sky::Rain) as u8 as f32;
    }

    pub fn raining(&self) -> bool {
        self.rain > 0.01
    }

    /// Is anything moving beyond the gentle sway / leaves / fireflies? (decides 24 vs 8 fps)
    pub fn busy(&self) -> bool {
        let target = Grade::at(self.hour);
        let grading = (0..3).any(|i| (self.shown.mul[i] - target.mul[i]).abs() > 0.002);
        let w_target = (self.sky == Sky::Rain) as u8 as f32;
        let c_target = (self.sky == Sky::Cloudy) as u8 as f32;
        self.raining() || !self.ripples.is_empty() || self.splash > 0.01 || grading || (self.rain - w_target).abs() > 0.01 || (self.cloud - c_target).abs() > 0.01
    }

    /// Cursor at design coords over the raked gravel: spawn a ripple at most every 70 ms.
    pub fn rake(&mut self, x: f32, y: f32) {
        if self.t - self.last_ripple >= 0.07 && self.ripples.len() < MAX_RIPPLES {
            self.ripples.push(Ripple { x, y, born: self.t });
            self.last_ripple = self.t;
        }
    }

    pub fn drink(&mut self) {
        self.splash = 1.0;
    }

    pub fn step(&mut self, dt: f32) -> (FrameCb, &[Particle]) {
        let dt = dt.min(0.25);
        self.t += dt;
        let t = self.t;

        // Ease grade and weather. Slow enough to never look sudden (~3 s time constant);
        // with the auto clock the target itself only moves a few minutes' worth per minute.
        let k = 1.0 - (-dt / 3.0).exp();
        let target = Grade::at(self.hour);
        self.shown = self.shown.lerp(&target, k);
        let kw = 1.0 - (-dt / 4.0).exp();
        self.cloud += (((self.sky == Sky::Cloudy) as u8 as f32) - self.cloud) * kw;
        self.rain += (((self.sky == Sky::Rain) as u8 as f32) - self.rain) * kw;
        let (cl, rn) = (self.cloud, self.rain);

        let mut cb = FrameCb::default();
        cb.time = t;
        let sway = |amp: f32, ph: f32| self.wind * (amp * (t * 0.9 + ph).sin() + amp * 0.3 * (t * 2.3 + ph).sin());
        cb.sway_fern = sway(0.012, 0.0);
        cb.sway_canopy = sway(0.006, 1.0);
        let g = self.shown;
        cb.grade_mul = g.mul;
        cb.grade_screen = g.screen;
        cb.night = g.night;
        // weather filters (scene.json rain_grade / overcast_grade)
        cb.desat = 0.38 * rn + 0.30 * cl;
        cb.contrast = 1.0 + 0.08 * rn;
        cb.bright = 1.0 - 0.10 * rn - 0.08 * cl;
        cb.layer_desat = 0.35 * rn + 0.25 * cl;
        cb.layer_bright = 1.0 - 0.10 * rn - 0.06 * cl;
        let wm = |c: f32, r: f32| 1.0 + (c / 255.0 - 1.0) * cl + (r / 255.0 - 1.0) * rn;
        cb.weather_mul = [wm(214.0, 202.0), wm(222.0, 218.0), wm(232.0, 234.0)];

        // basin
        self.splash = (self.splash - dt * 0.6).max(0.0);
        self.spout_phase = (self.spout_phase + dt * (1.3 + self.splash * 2.0)).fract();
        if self.basin_enabled {
            cb.basin_level = 0.35 + 0.65 * self.water.clamp(0.0, 1.0);
            cb.spout_phase = self.spout_phase;
            cb.splash = self.splash;
        }

        // ripples
        self.ripples.retain(|r| t - r.born < 1.6);
        cb.ripple_count = self.ripples.len() as f32;
        for (i, r) in self.ripples.iter().enumerate() {
            cb.ripples[i] = [r.x, r.y, t - r.born, 0.0];
        }

        self.particles.clear();
        self.push_leaves(t);
        let night = g.night * (1.0 - rn);
        if g.night > 0.3 && night > 0.0 {
            self.push_flies(t, night);
        }
        if rn > 0.01 {
            self.push_rain(rn);
        }
        (cb, &self.particles)
    }

    fn push_leaves(&mut self, t: f32) {
        if t - self.last_leaf > 1.4 && self.leaves.len() < 22 {
            self.last_leaf = t;
            let c = LEAF_COLORS[(self.rng.next() * 3.0) as usize % 3];
            self.leaves.push(Leaf { x: self.rng.range(30.0, 495.0), y: self.rng.range(30.0, 180.0), born: t, r: self.rng.range(0.0, 6.0), color: c });
        }
        self.leaves.retain(|l| t - l.born < 9.0);
        let wind = self.wind;
        for l in &self.leaves {
            let age = t - l.born;
            // scene.json leaves: fall 34 px/s, drift 20 px/s; sway and size from the mockup (x1.5)
            let x = l.x + age * 20.0 + (age * 1.7 + l.r).sin() * 27.0 * wind;
            let y = l.y + age * 34.0;
            let rot = l.r + age * 1.4;
            let sq = 0.55 + 0.45 * (age * 2.0 + l.r).cos().abs();
            let r = 10.5;
            let fade = (age / 0.4).min(1.0) * ((9.0 - age) / 0.6).min(1.0);
            let leaf = |px: f32, py: f32, col: [f32; 4]| Particle {
                pos: [px, py],
                ext: [r + 2.0, r + 2.0],
                col,
                prm: [P_LEAF, 0.0, r, 1.0],
                shp: [rot, sq, 0.0, 0.0],
            };
            self.particles.push(leaf(x + 9.0, y + 13.5, [0.0, 0.0, 0.0, 0.25 * fade]));
            self.particles.push(leaf(x, y, [l.color[0] / 255.0, l.color[1] / 255.0, l.color[2] / 255.0, fade]));
        }
    }

    fn push_flies(&mut self, t: f32, night: f32) {
        for &(fx, fy, p) in &self.flies {
            let a = (t * 2.2 + p).sin().max(0.0).powi(2) * night;
            if a < 0.01 {
                continue;
            }
            let x = fx + 18.0 * (t * 0.6 + p).sin();
            let y = fy + 15.0 * (t * 0.5 + p).cos();
            let disc = |r: f32, col: [f32; 4]| Particle { pos: [x, y], ext: [r + 2.0, r + 2.0], col, prm: [P_DISC, 0.0, 0.0, 0.0], shp: [r, r, 0.0, 0.0] };
            self.particles.push(disc(7.5, [1.0, 240.0 / 255.0, 150.0 / 255.0, a * 0.25]));
            self.particles.push(disc(2.7, [1.0, 246.0 / 255.0, 175.0 / 255.0, a]));
        }
    }

    fn push_rain(&mut self, rn: f32) {
        let rng = &mut self.rng;
        let n = |c: f32| (c * rn).round() as usize;
        // drop rings on stones, sand and moss (fresh every frame)
        for _ in 0..n(26.0) {
            let (x, y, r) = (rng.range(0.0, 1920.0), rng.range(0.0, 1080.0), rng.range(3.0, 9.0));
            self.particles.push(Particle { pos: [x, y], ext: [r + 2.0, r + 2.0], col: [220.0 / 255.0, 232.0 / 255.0, 236.0 / 255.0, 0.27], prm: [P_RING, 0.0, 1.0, 0.0], shp: [r, r * 0.9, 0.0, 0.0] });
        }
        // rings on the basin water
        for _ in 0..n(4.0) {
            let (x, y, r) = (245.0 + rng.range(-30.0, 30.0), 600.0 + rng.range(-27.0, 27.0), rng.range(4.5, 12.0));
            self.particles.push(Particle { pos: [x, y], ext: [r + 2.0, r + 2.0], col: [210.0 / 255.0, 236.0 / 255.0, 232.0 / 255.0, 0.43], prm: [P_RING, 0.0, 1.0, 0.0], shp: [r, r, 0.0, 0.0] });
        }
        // streaks: slant 0.25, length 14..30, alpha .16..35
        let ang = (1.0f32).atan2(0.25);
        for _ in 0..n(420.0) {
            let (x, y, l) = (rng.range(-90.0, 1920.0), rng.range(-60.0, 1080.0), rng.range(14.0, 30.0));
            let a = rng.range(0.16, 0.35);
            let len = l * (1.0f32 + 0.0625).sqrt();
            self.particles.push(Particle {
                pos: [x + l * 0.125, y + l * 0.5],
                ext: [len * 0.5, 0.6],
                col: [225.0 / 255.0, 232.0 / 255.0, 240.0 / 255.0, a],
                prm: [P_STREAK, ang, 1.0, 0.0],
                shp: [0.0; 4],
            });
        }
    }
}
