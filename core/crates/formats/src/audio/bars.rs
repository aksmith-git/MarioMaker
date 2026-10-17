//! BARS: an archive of sounds, each an AMTA (metadata, holds the name) plus a BWAV.
//!
//! Layout of version 1.1 (little-endian), from inspection of the files themselves:
//!
//! ```text
//! 0x00  "BARS"  u32 file size  u16 BOM (FF FE)  u16 version (0x0101)
//! 0x0C  u32 entry count n
//! 0x10  u32 CRC32(name) × n, ascending
//!       (u32 AMTA offset, u32 BWAV offset) × n, in the same order
//! AMTA: "AMTA" u16 BOM u16 version u32 size, then u32 offsets (relative to the AMTA) of the
//!       DATA, MARK, EXT_ and STRG sections. STRG = "STRG" u32 length, then the name.
//! ```
//!
//! A BWAV offset may point at a prefetch BWAV whose remaining samples live in a separate stream
//! file (`Sound/Resource/Stream/<name>.bwav`).

use crate::audio::bwav::Bwav;
use crate::error::{Error, Result};

fn bad(detail: impl Into<String>) -> Error {
    Error::Format {
        what: "BARS",
        detail: detail.into(),
    }
}

fn u32_at(d: &[u8], o: usize) -> Result<u32> {
    d.get(o..o + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or_else(|| bad(format!("truncated at {o:#x}")))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub name_crc: u32,
    pub amta_offset: u32,
    pub bwav_offset: u32,
}

#[derive(Clone, Debug)]
pub struct Bars<'a> {
    pub version: u16,
    pub entries: Vec<Entry>,
    data: &'a [u8],
}

impl<'a> Bars<'a> {
    pub fn parse(data: &'a [u8]) -> Result<Self> {
        if data.len() < 0x10 || &data[0..4] != b"BARS" {
            return Err(Error::Magic { what: "BARS" });
        }
        if u16::from_le_bytes([data[8], data[9]]) != 0xFEFF {
            return Err(bad("big-endian BARS not supported"));
        }
        let version = u16::from_le_bytes([data[10], data[11]]);
        if version != 0x0101 {
            return Err(bad(format!("unsupported version {version:#06x}")));
        }
        let n = u32_at(data, 0x0C)? as usize;
        let pairs = 0x10 + 4 * n;
        if data.len() < pairs + 8 * n {
            return Err(bad("truncated entry table"));
        }
        let mut entries = Vec::with_capacity(n);
        for i in 0..n {
            let name_crc = u32_at(data, 0x10 + 4 * i)?;
            let amta = u32_at(data, pairs + 8 * i)? as usize;
            let bwav = u32_at(data, pairs + 8 * i + 4)?;
            if data.get(amta..amta + 4) != Some(b"AMTA") {
                return Err(bad(format!("entry {i}: no AMTA at {amta:#x}")));
            }
            let strg = amta + u32_at(data, amta + 0x18)? as usize;
            if data.get(strg..strg + 4) != Some(b"STRG") {
                return Err(bad(format!("entry {i}: no STRG")));
            }
            let len = u32_at(data, strg + 4)? as usize;
            let raw = data
                .get(strg + 8..strg + 8 + len)
                .ok_or_else(|| bad(format!("entry {i}: name out of range")))?;
            let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
            let name = String::from_utf8_lossy(&raw[..end]).into_owned();
            entries.push(Entry {
                name,
                name_crc,
                amta_offset: amta as u32,
                bwav_offset: bwav,
            });
        }
        Ok(Self {
            version,
            entries,
            data,
        })
    }

    /// Look an entry up by name (via the CRC32 table, then the stored name).
    pub fn find(&self, name: &str) -> Option<&Entry> {
        let crc = crc32fast::hash(name.as_bytes());
        self.entries
            .iter()
            .find(|e| e.name_crc == crc && e.name == name)
    }

    /// The entry's BWAV (None if the offset does not point at one).
    pub fn bwav(&self, entry: &Entry) -> Option<Bwav<'a>> {
        let d = self.data.get(entry.bwav_offset as usize..)?;
        Bwav::parse(d).ok()
    }
}

