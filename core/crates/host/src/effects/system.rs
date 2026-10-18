//! Emitter simulation: playing emitter sets → per-frame textured quads.
//!
//! Deterministic per frame (one `step` = one game frame; every random draw comes from each
//! emitter's own linear congruential generator, seeded from the play order). The model follows
//! how the game's effect runtime emits and moves particles (private notes, `effects.md`):
//! emission every interval + 1 frames from the start frame (for the duration, or until
//! stopped), particles placed by the emitter shape, launched along the shape normal, a
//! direction (with a spread cone), random and inherited velocity; each frame position +=
//! velocity, velocity × air resistance + gravity; colour, alpha and scale from their key
//! tables over the life; texture cells from the pattern animation. Everything is simulated in
//! 3D and drawn with an orthographic side view (x right, y up, z towards the camera dropped).

use std::sync::{Arc, OnceLock};

use super::library::{Asset, ColorMode, EmitterDef};

/// Blend of a quad (the emitter's render state).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Blend {
    /// src·a + dst·(1 − a)
    #[default]
    Alpha = 0,
    /// src·a + dst
    Add = 1,
    /// dst − src·a
    Sub = 2,
    /// dst·src
    Mul = 3,
    /// src·(1 − dst) + dst
    Screen = 4,
    /// src + dst·(1 − a)
    Premul = 5,
}

impl Blend {
    pub fn from_raw(v: u8) -> Self {
        match v {
            1 => Self::Add,
            2 => Self::Sub,
            3 => Self::Mul,
            4 => Self::Screen,
            5 => Self::Premul,
            _ => Self::Alpha,
        }
    }
}

/// One textured quad of an effect, in world pixels (y up).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FxQuad {
    /// Effect texture name index (see [`EffectPlayer::texture_names`]); None: untextured.
    pub texture: Option<u32>,
    /// Corners: bottom-left, bottom-right, top-right, top-left (before rotation).
    pub pos: [[f32; 2]; 4],
    /// Texture coordinates of the corners ((0, 0) = top-left of the texture).
    pub uv: [[f32; 2]; 4],
    /// Colour multiplier (straight alpha; colour may exceed 1).
    pub color: [f32; 4],
    pub blend: Blend,
    /// Texture wrap per axis (0 mirror, 1 repeat, 2 clamp, 3 mirror once) and nearest sampling.
    pub wrap: [u8; 2],
    pub nearest: bool,
}

/// Handle of a playing effect (to move or stop it).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FxHandle(pub u32);

type V3 = [f32; 3];

fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn mul(a: V3, s: f32) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}
fn hadamard(a: V3, b: V3) -> V3 {
    [a[0] * b[0], a[1] * b[1], a[2] * b[2]]
}
fn len(a: V3) -> f32 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}
fn norm(a: V3) -> V3 {
    let l = len(a);
    if l > 1e-6 { mul(a, 1.0 / l) } else { [0.0; 3] }
}

/// Rotation matrix (rows) for Euler angles applied X, then Y, then Z.
fn rot_matrix(r: V3) -> [V3; 3] {
    let (sx, cx) = r[0].sin_cos();
    let (sy, cy) = r[1].sin_cos();
    let (sz, cz) = r[2].sin_cos();
    [
        [cy * cz, sx * sy * cz - cx * sz, cx * sy * cz + sx * sz],
        [cy * sz, sx * sy * sz + cx * cz, cx * sy * sz - sx * cz],
        [-sy, sx * cy, cx * cy],
    ]
}

fn apply(m: &[V3; 3], v: V3) -> V3 {
    [
        m[0][0] * v[0] + m[0][1] * v[1] + m[0][2] * v[2],
        m[1][0] * v[0] + m[1][1] * v[1] + m[1][2] * v[2],
        m[2][0] * v[0] + m[2][1] * v[1] + m[2][2] * v[2],
    ]
}

/// Scale, rotate, translate.
#[derive(Clone, Copy, Debug)]
struct Xform {
    scale: V3,
    rot: [V3; 3],
    pos: V3,
}

