//! VFXB particle effect resources (`Effect/Static.Nin_NX_NVN.szs` → SARC →
//! `Particle.Nin_NX_NVN.ptcl`): the emitter sets the game plays by name, their emitters'
//! parameters, the emitter / particle animations, and the textures (one embedded BNTX).
//!
//! Layout (VFX version 0x25, little-endian), from inspection of the owner's file and of how the
//! game's effect runtime reads it (addresses in the private notes, `effects.md`):
//!
//! ```text
//! 0x00  "VFXB    "  u16 graphics API version (0x0400)  u16 VFX version (0x25)  u16 BOM FEFF
//!       u8 alignment shift  u8 0x40  u32 header size  u16 flags  u16 first section (0x40)
//!       u32  u32 file size
//! section (32-byte header): magic[4], u32 size, u32 child offset, u32 next offset, u32
//!       sub-section offset, u32 data offset, u32, u16 child count, u16 (offsets relative to
//!       the section, 0xFFFFFFFF = none)
//! top level: ESTA (emitter sets) → ESET (name at +0x30, 64 bytes) → EMTR (+ child EMTRs, the
//!       child emitters) → sub-sections (EA** emitter animations, F*** fields, C*DP custom
//!       data, EP0n plugins); GRTF (textures: GTNT name table, BNTX at the data offset); PRMA /
//!       G3PR (primitives); GRSN (shaders, not read)
//! GTNT entry: u64 texture id, u32 entry size, u32 name length, name
//! EMTR data (at the data offset; 0xB08 bytes, name at +0x10): see `EmitterParams::decode`
//! ```

use crate::error::{Error, Result};

fn bad(detail: impl Into<String>) -> Error {
    Error::Format {
        what: "VFXB",
        detail: detail.into(),
    }
}

const NONE: u32 = 0xFFFF_FFFF;
/// Size of an emitter's fixed data block.
pub const EMITTER_DATA: usize = 0xB08;

fn u16_at(d: &[u8], o: usize) -> Result<u16> {
    d.get(o..o + 2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .ok_or_else(|| bad(format!("truncated at {o:#x}")))
}

fn u32_at(d: &[u8], o: usize) -> Result<u32> {
    d.get(o..o + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or_else(|| bad(format!("truncated at {o:#x}")))
}

fn u64_at(d: &[u8], o: usize) -> Result<u64> {
    Ok(u32_at(d, o)? as u64 | (u32_at(d, o + 4)? as u64) << 32)
}

fn name_at(d: &[u8], o: usize, max: usize) -> String {
    let raw = d.get(o..(o + max).min(d.len())).unwrap_or_default();
    let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
    String::from_utf8_lossy(&raw[..end]).into_owned()
}

#[derive(Clone, Copy, Debug)]
struct Section {
    at: usize,
    magic: [u8; 4],
    size: u32,
    child: u32,
    next: u32,
    sub: u32,
    data: u32,
}

impl Section {
    fn read(d: &[u8], at: usize) -> Result<Self> {
        let m = d
            .get(at..at + 4)
            .ok_or_else(|| bad(format!("section past end at {at:#x}")))?;
        Ok(Self {
            at,
            magic: [m[0], m[1], m[2], m[3]],
            size: u32_at(d, at + 4)?,
            child: u32_at(d, at + 8)?,
            next: u32_at(d, at + 12)?,
            sub: u32_at(d, at + 16)?,
            data: u32_at(d, at + 20)?,
        })
    }

    /// This section and its following siblings.
    fn chain(d: &[u8], mut at: usize) -> Result<Vec<Self>> {
        let mut out = Vec::new();
        loop {
            let s = Self::read(d, at)?;
            out.push(s);
            if s.next == NONE || s.next == 0 || out.len() > 100_000 {
                break;
            }
            at += s.next as usize;
        }
        Ok(out)
    }

    fn data_slice<'a>(&self, d: &'a [u8]) -> Result<&'a [u8]> {
        let start = self.at + self.data as usize;
        let end = self.at + self.size as usize;
        d.get(start..end.max(start))
            .ok_or_else(|| bad(format!("section data outside file at {:#x}", self.at)))
    }
}

