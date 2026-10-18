//! Sound engine settings in `Pack/StaticSound.pack`: the group tree (`Sound/Group/
//! GroupSetting.bagst`, "AGST") and the distance attenuation sets (`Sound/Attenuation/
//! Attenuation.baatarc`, a SARC of "AATN" sets that name "AROC" roll-off curves and "AACL"
//! culling ranges).
//!
//! AGST version 4 (little-endian), from inspection of the owner's file:
//!
//! ```text
//! 0x00  "AGST" u16 BOM u16 version  u32 size  u32 TREE offset  u32 GRP offset  u32 MAND offset
//!       u32 STRG offset
//! TREE: "TREE" u32 size, then nodes {u32 node size, u32 group index, u32 children, children…}
//! GRP:  "GRP " u32 size  u32 groups  u32 record offset × groups (file offsets); a record is
//!       u32 words: [0] name (STRG offset), [1] folder flag, [4] volume, [22] limit type,
//!       [23] voice limit (−1: none), [38] ducking on, [39] ducked groups n, then n × (name,
//!       volume, fade-in s, fade-out s, ?)
//! STRG: "STRG" u32 size, NUL-terminated names
//! ```
//!
//! AATN: `u32 0x44` (string table) then name offsets: [1] volume roll-off, [9] second roll-off,
//! [12] culling. AROC: u32 kind (0 none, 1 linear), f32 max distance, f32 min distance, f32 gain
//! at the max distance, f32 gain at the min distance. AACL: f32 distance, f32 margin, u32 on.

use crate::error::{Error, Result};

fn bad(what: &'static str, detail: impl Into<String>) -> Error {
    Error::Format {
        what,
        detail: detail.into(),
    }
}

fn u32_at(what: &'static str, d: &[u8], o: usize) -> Result<u32> {
    d.get(o..o + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or_else(|| bad(what, format!("truncated at {o:#x}")))
}

fn cstr(what: &'static str, d: &[u8], o: usize) -> Result<String> {
    let rest = d.get(o..).ok_or_else(|| bad(what, "string past end"))?;
    let end = rest.iter().position(|&b| b == 0).unwrap_or(rest.len());
    Ok(String::from_utf8_lossy(&rest[..end]).into_owned())
}

fn find(d: &[u8], magic: &[u8; 4]) -> Option<usize> {
    d.windows(4).position(|w| w == magic)
}

/// A group another group lowers while it plays.
#[derive(Clone, Debug, PartialEq)]
pub struct Duck {
    pub group: String,
    pub volume: f32,
    pub fade_in: f32,
    pub fade_out: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    pub name: String,
    /// Parent folder (None for the root).
    pub parent: Option<String>,
    pub folder: bool,
    pub volume: f32,
    pub limit_type: u32,
    /// Voices the group (folder: all its groups together) may play at once.
    pub limit: Option<u32>,
    pub duck: Vec<Duck>,
}

/// Parse `GroupSetting.bagst`: every group, parents before children.
pub fn parse_groups(d: &[u8]) -> Result<Vec<Group>> {
    const W: &str = "AGST";
    if d.len() < 0x1C || &d[0..4] != b"AGST" {
        return Err(Error::Magic { what: W });
    }
    let strg = find(d, b"STRG").ok_or_else(|| bad(W, "no STRG"))? + 8;
    let grp = find(d, b"GRP ").ok_or_else(|| bad(W, "no GRP"))?;
    let tree = find(d, b"TREE").ok_or_else(|| bad(W, "no TREE"))?;
    let n = u32_at(W, d, grp + 8)? as usize;
    if n > 4096 {
        return Err(bad(W, "group count"));
    }
    let offs: Vec<usize> = (0..n)
        .map(|i| u32_at(W, d, grp + 12 + 4 * i).map(|v| v as usize))
        .collect::<Result<_>>()?;
    let f = |o: usize| u32_at(W, d, o).map(f32::from_bits);
    let record = |i: usize| -> Result<Group> {
        let o = *offs.get(i).ok_or_else(|| bad(W, "group index"))?;
        let w = |k: usize| u32_at(W, d, o + 4 * k);
        let limit = w(23)?;
        let n_duck = if w(38)? != 0 { w(39)? as usize } else { 0 };
        let mut duck = Vec::new();
        for k in 0..n_duck.min(16) {
            let e = o + 4 * (40 + 5 * k);
            duck.push(Duck {
                group: cstr(W, d, strg + u32_at(W, d, e)? as usize)?,
                volume: f(e + 4)?,
                fade_in: f(e + 8)?,
                fade_out: f(e + 12)?,
            });
        }
        Ok(Group {
            name: cstr(W, d, strg + w(0)? as usize)?,
            parent: None,
            folder: w(1)? != 0,
            volume: f(o + 16)?,
            limit_type: w(22)?,
            limit: (limit != u32::MAX).then_some(limit),
            duck,
        })
    };
    let mut out = Vec::with_capacity(n);
    // walk the tree: (node offset, parent name)
    fn walk(
        d: &[u8],
        p: usize,
        parent: Option<String>,
        depth: usize,
        record: &dyn Fn(usize) -> Result<Group>,
        out: &mut Vec<Group>,
    ) -> Result<usize> {
        if depth > 32 {
            return Err(bad("AGST", "tree too deep"));
        }
        let size = u32_at("AGST", d, p)? as usize;
        let idx = u32_at("AGST", d, p + 4)? as usize;
        let nch = u32_at("AGST", d, p + 8)? as usize;
        let mut g = record(idx)?;
        g.parent = parent;
        let name = g.name.clone();
        out.push(g);
        let mut q = p + 12;
        for _ in 0..nch {
            q = walk(d, q, Some(name.clone()), depth + 1, record, out)?;
        }
        Ok(p + size.max(12))
    }
    walk(d, tree + 12, None, 0, &record, &mut out)?;
    Ok(out)
}

/// A distance roll-off: gain `near_gain` up to `min` distance, then linear to `far_gain` at `max`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rolloff {
    pub linear: bool,
    pub max: f32,
    pub min: f32,
    pub far_gain: f32,
    pub near_gain: f32,
}