impl Xform {
    fn new(scale: V3, rot: V3, pos: V3) -> Self {
        Self {
            scale,
            rot: rot_matrix(rot),
            pos,
        }
    }
    fn point(&self, v: V3) -> V3 {
        add(apply(&self.rot, hadamard(v, self.scale)), self.pos)
    }
    fn vector(&self, v: V3) -> V3 {
        apply(&self.rot, hadamard(v, self.scale))
    }
}

/// The game's random generator step (a 32-bit LCG); returns the value in [0, 1) drawn from the
/// state before the step.
#[derive(Clone, Copy, Debug)]
struct Rng(u32);

impl Rng {
    fn next(&mut self) -> f32 {
        let v = self.0;
        self.0 = self.0.wrapping_mul(0x41c6_4e6d).wrapping_add(0x3039);
        (v >> 8) as f32 / (1u32 << 24) as f32
    }
    fn signed(&mut self) -> f32 {
        self.next() * 2.0 - 1.0
    }
}

/// 512 random unit vectors (the runtime keeps such a table for directions).
fn random_dirs() -> &'static [V3] {
    static T: OnceLock<Vec<V3>> = OnceLock::new();
    T.get_or_init(|| {
        let mut r = Rng(0x1234_5678);
        (0..512)
            .map(|_| {
                loop {
                    let v = [r.signed(), r.signed(), r.signed()];
                    let l = len(v);
                    if l > 0.05 && l <= 1.0 {
                        break mul(v, 1.0 / l);
                    }
                }
            })
            .collect()
    })
}

#[derive(Clone, Debug)]
struct Particle {
    pos: V3,
    vel: V3,
    age: f32,
    life: f32,
    size: [f32; 2],
    rot: V3,
    rot_vel: V3,
    rnd: [f32; 4],
    pick: usize,
}

#[derive(Clone, Debug)]
struct EmitterState {
    frame: f32,
    since: f32,
    next: f32,
    carry: f32,
    rng: Rng,
    dir_index: usize,
    particles: Vec<Particle>,
    /// Frames of fade left after a stop (None: not fading).
    fade: Option<(f32, f32)>,
    emitting: bool,
}

#[derive(Clone, Debug)]
struct Instance {
    id: u32,
    set: Arc<Vec<EmitterDef>>,
    origin: V3,
    last_origin: V3,
    asset: Asset,
    flip: bool,
    age: i32,
    /// Fire-and-forget plays stop looping emitters after this many frames.
    auto_stop: Option<i32>,
    stopped: bool,
    emitters: Vec<EmitterState>,
}

/// Every effect playing in a course.
#[derive(Debug, Default)]
pub struct EffectPlayer {
    instances: Vec<Instance>,
    next_id: u32,
    seed: u32,
    textures: Vec<String>,
}

/// Frames a fire-and-forget play of a looping effect lasts.
const LOOP_ONE_SHOT: i32 = 60;
/// Particles per emitter, at most.
const MAX_PARTICLES: usize = 512;