/// One emitter: its fixed data block, its sub-sections (magic, data) and its child emitters.
#[derive(Clone, Debug)]
pub struct Emitter<'a> {
    pub name: String,
    pub data: &'a [u8],
    pub subs: Vec<([u8; 4], &'a [u8])>,
    pub children: Vec<Emitter<'a>>,
}

#[derive(Clone, Debug)]
pub struct EmitterSet<'a> {
    pub name: String,
    pub emitters: Vec<Emitter<'a>>,
}

pub struct Vfxb<'a> {
    d: &'a [u8],
    pub sets: Vec<EmitterSet<'a>>,
    /// Texture id → name (GTNT).
    pub textures: Vec<(u64, String)>,
    bntx: Option<(usize, usize)>,
}

impl<'a> Vfxb<'a> {
    pub fn parse(d: &'a [u8]) -> Result<Self> {
        if d.len() < 0x40 || &d[0..4] != b"VFXB" {
            return Err(Error::Magic { what: "VFXB" });
        }
        let version = u16_at(d, 0x0A)?;
        if version != 0x25 {
            return Err(bad(format!("unsupported VFX version {version:#x}")));
        }
        let first = u16_at(d, 0x16)? as usize;
        let mut sets = Vec::new();
        let mut textures = Vec::new();
        let mut bntx = None;
        for top in Section::chain(d, first)? {
            match &top.magic {
                b"ESTA" if top.child != NONE => {
                    for es in Section::chain(d, top.at + top.child as usize)? {
                        let name = name_at(d, es.at + 0x30, 64);
                        let emitters = if es.child != NONE {
                            Self::emitters(d, es.at + es.child as usize)?
                        } else {
                            Vec::new()
                        };
                        sets.push(EmitterSet { name, emitters });
                    }
                }
                b"GRTF" => {
                    if top.child != NONE {
                        let gtnt = Section::read(d, top.at + top.child as usize)?;
                        let mut p = gtnt.at + gtnt.data as usize;
                        let end = gtnt.at + gtnt.size as usize;
                        while p + 16 <= end {
                            let id = u64_at(d, p)?;
                            let size = u32_at(d, p + 8)? as usize;
                            let len = u32_at(d, p + 12)? as usize;
                            if size == 0 {
                                break;
                            }
                            textures.push((id, name_at(d, p + 16, len)));
                            p += size;
                        }
                    }
                    let start = top.at + top.data as usize;
                    if d.get(start..start + 4) == Some(b"BNTX") {
                        let size = u32_at(d, start + 0x1C)? as usize;
                        bntx = Some((start, size.min(d.len() - start)));
                    }
                }
                _ => {}
            }
        }
        Ok(Self {
            d,
            sets,
            textures,
            bntx,
        })
    }

    fn emitters(d: &'a [u8], at: usize) -> Result<Vec<Emitter<'a>>> {
        let mut out = Vec::new();
        for em in Section::chain(d, at)? {
            if &em.magic != b"EMTR" {
                continue;
            }
            let start = em.at + em.data as usize;
            let data = d
                .get(start..start + EMITTER_DATA)
                .ok_or_else(|| bad(format!("emitter data outside file at {:#x}", em.at)))?;
            let mut subs = Vec::new();
            if em.sub != NONE {
                for s in Section::chain(d, em.at + em.sub as usize)? {
                    subs.push((s.magic, s.data_slice(d)?));
                }
            }
            let children = if em.child != NONE {
                Self::emitters(d, em.at + em.child as usize)?
            } else {
                Vec::new()
            };
            out.push(Emitter {
                name: name_at(data, 0x10, 64),
                data,
                subs,
                children,
            });
        }
        Ok(out)
    }

    /// The embedded BNTX holding every texture.
    pub fn bntx(&self) -> Option<&'a [u8]> {
        self.bntx.map(|(s, n)| &self.d[s..s + n])
    }

