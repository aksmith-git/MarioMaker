//! AAMP parameter archives (version 2, little endian): the `agl` / `gsys` environment files
//! (`.baglenv` lights and fog, `.bagllmap` light maps, …) inside the `Env/*.genvb` archives.
//!
//! A file is a tree of lists; each list holds lists and objects, each object holds parameters.
//! Every list, object and parameter is named by the CRC-32 of its name; [`hash`] makes the key
//! to look one up.

use crate::error::{Error, Result};

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Bool(bool),
    F32(f32),
    Int(i32),
    U32(u32),
    /// vec2 / vec3 / vec4 / colour / quaternion.
    Vec(Vec<f32>),
    Str(String),
    /// Curves, buffers and other types this reader leaves undecoded (type id).
    Other(u8),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Object {
    pub hash: u32,
    pub params: Vec<(u32, Value)>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct List {
    pub hash: u32,
    pub lists: Vec<List>,
    pub objects: Vec<Object>,
}

/// Key of a name (CRC-32).
pub fn hash(name: &str) -> u32 {
    crc32fast::hash(name.as_bytes())
}

impl Object {
    pub fn get(&self, name: &str) -> Option<&Value> {
        let h = hash(name);
        self.params.iter().find(|(k, _)| *k == h).map(|(_, v)| v)
    }
    pub fn f32(&self, name: &str) -> Option<f32> {
        match self.get(name)? {
            Value::F32(v) => Some(*v),
            Value::Int(v) => Some(*v as f32),
            _ => None,
        }
    }
    pub fn bool(&self, name: &str) -> Option<bool> {
        match self.get(name)? {
            Value::Bool(v) => Some(*v),
            _ => None,
        }
    }
    pub fn vec(&self, name: &str) -> Option<&[f32]> {
        match self.get(name)? {
            Value::Vec(v) => Some(v),
            _ => None,
        }
    }
    pub fn str(&self, name: &str) -> Option<&str> {
        match self.get(name)? {
            Value::Str(v) => Some(v),
            _ => None,
        }
    }
}

impl List {
    pub fn list(&self, name: &str) -> Option<&List> {
        let h = hash(name);
        self.lists.iter().find(|l| l.hash == h)
    }
    pub fn object(&self, name: &str) -> Option<&Object> {
        let h = hash(name);
        self.objects.iter().find(|o| o.hash == h)
    }
}

fn bad(s: impl Into<String>) -> Error {
    Error::Format {
        what: "AAMP",
        detail: s.into(),
    }
}

struct R<'a>(&'a [u8]);

impl R<'_> {
    fn u8(&self, o: usize) -> Result<u8> {
        self.0
            .get(o)
            .copied()
            .ok_or_else(|| bad("read outside the file"))
    }
    fn u16(&self, o: usize) -> Result<u16> {
        Ok(u16::from_le_bytes([self.u8(o)?, self.u8(o + 1)?]))
    }
    fn u32(&self, o: usize) -> Result<u32> {
        Ok(u32::from_le_bytes([
            self.u8(o)?,
            self.u8(o + 1)?,
            self.u8(o + 2)?,
            self.u8(o + 3)?,
        ]))
    }
    fn f32(&self, o: usize) -> Result<f32> {
        Ok(f32::from_bits(self.u32(o)?))
    }
    fn cstr(&self, o: usize) -> Result<String> {
        let rest = self
            .0
            .get(o..)
            .ok_or_else(|| bad("string outside the file"))?;
        let end = rest.iter().position(|&b| b == 0).unwrap_or(rest.len());
        Ok(String::from_utf8_lossy(&rest[..end]).into_owned())
    }
}

/// Parse a file: (data type, e.g. "agle", root list).
pub fn parse(data: &[u8]) -> Result<(String, List)> {
    let r = R(data);
    if data.get(..4) != Some(b"AAMP") {
        return Err(bad("not an AAMP file"));
    }
    if r.u32(4)? != 2 {
        return Err(bad("unsupported version"));
    }
    let pio_type_len = r.u32(0x14)? as usize;
    let kind = r.cstr(0x30)?;
    let root = 0x30 + pio_type_len;
    Ok((kind, list(&r, root, 0)?))
}

fn list(r: &R, at: usize, depth: usize) -> Result<List> {
    if depth > 32 {
        return Err(bad("lists nested too deep"));
    }
    let hash = r.u32(at)?;
    let lists_at = at + r.u16(at + 4)? as usize * 4;
    let list_count = r.u16(at + 6)? as usize;
    let objs_at = at + r.u16(at + 8)? as usize * 4;
    let obj_count = r.u16(at + 10)? as usize;
    let lists = (0..list_count)
        .map(|i| list(r, lists_at + 12 * i, depth + 1))
        .collect::<Result<Vec<_>>>()?;
    let objects = (0..obj_count)
        .map(|i| object(r, objs_at + 8 * i))
        .collect::<Result<Vec<_>>>()?;
    Ok(List {
        hash,
        lists,
        objects,
    })
}

