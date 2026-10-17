//! Switch scalable-font support: BFCPX font combinations and the BFTTF/BFOTF wrapper.
//!
//! SMM2 draws its text with TrueType/OpenType fonts. A layout names a combination
//! (`MessageFont.fcpx`); the combination lists the font files to take characters from, each
//! with a scale and a baseline shift. Worked out from the SMM2 files (no public description was
//! at hand), so the field names are inferences:
//!
//! ```text
//! header  "FCPX" bom u16 hdr_size(0x14) u32 version(9.3) u32 file_size
//! 0x10    u32 1, u32 4, u32 1
//! 0x1C    f32 font size (pixels per em at which the layouts were authored)
//! 0x20    u32 (6, 20, 0 …: unknown)
//! 0x24    u32 entry count
//! 0x28    u32 0x20
//! 0x2C    u32 alternate character (U+E06B)
//! 0x30    entries[count], 0x28 bytes each:
//!           +0x00 u32 range mode (0 every character, 1 only the listed ranges,
//!                 2 every character except the listed ranges)
//!           +0x04 u32 range list offset  (offsets are from 0x18)
//!           +0x08 f32 (unknown, 0 or 0.04)
//!           +0x0C f32 glyph scale
//!           +0x14 u32 file name offset
//!           +0x18 u8 border width (matches the FontInfo "FF_<font>_B<n>" faces), u8,
//!                 i16 baseline shift (pixels at the font size, positive = up) [inf]
//!           +0x1C f32 glyph scale (again)
//! range lists: (u32 first, u32 last) pairs; a list runs to the next list or the name table
//! ```
//! Where several entries can give a character, later entries win [inf: makes every entry of
//! SMM2's combinations reachable — the stylised MARIOMAKER kana and digits over SouGei].
//!
//! BFTTF / BFOTF: `u32 magic ^ key, u32 size ^ key`, then the TrueType/OpenType file with every
//! big-endian 32-bit word XORed with the key 0x49621806 (the same scheme as the Switch shared
//! fonts).

use crate::error::{Error, Result};
use crate::lyt::{Reader, file_header};

pub const MAGIC: &[u8; 4] = b"FCPX";

