//! BFLAN layout animations.
//!
//! ```text
//! header "FLAN" …
//! pat1   u16 tag_order, u16 group_count, u32 name_offset, u32 groups_offset,
//!        s16 start_frame, s16 end_frame, u8 child_binding, pad[3]   (offsets from the section)
//! pai1   u16 frame_size, u8 loop, pad, u16 texture_count, u16 content_count,
//!        u32 content_table_offset (from the section);
//!        u32 texture_name_offsets[texture_count] (from the start of that table), names
//! content  name[28], u8 tag_count, u8 kind (0 pane, 1 material), pad[2],
//!          u32 tag_offsets[tag_count] (from the content)
//! tag      magic[4] (FLPA FLVI FLVC FLMC FLTS FLTP …), u8 entry_count, pad[3],
//!          u32 entry_offsets[entry_count] (from the tag)
//! entry    u8 index, u8 target, u8 curve (1 step, 2 hermite), pad, u16 key_count, pad[2],
//!          u32 keys_offset (from the entry)
//! keys     hermite: f32 frame, f32 value, f32 slope · step: f32 frame, u16 value, pad[2]
//! ```
//! Switch files (version 8+, SMM2: 9.0) insert a u32 (zero in every SMM2 file) before the start /
//! end frames of pat1; everything else is laid out the same.

use super::{Reader, file_header, sections};
use crate::error::Result;

