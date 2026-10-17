//! SARC archives (read-only).

use crate::error::{Error, Result};

#[derive(Clone, Debug)]
pub struct SarcEntry {
    pub name: String,
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Debug)]
pub struct Sarc {
    data: Vec<u8>,
    pub entries: Vec<SarcEntry>,
}

impl Sarc {
    pub fn parse(data: Vec<u8>) -> Result<Self> {
        let bad = |d: &str| Error::Format {
            what: "SARC",
            detail: d.to_string(),
        };
        if data.len() < 0x20 || &data[..4] != b"SARC" {
            return Err(bad("missing magic"));
        }
        let be = data[6..8] == [0xFE, 0xFF];
        let u16_at = |o: usize| -> Result<u16> {
            let b: [u8; 2] = data
                .get(o..o + 2)
                .ok_or_else(|| bad("truncated"))?
                .try_into()
                .unwrap();
            Ok(if be {
                u16::from_be_bytes(b)
            } else {
                u16::from_le_bytes(b)
            })
        };
        let u32_at = |o: usize| -> Result<u32> {
            let b: [u8; 4] = data
                .get(o..o + 4)
                .ok_or_else(|| bad("truncated"))?
                .try_into()
                .unwrap();
            Ok(if be {
                u32::from_be_bytes(b)
            } else {
                u32::from_le_bytes(b)
            })
        };
        let header_size = u16_at(4)? as usize;
        let data_offset = u32_at(0xC)? as usize;
        let sfat = header_size;
        if data.get(sfat..sfat + 4) != Some(b"SFAT") {
            return Err(bad("missing SFAT"));
        }
        let sfat_header = u16_at(sfat + 4)? as usize;
        let count = u16_at(sfat + 6)? as usize;
        let nodes = sfat + sfat_header;
        let sfnt = nodes + count * 16;
        if data.get(sfnt..sfnt + 4) != Some(b"SFNT") {
            return Err(bad("missing SFNT"));
        }
        let names = sfnt + u16_at(sfnt + 4)? as usize;
        let mut entries = Vec::with_capacity(count);
        for i in 0..count {
            let n = nodes + i * 16;
            let attrs = u32_at(n + 4)?;
            let start = data_offset + u32_at(n + 8)? as usize;
            let end = data_offset + u32_at(n + 12)? as usize;
            if end > data.len() || start > end {
                return Err(bad("entry outside archive"));
            }
            let name = if attrs & 0x0100_0000 != 0 {
                let off = names + (attrs & 0xFF_FFFF) as usize * 4;
                let raw = data.get(off..).ok_or_else(|| bad("name outside archive"))?;
                let len = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
                String::from_utf8_lossy(&raw[..len]).into_owned()
            } else {
                format!("{:08x}", u32_at(n)?)
            };
            entries.push(SarcEntry { name, start, end });
        }
        Ok(Self { data, entries })
    }

    pub fn get(&self, name: &str) -> Option<&[u8]> {
        self.entries
            .iter()
            .find(|e| e.name == name)
            .map(|e| &self.data[e.start..e.end])
    }

    pub fn files(&self) -> impl Iterator<Item = (&str, &[u8])> {
        self.entries
            .iter()
            .map(|e| (e.name.as_str(), &self.data[e.start..e.end]))
    }
}