fn bad(detail: impl Into<String>) -> Error {
    Error::Format {
        what: "BFCPX",
        detail: detail.into(),
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RangeMode {
    #[default]
    All,
    Only,
    Except,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SubFont {
    /// Font file, e.g. `MARIOMAKER.bfotf`.
    pub file: String,
    pub mode: RangeMode,
    pub ranges: Vec<(u32, u32)>,
    pub scale: f32,
    pub border: u8,
    pub baseline_shift: i16,
}

impl SubFont {
    /// Whether this entry may give character `c` (before checking the font has a glyph).
    pub fn covers(&self, c: u32) -> bool {
        let listed = self.ranges.iter().any(|&(a, b)| (a..=b).contains(&c));
        match self.mode {
            RangeMode::All => true,
            RangeMode::Only => listed,
            RangeMode::Except => !listed,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Combination {
    pub version: u32,
    /// Authored font size in pixels per em.
    pub size: f32,
    pub alternate_char: u32,
    /// In file order; later entries take priority (see the module notes).
    pub fonts: Vec<SubFont>,
}

impl Combination {
    pub fn parse(data: &[u8]) -> Result<Self> {
        let h = file_header(data, MAGIC, "BFCPX")?;
        let r = Reader::new(data, h.be, "BFCPX");
        let count = r.u32(0x24)? as usize;
        if count > 64 {
            return Err(bad(format!("{count} entries")));
        }
        let base = 0x18;
        struct Raw {
            mode: u32,
            list: usize,
            name: usize,
            scale: f32,
            border: u8,
            shift: i16,
        }
        let mut raws = Vec::with_capacity(count);
        for i in 0..count {
            let e = 0x30 + i * 0x28;
            raws.push(Raw {
                mode: r.u32(e)?,
                list: r.u32(e + 4)? as usize,
                name: base + r.u32(e + 0x14)? as usize,
                scale: r.f32(e + 0x0C)?,
                border: r.u8(e + 0x18)?,
                shift: r.i16(e + 0x1A)?,
            });
        }
        let names_start = raws.iter().map(|x| x.name).min().unwrap_or(data.len());
        let mut lists: Vec<usize> = raws
            .iter()
            .filter(|x| x.list != 0)
            .map(|x| base + x.list)
            .collect();
        lists.sort_unstable();
        lists.dedup();
        let ranges_at = |start: usize| -> Result<Vec<(u32, u32)>> {
            let end = lists
                .iter()
                .copied()
                .find(|&o| o > start)
                .unwrap_or(names_start)
                .min(names_start);
            let mut v = Vec::new();
            let mut o = start;
            while o + 8 <= end {
                v.push((r.u32(o)?, r.u32(o + 4)?));
                o += 8;
            }
            Ok(v)
        };
        let mut fonts = Vec::with_capacity(count);
        for x in raws {
            let mode = match x.mode {
                0 => RangeMode::All,
                1 => RangeMode::Only,
                2 => RangeMode::Except,
                m => return Err(bad(format!("range mode {m}"))),
            };
            let ranges = if mode == RangeMode::All || x.list == 0 {
                Vec::new()
            } else {
                ranges_at(base + x.list)?
            };
            fonts.push(SubFont {
                file: r.cstr(x.name)?,
                mode,
                ranges,
                scale: x.scale,
                border: x.border,
                baseline_shift: x.shift,
            });
        }
        Ok(Self {
            version: h.version,
            size: r.f32(0x1C)?,
            alternate_char: r.u32(0x2C)?,
            fonts,
        })
    }

    /// Indices into `fonts` that may give `c`, highest priority first.
    pub fn candidates(&self, c: u32) -> impl Iterator<Item = usize> + '_ {
        (0..self.fonts.len())
            .rev()
            .filter(move |&i| self.fonts[i].covers(c))
    }
}

/// XOR key of the BFTTF/BFOTF wrapper.
pub const SCALABLE_KEY: u32 = 0x4962_1806;
/// First word of a BFTTF/BFOTF file once unmasked.
pub const SCALABLE_MAGIC: u32 = 0x7F9A_0218;

/// A BFTTF / BFOTF file → the plain TrueType / OpenType font inside.
pub fn decrypt_scalable(data: &[u8]) -> Result<Vec<u8>> {
    let word = |o: usize| -> Option<u32> {
        Some(u32::from_be_bytes(data.get(o..o + 4)?.try_into().ok()?) ^ SCALABLE_KEY)
    };
    let err = |d: &str| Error::Format {
        what: "BFTTF",
        detail: d.into(),
    };
    if word(0) != Some(SCALABLE_MAGIC) {
        return Err(err("not a BFTTF/BFOTF file"));
    }
    let size = word(4).ok_or_else(|| err("truncated"))? as usize;
    let body = data
        .get(8..8 + size)
        .ok_or_else(|| err("size past the end of the file"))?;
    let mut out = Vec::with_capacity(size);
    for chunk in body.chunks(4) {
        let mut w = [0u8; 4];
        w[..chunk.len()].copy_from_slice(chunk);
        let v = u32::from_be_bytes(w) ^ SCALABLE_KEY;
        out.extend_from_slice(&v.to_be_bytes()[..chunk.len()]);
    }
    Ok(out)
}

/// Wrap a plain font in the BFTTF scheme (tests and tools).
pub fn encrypt_scalable(font: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(font.len() + 8);
    out.extend_from_slice(&(SCALABLE_MAGIC ^ SCALABLE_KEY).to_be_bytes());
    out.extend_from_slice(&(font.len() as u32 ^ SCALABLE_KEY).to_be_bytes());
    for chunk in font.chunks(4) {
        let mut w = [0u8; 4];
        w[..chunk.len()].copy_from_slice(chunk);
        let v = u32::from_be_bytes(w) ^ SCALABLE_KEY;
        out.extend_from_slice(&v.to_be_bytes()[..chunk.len()]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A synthetic combination: [A.bfotf all, B.bfttf only 0x30..0x39, C.bfttf except 0..0x7F].
    pub fn sample() -> Vec<u8> {
        let mut d = vec![0u8; 0x30 + 3 * 0x28];
        d[..4].copy_from_slice(b"FCPX");
        d[4..6].copy_from_slice(&[0xFF, 0xFE]);
        d[6..8].copy_from_slice(&0x14u16.to_le_bytes());
        d[8..12].copy_from_slice(&0x0903_0000u32.to_le_bytes());
        d[0x10..0x14].copy_from_slice(&1u32.to_le_bytes());
        d[0x1C..0x20].copy_from_slice(&40.0f32.to_le_bytes());
        d[0x24..0x28].copy_from_slice(&3u32.to_le_bytes());
        d[0x2C..0x30].copy_from_slice(&0xE06Bu32.to_le_bytes());
        let lists = d.len();
        // list 1 at `lists`, list 2 right after
        d.extend_from_slice(&0x30u32.to_le_bytes());
        d.extend_from_slice(&0x39u32.to_le_bytes());
        d.extend_from_slice(&0u32.to_le_bytes());
        d.extend_from_slice(&0x7Fu32.to_le_bytes());
        let names = d.len();
        d.extend_from_slice(b"A.bfotf\0B.bfttf\0C.bfttf\0");
        let put =
            |d: &mut Vec<u8>, at: usize, v: u32| d[at..at + 4].copy_from_slice(&v.to_le_bytes());
        for (i, (mode, list, name, scale, shift)) in [
            (0u32, 0usize, names, 1.5f32, -5i16),
            (1, lists, names + 8, 1.0, 0),
            (2, lists + 8, names + 16, 1.2, 3),
        ]
        .into_iter()
        .enumerate()
        {
            let e = 0x30 + i * 0x28;
            put(&mut d, e, mode);
            put(
                &mut d,
                e + 4,
                if list == 0 { 0 } else { (list - 0x18) as u32 },
            );
            put(&mut d, e + 0x0C, scale.to_bits());
            put(&mut d, e + 0x14, (name - 0x18) as u32);
            d[e + 0x18] = 4;
            d[e + 0x1A..e + 0x1C].copy_from_slice(&shift.to_le_bytes());
        }
        let len = d.len() as u32;
        put(&mut d, 0x0C, len);
        d
    }

    #[test]
    fn parses_combination() {
        let c = Combination::parse(&sample()).unwrap();
        assert_eq!(c.size, 40.0);
        assert_eq!(c.alternate_char, 0xE06B);
        let files: Vec<&str> = c.fonts.iter().map(|f| f.file.as_str()).collect();
        assert_eq!(files, ["A.bfotf", "B.bfttf", "C.bfttf"]);
        assert_eq!(c.fonts[0].baseline_shift, -5);
        assert_eq!(c.fonts[1].ranges, [(0x30, 0x39)]);
        assert_eq!(c.fonts[2].ranges, [(0, 0x7F)]);
        // '5': C excludes ASCII, B lists digits, A takes everything
        assert_eq!(c.candidates('5' as u32).collect::<Vec<_>>(), [1, 0]);
        assert_eq!(c.candidates('A' as u32).collect::<Vec<_>>(), [0]);
        assert_eq!(c.candidates(0x3042).collect::<Vec<_>>(), [2, 0]);
    }

    #[test]
    fn scalable_round_trip() {
        let font = b"OTTO\x00\x09 some font bytes, odd length".to_vec();
        let wrapped = encrypt_scalable(&font);
        assert_ne!(&wrapped[8..12], b"OTTO");
        assert_eq!(decrypt_scalable(&wrapped).unwrap(), font);
        assert!(decrypt_scalable(b"OTTOxxxx").is_err());
    }
}