impl EffectPlayer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Forget everything playing (a restart).
    pub fn clear(&mut self) {
        self.instances.clear();
    }

    pub fn len(&self) -> usize {
        self.instances.len()
    }

    pub fn is_empty(&self) -> bool {
        self.instances.is_empty()
    }

    /// Texture names by the index [`FxQuad::texture`] holds.
    pub fn texture_names(&self) -> &[String] {
        &self.textures
    }

    /// Start `set` at world (x, y) placed and tuned by `asset`; `flip` mirrors it. `held`:
    /// the caller stops it (a looping effect), else looping emitters stop by themselves.
    pub fn play(
        &mut self,
        set: Arc<Vec<EmitterDef>>,
        asset: &Asset,
        x: f32,
        y: f32,
        flip: bool,
        held: bool,
    ) -> FxHandle {
        self.next_id += 1;
        let id = self.next_id;
        let emitters = set
            .iter()
            .enumerate()
            .map(|(k, _)| {
                self.seed = self
                    .seed
                    .wrapping_mul(0x41c6_4e6d)
                    .wrapping_add(0x3039 + k as u32);
                EmitterState {
                    frame: 0.0,
                    since: 0.0,
                    next: 0.0,
                    carry: 0.0,
                    rng: Rng(self.seed ^ 0x9e37_79b9),
                    dir_index: (self.seed >> 7) as usize & 511,
                    particles: Vec::new(),
                    fade: None,
                    emitting: true,
                }
            })
            .collect();
        let origin = [x, y, 0.0];
        let duration = (asset.duration > 0).then_some(asset.duration);
        self.instances.push(Instance {
            id,
            set,
            origin,
            last_origin: origin,
            asset: asset.clone(),
            flip,
            age: -asset.delay.max(0),
            auto_stop: if held {
                duration
            } else {
                Some(duration.unwrap_or(LOOP_ONE_SHOT))
            },
            stopped: false,
            emitters,
        });
        FxHandle(id)
    }

    /// Move a playing effect (looping effects that follow an object).
    pub fn move_to(&mut self, h: FxHandle, x: f32, y: f32, flip: bool) {
        if let Some(i) = self.instances.iter_mut().find(|i| i.id == h.0) {
            i.origin = [x, y, 0.0];
            i.flip = flip;
        }
    }

    /// Stop emitting; particles live out their lives (or fade out).
    pub fn stop(&mut self, h: FxHandle) {
        if let Some(i) = self.instances.iter_mut().find(|i| i.id == h.0) {
            i.stopped = true;
        }
    }

    pub fn is_playing(&self, h: FxHandle) -> bool {
        self.instances.iter().any(|i| i.id == h.0)
    }

    /// One frame.
    pub fn step(&mut self) {
        for inst in &mut self.instances {
            inst.age += 1;
            if inst.age <= 0 {
                inst.last_origin = inst.origin;
                continue;
            }
            if inst.auto_stop.is_some_and(|n| inst.age > n) {
                inst.stopped = true;
            }
            let set_x = set_xform(inst);
            let moved = [
                inst.origin[0] - inst.last_origin[0],
                inst.origin[1] - inst.last_origin[1],
                0.0,
            ];
            let set = inst.set.clone();
            for (def, st) in set.iter().zip(inst.emitters.iter_mut()) {
                step_emitter(def, st, &inst.asset, &set_x, moved, inst.stopped);
            }
            inst.last_origin = inst.origin;
        }
        self.instances.retain(|i| {
            i.age <= 0
                || i.emitters
                    .iter()
                    .any(|e| e.emitting || !e.particles.is_empty())
        });
    }

    /// The quads of every live particle, in play order.
    pub fn quads(&mut self, out: &mut Vec<FxQuad>) {
        out.clear();
        let mut names = std::mem::take(&mut self.textures);
        for inst in &self.instances {
            if inst.age <= 0 {
                continue;
            }
            let set_x = set_xform(inst);
            for (def, st) in inst.set.iter().zip(&inst.emitters) {
                emitter_quads(def, st, &inst.asset, &set_x, &mut names, out);
            }
        }
        self.textures = names;
    }
}

fn set_xform(inst: &Instance) -> Xform {
    let s = inst.asset.scale;
    let flip = if inst.flip { -1.0 } else { 1.0 };
    let offset = [
        inst.asset.pos[0] * flip,
        inst.asset.pos[1],
        inst.asset.pos[2],
    ];
    Xform::new([s * flip, s, s], inst.asset.rot, add(inst.origin, offset))
}

fn emitter_xform(def: &EmitterDef, frame: f32) -> Xform {
    let pick = |kind: &str, base: V3| def.anim(kind).map(|a| a.at(frame)).unwrap_or(base);
    Xform::new(
        pick("EAES", def.scale),
        pick("EAER", def.rotate),
        pick("EAET", def.translate),
    )
}

fn anim1(def: &EmitterDef, kind: &str, base: f32, frame: f32) -> f32 {
    def.anim(kind).map(|a| a.at(frame)[0]).unwrap_or(base)
}