    pub fn texture_name(&self, id: u64) -> Option<&str> {
        self.textures
            .iter()
            .find(|(i, _)| *i == id)
            .map(|(_, n)| n.as_str())
    }
}

/// An animation key: value (x, y, z) at `t` (particle anims: fraction of the life; emitter
/// anims: frames).
pub type Key = [f32; 4];

/// A particle animation table (colour, alpha or scale over the particle's life).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct KeyTable {
    pub keys: Vec<Key>,
    /// Loop every `loop_frames` frames instead of over the life (0: no loop); random start.
    pub loop_frames: i32,
    pub loop_random: bool,
}

/// How a colour or alpha is chosen.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ColorMode {
    /// The fixed value.
    #[default]
    Fixed,
    /// The key table over the life.
    Anim,
    /// One of the table's values, picked per particle.
    Random,
}

impl ColorMode {
    fn from_raw(v: u8) -> Self {
        match v {
            2 => Self::Anim,
            3 => Self::Random,
            _ => Self::Fixed,
        }
    }
}

/// Texture pattern (flip-book) animation of one sampler.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TexturePattern {
    /// 0 none, 1 over the life, 2 clamped, 3 looped, 4 random per particle [inf from the
    /// shader key bits and the data].
    pub mode: u8,
    /// Frame cells across / down the texture.
    pub div_u: u32,
    pub div_v: u32,
    /// Cell indices played in order; `period` frames each.
    pub table: Vec<u32>,
    pub period: f32,
    /// Random mode: cells to choose from.
    pub random_cells: u32,
    /// UV scroll per frame, UV scale, UV rotation per frame (shift animation) [inf].
    pub scroll: [f32; 2],
    pub uv_scale: [f32; 2],
}

/// One texture sampler: the texture id (None: unused) and its pattern animation.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Sampler {
    pub texture: Option<u64>,
    /// Wrap per axis: 0 mirror, 1 repeat, 2 clamp, 3 mirror once (the runtime's sampler table).
    pub wrap: [u8; 2],
    /// 0 bilinear, 1 nearest [inf: the pixel-art styles' textures use 1].
    pub filter: u8,
    /// The texture repeats this many times across the quad (1 or 2 per axis; with mirror
    /// wrap a quarter texture draws a whole ring).
    pub repeat: [f32; 2],
    pub pattern: TexturePattern,
}

/// Emitter animation (EA** sub-section): a parameter over the emitter's frames.
#[derive(Clone, Debug, PartialEq)]
pub struct EmitterAnim {
    pub kind: [u8; 4],
    pub looped: bool,
    pub step: bool,
    pub keys: Vec<Key>,
}