pub fn parse_rolloff(d: &[u8]) -> Result<Rolloff> {
    const W: &str = "AROC";
    if d.len() < 0x1C || &d[0..4] != b"AROC" {
        return Err(Error::Magic { what: W });
    }
    let f = |o: usize| u32_at(W, d, o).map(f32::from_bits);
    let finite = |v: f32| if v.is_finite() { v } else { 0.0 };
    Ok(Rolloff {
        linear: u32_at(W, d, 8)? == 1,
        max: finite(f(12)?),
        min: finite(f(16)?),
        far_gain: finite(f(20)?),
        near_gain: finite(f(24)?),
    })
}

/// Culling: sounds farther than `distance` don't start.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Culling {
    pub distance: f32,
    pub margin: f32,
    pub on: bool,
}

pub fn parse_culling(d: &[u8]) -> Result<Culling> {
    const W: &str = "AACL";
    if d.len() < 0x14 || &d[0..4] != b"AACL" {
        return Err(Error::Magic { what: W });
    }
    Ok(Culling {
        distance: f32::from_bits(u32_at(W, d, 8)?),
        margin: f32::from_bits(u32_at(W, d, 12)?),
        on: u32_at(W, d, 16)? != 0,
    })
}

/// An attenuation set: names of its volume roll-off and culling (empty = none).
#[derive(Clone, Debug, PartialEq)]
pub struct AttenuationSet {
    pub volume: String,
    pub culling: String,
}