/// A point of the emitter's shape and the direction it launches along (unit; times omni).
fn shape_point(def: &EmitterDef, r: &mut Rng, k: u32, n: u32, shape_scale: V3) -> (V3, V3) {
    let ext = hadamard(def.extent, shape_scale);
    let angle = |r: &mut Rng| {
        if def.shape_uniform && n > 0 {
            std::f32::consts::TAU * k as f32 / n as f32
        } else {
            def.sweep_start + def.sweep * r.next() - def.sweep * 0.5
        }
    };
    let fill_radius = |r: &mut Rng| {
        let inner = (1.0 - def.fill).clamp(0.0, 1.0);
        inner + (1.0 - inner) * r.next().sqrt()
    };
    match def.shape {
        // circle (XZ plane); 3 filled; 8/9 cylinders add a height
        1..=3 | 8 | 9 => {
            let a = angle(r);
            let (s, c) = a.sin_cos();
            let f = if matches!(def.shape, 3 | 9) {
                fill_radius(r)
            } else {
                1.0
            };
            let y = if def.shape >= 8 {
                ext[1] * r.signed()
            } else {
                0.0
            };
            ([s * ext[0] * f, y, c * ext[2] * f], [s, 0.0, c])
        }
        // sphere (7 filled): a cap of `latitude` round +Y, longitude over the sweep
        4..=7 => {
            let a = angle(r);
            let y = 1.0 - r.next() * (1.0 - def.latitude.cos());
            let ring = (1.0 - y * y).max(0.0).sqrt();
            let (s, c) = a.sin_cos();
            let d = [ring * s, y, ring * c];
            let f = if def.shape == 7 { fill_radius(r) } else { 1.0 };
            (mul(hadamard(d, ext), f), d)
        }
        // box surface / filled
        10 | 11 => {
            let mut p = [r.signed(), r.signed(), r.signed()];
            if def.shape == 10 {
                let axis = (r.next() * 3.0) as usize % 3;
                p[axis] = if r.next() < 0.5 { -1.0 } else { 1.0 };
            }
            let p = hadamard(p, ext);
            (p, norm(p))
        }
        // line along Z
        12 | 13 => {
            let l = def.line_length * shape_scale[2];
            let z = if def.shape == 13 && n > 1 {
                l * k as f32 / (n - 1) as f32
            } else {
                l * r.next()
            } - (l + l * def.line_center) * 0.5;
            ([0.0, 0.0, z], [0.0, 0.0, 1.0])
        }
        // rectangle edges (XZ plane)
        14 => {
            let p = if r.next() < 0.5 {
                [
                    ext[0] * r.signed(),
                    0.0,
                    if r.next() < 0.5 { -ext[2] } else { ext[2] },
                ]
            } else {
                [
                    if r.next() < 0.5 { -ext[0] } else { ext[0] },
                    0.0,
                    ext[2] * r.signed(),
                ]
            };
            (p, norm(p))
        }
        // point (and primitives, drawn from their origin)
        _ => {
            let d = random_dirs()[(r.next() * 512.0) as usize & 511];
            ([0.0; 3], d)
        }
    }
}

/// Rotate `v` (given round +Y) so +Y points along `dir`.
fn from_y(v: V3, dir: V3) -> V3 {
    let d = norm(dir);
    if len(d) == 0.0 {
        return v;
    }
    // axis = Y × d, angle = acos(Y · d)
    let axis = [d[2], 0.0, -d[0]];
    let s = len(axis);
    let c = d[1];
    if s < 1e-6 {
        return if c >= 0.0 { v } else { [v[0], -v[1], -v[2]] };
    }
    let k = mul(axis, 1.0 / s);
    // Rodrigues
    let kxv = [
        k[1] * v[2] - k[2] * v[1],
        k[2] * v[0] - k[0] * v[2],
        k[0] * v[1] - k[1] * v[0],
    ];
    let kdv = k[0] * v[0] + k[1] * v[1] + k[2] * v[2];
    add(add(mul(v, c), mul(kxv, s)), mul(k, kdv * (1.0 - c)))
}

