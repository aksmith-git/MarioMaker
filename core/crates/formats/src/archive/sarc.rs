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

/// Write a little-endian SARC (named entries, sorted by name hash, data 0x40-aligned). For
/// synthetic test fixtures.
pub fn build(files: &[(&str, &[u8])]) -> Vec<u8> {
    let hash = |name: &str| {
        name.bytes()
            .fold(0u32, |h, c| h.wrapping_mul(0x65).wrapping_add(c as u32))
    };
    let mut files: Vec<(u32, &str, &[u8])> = files.iter().map(|&(n, d)| (hash(n), n, d)).collect();
    files.sort_by_key(|f| f.0);
    let mut names = Vec::new();
    let mut name_offsets = Vec::new();
    for (_, n, _) in &files {
        name_offsets.push(names.len() / 4);
        names.extend_from_slice(n.as_bytes());
        names.push(0);
        while !names.len().is_multiple_of(4) {
            names.push(0);
        }
    }
    let header = 0x14;
    let sfat = header;
    let sfnt = sfat + 0x0C + 16 * files.len();
    let data_offset = (sfnt + 8 + names.len()).next_multiple_of(0x40);
    let mut body = Vec::new();
    let mut ranges = Vec::new();
    for (_, _, d) in &files {
        while !body.len().is_multiple_of(0x40) {
            body.push(0);
        }
        ranges.push((body.len(), body.len() + d.len()));
        body.extend_from_slice(d);
    }
    let mut out = Vec::with_capacity(data_offset + body.len());
    out.extend_from_slice(b"SARC");
    out.extend_from_slice(&(header as u16).to_le_bytes());
    out.extend_from_slice(&0xFEFFu16.to_le_bytes());
    out.extend_from_slice(&((data_offset + body.len()) as u32).to_le_bytes());
    out.extend_from_slice(&(data_offset as u32).to_le_bytes());
    out.extend_from_slice(&0x0100u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(b"SFAT");
    out.extend_from_slice(&0x0Cu16.to_le_bytes());
    out.extend_from_slice(&(files.len() as u16).to_le_bytes());
    out.extend_from_slice(&0x65u32.to_le_bytes());
    for (i, (h, _, _)) in files.iter().enumerate() {
        out.extend_from_slice(&h.to_le_bytes());
        out.extend_from_slice(&(0x0100_0000 | name_offsets[i] as u32).to_le_bytes());
        out.extend_from_slice(&(ranges[i].0 as u32).to_le_bytes());
        out.extend_from_slice(&(ranges[i].1 as u32).to_le_bytes());
    }
    out.extend_from_slice(b"SFNT");
    out.extend_from_slice(&8u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&names);
    out.resize(data_offset, 0);
    out.extend_from_slice(&body);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_round_trip() {
        let a = b"first file".to_vec();
        let b = vec![7u8; 100];
        let s = Sarc::parse(build(&[("dir/a.bin", &a), ("b.bin", &b)])).unwrap();
        assert_eq!(s.entries.len(), 2);
        assert_eq!(s.get("dir/a.bin"), Some(&a[..]));
        assert_eq!(s.get("b.bin"), Some(&b[..]));
        assert!(s.entries.iter().all(|e| e.start % 0x40 == 0));
    }
}