/// The emitter parameters the engine uses, decoded from the 0xB08-byte data block.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EmitterParams {
    pub name: String,
    /// 0 CPU, 1 GPU, 2 GPU stream-out (the game's calculation path; the engine always steps
    /// particles on the CPU).
    pub calc_type: u8,
    /// 0 particles follow the emitter fully, 1 not at all, 2 position only.
    pub follow: u8,
    /// Emitter transform relative to the set: translation, rotation (radians, XYZ), scale.
    pub translate: [f32; 3],
    pub rotate: [f32; 3],
    pub scale: [f32; 3],
    /// Emitter colour / alpha multipliers (colour 0 and 1).
    pub color0: [f32; 3],
    pub alpha0: f32,
    pub color1: [f32; 3],
    pub alpha1: f32,
    /// Emission: start frame, finite duration (`one_time`: emits for `duration` frames, else
    /// until stopped), particles per emission (± `rate_random` %), interval frames
    /// (+ up to `interval_random`).
    pub one_time: bool,
    pub start: u32,
    pub duration: i32,
    pub rate: f32,
    pub rate_random: i32,
    pub interval: i32,
    pub interval_random: i32,
    /// Fade-out frames when stopped (0: stop at once), fade-in frames.
    pub fade_out: Option<i32>,
    pub fade_in: Option<i32>,
    /// Emitter shape (0 point, 1 circle, 2 circle same division, 3 circle fill, 4 sphere, 5/6
    /// sphere same division, 7 sphere fill, 8 cylinder, 9 cylinder fill, 10 box, 11 box fill,
    /// 12 line, 13 line same division, 14 rectangle, 15 primitive).
    pub shape: u8,
    /// Emit at evenly spaced angles instead of random ones.
    pub shape_uniform: bool,
    pub sweep_start: f32,
    pub sweep: f32,
    pub latitude: f32,
    pub fill: f32,
    pub extent: [f32; 3],
    pub shape_scale: [f32; 3],
    pub line_length: f32,
    pub line_center: f32,
    pub division: u32,
    /// Particle life in frames (± `life_random` %; `infinite`: until the emitter goes).
    pub life: i32,
    pub life_random: i32,
    pub infinite_life: bool,
    /// Initial velocity: along the shape normal (`omni`), along `dir` (± `spread` degrees),
    /// random per axis, sideways in the XZ plane, and a share of the emitter's motion;
    /// `speed_random` % less at random.
    pub omni: f32,
    pub dir_speed: f32,
    pub dir: [f32; 3],
    pub spread: f32,
    pub xz_diffusion: f32,
    pub random_vel: [f32; 3],
    pub speed_random: f32,
    pub inherit_vel: f32,
    /// Random offset of the emission point.
    pub position_random: f32,
    /// Velocity × `air` per frame; gravity `gravity` × `gravity_dir` per frame (emitter space
    /// when `gravity_local`).
    pub air: f32,
    pub gravity: f32,
    pub gravity_dir: [f32; 3],
    pub gravity_local: bool,
    /// Particle size (world units) and its random reduction in % per axis.
    pub size: [f32; 3],
    pub size_random: [f32; 3],
    /// Rotation (radians) initial, + random, velocity per frame, + random; per axis enable.
    pub rot_axes: [bool; 3],
    pub rot_init: [f32; 3],
    pub rot_init_random: [f32; 3],
    pub rot_vel: [f32; 3],
    pub rot_vel_random: [f32; 3],
    /// Particle drawing: 0 billboard, 1 complex billboard, 2 Y billboard, 3 polygon XY, 4
    /// polygon XZ, 5 velocity look, 6 velocity look polygon, 7 Y billboard parallel [inf].
    pub particle_type: u8,
    /// Blend: 0 alpha, 1 add, 2 subtract, 3 multiply, 4 screen, 5 premultiplied.
    pub blend: u8,
    pub depth_test: bool,
    /// Colour / alpha 0 and 1 over the life, and the scale over the life (× size).
    pub color0_mode: ColorMode,
    pub color0_fixed: [f32; 3],
    pub color0_keys: KeyTable,
    pub alpha0_mode: ColorMode,
    pub alpha0_fixed: f32,
    pub alpha0_keys: KeyTable,
    pub color1_mode: ColorMode,
    pub color1_fixed: [f32; 3],
    pub color1_keys: KeyTable,
    pub alpha1_mode: ColorMode,
    pub alpha1_fixed: f32,
    pub alpha1_keys: KeyTable,
    pub scale_keys: KeyTable,
    pub samplers: [Sampler; 3],
    /// A primitive (mesh) instead of a quad: drawn as a quad by the engine.
    pub primitive: Option<u64>,
}

struct R<'b>(&'b [u8]);