fn emit_one(
    def: &EmitterDef,
    st: &mut EmitterState,
    asset: &Asset,
    k: u32,
    n: u32,
    moved: V3,
) -> Particle {
    let frame = st.frame;
    let r = &mut st.rng;
    let shape_scale = def
        .anim("EASS")
        .map(|a| a.at(frame))
        .unwrap_or(def.shape_scale);
    let (mut pos, normal) = shape_point(def, r, k, n, shape_scale);
    let omni = anim1(def, "EAOV", def.omni, frame);
    let mut vel = mul(normal, omni);
    if def.position_random != 0.0 {
        st.dir_index = (st.dir_index + 1) & 511;
        pos = add(pos, mul(random_dirs()[st.dir_index], def.position_random));
    }
    if def.xz_diffusion != 0.0 {
        let side = norm([pos[0], 0.0, pos[2]]);
        let side = if len(side) == 0.0 {
            let a = r.next() * std::f32::consts::TAU;
            [a.sin(), 0.0, a.cos()]
        } else {
            side
        };
        vel = add(vel, mul(side, def.xz_diffusion));
    }
    let dir_speed = anim1(def, "EADV", def.dir_speed, frame) * asset.dir_vel;
    if dir_speed != 0.0 {
        let d = if def.spread == 0.0 {
            norm(def.dir)
        } else {
            let floor = 1.0 - def.spread / 90.0;
            let y = floor + (1.0 - floor) * r.next();
            let ring = (1.0 - y * y).max(0.0).sqrt();
            let a = r.next() * std::f32::consts::TAU;
            from_y([ring * a.sin(), y, ring * a.cos()], def.dir)
        };
        vel = add(vel, mul(d, dir_speed));
    }
    if def.speed_random != 0.0 {
        vel = mul(vel, 1.0 - r.next() * def.speed_random / 100.0);
    }
    st.dir_index = (st.dir_index + 1) & 511;
    vel = add(vel, hadamard(random_dirs()[st.dir_index], def.random_vel));
    vel = add(vel, mul(moved, def.inherit_vel));
    let life_base = anim1(def, "EAPL", def.life as f32, frame) * asset.life;
    let life = if def.infinite_life {
        f32::MAX
    } else {
        let cut = (r.next() * def.life_random as f32).floor() * 0.01;
        (life_base * (1.0 - cut)).max(1.0)
    };
    let size_base = def.anim("EASL").map(|a| a.at(frame)).unwrap_or(def.size);
    let sr = if def.size_random[0] == def.size_random[1] {
        let v = r.next();
        [v, v]
    } else {
        [r.next(), r.next()]
    };
    let size = [
        size_base[0] * (1.0 - def.size_random[0] / 100.0 * sr[0]),
        size_base[1] * (1.0 - def.size_random[1] / 100.0 * sr[1]),
    ];
    let mut rot = [0.0; 3];
    let mut rot_vel = [0.0; 3];
    for a in 0..3 {
        if def.rot_axes[a] {
            rot[a] = def.rot_init[a] + def.rot_init_random[a] * r.next();
            rot_vel[a] = def.rot_vel[a] + def.rot_vel_random[a] * r.signed();
        }
    }
    let rnd = [r.next(), r.next(), r.next(), r.next()];
    let pick = match def.color0_mode {
        ColorMode::Random => (rnd[0] * def.color0_keys.keys.len().max(1) as f32) as usize,
        _ => 0,
    };
    Particle {
        pos,
        vel,
        age: 0.0,
        life,
        size,
        rot,
        rot_vel,
        rnd,
        pick,
    }
}

