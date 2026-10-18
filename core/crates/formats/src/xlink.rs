//! XLNK ("SLink2DB.bslnk"): the database that links a game object's named events to sound
//! assets. Read here as far as SMM2's sounds need: the users (by name), each user's asset calls
//! (event key → asset, or a container choosing among child calls), the containers' kinds and
//! watched properties, the children's conditions, the asset parameters (sound name, volume,
//! pitch, delay, …, including random ranges and property curves) and the user-level parameters.
//!
//! Layout (version 0x1C, little-endian), from inspection of the owner's file:
//!
//! ```text
//! 0x00  "XLNK" u32 size  u32 version
//! 0x0C  i32 param count, asset param count, trigger-overwrite param count
//! 0x18  u32 trigger-overwrite table pos, local property name ref table pos
//! 0x20  i32 local property names, local property enum names, direct values, random tables,
//!       curves, curve points
//! 0x38  u32 ex-region pos   i32 user count   u32 condition table pos   u32 name table pos
//! 0x48  u32 CRC32(user name) × users (ascending), u32 user offset × users
//!       param define table: u32 size, u32 user / asset / ? / trigger param counts,
//!       12-byte defines (u32 name offset into the strings after them, u32 type, u32 default)
//!       asset param table (at define table + size): per asset, u64 mask of the set params,
//!       then one u32 per set bit: kind << 24 | value (kind 0: index into the direct value
//!       table, 1: name table offset, 2: curve index, 3: random range index)
//! direct value table: u32 (float or int) × direct values, right after the local property
//!       name refs (local property name ref table pos + 4 × (names + enum names)); then the
//!       random ranges (f32 min, f32 max), the curves (20 bytes: u16 first point, u16 points,
//!       u16 type, u16 global, u32 property name offset, u32, i16 local property, i16) and the
//!       curve points (f32 x, f32 y)
//! user: 12 × i32 header (local properties, calls, assets at [1..=3]; trigger table offset at
//!       [11]), u32 local property name refs, u32 user params (param define table's user count,
//!       encoded like asset params), u16 call sort table × calls (padded to 4), then 32-byte
//!       calls: u32 key name pos, i16 asset index (−1: container), u16 flag, i32 duration,
//!       i32 parent call, u32 guid, u32 key hash, u32 param pos, i32 condition pos (−1: none)
//!       A call's param pos is an offset into the asset param table, or for a container an
//!       offset from the end of the user's call table to its container param: u32 kind
//!       (0 switch, 1 random, 2 random2, 3 blend, 4 sequence), i32 first child, i32 last
//!       child, and for a switch u32 property name pos, i32 property id, i16 local property
//!       index, u8 global.
//! condition (at condition table pos + the call's condition pos): u32 parent container kind,
//!       then for a switch child u32 property type (0 enum, 1 int, 2 float), u32 compare
//!       (0 ==, 1 >, 2 >=, 3 <, 4 <=, 5 !=), u32 value (enum: name pos), i16 enum index,
//!       u8 solved, u8 global; for a random child f32 weight.
//! ```

use std::collections::BTreeMap;

use crate::error::{Error, Result};

fn bad(detail: impl Into<String>) -> Error {
    Error::Format {
        what: "XLNK",
        detail: detail.into(),
    }
}

/// How a container picks among its children.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContainerKind {
    /// The first child whose condition on a watched property holds (an unconditioned child is
    /// the fallback).
    Switch,
    /// One child at random, by weight.
    Random,
    /// Like `Random` (the format's second random kind; no repeat of the last pick [inf]).
    Random2,
    /// Every child at once.
    Blend,
    /// The children one after another.
    Sequence,
    Other(u32),
}

impl ContainerKind {
    fn from_raw(v: u32) -> Self {
        match v {
            0 => Self::Switch,
            1 => Self::Random,
            2 => Self::Random2,
            3 => Self::Blend,
            4 => Self::Sequence,
            o => Self::Other(o),
        }
    }
}

/// Comparison of a switch child's condition: property `cmp` value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Compare {
    Eq,
    Gt,
    Ge,
    Lt,
    Le,
    Ne,
}

