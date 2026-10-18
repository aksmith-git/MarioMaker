//! Wii U RPX/RPL executables (ELF32, big-endian): the contents of their sections, inflated.
//!
//! An RPX is an ELF file whose sections may be zlib-compressed: a section with flag
//! `0x0800_0000` (RPL_ZLIB) stores a big-endian u32 with the inflated size, then a zlib stream.
//! The importer only reads data out of the owner's own executable (e.g. a name list) — nothing
//! from it is written into this repository.

use std::io::Read;

use crate::error::{Error, Result};

fn bad(detail: impl Into<String>) -> Error {
    Error::Format {
        what: "RPX",
        detail: detail.into(),
    }
}

const SHF_RPL_ZLIB: u32 = 0x0800_0000;
const SHT_NOBITS: u32 = 8;

fn u16_at(d: &[u8], o: usize) -> Result<u16> {
    d.get(o..o + 2)
        .map(|b| u16::from_be_bytes([b[0], b[1]]))
        .ok_or_else(|| bad(format!("truncated at {o:#x}")))
}

fn u32_at(d: &[u8], o: usize) -> Result<u32> {
    d.get(o..o + 4)
        .map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or_else(|| bad(format!("truncated at {o:#x}")))
}

/// The contents of every section that has bytes in the file (inflated when compressed), in
/// section order.
pub fn sections(file: &[u8]) -> Result<Vec<Vec<u8>>> {
    if file.get(..4) != Some(b"\x7fELF") || file.get(4) != Some(&1) || file.get(5) != Some(&2) {
        return Err(bad("not a big-endian ELF32 file"));
    }
    let shoff = u32_at(file, 0x20)? as usize;
    let shentsize = u16_at(file, 0x2E)? as usize;
    let shnum = u16_at(file, 0x30)? as usize;
    if shentsize < 0x28 {
        return Err(bad("section header too small"));
    }
    let mut out = Vec::new();
    for i in 0..shnum {
        let h = shoff + i * shentsize;
        let kind = u32_at(file, h + 4)?;
        let flags = u32_at(file, h + 8)?;
        let (off, size) = (
            u32_at(file, h + 0x10)? as usize,
            u32_at(file, h + 0x14)? as usize,
        );
        if kind == SHT_NOBITS || size == 0 || kind == 0 {
            continue;
        }
        let Some(raw) = file.get(off..off + size) else {
            continue;
        };
        if flags & SHF_RPL_ZLIB != 0 && raw.len() > 4 {
            let want = u32_at(raw, 0)? as usize;
            let mut v = Vec::with_capacity(want.min(64 << 20));
            flate2::read::ZlibDecoder::new(&raw[4..])
                .read_to_end(&mut v)
                .map_err(|e| bad(format!("section {i}: {e}")))?;
            out.push(v);
        } else {
            out.push(raw.to_vec());
        }
    }
    Ok(out)
}

/// NUL-terminated ASCII strings in `data` that start with `prefix`.
pub fn strings_with_prefix<'a>(data: &'a [u8], prefix: &[u8]) -> Vec<&'a str> {
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(p) = data[i..]
        .windows(prefix.len())
        .position(|w| w == prefix)
        .map(|p| p + i)
    {
        let end = data[p..]
            .iter()
            .position(|&b| b == 0)
            .map_or(data.len(), |e| p + e);
        if let Ok(s) = std::str::from_utf8(&data[p..end]) {
            out.push(s);
        }
        i = end.max(p + 1);
        if i >= data.len() {
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// A two-section ELF: one plain, one zlib-compressed.
    fn elf(plain: &[u8], packed: &[u8]) -> Vec<u8> {
        let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        z.write_all(packed).unwrap();
        let mut comp = (packed.len() as u32).to_be_bytes().to_vec();
        comp.extend(z.finish().unwrap());
        let mut f = vec![0u8; 0x34];
        f[..6].copy_from_slice(b"\x7fELF\x01\x02");
        let data0 = f.len();
        f.extend_from_slice(plain);
        let data1 = f.len();
        f.extend_from_slice(&comp);
        let shoff = f.len();
        f[0x20..0x24].copy_from_slice(&(shoff as u32).to_be_bytes());
        f[0x2E..0x30].copy_from_slice(&0x28u16.to_be_bytes());
        f[0x30..0x32].copy_from_slice(&3u16.to_be_bytes());
        f.extend_from_slice(&[0u8; 0x28]); // null section
        for (off, size, flags) in [
            (data0, plain.len(), 0u32),
            (data1, comp.len(), SHF_RPL_ZLIB),
        ] {
            let mut h = [0u8; 0x28];
            h[4..8].copy_from_slice(&1u32.to_be_bytes());
            h[8..12].copy_from_slice(&flags.to_be_bytes());
            h[0x10..0x14].copy_from_slice(&(off as u32).to_be_bytes());
            h[0x14..0x18].copy_from_slice(&(size as u32).to_be_bytes());
            f.extend_from_slice(&h);
        }
        f
    }

    #[test]
    fn reads_plain_and_compressed_sections() {
        let s = sections(&elf(b"abc", b"\0Invalid, A, B\0Invalid, C\0")).unwrap();
        assert_eq!(s.len(), 2);
        assert_eq!(s[0], b"abc");
        assert_eq!(
            strings_with_prefix(&s[1], b"Invalid, "),
            ["Invalid, A, B", "Invalid, C"]
        );
        assert!(sections(b"FRES").is_err());
    }
}