fn step_emitter(
    def: &EmitterDef,
    st: &mut EmitterState,
    asset: &Asset,
    set_x: &Xform,
    moved: V3,
    stopped: bool,
) {
    let em_x = emitter_xform(def, st.frame);
    // particles: position += velocity, then velocity × air + gravity
    let gravity = anim1(def, "EAGV", def.gravity, st.frame);
    let g = mul(norm(def.gravity_dir), gravity);
    let fading = st.fade.is_some();
    for p in &mut st.particles {
        p.pos = add(p.pos, p.vel);
        p.vel = mul(p.vel, def.air);
        if gravity > 0.0 {
            p.vel = add(p.vel, g);
        }
        p.rot = add(p.rot, p.rot_vel);
        p.age += 1.0;
    }
    st.particles.retain(|p| p.age < p.life);
    if let Some((left, _)) = &mut st.fade {
        *left -= 1.0;
        if *left <= 0.0 {
            st.particles.clear();
        }
    }
    // emission
    let f = st.frame;
    let start = def.start as f32;
    let in_window = f >= start && (!def.one_time || f < start + def.duration.max(1) as f32);
    if stopped && st.emitting {
        st.emitting = false;
        if let Some(n) = def.fade_out.filter(|&n| n > 0) {
            st.fade = Some((n as f32, n as f32));
        }
    }
    if def.one_time && f >= start + def.duration.max(1) as f32 {
        st.emitting = false;
    }
    if st.emitting && !fading && in_window {
        let first = f == start;
        if first || st.since >= st.next {
            let rate = anim1(def, "EATR", def.rate, f) * asset.rate;
            let r = st.rng.next();
            let count = rate * (1.0 - def.rate_random as f32 / 100.0 * r) + st.carry;
            let n = count.max(0.0).floor();
            st.carry = count - n;
            let n = n as u32;
            let uniform_n = if def.shape_uniform {
                n.max(def.division)
            } else {
                n
            };
            for k in 0..n {
                if st.particles.len() >= MAX_PARTICLES {
                    break;
                }
                let mut p = emit_one(def, st, asset, k, uniform_n, moved);
                // into the set's space (particles follow the set); not following: world
                p.pos = em_x.point(p.pos);
                p.vel = em_x.vector(p.vel);
                if def.follow == 1 {
                    p.pos = set_x.point(p.pos);
                    p.vel = set_x.vector(p.vel);
                }
                st.particles.push(p);
            }
            st.since = 0.0;
            st.next =
                (def.interval + 1) as f32 + (st.rng.next() * def.interval_random as f32).floor();
        }
    }
    st.since += 1.0;
    st.frame += 1.0;
}

/// Texture cell of a particle (pattern animation of sampler 0).
fn cell(def: &EmitterDef, p: &Particle) -> u32 {
    let Some(s) = def.samplers.first() else {
        return 0;
    };
    let table_at = |i: usize| s.table.get(i).copied().unwrap_or(i as u32);
    let n = s.table.len().max(1);
    let step = (p.age / s.period.max(1.0)) as usize;
    match s.pattern {
        1 => table_at(((p.age / p.life.max(1.0)) * n as f32) as usize % n),
        2 => table_at(step.min(n - 1)),
        3 => table_at(step % n),
        4 => (p.rnd[1] * s.random_cells.max(1) as f32) as u32,
        _ => 0,
    }
}

fn life_t(p: &Particle, loop_frames: i32, loop_random: bool) -> f32 {
    if loop_frames > 0 {
        let l = loop_frames as f32;
        let off = if loop_random { p.rnd[2] * l } else { 0.0 };
        ((p.age + off) % l) / l
    } else if p.life == f32::MAX {
        0.0
    } else {
        p.age / p.life.max(1.0)
    }
}