fn object(r: &R, at: usize) -> Result<Object> {
    let hash = r.u32(at)?;
    let params_at = at + r.u16(at + 4)? as usize * 4;
    let count = r.u16(at + 6)? as usize;
    let params = (0..count)
        .map(|i| {
            let p = params_at + 8 * i;
            let key = r.u32(p)?;
            let w = r.u32(p + 4)?;
            let d = p + (w & 0xFF_FFFF) as usize * 4;
            let ty = (w >> 24) as u8;
            let v = match ty {
                0 => Value::Bool(r.u32(d)? != 0),
                1 => Value::F32(r.f32(d)?),
                2 => Value::Int(r.u32(d)? as i32),
                3..=6 | 16 => {
                    let n = match ty {
                        3 => 2,
                        4 => 3,
                        _ => 4,
                    };
                    Value::Vec((0..n).map(|k| r.f32(d + 4 * k)).collect::<Result<_>>()?)
                }
                7 | 8 | 15 | 20 => Value::Str(r.cstr(d)?),
                17 => Value::U32(r.u32(d)?),
                t => Value::Other(t),
            };
            Ok((key, v))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(Object { hash, params })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A synthetic file: root → list "Fog" → object with Start / Color / enable / name.
    fn sample() -> Vec<u8> {
        let mut d = vec![0u8; 0x30];
        d[..4].copy_from_slice(b"AAMP");
        d[4..8].copy_from_slice(&2u32.to_le_bytes());
        let ty = b"agle\0\0\0\0";
        d[0x14..0x18].copy_from_slice(&(ty.len() as u32).to_le_bytes());
        d.extend_from_slice(ty);
        let root = d.len(); // 0x38
        // root list: 1 list at +12, 0 objects
        let mut l = Vec::new();
        l.extend_from_slice(&hash("param_root").to_le_bytes());
        l.extend_from_slice(&3u16.to_le_bytes()); // lists at +12 bytes
        l.extend_from_slice(&1u16.to_le_bytes());
        l.extend_from_slice(&0u16.to_le_bytes());
        l.extend_from_slice(&0u16.to_le_bytes());
        // "Fog" list at root+12: 0 lists, 1 object at +12
        l.extend_from_slice(&hash("Fog").to_le_bytes());
        l.extend_from_slice(&0u16.to_le_bytes());
        l.extend_from_slice(&0u16.to_le_bytes());
        l.extend_from_slice(&3u16.to_le_bytes());
        l.extend_from_slice(&1u16.to_le_bytes());
        // object at root+24: 4 params at +8
        l.extend_from_slice(&0x1234u32.to_le_bytes());
        l.extend_from_slice(&2u16.to_le_bytes());
        l.extend_from_slice(&4u16.to_le_bytes());
        // params at root+32 (4 × 8 bytes), data at root+64
        let data_at = 32 + 4 * 8;
        let mut data = Vec::new();
        let mut params = Vec::new();
        let put = |name: &str, ty: u8, bytes: &[u8], params: &mut Vec<u8>, data: &mut Vec<u8>| {
            let p = 32 + params.len();
            let off = (data_at + data.len() - p) / 4;
            params.extend_from_slice(&hash(name).to_le_bytes());
            params.extend_from_slice(&((ty as u32) << 24 | off as u32).to_le_bytes());
            data.extend_from_slice(bytes);
            while !data.len().is_multiple_of(4) {
                data.push(0);
            }
        };
        put("Start", 1, &1750f32.to_le_bytes(), &mut params, &mut data);
        let c: Vec<u8> = [0.0f32, 0.125, 1.0, 0.4]
            .iter()
            .flat_map(|f| f.to_le_bytes())
            .collect();
        put("Color", 6, &c, &mut params, &mut data);
        put("enable", 0, &1u32.to_le_bytes(), &mut params, &mut data);
        put("name", 7, b"FogZ\0", &mut params, &mut data);
        l.extend_from_slice(&params);
        l.extend_from_slice(&data);
        assert_eq!(root, 0x38);
        d.extend_from_slice(&l);
        d
    }

    #[test]
    fn reads_lists_objects_and_values() {
        let (kind, root) = parse(&sample()).unwrap();
        assert_eq!(kind, "agle");
        let fog = root.list("Fog").unwrap();
        let o = &fog.objects[0];
        assert_eq!(o.f32("Start"), Some(1750.0));
        assert_eq!(o.vec("Color"), Some(&[0.0, 0.125, 1.0, 0.4][..]));
        assert_eq!(o.bool("enable"), Some(true));
        assert_eq!(o.str("name"), Some("FogZ"));
        assert!(parse(b"nope").is_err());
    }
}