pub fn parse_attenuation(d: &[u8]) -> Result<AttenuationSet> {
    const W: &str = "AATN";
    if d.len() < 0x44 || &d[0..4] != b"AATN" {
        return Err(Error::Magic { what: W });
    }
    let strings = u32_at(W, d, 8)? as usize;
    let s =
        |k: usize| -> Result<String> { cstr(W, d, strings + u32_at(W, d, 8 + 4 * k)? as usize) };
    Ok(AttenuationSet {
        volume: s(1)?,
        culling: s(12)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn le(words: &[u32]) -> Vec<u8> {
        words.iter().flat_map(|w| w.to_le_bytes()).collect()
    }

    #[test]
    fn rolloff_culling_and_set() {
        let mut d = b"AROC\xff\xfe\x02\x00".to_vec();
        d.extend(le(&[
            1,
            70f32.to_bits(),
            0,
            0.6f32.to_bits(),
            1f32.to_bits(),
            0,
            0,
        ]));
        let r = parse_rolloff(&d).unwrap();
        assert!(r.linear && r.max == 70.0 && r.far_gain == 0.6 && r.near_gain == 1.0);
        let mut c = b"AACL\xff\xfe\x02\x00".to_vec();
        c.extend(le(&[240f32.to_bits(), 50f32.to_bits(), 1]));
        assert_eq!(
            parse_culling(&c).unwrap(),
            Culling {
                distance: 240.0,
                margin: 50.0,
                on: true
            }
        );
        let mut a = b"AATN\xff\xfe\x01\x00".to_vec();
        a.extend(le(&[0x44, 0, 0, 10, 2, 10, 2, 10, 2, 11, 0, 10, 0, 0, 0]));
        a.resize(0x44, 0);
        a.extend_from_slice(b"3W_Middle\0\0NoDecay\0");
        let s = parse_attenuation(&a).unwrap();
        assert_eq!(s.volume, "3W_Middle");
        assert_eq!(s.culling, "3W_Middle");
    }

    #[test]
    fn group_tree() {
        // two groups: Root (folder) → Child (limit 1, ducks Root to 0.5)
        let rec = |name: u32, folder: u32, limit: u32, duck: Option<u32>| {
            let mut w = vec![0u32; 50];
            w[0] = name;
            w[1] = folder;
            w[4] = 1f32.to_bits();
            w[22] = 2;
            w[23] = limit;
            if let Some(t) = duck {
                w[38] = 1;
                w[39] = 1;
                w[40] = t;
                w[41] = 0.5f32.to_bits();
                w[42] = 0.1f32.to_bits();
                w[43] = 0.2f32.to_bits();
            }
            le(&w)
        };
        let strs = b"Root\0Child\0";
        let tree = le(&[24, 0, 1, 12, 1, 0]);
        let r0 = rec(0, 1, u32::MAX, None);
        let r1 = rec(5, 0, 1, Some(0));
        let tree_at = 0x1C;
        let grp_at = tree_at + 8 + 4 + tree.len();
        let rec0 = grp_at + 12 + 8;
        let rec1 = rec0 + r0.len();
        let strg_at = rec1 + r1.len();
        let mut d = b"AGST\xff\xfe\x04\x00".to_vec();
        d.extend(le(&[0, tree_at as u32, grp_at as u32, 0, strg_at as u32]));
        d.extend_from_slice(b"TREE");
        d.extend(le(&[tree.len() as u32 + 4, 0]));
        d.extend(&tree);
        d.extend_from_slice(b"GRP ");
        d.extend(le(&[0, 2, rec0 as u32, rec1 as u32]));
        d.extend(&r0);
        d.extend(&r1);
        d.extend_from_slice(b"STRG");
        d.extend(le(&[strs.len() as u32]));
        d.extend_from_slice(strs);
        let g = parse_groups(&d).unwrap();
        assert_eq!(g.len(), 2);
        assert_eq!(g[0].name, "Root");
        assert!(g[0].folder && g[0].limit.is_none());
        assert_eq!(g[1].parent.as_deref(), Some("Root"));
        assert_eq!(g[1].limit, Some(1));
        assert_eq!(g[1].duck[0].group, "Root");
        assert_eq!(g[1].duck[0].volume, 0.5);
    }
}