pub const MAGIC: &[u8; 4] = b"FLAN";

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Animation {
    pub version: u32,
    /// Tag name from pat1 (e.g. `In`, `MoveMario`); the file name carries the layout prefix.
    pub name: String,
    pub frame_size: u16,
    pub looping: bool,
    pub start_frame: i16,
    pub end_frame: i16,
    pub groups: Vec<String>,
    /// Texture names used by texture-pattern (FLTP) keys, indexed by key value.
    pub textures: Vec<String>,
    pub contents: Vec<AnimContent>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct AnimContent {
    /// Pane or material name.
    pub name: String,
    /// 0 = pane, 1 = material.
    pub kind: u8,
    pub tags: Vec<AnimTag>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct AnimTag {
    /// Four-character tag, e.g. `FLPA` (pane SRT), `FLVI` (visibility), `FLVC` (vertex colour
    /// and pane alpha), `FLMC` (material colour), `FLTS` (texture SRT), `FLTP` (texture pattern).
    pub magic: String,
    pub entries: Vec<AnimEntry>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct AnimEntry {
    /// Sub-index (texture map / matrix for material tags).
    pub index: u8,
    /// Animated channel within the tag (see the per-tag target tables in the ui crate).
    pub target: u8,
    pub curve: CurveType,
    pub keys: Vec<AnimKey>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CurveType {
    Constant,
    Step,
    #[default]
    Hermite,
}

impl CurveType {
    pub fn code(self) -> u8 {
        match self {
            Self::Constant => 0,
            Self::Step => 1,
            Self::Hermite => 2,
        }
    }
    pub fn from_code(c: u8) -> Self {
        match c {
            0 => Self::Constant,
            1 => Self::Step,
            _ => Self::Hermite,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AnimKey {
    pub frame: f32,
    pub value: f32,
    /// Hermite slope (value per frame); 0 for step keys.
    pub slope: f32,
}

impl AnimEntry {
    /// Value at `frame`: held before the first and after the last key; step curves hold each
    /// key until the next; hermite curves interpolate with the stored slopes.
    pub fn eval(&self, frame: f32) -> f32 {
        let keys = &self.keys;
        let Some(first) = keys.first() else {
            return 0.0;
        };
        if keys.len() == 1 || frame <= first.frame {
            return first.value;
        }
        let last = keys.last().unwrap();
        if frame >= last.frame {
            return last.value;
        }
        // last key with k.frame <= frame (ties: the later key wins, giving instant jumps)
        let i = keys.partition_point(|k| k.frame <= frame) - 1;
        let (a, b) = (keys[i], keys[i + 1]);
        if self.curve != CurveType::Hermite {
            return a.value;
        }
        let span = b.frame - a.frame;
        if span <= 0.0 {
            return b.value;
        }
        let t = (frame - a.frame) / span;
        let (t2, t3) = (t * t, t * t * t);
        let h00 = 2.0 * t3 - 3.0 * t2 + 1.0;
        let h10 = t3 - 2.0 * t2 + t;
        let h01 = -2.0 * t3 + 3.0 * t2;
        let h11 = t3 - t2;
        h00 * a.value + h10 * span * a.slope + h01 * b.value + h11 * span * b.slope
    }
}

impl Animation {
    pub fn parse(data: &[u8]) -> Result<Self> {
        let h = file_header(data, MAGIC, "BFLAN")?;
        let r = Reader::new(data, h.be, "BFLAN");
        let mut anim = Animation {
            version: h.version,
            ..Default::default()
        };
        for (magic, o, _size) in sections(&r, &h)? {
            let b = o + 8;
            match &magic {
                b"pat1" => {
                    let groups = r.u16(b + 2)? as usize;
                    anim.name = r.cstr(o + r.u32(b + 4)? as usize)?;
                    let go = o + r.u32(b + 8)? as usize;
                    let frames = if super::bflyt::is_nx(h.version) {
                        b + 16
                    } else {
                        b + 12
                    };
                    anim.start_frame = r.i16(frames)?;
                    anim.end_frame = r.i16(frames + 2)?;
                    anim.groups = (0..groups)
                        .map(|i| r.cstr(go + i * 0x24))
                        .collect::<Result<_>>()?;
                }
                b"pai1" => {
                    anim.frame_size = r.u16(b)?;
                    anim.looping = r.u8(b + 2)? != 0;
                    let tex_count = r.u16(b + 4)? as usize;
                    let content_count = r.u16(b + 6)? as usize;
                    let table = o + r.u32(b + 8)? as usize;
                    let tex_table = b + 12;
                    anim.textures = (0..tex_count)
                        .map(|i| r.cstr(tex_table + r.u32(tex_table + i * 4)? as usize))
                        .collect::<Result<_>>()?;
                    for i in 0..content_count {
                        let c = o + r.u32(table + i * 4)? as usize;
                        anim.contents.push(parse_content(&r, c)?);
                    }
                }
                _ => {}
            }
        }
        Ok(anim)
    }

    pub fn content(&self, name: &str) -> Option<&AnimContent> {
        self.contents.iter().find(|c| c.name == name)
    }
}

fn parse_content(r: &Reader, c: usize) -> Result<AnimContent> {
    let tag_count = r.u8(c + 28)? as usize;
    let mut content = AnimContent {
        name: r.fixed_str(c, 28)?,
        kind: r.u8(c + 29)?,
        tags: Vec::with_capacity(tag_count),
    };
    for t in 0..tag_count {
        let to = c + r.u32(c + 32 + t * 4)? as usize;
        let magic = String::from_utf8_lossy(r.bytes(to, 4)?).into_owned();
        let n = r.u8(to + 4)? as usize;
        let mut tag = AnimTag {
            magic,
            entries: Vec::with_capacity(n),
        };
        for e in 0..n {
            let eo = to + r.u32(to + 8 + e * 4)? as usize;
            let curve = CurveType::from_code(r.u8(eo + 2)?);
            let count = r.u16(eo + 4)? as usize;
            let ko = eo + r.u32(eo + 8)? as usize;
            let keys = (0..count)
                .map(|k| {
                    if curve == CurveType::Hermite {
                        let p = ko + k * 12;
                        Ok(AnimKey {
                            frame: r.f32(p)?,
                            value: r.f32(p + 4)?,
                            slope: r.f32(p + 8)?,
                        })
                    } else {
                        let p = ko + k * 8;
                        Ok(AnimKey {
                            frame: r.f32(p)?,
                            value: r.u16(p + 4)? as f32,
                            slope: 0.0,
                        })
                    }
                })
                .collect::<Result<_>>()?;
            tag.entries.push(AnimEntry {
                index: r.u8(eo)?,
                target: r.u8(eo + 1)?,
                curve,
                keys,
            });
        }
        content.tags.push(tag);
    }
    Ok(content)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::sample_bflan;

    #[test]
    fn parses_synthetic_animation() {
        let a = Animation::parse(&sample_bflan()).unwrap();
        assert_eq!(a.name, "Move");
        assert_eq!(a.frame_size, 10);
        assert_eq!(a.groups, ["G_All"]);
        assert_eq!(a.textures, ["Tex0", "Tex1"]);
        let p = a.content("P_Box").unwrap();
        assert_eq!(p.kind, 0);
        assert_eq!(p.tags[0].magic, "FLPA");
        assert_eq!(p.tags[1].entries[0].curve, CurveType::Step);
        let m = a.content("M_Box").unwrap();
        assert_eq!(m.kind, 1);
        assert_eq!(m.tags[0].entries[0].keys[1].value, 1.0);
    }

    #[test]
    fn parses_switch_animation() {
        let a = Animation::parse(&crate::fixtures::sample_bflan_nx()).unwrap();
        assert_eq!(a.name, "Fade");
        assert_eq!((a.start_frame, a.end_frame), (300, 310));
        assert_eq!(a.groups, ["G_All"]);
        let e = &a.content("T_Msg").unwrap().tags[0].entries[0];
        assert_eq!(e.target, 16);
        assert!((e.eval(5.0) - 127.5).abs() < 1e-3);
    }

    #[test]
    fn hermite_and_step_evaluation() {
        let a = Animation::parse(&sample_bflan()).unwrap();
        let tx = &a.content("P_Box").unwrap().tags[0].entries[0];
        assert_eq!(tx.eval(-5.0), 0.0);
        assert_eq!(tx.eval(0.0), 0.0);
        assert!((tx.eval(5.0) - 50.0).abs() < 1e-4); // flat slopes: symmetric ease
        assert!(tx.eval(2.0) < 20.0); // ease-in
        assert_eq!(tx.eval(10.0), 100.0);
        assert_eq!(tx.eval(99.0), 100.0);
        let vis = &a.content("P_Box").unwrap().tags[1].entries[0];
        assert_eq!(vis.eval(4.9), 1.0);
        assert_eq!(vis.eval(5.0), 0.0);
        // linear hermite: slope = (v1 - v0) / span gives a straight line
        let lin = AnimEntry {
            curve: CurveType::Hermite,
            keys: vec![
                AnimKey {
                    frame: 0.0,
                    value: 0.0,
                    slope: 2.0,
                },
                AnimKey {
                    frame: 10.0,
                    value: 20.0,
                    slope: 2.0,
                },
            ],
            ..Default::default()
        };
        for f in 0..=10 {
            assert!((lin.eval(f as f32) - 2.0 * f as f32).abs() < 1e-4);
        }
    }
}