impl R<'_> {
    fn u8(&self, o: usize) -> u8 {
        self.0.get(o).copied().unwrap_or(0)
    }
    fn b(&self, o: usize) -> bool {
        self.u8(o) != 0
    }
    fn u32(&self, o: usize) -> u32 {
        u32_at(self.0, o).unwrap_or(0)
    }
    fn i32(&self, o: usize) -> i32 {
        self.u32(o) as i32
    }
    fn u64(&self, o: usize) -> u64 {
        u64_at(self.0, o).unwrap_or(u64::MAX)
    }
    fn f(&self, o: usize) -> f32 {
        f32::from_bits(self.u32(o))
    }
    fn v3(&self, o: usize) -> [f32; 3] {
        [self.f(o), self.f(o + 4), self.f(o + 8)]
    }
    fn keys(&self, o: usize, n: i32) -> Vec<Key> {
        (0..n.clamp(0, 8) as usize)
            .map(|k| {
                let p = o + 16 * k;
                [self.f(p), self.f(p + 4), self.f(p + 8), self.f(p + 12)]
            })
            .collect()
    }
}

impl EmitterParams {
    /// Decode an emitter data block (see the module docs for where the offsets come from).
    pub fn decode(data: &[u8]) -> Self {
        let r = R(data);
        let table = |keys: usize, count: usize, lp: usize| KeyTable {
            keys: r.keys(keys, r.i32(count)),
            loop_frames: if r.b(0x918 + lp) {
                r.i32(0x924 + 4 * lp)
            } else {
                0
            },
            loop_random: r.b(0x91d + lp),
        };
        let sampler = |k: usize| {
            let id = r.u64(0xa38 + 0x20 * k);
            let pat = 0x110 + 0x90 * k;
            let shift = 0x2c0 + 0x50 * k;
            let count = r.f(pat).max(0.0) as usize;
            let rep = r.u8(0xa9c + 0x10 * k);
            Sampler {
                texture: (id != u64::MAX).then_some(id),
                wrap: [r.u8(0xa40 + 0x20 * k), r.u8(0xa41 + 0x20 * k)],
                filter: r.u8(0xa42 + 0x20 * k),
                repeat: if rep < 4 {
                    [1.0 + (rep & 1) as f32, 1.0 + (rep >> 1) as f32]
                } else {
                    [1.0; 2]
                },
                pattern: TexturePattern {
                    mode: r.u8(0xa98 + 0x10 * k),
                    div_u: r.f(shift + 0x48).max(1.0) as u32,
                    div_v: r.f(shift + 0x4c).max(1.0) as u32,
                    table: (0..count.min(32))
                        .map(|i| r.u32(pat + 0x10 + 4 * i))
                        .collect(),
                    period: r.f(pat + 4),
                    random_cells: r.f(pat + 8).max(0.0) as u32,
                    scroll: if r.b(0xa99 + 0x10 * k) {
                        [r.f(shift), r.f(shift + 4)]
                    } else {
                        [0.0; 2]
                    },
                    uv_scale: if r.b(0xa9b + 0x10 * k) {
                        [r.f(shift + 0x20), r.f(shift + 0x24)]
                    } else {
                        [1.0; 2]
                    },
                },
            }
        };
        let fade_out = (r.b(0x79b) || r.b(0x79c)).then(|| r.i32(0x7a8));
        let fade_in = (r.b(0x799) || r.b(0x79a)).then(|| r.i32(0x7ac));
        let prim = r.u64(0x8b8);
        Self {
            name: name_at(data, 0x10, 64),
            calc_type: r.u8(0x792),
            follow: r.u8(0x793),
            translate: r.v3(0x7b0),
            rotate: r.v3(0x7c8),
            scale: r.v3(0x7e0),
            color0: r.v3(0x7ec),
            alpha0: r.f(0x7f8),
            color1: r.v3(0x7fc),
            alpha1: r.f(0x808),
            one_time: r.b(0x830),
            start: r.u32(0x834),
            duration: r.i32(0x83c),
            rate: r.f(0x840),
            rate_random: r.i32(0x844),
            interval: r.i32(0x848),
            interval_random: r.i32(0x84c),
            fade_out,
            fade_in,
            shape: r.u8(0x878),
            shape_uniform: r.b(0x879),
            sweep: r.f(0x880),
            latitude: r.f(0x884),
            sweep_start: r.f(0x888),
            fill: r.f(0x890),
            line_center: r.f(0x894),
            line_length: r.f(0x898),
            extent: r.v3(0x89c),
            shape_scale: r.v3(0x8a8),
            division: r.u32(0x8c0).max(r.u32(0x8c8)),
            life: r.i32(0x8f8),
            life_random: r.i32(0x8fc),
            infinite_life: r.b(0x8e8),
            omni: r.f(0x99c),
            dir_speed: r.f(0x9a0),
            dir: r.v3(0x9a4),
            spread: r.f(0x9b0),
            xz_diffusion: r.f(0x9b4),
            random_vel: r.v3(0x9b8),
            speed_random: r.f(0x9c4),
            inherit_vel: r.f(0x9c8),
            position_random: r.f(0x850),
            air: r.f(0xc0),
            gravity: r.f(0x854),
            gravity_dir: r.v3(0x858),
            gravity_local: r.b(0x831),
            size: r.v3(0xa08),
            size_random: r.v3(0xa14),
            rot_axes: [r.b(0x8f0), r.b(0x8f1), r.b(0x8f2)],
            rot_init: r.v3(0x700),
            rot_init_random: r.v3(0x710),
            rot_vel: r.v3(0x720),
            rot_vel_random: r.v3(0x730),
            particle_type: r.u8(0x8ea),
            blend: r.u8(0x8de),
            depth_test: r.b(0x8d9),
            color0_mode: ColorMode::from_raw(r.u8(0x9e4)),
            color0_fixed: r.v3(0x9e8),
            color0_keys: table(0x3c0, 0x60, 0),
            alpha0_mode: ColorMode::from_raw(r.u8(0x9e6)),
            alpha0_fixed: r.f(0x9f4),
            alpha0_keys: table(0x440, 0x64, 1),
            color1_mode: ColorMode::from_raw(r.u8(0x9e5)),
            color1_fixed: r.v3(0x9f8),
            color1_keys: table(0x4c0, 0x68, 2),
            alpha1_mode: ColorMode::from_raw(r.u8(0x9e7)),
            alpha1_fixed: r.f(0xa04),
            alpha1_keys: table(0x540, 0x6c, 3),
            scale_keys: table(0x600, 0x70, 4),
            samplers: [sampler(0), sampler(1), sampler(2)],
            primitive: (prim != u64::MAX).then_some(prim),
        }
    }
}