impl Compare {
    fn from_raw(v: u32) -> Option<Self> {
        Some(match v {
            0 => Self::Eq,
            1 => Self::Gt,
            2 => Self::Ge,
            3 => Self::Lt,
            4 => Self::Le,
            5 => Self::Ne,
            _ => return None,
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum CondValue {
    Enum(String),
    Int(i32),
    Float(f32),
}

/// What selects a container's child.
#[derive(Clone, Debug, PartialEq)]
pub enum Condition {
    Switch {
        cmp: Compare,
        value: CondValue,
        /// The property is a global one (set by the game for every user).
        global: bool,
    },
    Random {
        weight: f32,
    },
    /// A condition of another container kind (raw words).
    Other(Vec<u32>),
}

/// A property-driven parameter: piecewise linear over the property's value.
#[derive(Clone, Debug, PartialEq)]
pub struct Curve {
    pub property: String,
    pub global: bool,
    pub points: Vec<(f32, f32)>,
}

/// One event of a user: a sound asset, or a container choosing among its child calls.
#[derive(Clone, Debug, PartialEq)]
pub struct Call {
    pub key: String,
    /// The asset's parameters by name; empty for a container.
    pub params: BTreeMap<String, Param>,
    pub container: bool,
    /// Index of the parent call (a container), if any.
    pub parent: Option<usize>,
    /// The call has a condition (one of a container's alternatives).
    pub conditional: bool,
    /// For a container: its kind and the indices of its children (inclusive range).
    pub kind: Option<ContainerKind>,
    pub children: Option<(usize, usize)>,
    /// For a switch: the watched property and whether it is global.
    pub watch: Option<(String, bool)>,
    pub condition: Option<Condition>,
}

/// An action trigger: entering `action` (in action slot `slot`) starts call `call` (index
/// into [`Xlnk::user_calls`]), from `start_frame` (to `end_frame`), or only when the object
/// comes from action `after`.
#[derive(Clone, Debug, PartialEq)]
pub struct ActionTrigger {
    pub slot: String,
    pub action: String,
    pub call: usize,
    pub start_frame: i32,
    pub end_frame: Option<i32>,
    pub after: Option<String>,
    pub flag: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Param {
    Str(String),
    Num(f32),
    Int(i32),
    /// A random value in [min, max] each time the asset plays.
    Range(f32, f32),
    /// A random value in [min, max] from a uniform draw raised to the power (min, max, power).
    RangePow(f32, f32, f32),
    Curve(Curve),
    Raw(u32),
}

impl Param {
    pub fn as_str(&self) -> Option<&str> {
        if let Param::Str(s) = self {
            Some(s)
        } else {
            None
        }
    }
    pub fn as_f32(&self) -> Option<f32> {
        match self {
            Param::Num(v) => Some(*v),
            Param::Int(v) => Some(*v as f32),
            _ => None,
        }
    }
}

pub struct Xlnk<'a> {
    d: &'a [u8],
    users: BTreeMap<u32, u32>,
    name_pos: usize,
    cond_pos: usize,
    asset_params: Vec<(String, u32)>,
    user_params: Vec<(String, u32)>,
    asset_param_base: usize,
    direct_base: usize,
    random_base: usize,
    curve_base: usize,
    point_base: usize,
    n_random: usize,
    n_curve: usize,
    n_point: usize,
}

fn u32_at(d: &[u8], o: usize) -> Result<u32> {
    d.get(o..o + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or_else(|| bad(format!("truncated at {o:#x}")))
}

fn u16_at(d: &[u8], o: usize) -> Result<u16> {
    d.get(o..o + 2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .ok_or_else(|| bad(format!("truncated at {o:#x}")))
}

fn i32_at(d: &[u8], o: usize) -> Result<i32> {
    u32_at(d, o).map(|v| v as i32)
}

fn f32_at(d: &[u8], o: usize) -> Result<f32> {
    u32_at(d, o).map(f32::from_bits)
}

fn cstr(d: &[u8], o: usize) -> Result<String> {
    let rest = d.get(o..).ok_or_else(|| bad("string past end"))?;
    let end = rest
        .iter()
        .position(|&b| b == 0)
        .ok_or_else(|| bad("unterminated string"))?;
    Ok(String::from_utf8_lossy(&rest[..end]).into_owned())
}

impl<'a> Xlnk<'a> {
    pub fn parse(d: &'a [u8]) -> Result<Self> {
        if d.len() < 0x48 || &d[0..4] != b"XLNK" {
            return Err(Error::Magic { what: "XLNK" });
        }
        let version = u32_at(d, 8)?;
        // 0x1E: the effect database (ELink2DB), read the same way as far as this goes
        if version != 0x1C && version != 0x1E {
            return Err(bad(format!("unsupported version {version:#x}")));
        }
        let local_names = i32_at(d, 0x1C)? as usize;
        let n_local = i32_at(d, 0x20)?.max(0) as usize;
        let n_enum = i32_at(d, 0x24)?.max(0) as usize;
        let n_direct = i32_at(d, 0x28)?.max(0) as usize;
        let n_random = i32_at(d, 0x2C)?.max(0) as usize;
        let n_curve = i32_at(d, 0x30)?.max(0) as usize;
        let n_point = i32_at(d, 0x34)?.max(0) as usize;
        let n_users = i32_at(d, 0x3C)?.max(0) as usize;
        let cond_pos = u32_at(d, 0x40)? as usize;
        let name_pos = u32_at(d, 0x44)? as usize;
        let mut users = BTreeMap::new();
        for i in 0..n_users {
            users.insert(
                u32_at(d, 0x48 + 4 * i)?,
                u32_at(d, 0x48 + 4 * (n_users + i))?,
            );
        }
        let def = 0x48 + 8 * n_users;
        let def_size = u32_at(d, def)? as usize;
        let (nu, na, nt) = (
            u32_at(d, def + 4)? as usize,
            u32_at(d, def + 8)? as usize,
            u32_at(d, def + 16)? as usize,
        );
        let n_defs = nu + na + nt;
        let strings = def + 20 + 12 * n_defs;
        let define = |i: usize| -> Result<(String, u32)> {
            let o = def + 20 + 12 * i;
            Ok((
                cstr(d, strings + u32_at(d, o)? as usize)?,
                u32_at(d, o + 4)?,
            ))
        };
        let user_params = (0..nu).map(define).collect::<Result<Vec<_>>>()?;
        let asset_params = (nu..nu + na).map(define).collect::<Result<Vec<_>>>()?;
        let direct_base = local_names + 4 * (n_local + n_enum);
        let random_base = direct_base + 4 * n_direct;
        let curve_base = random_base + 8 * n_random;
        Ok(Self {
            d,
            users,
            name_pos,
            cond_pos,
            asset_params,
            user_params,
            asset_param_base: def + def_size,
            direct_base,
            random_base,
            curve_base,
            point_base: curve_base + 20 * n_curve,
            n_random,
            n_curve,
            n_point,
        })
    }

    fn name(&self, o: usize) -> Result<String> {
        cstr(self.d, self.name_pos + o)
    }

    fn user_offset(&self, name: &str) -> Option<usize> {
        self.users
            .get(&crc32fast::hash(name.as_bytes()))
            .map(|&u| u as usize)
    }

    /// True if the database has a user called `name`.
    pub fn has_user(&self, name: &str) -> bool {
        self.user_offset(name).is_some()
    }

    /// Number of users (names are not stored; callers supply candidates).
    pub fn user_count(&self) -> usize {
        self.users.len()
    }

    /// The action triggers of user `name`: which call each of its actions (named states or
    /// animations of the object, grouped in action slots) starts.
    ///
    /// Layout at user + header[11]: action slots (header[5]) {u32 name pos, u16 first action,
    /// u16 last action}; actions (header[6]) {u32 name pos, u32 first trigger, u32 last
    /// trigger}; triggers (header[7], 24 bytes) {u32 guid, u32 call index × 32, u32 start
    /// (a frame, or with flag 0x10 the name pos of the action it must follow), i32 end frame
    /// (0x7FFFFFFF: none), u32 flag, i32 overwrite param pos}.
    pub fn user_actions(&self, name: &str) -> Result<Option<Vec<ActionTrigger>>> {
        let Some(u) = self.user_offset(name) else {
            return Ok(None);
        };
        let d = self.d;
        let n_call = i32_at(d, u + 8)?.max(0) as usize;
        let n_slot = i32_at(d, u + 20)?.max(0) as usize;
        let n_act = i32_at(d, u + 24)?.max(0) as usize;
        let n_trig = i32_at(d, u + 28)?.max(0) as usize;
        let mut t = u + u32_at(d, u + 44)? as usize;
        let mut slots = Vec::with_capacity(n_slot);
        for _ in 0..n_slot {
            let name = self.name(u32_at(d, t)? as usize)?;
            let a = u16_at(d, t + 4)? as usize;
            let b = u16_at(d, t + 6)? as usize;
            slots.push((name, a, b));
            t += 8;
        }
        let mut acts = Vec::with_capacity(n_act);
        for _ in 0..n_act {
            acts.push((
                self.name(u32_at(d, t)? as usize)?,
                u32_at(d, t + 4)? as usize,
                u32_at(d, t + 8)? as usize,
            ));
            t += 12;
        }
        let mut trigs = Vec::with_capacity(n_trig);
        for _ in 0..n_trig {
            let call = u32_at(d, t + 4)? as usize / 32;
            let start = u32_at(d, t + 8)?;
            let end = i32_at(d, t + 12)?;
            let flag = u32_at(d, t + 16)?;
            trigs.push((call, start, end, flag));
            t += 24;
        }
        let mut out = Vec::new();
        for (ai, (action, first, last)) in acts.iter().enumerate() {
            let slot = slots
                .iter()
                .find(|(_, a, b)| (*a..=*b).contains(&ai))
                .map(|s| s.0.clone())
                .unwrap_or_default();
            for &(call, start, end, flag) in trigs.get(*first..=*last).into_iter().flatten() {
                if call >= n_call {
                    continue;
                }
                let after = if flag & 0x10 != 0 && start != 0 {
                    Some(self.name(start as usize)?)
                } else {
                    None
                };
                out.push(ActionTrigger {
                    slot: slot.clone(),
                    action: action.clone(),
                    call,
                    start_frame: if after.is_none() { start as i32 } else { 0 },
                    end_frame: (end != i32::MAX).then_some(end),
                    after,
                    flag,
                });
            }
        }
        Ok(Some(out))
    }

    /// The user-level parameters of `name` (default distance set, group, limits …).
    pub fn user_params(&self, name: &str) -> Result<Option<BTreeMap<String, Param>>> {
        let Some(u) = self.user_offset(name) else {
            return Ok(None);
        };
        let n_local = i32_at(self.d, u + 4)?.max(0) as usize;
        let base = u + 48 + 4 * n_local;
        let mut out = BTreeMap::new();
        for (i, (pname, ptype)) in self.user_params.iter().enumerate() {
            let raw = u32_at(self.d, base + 4 * i)?;
            if let Some(v) = self.value(raw, *ptype)? {
                out.insert(pname.clone(), v);
            }
        }
        Ok(Some(out))
    }

    /// The asset calls of user `name` (None if there is no such user).
    pub fn user_calls(&self, name: &str) -> Result<Option<Vec<Call>>> {
        let Some(u) = self.user_offset(name) else {
            return Ok(None);
        };
        let d = self.d;
        let n_local = i32_at(d, u + 4)?.max(0) as usize;
        let n_call = i32_at(d, u + 8)?.max(0) as usize;
        let mut p = u + 48 + 4 * n_local + 4 * self.user_params.len() + 2 * n_call;
        p = (p + 3) & !3;
        let ex = p + 32 * n_call;
        let mut out = Vec::with_capacity(n_call);
        for i in 0..n_call {
            let o = p + 32 * i;
            let key = self.name(u32_at(d, o)? as usize)?;
            let asset = d
                .get(o + 4..o + 6)
                .map(|b| i16::from_le_bytes([b[0], b[1]]))
                .ok_or_else(|| bad("call"))?;
            let parent = i32_at(d, o + 12)?;
            let param_pos = u32_at(d, o + 24)? as usize;
            let cond = i32_at(d, o + 28)?;
            let container = asset < 0;
            let (mut kind, mut children, mut watch) = (None, None, None);
            let params = if container {
                let c = ex + param_pos;
                let k = ContainerKind::from_raw(u32_at(d, c)?);
                let (a, b) = (i32_at(d, c + 4)?, i32_at(d, c + 8)?);
                if a >= 0 && b >= a && (b as usize) < n_call {
                    children = Some((a as usize, b as usize));
                }
                if k == ContainerKind::Switch {
                    let name = self.name(u32_at(d, c + 12)? as usize)?;
                    let global = d.get(c + 22).copied().unwrap_or(0) != 0;
                    watch = Some((name, global));
                }
                kind = Some(k);
                BTreeMap::new()
            } else {
                self.params(param_pos)?
            };
            let condition = if cond >= 0 {
                Some(self.condition(cond as usize)?)
            } else {
                None
            };
            out.push(Call {
                key,
                params,
                container,
                parent: (parent >= 0).then_some(parent as usize),
                conditional: cond >= 0,
                kind,
                children,
                watch,
                condition,
            });
        }
        Ok(Some(out))
    }

    fn condition(&self, pos: usize) -> Result<Condition> {
        let d = self.d;
        let o = self.cond_pos + pos;
        Ok(match u32_at(d, o)? {
            0 => {
                let ptype = u32_at(d, o + 4)?;
                let cmp = Compare::from_raw(u32_at(d, o + 8)?).unwrap_or(Compare::Eq);
                let raw = u32_at(d, o + 12)?;
                let value = match ptype {
                    0 => CondValue::Enum(self.name(raw as usize)?),
                    2 => CondValue::Float(f32::from_bits(raw)),
                    _ => CondValue::Int(raw as i32),
                };
                let global = d.get(o + 19).copied().unwrap_or(0) != 0;
                Condition::Switch { cmp, value, global }
            }
            1 | 2 => Condition::Random {
                weight: f32_at(d, o + 4)?,
            },
            k => Condition::Other(vec![k, u32_at(d, o + 4)?, u32_at(d, o + 8)?]),
        })
    }

    fn curve(&self, i: usize) -> Result<Curve> {
        if i >= self.n_curve {
            return Err(bad(format!("curve {i} out of range")));
        }
        let d = self.d;
        let o = self.curve_base + 20 * i;
        let first = u16_at(d, o)? as usize;
        let n = u16_at(d, o + 2)? as usize;
        let global = u16_at(d, o + 6)? != 0;
        let property = self.name(u32_at(d, o + 8)? as usize)?;
        let mut points = Vec::with_capacity(n);
        for k in first..(first + n).min(self.n_point) {
            let p = self.point_base + 8 * k;
            points.push((f32_at(d, p)?, f32_at(d, p + 4)?));
        }
        Ok(Curve {
            property,
            global,
            points,
        })
    }

    /// One encoded parameter value (kind << 24 | value) of a define of type `ptype`.
    fn value(&self, raw: u32, ptype: u32) -> Result<Option<Param>> {
        let (kind, value) = (raw >> 24, (raw & 0xFF_FFFF) as usize);
        Ok(Some(match kind {
            1 => Param::Str(self.name(value)?),
            0 => {
                let bits = u32_at(self.d, self.direct_base + 4 * value)?;
                // define type 1 = float, else an integer
                if ptype == 1 {
                    Param::Num(f32::from_bits(bits))
                } else {
                    Param::Int(bits as i32)
                }
            }
            2 => Param::Curve(self.curve(value)?),
            3 if value < self.n_random => {
                let o = self.random_base + 8 * value;
                Param::Range(f32_at(self.d, o)?, f32_at(self.d, o + 4)?)
            }
            // random ranges with a skewed distribution: the uniform draw raised to a power
            // (kinds 6–13: ½, ⅓, ¼, 1/1.5, 2, 3, 4, 1.5) [inf: by analogy with the kind 3
            // range they share the table with]
            6..=13 if value < self.n_random => {
                let o = self.random_base + 8 * value;
                let power =
                    [0.5, 1.0 / 3.0, 0.25, 1.0 / 1.5, 2.0, 3.0, 4.0, 1.5][kind as usize - 6];
                Param::RangePow(f32_at(self.d, o)?, f32_at(self.d, o + 4)?, power)
            }
            _ => Param::Raw(raw),
        }))
    }

    fn params(&self, pos: usize) -> Result<BTreeMap<String, Param>> {
        let d = self.d;
        let at = self.asset_param_base + pos;
        let mask = u32_at(d, at)? as u64 | (u32_at(d, at + 4)? as u64) << 32;
        let mut out = BTreeMap::new();
        let mut k = 0;
        for bit in 0..64 {
            if mask >> bit & 1 == 0 {
                continue;
            }
            let raw = u32_at(d, at + 8 + 4 * k)?;
            k += 1;
            let Some((pname, ptype)) = self.asset_params.get(bit) else {
                continue;
            };
            if let Some(v) = self.value(raw, *ptype)? {
                out.insert(pname.clone(), v);
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_other_files() {
        assert!(Xlnk::parse(b"NOPE").is_err());
        let mut d = vec![0u8; 0x48];
        d[0..4].copy_from_slice(b"XLNK");
        d[8] = 0x21;
        assert!(Xlnk::parse(&d).is_err());
    }

    /// A tiny synthetic database: one user with a switch container over two assets.
    fn synthetic() -> Vec<u8> {
        let mut d = vec![0u8; 0x48];
        d[0..4].copy_from_slice(b"XLNK");
        d[8] = 0x1C;
        // layout: header | user crc + offset | param defines | asset params | local names |
        // direct values | random | curves | points | user | conditions | names
        let names: &[&str] = &[
            "key_sw",
            "a",
            "b",
            "WaveA",
            "WaveB",
            "Prop",
            "On",
            "RuntimeAssetName",
            "Volume",
            "Pitch",
        ];
        let mut name_tab = Vec::new();
        let mut name_off = Vec::new();
        for n in names {
            name_off.push(name_tab.len() as u32);
            name_tab.extend_from_slice(n.as_bytes());
            name_tab.push(0);
        }
        let push32 = |d: &mut Vec<u8>, v: u32| d.extend_from_slice(&v.to_le_bytes());
        // users
        let user_crc = crc32fast::hash(b"U");
        let user_off_pos = d.len() + 4;
        push32(&mut d, user_crc);
        push32(&mut d, 0); // patched
        // param defines: 0 user, 3 asset, 0, 0 trigger; names after
        let def = d.len();
        let pnames = ["RuntimeAssetName", "Volume", "Pitch"];
        let mut pstr = Vec::new();
        let mut defs = Vec::new();
        for (i, p) in pnames.iter().enumerate() {
            defs.push((pstr.len() as u32, if i == 0 { 4 } else { 1 }));
            pstr.extend_from_slice(p.as_bytes());
            pstr.push(0);
        }
        let def_size = 20 + 12 * defs.len() + pstr.len();
        push32(&mut d, def_size as u32);
        push32(&mut d, 0);
        push32(&mut d, 3);
        push32(&mut d, 0);
        push32(&mut d, 0);
        for (n, t) in &defs {
            push32(&mut d, *n);
            push32(&mut d, *t);
            push32(&mut d, 0);
        }
        d.extend_from_slice(&pstr);
        assert_eq!(d.len(), def + def_size);
        // asset params: A = {name WaveA, volume direct 0, pitch random 0}; B = {name WaveB}
        let ap_a = d.len() - (def + def_size);
        push32(&mut d, 0b111);
        push32(&mut d, 0);
        push32(&mut d, 1 << 24 | name_off[3]);
        push32(&mut d, 0);
        push32(&mut d, 3 << 24);
        let ap_b = d.len() - (def + def_size);
        push32(&mut d, 0b1);
        push32(&mut d, 0);
        push32(&mut d, 1 << 24 | name_off[4]);
        // local property names (none) → direct values: [0.5]; random: [(0.9, 1.1)]
        let local = d.len();
        push32(&mut d, 0.5f32.to_bits());
        push32(&mut d, 0.9f32.to_bits());
        push32(&mut d, 1.1f32.to_bits());
        // user: header (12 words), no locals, no user params, 3 calls (+ sort table, padded)
        let user = d.len();
        let mut hdr = [0u32; 12];
        hdr[2] = 3;
        hdr[3] = 2;
        for v in hdr {
            push32(&mut d, v);
        }
        for i in 0..3u16 {
            d.extend_from_slice(&i.to_le_bytes());
        }
        while !d.len().is_multiple_of(4) {
            d.push(0);
        }
        // calls: 0 switch container (children 1..2), 1 A (cond 0), 2 B (no cond: fallback)
        let call =
            |d: &mut Vec<u8>, key: u32, asset: i16, flag: u16, parent: i32, pp: u32, cond: i32| {
                push32(d, key);
                d.extend_from_slice(&asset.to_le_bytes());
                d.extend_from_slice(&flag.to_le_bytes());
                push32(d, 1);
                push32(d, parent as u32);
                push32(d, 0);
                push32(d, 0);
                push32(d, pp);
                push32(d, cond as u32);
            };
        call(&mut d, name_off[0], -1, 1, -1, 0, -1);
        call(&mut d, name_off[1], 0, 0, 0, ap_a as u32, 0);
        call(&mut d, name_off[2], 1, 0, 0, ap_b as u32, -1);
        // container param right after the call table: switch, children 1..2, watch "Prop"
        push32(&mut d, 0);
        push32(&mut d, 1);
        push32(&mut d, 2);
        push32(&mut d, name_off[5]);
        push32(&mut d, u32::MAX);
        push32(&mut d, 0x0001_FFFF); // local index −1, global
        // conditions: switch, enum, ==, "On", enum idx −1, solved 0, global 1
        let cond = d.len();
        push32(&mut d, 0);
        push32(&mut d, 0);
        push32(&mut d, 0);
        push32(&mut d, name_off[6]);
        push32(&mut d, 0x0100_FFFF);
        let names_at = d.len();
        d.extend_from_slice(&name_tab);
        // patch header fields
        let put =
            |d: &mut Vec<u8>, at: usize, v: u32| d[at..at + 4].copy_from_slice(&v.to_le_bytes());
        put(&mut d, 0x1C, local as u32);
        put(&mut d, 0x28, 1);
        put(&mut d, 0x2C, 1);
        put(&mut d, 0x3C, 1);
        put(&mut d, 0x40, cond as u32);
        put(&mut d, 0x44, names_at as u32);
        put(&mut d, user_off_pos, user as u32);
        d
    }

    #[test]
    fn reads_containers_conditions_and_ranges() {
        let d = synthetic();
        let x = Xlnk::parse(&d).unwrap();
        assert!(x.has_user("U") && !x.has_user("V"));
        let calls = x.user_calls("U").unwrap().unwrap();
        assert_eq!(calls.len(), 3);
        assert_eq!(calls[0].kind, Some(ContainerKind::Switch));
        assert_eq!(calls[0].children, Some((1, 2)));
        assert_eq!(calls[0].watch, Some(("Prop".to_string(), true)));
        assert_eq!(
            calls[1].condition,
            Some(Condition::Switch {
                cmp: Compare::Eq,
                value: CondValue::Enum("On".into()),
                global: true
            })
        );
        assert_eq!(calls[1].params["RuntimeAssetName"].as_str(), Some("WaveA"));
        assert_eq!(calls[1].params["Volume"].as_f32(), Some(0.5));
        assert_eq!(calls[1].params["Pitch"], Param::Range(0.9, 1.1));
        assert_eq!(calls[2].condition, None);
        assert_eq!(calls[2].parent, Some(0));
    }
}