fn emitter_quads(
    def: &EmitterDef,
    st: &EmitterState,
    asset: &Asset,
    set_x: &Xform,
    names: &mut Vec<String>,
    out: &mut Vec<FxQuad>,
) {
    let tex_name = def.samplers.first().and_then(|s| s.texture.as_deref());
    let texture = tex_name.map(|n| match names.iter().position(|t| t == n) {
        Some(i) => i as u32,
        None => {
            names.push(n.to_string());
            (names.len() - 1) as u32
        }
    });
    let s0 = def.samplers.first();
    let (du, dv) = s0
        .map(|s| (s.div[0].max(1), s.div[1].max(1)))
        .unwrap_or((1, 1));
    let (wrap, nearest, rep) =
        s0.map(|s| (s.wrap, s.filter == 1, s.repeat))
            .unwrap_or(([2, 2], false, [1.0, 1.0]));
    let em_color = def
        .anim("EAC0")
        .map(|a| a.at(st.frame))
        .unwrap_or(def.color0);
    let em_alpha = anim1(def, "EAA0", def.alpha0, st.frame);
    let fade = st.fade.map(|(l, n)| (l / n).clamp(0.0, 1.0)).unwrap_or(1.0);
    let fade_in = def
        .fade_in
        .filter(|&n| n > 0)
        .map(|n| (st.frame / n as f32).min(1.0))
        .unwrap_or(1.0);
    let set_scale = set_x.scale[1].abs();
    let blend = Blend::from_raw(def.blend);
    for p in &st.particles {
        let world = if def.follow == 1 {
            p.pos
        } else {
            set_x.point(p.pos)
        };
        // colour and alpha over the life
        let c = match def.color0_mode {
            ColorMode::Fixed => def.color0_fixed,
            ColorMode::Anim => def.color0_keys.eval(life_t(
                p,
                def.color0_keys.loop_frames,
                def.color0_keys.loop_random,
            )),
            ColorMode::Random => def
                .color0_keys
                .keys
                .get(p.pick)
                .map(|k| [k[0], k[1], k[2]])
                .unwrap_or([1.0; 3]),
        };
        let a = match def.alpha0_mode {
            ColorMode::Anim => def.alpha0_keys.eval(life_t(
                p,
                def.alpha0_keys.loop_frames,
                def.alpha0_keys.loop_random,
            ))[0],
            _ => def.alpha0_fixed,
        };
        let color = [
            c[0] * em_color[0] * asset.color[0],
            c[1] * em_color[1] * asset.color[1],
            c[2] * em_color[2] * asset.color[2],
            (a * em_alpha * asset.color[3] * fade * fade_in).clamp(0.0, 1.0),
        ];
        if color[3] <= 0.0 {
            continue;
        }
        let sc = if def.scale_keys.keys.is_empty() {
            [1.0; 3]
        } else {
            def.scale_keys.eval(life_t(
                p,
                def.scale_keys.loop_frames,
                def.scale_keys.loop_random,
            ))
        };
        let hw = p.size[0] * sc[0] * set_scale * 0.5;
        let hh = p.size[1] * sc[1] * set_scale * 0.5;
        if hw <= 0.0 || hh <= 0.0 {
            continue;
        }
        // the quad's axes on screen
        let (ax, ay): ([f32; 2], [f32; 2]) = match def.particle_type {
            // velocity look: up along the (projected) velocity
            5 | 6 => {
                let v = if def.follow == 1 {
                    p.vel
                } else {
                    set_x.vector(p.vel)
                };
                let l = (v[0] * v[0] + v[1] * v[1]).sqrt();
                if l > 1e-4 {
                    let up = [v[0] / l, v[1] / l];
                    ([up[1], -up[0]], up)
                } else {
                    ([1.0, 0.0], [0.0, 1.0])
                }
            }
            // polygons: their plane rotated by the particle, emitter and set rotations
            3 | 4 => {
                let m = rot_matrix(p.rot);
                let ex = apply(&m, [1.0, 0.0, 0.0]);
                let ey = apply(
                    &m,
                    if def.particle_type == 3 {
                        [0.0, 1.0, 0.0]
                    } else {
                        [0.0, 0.0, -1.0]
                    },
                );
                let em = emitter_xform(def, st.frame);
                let (ex, ey) = (apply(&em.rot, ex), apply(&em.rot, ey));
                let (ex, ey) = (apply(&set_x.rot, ex), apply(&set_x.rot, ey));
                let fx = set_x.scale[0].signum();
                ([ex[0] * fx, ex[1]], [ey[0] * fx, ey[1]])
            }
            // billboards: turned by the Z rotation
            _ => {
                let (s, c) = p.rot[2].sin_cos();
                let fx = set_x.scale[0].signum();
                ([c * fx, s], [-s * fx, c])
            }
        };
        let corner = |u: f32, v: f32| {
            [
                world[0] + ax[0] * hw * u + ay[0] * hh * v,
                world[1] + ax[1] * hw * u + ay[1] * hh * v,
            ]
        };
        let k = cell(def, p);
        let (cu, cv) = ((k % du) as f32, ((k / du) % dv) as f32);
        let (u0, v0) = (cu / du as f32, cv / dv as f32);
        let (u1, v1) = ((cu + rep[0]) / du as f32, (cv + rep[1]) / dv as f32);
        out.push(FxQuad {
            texture,
            pos: [
                corner(-1.0, -1.0),
                corner(1.0, -1.0),
                corner(1.0, 1.0),
                corner(-1.0, 1.0),
            ],
            uv: [[u0, v1], [u1, v1], [u1, v0], [u0, v0]],
            color,
            blend,
            wrap,
            nearest,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asset() -> Asset {
        Asset {
            set: "S".into(),
            scale: 1.0,
            pos: [0.0; 3],
            rot: [0.0; 3],
            color: [1.0; 4],
            rate: 1.0,
            life: 1.0,
            dir_vel: 1.0,
            delay: 0,
            duration: 0,
        }
    }

    fn burst() -> EmitterDef {
        EmitterDef {
            one_time: true,
            duration: 1,
            rate: 4.0,
            life: 10,
            dir_speed: 2.0,
            dir: [0.0, 1.0, 0.0],
            air: 1.0,
            gravity: 0.5,
            gravity_dir: [0.0, -1.0, 0.0],
            size: [8.0, 8.0, 8.0],
            alpha0_fixed: 1.0,
            color0_fixed: [1.0; 3],
            color0: [1.0; 3],
            alpha0: 1.0,
            scale: [1.0; 3],
            shape_scale: [1.0; 3],
            ..Default::default()
        }
    }

    #[test]
    fn a_burst_rises_falls_and_ends() {
        let mut fx = EffectPlayer::new();
        fx.play(Arc::new(vec![burst()]), &asset(), 100.0, 50.0, false, false);
        let mut q = Vec::new();
        fx.step();
        fx.quads(&mut q);
        assert_eq!(q.len(), 4);
        // drawn where emitted, then moved by its launch speed (2) each step
        let cy = (q[0].pos[0][1] + q[0].pos[2][1]) / 2.0;
        assert!((cy - 50.0).abs() < 1e-4, "{cy}");
        fx.step();
        fx.quads(&mut q);
        let cy = (q[0].pos[0][1] + q[0].pos[2][1]) / 2.0;
        assert!((cy - 52.0).abs() < 1e-4, "{cy}");
        assert!((q[0].pos[1][0] - q[0].pos[0][0] - 8.0).abs() < 1e-4);
        for _ in 0..20 {
            fx.step();
        }
        fx.quads(&mut q);
        assert!(q.is_empty());
        assert!(fx.is_empty());
    }

    #[test]
    fn same_plays_give_the_same_frames() {
        let run = || {
            let mut def = burst();
            def.spread = 45.0;
            def.random_vel = [1.0, 1.0, 0.0];
            let mut fx = EffectPlayer::new();
            fx.play(Arc::new(vec![def]), &asset(), 0.0, 0.0, false, false);
            let mut q = Vec::new();
            for _ in 0..5 {
                fx.step();
            }
            fx.quads(&mut q);
            q
        };
        assert_eq!(run(), run());
    }

    #[test]
    fn looping_effects_stop_when_asked() {
        let mut def = burst();
        def.one_time = false;
        def.interval = 0;
        def.rate = 1.0;
        let mut fx = EffectPlayer::new();
        let h = fx.play(Arc::new(vec![def]), &asset(), 0.0, 0.0, false, true);
        for _ in 0..100 {
            fx.step();
        }
        assert!(fx.is_playing(h));
        fx.stop(h);
        for _ in 0..12 {
            fx.step();
        }
        assert!(!fx.is_playing(h));
    }

    #[test]
    fn pattern_cells_follow_the_table() {
        let mut def = burst();
        def.samplers = vec![super::super::library::Sampler {
            texture: Some("t".into()),
            pattern: 2,
            div: [2, 2],
            table: vec![3, 1],
            period: 2.0,
            ..Default::default()
        }];
        let p = Particle {
            pos: [0.0; 3],
            vel: [0.0; 3],
            age: 0.0,
            life: 10.0,
            size: [1.0; 2],
            rot: [0.0; 3],
            rot_vel: [0.0; 3],
            rnd: [0.0; 4],
            pick: 0,
        };
        assert_eq!(cell(&def, &p), 3);
        assert_eq!(
            cell(
                &def,
                &Particle {
                    age: 2.0,
                    ..p.clone()
                }
            ),
            1
        );
        assert_eq!(cell(&def, &Particle { age: 9.0, ..p }), 1);
    }
}