impl Emitter<'_> {
    pub fn params(&self) -> EmitterParams {
        EmitterParams::decode(self.data)
    }

    /// The emitter animations (EA** sub-sections): u8 enabled, u8 loop, u8 step, u32 key count
    /// at +4, 16-byte keys from +0xC.
    pub fn anims(&self) -> Vec<EmitterAnim> {
        let mut out = Vec::new();
        for (magic, d) in &self.subs {
            if &magic[..2] != b"EA" || d.first() != Some(&1) {
                continue;
            }
            let r = R(d);
            out.push(EmitterAnim {
                kind: *magic,
                looped: r.b(1),
                step: r.b(2),
                keys: (0..r.i32(4).clamp(0, 64) as usize)
                    .map(|k| {
                        let p = 0xc + 16 * k;
                        [r.f(p), r.f(p + 4), r.f(p + 8), r.f(p + 12)]
                    })
                    .collect(),
            });
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn section(magic: &[u8; 4], size: u32, child: u32, next: u32, sub: u32, data: u32) -> Vec<u8> {
        let mut s = Vec::new();
        s.extend_from_slice(magic);
        for v in [size, child, next, sub, data, 0] {
            s.extend_from_slice(&v.to_le_bytes());
        }
        s.extend_from_slice(&[0; 4]);
        s
    }

    /// A tiny file: one emitter set with one emitter, a texture name table.
    fn synthetic() -> Vec<u8> {
        let mut d = vec![0u8; 0x40];
        d[0..8].copy_from_slice(b"VFXB    ");
        d[0x0A] = 0x25;
        d[0x0C..0x0E].copy_from_slice(&[0xFF, 0xFE]);
        d[0x16] = 0x40;
        // ESTA → ESET (0x98 bytes incl. name) → EMTR (header + data)
        let emtr_size = 0x20 + EMITTER_DATA as u32;
        let eset_size = 0x98 + emtr_size;
        let esta_size = 0x20 + eset_size;
        d.extend(section(b"ESTA", esta_size, 0x20, esta_size, NONE, 0x20));
        let mut eset = section(b"ESET", eset_size, 0x98, NONE, NONE, 0x20);
        eset.resize(0x98, 0);
        eset[0x30..0x34].copy_from_slice(b"Puff");
        d.extend(eset);
        d.extend(section(b"EMTR", emtr_size, NONE, NONE, NONE, 0x20));
        let mut data = vec![0u8; EMITTER_DATA];
        data[0x10..0x15].copy_from_slice(b"smoke");
        data[0x840..0x844].copy_from_slice(&3.0f32.to_le_bytes());
        data[0x8f8..0x8fc].copy_from_slice(&24i32.to_le_bytes());
        data[0x8de] = 1;
        data[0x64..0x68].copy_from_slice(&2i32.to_le_bytes());
        for (k, (a, t)) in [(1.0f32, 0.0f32), (0.0, 1.0)].iter().enumerate() {
            let p = 0x440 + 16 * k;
            data[p..p + 4].copy_from_slice(&a.to_le_bytes());
            data[p + 12..p + 16].copy_from_slice(&t.to_le_bytes());
        }
        data[0xa38..0xa40].copy_from_slice(&7u64.to_le_bytes());
        data[0xa58..0xa60].copy_from_slice(&u64::MAX.to_le_bytes());
        data[0xa78..0xa80].copy_from_slice(&u64::MAX.to_le_bytes());
        d.extend(data);
        // GRTF with GTNT (no BNTX)
        let mut gtnt_entry = Vec::new();
        gtnt_entry.extend_from_slice(&7u64.to_le_bytes());
        gtnt_entry.extend_from_slice(&0x20u32.to_le_bytes());
        gtnt_entry.extend_from_slice(&5u32.to_le_bytes());
        gtnt_entry.extend_from_slice(b"puff\0");
        gtnt_entry.resize(0x20, 0);
        let gtnt_size = 0x20 + gtnt_entry.len() as u32;
        d.extend(section(
            b"GRTF",
            0x20 + gtnt_size,
            0x20,
            NONE,
            NONE,
            0x20 + gtnt_size,
        ));
        d.extend(section(b"GTNT", gtnt_size, NONE, NONE, NONE, 0x20));
        d.extend(gtnt_entry);
        d
    }

    #[test]
    fn reads_sets_emitters_and_textures() {
        let d = synthetic();
        let v = Vfxb::parse(&d).unwrap();
        assert_eq!(v.sets.len(), 1);
        assert_eq!(v.sets[0].name, "Puff");
        let em = &v.sets[0].emitters[0];
        assert_eq!(em.name, "smoke");
        let p = em.params();
        assert_eq!(p.rate, 3.0);
        assert_eq!(p.life, 24);
        assert_eq!(p.blend, 1);
        assert_eq!(p.alpha0_keys.keys.len(), 2);
        assert_eq!(p.alpha0_keys.keys[1][3], 1.0);
        assert_eq!(p.samplers[0].texture, Some(7));
        assert_eq!(p.samplers[1].texture, None);
        assert_eq!(v.texture_name(7), Some("puff"));
        assert!(v.bntx().is_none());
    }

    #[test]
    fn rejects_other_files() {
        assert!(Vfxb::parse(b"NOPE").is_err());
        let mut d = synthetic();
        d[0x0A] = 0x16;
        assert!(Vfxb::parse(&d).is_err());
    }
}