/// Write a BARS from (name, BWAV bytes) pairs, with a minimal AMTA per entry. For synthetic test
/// fixtures.
pub fn build(sounds: &[(&str, &[u8])]) -> Vec<u8> {
    let mut sorted: Vec<(u32, &str, &[u8])> = sounds
        .iter()
        .map(|&(n, b)| (crc32fast::hash(n.as_bytes()), n, b))
        .collect();
    sorted.sort_by_key(|s| s.0);
    let n = sorted.len();
    let mut out = vec![0u8; 0x10 + 12 * n];
    out[0..4].copy_from_slice(b"BARS");
    out[8..10].copy_from_slice(&0xFEFFu16.to_le_bytes());
    out[10..12].copy_from_slice(&0x0101u16.to_le_bytes());
    out[12..16].copy_from_slice(&(n as u32).to_le_bytes());
    let mut amtas = Vec::new();
    for (i, (crc, name, _)) in sorted.iter().enumerate() {
        out[0x10 + 4 * i..0x14 + 4 * i].copy_from_slice(&crc.to_le_bytes());
        // AMTA: header (0x1C) + empty DATA/MARK/EXT_ + STRG
        let at = out.len();
        let mut a = Vec::new();
        a.extend_from_slice(b"AMTA");
        a.extend_from_slice(&0xFEFFu16.to_le_bytes());
        a.extend_from_slice(&0x0400u16.to_le_bytes());
        a.extend_from_slice(&0u32.to_le_bytes()); // size, patched below
        for off in [0x1Cu32, 0x24, 0x2C, 0x34] {
            a.extend_from_slice(&off.to_le_bytes());
        }
        for tag in [b"DATA", b"MARK", b"EXT_"] {
            a.extend_from_slice(tag);
            a.extend_from_slice(&0u32.to_le_bytes());
        }
        a.extend_from_slice(b"STRG");
        a.extend_from_slice(&(name.len() as u32 + 1).to_le_bytes());
        a.extend_from_slice(name.as_bytes());
        a.push(0);
        while !a.len().is_multiple_of(4) {
            a.push(0);
        }
        let size = a.len() as u32;
        a[8..12].copy_from_slice(&size.to_le_bytes());
        out.extend_from_slice(&a);
        amtas.push(at);
    }
    let mut bwavs = Vec::new();
    for (_, _, b) in &sorted {
        while !out.len().is_multiple_of(0x40) {
            out.push(0);
        }
        bwavs.push(out.len());
        out.extend_from_slice(b);
    }
    let pairs = 0x10 + 4 * n;
    for i in 0..n {
        out[pairs + 8 * i..pairs + 8 * i + 4].copy_from_slice(&(amtas[i] as u32).to_le_bytes());
        out[pairs + 8 * i + 4..pairs + 8 * i + 8].copy_from_slice(&(bwavs[i] as u32).to_le_bytes());
    }
    let size = out.len() as u32;
    out[4..8].copy_from_slice(&size.to_le_bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::bwav::{self, BuildChannel, Codec};

    fn tone(rate: u32, samples: &[i16]) -> Vec<u8> {
        let data: Vec<u8> = samples.iter().flat_map(|v| v.to_le_bytes()).collect();
        bwav::build(&[BuildChannel {
            codec: Codec::Pcm16,
            pan: 2,
            sample_rate: rate,
            samples: samples.len() as u32,
            coefs: [0; 16],
            loop_range: None,
            data: &data,
        }])
    }

    #[test]
    fn find_and_decode_by_name() {
        let a = tone(32000, &[1, 2, 3]);
        let b = tone(44100, &[-5, -6]);
        let file = build(&[("TestSound_A", &a), ("TestSound_B", &b)]);
        let bars = Bars::parse(&file).unwrap();
        assert_eq!(bars.entries.len(), 2);
        let e = bars.find("TestSound_B").unwrap();
        let w = bars.bwav(e).unwrap();
        assert_eq!(w.sample_rate(), 44100);
        assert_eq!(w.decode_channel(0).unwrap(), vec![-5, -6]);
        assert_eq!(w.bytes().unwrap().len(), b.len());
        assert!(bars.find("TestSound_C").is_none());
        // names are in CRC order and the CRC is of the name
        for e in &bars.entries {
            assert_eq!(e.name_crc, crc32fast::hash(e.name.as_bytes()));
        }
        assert!(bars.entries[0].name_crc < bars.entries[1].name_crc);
    }

    #[test]
    fn rejects_bad_input() {
        assert!(Bars::parse(b"BARX\0\0\0\0\xff\xfe\x01\x01\0\0\0\0").is_err());
        let mut file = build(&[("X", &tone(1, &[0]))]);
        file[10] = 2; // version
        assert!(Bars::parse(&file).is_err());
        let file = build(&[("X", &tone(1, &[0]))]);
        assert!(Bars::parse(&file[..0x14]).is_err());
    }
}
