//! XLNK ("SLink2DB.bslnk"): the database that links a game object's named events to sound
//! assets. Read here only as far as SMM2's UI sounds need: the users (by name), each user's
//! asset calls (event key → asset or container of child calls) and the asset parameters
//! (sound name, volume, delay).
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
//!       then one u32 per set bit: type << 24 | value (1 = name table offset, 0 = index into
//!       the direct value table)
//! direct value table: u32 (float or int) × direct values, right after the local property
//!       name refs (local property name ref table pos + 4 × (names + enum names))
//! user: 12 × i32 header (local properties, calls, assets at [1..=3]), u32 local property name
//!       refs, u32 user params (param define table's user count), u16 call sort table × calls
//!       (padded to 4), then 32-byte calls: u32 key name pos, i16 asset index (−1: container),
//!       u16 flag, i32 duration, i32 parent call, u32 guid, u32 key hash, u32 param pos (asset
//!       param table offset), i32 condition pos (−1: none)
//! ```

use std::collections::BTreeMap;

use crate::error::{Error, Result};

fn bad(detail: impl Into<String>) -> Error {
    Error::Format {
        what: "XLNK",
        detail: detail.into(),
    }
}

/// One event of a user: a sound asset, or a container choosing among its child calls by
/// conditions (input device, scene, …).
#[derive(Clone, Debug, PartialEq)]
pub struct Call {
    pub key: String,
    /// The asset's parameters by name (strings from the name table, numbers from the direct
    /// value table); empty for a container.
    pub params: BTreeMap<String, Param>,
    pub container: bool,
    /// Index of the parent call (a container), if any.
    pub parent: Option<usize>,
    /// The call has a condition (one of a container's alternatives).
    pub conditional: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Param {
    Str(String),
    Num(f32),
    Int(i32),
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
    asset_params: Vec<(String, u32)>,
    user_param_count: usize,
    asset_param_base: usize,
    direct_base: usize,
}

fn u32_at(d: &[u8], o: usize) -> Result<u32> {
    d.get(o..o + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or_else(|| bad(format!("truncated at {o:#x}")))
}

fn i32_at(d: &[u8], o: usize) -> Result<i32> {
    u32_at(d, o).map(|v| v as i32)
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
        if version != 0x1C {
            return Err(bad(format!("unsupported version {version:#x}")));
        }
        let local_names = i32_at(d, 0x1C)? as usize;
        let n_local = i32_at(d, 0x20)?.max(0) as usize;
        let n_enum = i32_at(d, 0x24)?.max(0) as usize;
        let n_users = i32_at(d, 0x3C)?.max(0) as usize;
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
        let (nu, na, nx, nt) = (
            u32_at(d, def + 4)? as usize,
            u32_at(d, def + 8)? as usize,
            u32_at(d, def + 12)? as usize,
            u32_at(d, def + 16)? as usize,
        );
        let _ = nx;
        let n_defs = nu + na + nt;
        let strings = def + 20 + 12 * n_defs;
        let mut asset_params = Vec::with_capacity(na);
        for i in nu..nu + na {
            let o = def + 20 + 12 * i;
            asset_params.push((
                cstr(d, strings + u32_at(d, o)? as usize)?,
                u32_at(d, o + 4)?,
            ));
        }
        Ok(Self {
            d,
            users,
            name_pos,
            asset_params,
            user_param_count: nu,
            asset_param_base: def + def_size,
            direct_base: local_names + 4 * (n_local + n_enum),
        })
    }

    fn name(&self, o: usize) -> Result<String> {
        cstr(self.d, self.name_pos + o)
    }

    /// The asset calls of user `name` (None if there is no such user).
    pub fn user_calls(&self, name: &str) -> Result<Option<Vec<Call>>> {
        let Some(&u) = self.users.get(&crc32fast::hash(name.as_bytes())) else {
            return Ok(None);
        };
        let d = self.d;
        let u = u as usize;
        let n_local = i32_at(d, u + 4)?.max(0) as usize;
        let n_call = i32_at(d, u + 8)?.max(0) as usize;
        let mut p = u + 48 + 4 * n_local + 4 * self.user_param_count + 2 * n_call;
        p = (p + 3) & !3;
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
            let params = if asset >= 0 {
                self.params(param_pos)?
            } else {
                BTreeMap::new()
            };
            out.push(Call {
                key,
                params,
                container: asset < 0,
                parent: (parent >= 0).then_some(parent as usize),
                conditional: cond >= 0,
            });
        }
        Ok(Some(out))
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
            let (kind, value) = (raw >> 24, raw & 0xFF_FFFF);
            let Some((pname, ptype)) = self.asset_params.get(bit) else {
                continue;
            };
            let v = match kind {
                1 => Param::Str(self.name(value as usize)?),
                0 => {
                    let bits = u32_at(d, self.direct_base + 4 * value as usize)?;
                    // define type 1 = float, else an integer
                    if *ptype == 1 {
                        Param::Num(f32::from_bits(bits))
                    } else {
                        Param::Int(bits as i32)
                    }
                }
                _ => Param::Raw(raw),
            };
            out.insert(pname.clone(), v);
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
}
