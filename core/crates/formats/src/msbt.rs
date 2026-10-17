//! MSBT message tables (`MsgStdBn`): labels (LBL1) and texts (TXT2) with inline control tags.
//!
//! ```text
//! header  "MsgStdBn" bom u16 pad u8 encoding (0 UTF-8, 1 UTF-16) u8 version u16 sections
//!         u16 pad u32 file_size pad[10]                                       (0x20 bytes)
//! section magic[4] u32 size pad[8] data, next section 16-byte aligned
//! LBL1    u32 buckets; (u32 count, u32 offset)[buckets]; label: u8 len, chars, u32 text_index
//! TXT2    u32 count; u32 offsets[count] (from the data start); NUL-terminated strings
//! tags    0x0E group type param_size params… (open) · 0x0F group type (close, version 3+)
//! ```
//! MSBP project files are not needed by the engine (SMM1 ships none for English).

use serde_json::{Value, json};

use crate::error::{Error, Result};
use crate::lyt::Reader;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MsgPart {
    Text(String),
    /// A control tag: group, type, raw parameters (in the file's byte order).
    Tag {
        group: u16,
        kind: u16,
        params: Vec<u8>,
    },
    TagEnd {
        group: u16,
        kind: u16,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Message {
    pub label: String,
    pub parts: Vec<MsgPart>,
}

impl Message {
    /// Text with every control tag removed.
    pub fn plain(&self) -> String {
        self.parts
            .iter()
            .filter_map(|p| match p {
                MsgPart::Text(t) => Some(t.as_str()),
                _ => None,
            })
            .collect()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Msbt {
    pub big_endian: bool,
    pub messages: Vec<Message>,
}

fn bad(detail: impl Into<String>) -> Error {
    Error::Format {
        what: "MSBT",
        detail: detail.into(),
    }
}

impl Msbt {
    pub fn parse(data: &[u8]) -> Result<Self> {
        if data.len() < 0x20 || &data[..8] != b"MsgStdBn" {
            return Err(Error::Magic { what: "MSBT" });
        }
        let be = data[8..10] == [0xFE, 0xFF];
        let r = Reader::new(data, be, "MSBT");
        let encoding = r.u8(0x0C)?;
        if encoding > 1 {
            return Err(bad(format!("encoding {encoding} not supported")));
        }
        let sections = r.u16(0x0E)? as usize;
        let mut labels: Vec<(String, u32)> = Vec::new();
        let mut texts: Vec<Vec<MsgPart>> = Vec::new();
        let mut o = 0x20;
        for _ in 0..sections {
            let magic = r.bytes(o, 4)?;
            let size = r.u32(o + 4)? as usize;
            let d = o + 0x10;
            r.bytes(d, size)?;
            match magic {
                b"LBL1" => {
                    let buckets = r.u32(d)? as usize;
                    for b in 0..buckets {
                        let count = r.u32(d + 4 + b * 8)? as usize;
                        let mut p = d + r.u32(d + 8 + b * 8)? as usize;
                        for _ in 0..count {
                            let len = r.u8(p)? as usize;
                            let name = String::from_utf8_lossy(r.bytes(p + 1, len)?).into_owned();
                            let index = r.u32(p + 1 + len)?;
                            labels.push((name, index));
                            p += 1 + len + 4;
                        }
                    }
                }
                b"TXT2" => {
                    let count = r.u32(d)? as usize;
                    for i in 0..count {
                        let start = d + r.u32(d + 4 + i * 4)? as usize;
                        let end = if i + 1 < count {
                            d + r.u32(d + 8 + i * 4)? as usize
                        } else {
                            d + size
                        };
                        texts.push(parse_text(&r, start, end, encoding)?);
                    }
                }
                _ => {}
            }
            o = (d + size).div_ceil(16) * 16;
        }
        labels.sort_by_key(|(_, i)| *i);
        let messages = labels
            .into_iter()
            .map(|(label, i)| Message {
                label,
                parts: texts.get(i as usize).cloned().unwrap_or_default(),
            })
            .collect();
        Ok(Self {
            big_endian: be,
            messages,
        })
    }

    pub fn get(&self, label: &str) -> Option<&Message> {
        self.messages.iter().find(|m| m.label == label)
    }

    /// `{ label: [ "text", {"tag": [group, type], "params": "hex"}, {"end": [group, type]} ] }`
    pub fn to_json(&self) -> Value {
        let mut m = serde_json::Map::new();
        for msg in &self.messages {
            let parts: Vec<Value> = msg
                .parts
                .iter()
                .map(|p| match p {
                    MsgPart::Text(t) => json!(t),
                    MsgPart::Tag {
                        group,
                        kind,
                        params,
                    } => json!({
                        "tag": [group, kind],
                        "params": params.iter().map(|b| format!("{b:02x}")).collect::<String>(),
                    }),
                    MsgPart::TagEnd { group, kind } => json!({ "end": [group, kind] }),
                })
                .collect();
            m.insert(msg.label.clone(), Value::Array(parts));
        }
        Value::Object(m)
    }

    pub fn from_json(v: &Value) -> Result<Self> {
        let obj = v.as_object().ok_or_else(|| bad("expected an object"))?;
        let mut messages = Vec::with_capacity(obj.len());
        for (label, parts) in obj {
            let parts = parts
                .as_array()
                .ok_or_else(|| bad("expected an array"))?
                .iter()
                .map(|p| {
                    if let Some(t) = p.as_str() {
                        return Ok(MsgPart::Text(t.to_string()));
                    }
                    let pair = |k: &str| -> Option<(u16, u16)> {
                        let a = p.get(k)?.as_array()?;
                        Some((a.first()?.as_u64()? as u16, a.get(1)?.as_u64()? as u16))
                    };
                    if let Some((group, kind)) = pair("tag") {
                        let hex = p.get("params").and_then(Value::as_str).unwrap_or("");
                        let params = (0..hex.len() / 2)
                            .map(|i| u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16))
                            .collect::<std::result::Result<_, _>>()
                            .map_err(|_| bad("bad params"))?;
                        Ok(MsgPart::Tag {
                            group,
                            kind,
                            params,
                        })
                    } else if let Some((group, kind)) = pair("end") {
                        Ok(MsgPart::TagEnd { group, kind })
                    } else {
                        Err(bad("unknown message part"))
                    }
                })
                .collect::<Result<_>>()?;
            messages.push(Message {
                label: label.clone(),
                parts,
            });
        }
        Ok(Self {
            big_endian: true,
            messages,
        })
    }
}

fn parse_text(r: &Reader, start: usize, end: usize, encoding: u8) -> Result<Vec<MsgPart>> {
    let mut parts = Vec::new();
    let mut text = String::new();
    if encoding == 0 {
        let raw = r.bytes(start, end.saturating_sub(start))?;
        let len = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
        // tags in UTF-8 files are rare in this game; keep the text as-is
        parts.push(MsgPart::Text(
            String::from_utf8_lossy(&raw[..len]).into_owned(),
        ));
        return Ok(parts);
    }
    let mut units: Vec<u16> = Vec::new();
    let flush = |units: &mut Vec<u16>, text: &mut String, parts: &mut Vec<MsgPart>| {
        text.push_str(&String::from_utf16_lossy(units));
        units.clear();
        if !text.is_empty() {
            parts.push(MsgPart::Text(std::mem::take(text)));
        }
    };
    let mut p = start;
    while p + 2 <= end {
        let u = r.u16(p)?;
        p += 2;
        match u {
            0 => break,
            0x0E => {
                flush(&mut units, &mut text, &mut parts);
                let group = r.u16(p)?;
                let kind = r.u16(p + 2)?;
                let n = r.u16(p + 4)? as usize;
                let params = r.bytes(p + 6, n)?.to_vec();
                p += 6 + n;
                parts.push(MsgPart::Tag {
                    group,
                    kind,
                    params,
                });
            }
            0x0F => {
                flush(&mut units, &mut text, &mut parts);
                let group = r.u16(p)?;
                let kind = r.u16(p + 2)?;
                p += 4;
                parts.push(MsgPart::TagEnd { group, kind });
            }
            _ => units.push(u),
        }
    }
    flush(&mut units, &mut text, &mut parts);
    Ok(parts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::sample_msbt;

    #[test]
    fn parses_labels_texts_and_tags() {
        let m = Msbt::parse(&sample_msbt()).unwrap();
        assert_eq!(m.messages.len(), 2);
        assert_eq!(m.get("T_Title_00").unwrap().plain(), "Hello\nWorld");
        let lives = m.get("T_Lives_00").unwrap();
        assert_eq!(lives.plain(), "x!");
        assert_eq!(
            lives.parts[1],
            MsgPart::Tag {
                group: 2,
                kind: 2,
                params: vec![0, 2]
            }
        );
        assert_eq!(lives.parts[2], MsgPart::TagEnd { group: 2, kind: 2 });
        let back = Msbt::from_json(&m.to_json()).unwrap();
        assert_eq!(back.get("T_Lives_00"), m.get("T_Lives_00"));
    }

    #[test]
    fn rejects_garbage() {
        assert!(Msbt::parse(b"MsgStdBnxx").is_err());
    }
}
