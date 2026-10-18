//! BYML (binary YAML) reader, versions 1–4, either endianness → `serde_json::Value`. Version 1
//! (Wii U) has the same layout; a path table (node 0xC3) after the string table is skipped.
//!
//! Dictionaries keep file order. 64-bit integers that do not fit an f64 exactly are still exact
//! in JSON (serde_json stores u64/i64). Binary blobs become `{"binary": "<hex>"}`.

use serde_json::{Map, Number, Value};

use crate::error::{Error, Result};

struct Reader<'a> {
    d: &'a [u8],
    be: bool,
}

fn bad(detail: impl Into<String>) -> Error {
    Error::Format {
        what: "BYML",
        detail: detail.into(),
    }
}

impl<'a> Reader<'a> {
    fn bytes<const N: usize>(&self, o: usize) -> Result<[u8; N]> {
        self.d
            .get(o..o + N)
            .map(|b| b.try_into().unwrap())
            .ok_or_else(|| bad(format!("read past end at {o:#x}")))
    }
    fn u16(&self, o: usize) -> Result<u16> {
        let b = self.bytes::<2>(o)?;
        Ok(if self.be {
            u16::from_be_bytes(b)
        } else {
            u16::from_le_bytes(b)
        })
    }
    fn u24(&self, o: usize) -> Result<u32> {
        let b = self.bytes::<3>(o)?;
        Ok(if self.be {
            (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32
        } else {
            (b[2] as u32) << 16 | (b[1] as u32) << 8 | b[0] as u32
        })
    }
    fn u32(&self, o: usize) -> Result<u32> {
        let b = self.bytes::<4>(o)?;
        Ok(if self.be {
            u32::from_be_bytes(b)
        } else {
            u32::from_le_bytes(b)
        })
    }
    fn u64(&self, o: usize) -> Result<u64> {
        let b = self.bytes::<8>(o)?;
        Ok(if self.be {
            u64::from_be_bytes(b)
        } else {
            u64::from_le_bytes(b)
        })
    }

    fn string_table(&self, off: u32) -> Result<Vec<String>> {
        if off == 0 {
            return Ok(Vec::new());
        }
        let off = off as usize;
        if self.d.get(off) != Some(&0xC2) {
            return Err(bad("expected string table"));
        }
        let n = self.u24(off + 1)? as usize;
        (0..n)
            .map(|i| {
                let start = off + self.u32(off + 4 + i * 4)? as usize;
                let raw = self
                    .d
                    .get(start..)
                    .ok_or_else(|| bad("string outside file"))?;
                let len = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
                Ok(String::from_utf8_lossy(&raw[..len]).into_owned())
            })
            .collect()
    }
}

struct Ctx<'a> {
    r: Reader<'a>,
    keys: Vec<String>,
    strings: Vec<String>,
}

impl Ctx<'_> {
    fn value(&self, ty: u8, raw: u32, depth: usize) -> Result<Value> {
        if depth > 64 {
            return Err(bad("nesting too deep"));
        }
        Ok(match ty {
            0xA0 => Value::String(
                self.strings
                    .get(raw as usize)
                    .cloned()
                    .ok_or_else(|| bad("bad string index"))?,
            ),
            0xA1 => {
                let o = raw as usize;
                let size = self.r.u32(o)? as usize;
                let blob = self
                    .r
                    .d
                    .get(o + 4..o + 4 + size)
                    .ok_or_else(|| bad("binary outside file"))?;
                let hex: String = blob.iter().map(|b| format!("{b:02x}")).collect();
                serde_json::json!({ "binary": hex })
            }
            0xC0 | 0xC1 => self.container(raw as usize, depth + 1)?,
            0xD0 => Value::Bool(raw != 0),
            0xD1 => Value::from(raw as i32),
            0xD2 => Number::from_f64(f32::from_bits(raw) as f64)
                .map(Value::Number)
                .unwrap_or(Value::Null),
            0xD3 => Value::from(raw),
            0xD4 => Value::from(self.r.u64(raw as usize)? as i64),
            0xD5 => Value::from(self.r.u64(raw as usize)?),
            0xD6 => Number::from_f64(f64::from_bits(self.r.u64(raw as usize)?))
                .map(Value::Number)
                .unwrap_or(Value::Null),
            0xFF => Value::Null,
            t => return Err(bad(format!("unknown node type {t:#x}"))),
        })
    }

    fn container(&self, off: usize, depth: usize) -> Result<Value> {
        let ty = *self.r.d.get(off).ok_or_else(|| bad("node outside file"))?;
        let n = self.r.u24(off + 1)? as usize;
        match ty {
            0xC0 => {
                let types = self
                    .r
                    .d
                    .get(off + 4..off + 4 + n)
                    .ok_or_else(|| bad("array types outside file"))?;
                let values = off + 4 + n.div_ceil(4) * 4;
                let mut out = Vec::with_capacity(n);
                for (i, &t) in types.iter().enumerate() {
                    out.push(self.value(t, self.r.u32(values + i * 4)?, depth)?);
                }
                Ok(Value::Array(out))
            }
            0xC1 => {
                let mut m = Map::with_capacity(n);
                for i in 0..n {
                    let e = off + 4 + i * 8;
                    let key = self
                        .keys
                        .get(self.r.u24(e)? as usize)
                        .cloned()
                        .ok_or_else(|| bad("bad key index"))?;
                    let t = *self.r.d.get(e + 3).ok_or_else(|| bad("truncated dict"))?;
                    m.insert(key, self.value(t, self.r.u32(e + 4)?, depth)?);
                }
                Ok(Value::Object(m))
            }
            t => Err(bad(format!("expected array/dict, found {t:#x}"))),
        }
    }
}

pub fn parse(data: &[u8]) -> Result<Value> {
    let be = match data.get(..2) {
        Some(b"BY") => true,
        Some(b"YB") => false,
        _ => return Err(bad("missing magic")),
    };
    let r = Reader { d: data, be };
    let version = r.u16(2)?;
    if !(1..=4).contains(&version) {
        return Err(bad(format!("unsupported version {version}")));
    }
    let keys = r.string_table(r.u32(4)?)?;
    let strings = r.string_table(r.u32(8)?)?;
    let mut root = r.u32(12)?;
    if version == 1 && root != 0 && data.get(root as usize) == Some(&0xC3) {
        root = r.u32(16)?; // path table, then the root
    }
    let ctx = Ctx { r, keys, strings };
    if root == 0 {
        return Ok(Value::Null);
    }
    ctx.container(root as usize, 0)
}
