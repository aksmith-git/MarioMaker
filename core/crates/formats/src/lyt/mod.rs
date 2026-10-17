//! Nintendo "Cafe" layout family (Wii U / Switch UI): BFLYT layouts and BFLAN animations.
//!
//! Written from public format documentation of the layout family. Wii U files are big-endian,
//! Switch files little-endian; both are read through [`Reader`]. Only the parts needed to place,
//! animate and draw panes are decoded; everything else is skipped by section size.

pub mod bflan;
pub mod bflyt;
pub mod json;

pub use bflan::{AnimContent, AnimEntry, AnimKey, AnimTag, Animation, CurveType};
pub use bflyt::{
    BlendMode, Group, Layout, Material, Pane, PaneKind, Part, PartBasicInfo, PartProperty, Picture,
    ProjTexGen, TevStage, TexCoordGen, TexMap, TexSrt, Text, UserData, UserDataValue, Window,
    WindowFrame,
};

use crate::error::{Error, Result};

/// Bounds-checked reader for one file, big- or little-endian.
#[derive(Clone, Copy)]
pub(crate) struct Reader<'a> {
    pub data: &'a [u8],
    pub be: bool,
    pub what: &'static str,
}

impl<'a> Reader<'a> {
    pub fn new(data: &'a [u8], be: bool, what: &'static str) -> Self {
        Self { data, be, what }
    }

    pub fn err(&self, detail: impl Into<String>) -> Error {
        Error::Format {
            what: self.what,
            detail: detail.into(),
        }
    }

    pub fn bytes(&self, o: usize, n: usize) -> Result<&'a [u8]> {
        o.checked_add(n)
            .and_then(|end| self.data.get(o..end))
            .ok_or_else(|| self.err(format!("read of {n} bytes at {o:#x} past end")))
    }

    pub fn u8(&self, o: usize) -> Result<u8> {
        Ok(self.bytes(o, 1)?[0])
    }

    pub fn i8(&self, o: usize) -> Result<i8> {
        Ok(self.u8(o)? as i8)
    }

    pub fn u16(&self, o: usize) -> Result<u16> {
        let b: [u8; 2] = self.bytes(o, 2)?.try_into().unwrap();
        Ok(if self.be {
            u16::from_be_bytes(b)
        } else {
            u16::from_le_bytes(b)
        })
    }

    pub fn i16(&self, o: usize) -> Result<i16> {
        Ok(self.u16(o)? as i16)
    }

    pub fn u32(&self, o: usize) -> Result<u32> {
        let b: [u8; 4] = self.bytes(o, 4)?.try_into().unwrap();
        Ok(if self.be {
            u32::from_be_bytes(b)
        } else {
            u32::from_le_bytes(b)
        })
    }

    pub fn f32(&self, o: usize) -> Result<f32> {
        Ok(f32::from_bits(self.u32(o)?))
    }

    pub fn rgba(&self, o: usize) -> Result<[u8; 4]> {
        Ok(self.bytes(o, 4)?.try_into().unwrap())
    }

    /// NUL-terminated string starting at `o` (ASCII/UTF-8; lossy).
    pub fn cstr(&self, o: usize) -> Result<String> {
        let raw = self
            .data
            .get(o..)
            .ok_or_else(|| self.err(format!("string at {o:#x} past end")))?;
        let len = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
        Ok(String::from_utf8_lossy(&raw[..len]).into_owned())
    }

    /// Fixed-size, NUL-padded string field.
    pub fn fixed_str(&self, o: usize, n: usize) -> Result<String> {
        let raw = self.bytes(o, n)?;
        let len = raw.iter().position(|&b| b == 0).unwrap_or(n);
        Ok(String::from_utf8_lossy(&raw[..len]).into_owned())
    }

    /// NUL-terminated UTF-16 string (in the file's byte order), at most `max_units` units.
    pub fn utf16(&self, o: usize, max_units: usize) -> Result<String> {
        let mut units = Vec::new();
        for i in 0..max_units {
            let Ok(u) = self.u16(o + i * 2) else { break };
            if u == 0 {
                break;
            }
            units.push(u);
        }
        Ok(String::from_utf16_lossy(&units))
    }
}

/// Common header of the Cafe binary family (`FLYT`, `FLAN`, `FFNT`, `FLIM`):
/// magic, byte-order mark, header size, version, file size, section count.
pub(crate) struct FileHeader {
    pub be: bool,
    pub header_size: usize,
    pub version: u32,
    pub sections: usize,
}

pub(crate) fn file_header(data: &[u8], magic: &[u8; 4], what: &'static str) -> Result<FileHeader> {
    if data.len() < 0x14 || &data[..4] != magic {
        return Err(Error::Magic { what });
    }
    let be = match [data[4], data[5]] {
        [0xFE, 0xFF] => true,
        [0xFF, 0xFE] => false,
        _ => {
            return Err(Error::Format {
                what,
                detail: "bad byte-order mark".into(),
            });
        }
    };
    let r = Reader::new(data, be, what);
    Ok(FileHeader {
        be,
        header_size: r.u16(6)? as usize,
        version: r.u32(8)?,
        sections: r.u16(0x10)? as usize,
    })
}

/// Iterate the sections of a Cafe binary file: (magic, offset of the section header, size).
pub(crate) fn sections(r: &Reader, h: &FileHeader) -> Result<Vec<([u8; 4], usize, usize)>> {
    let mut out = Vec::with_capacity(h.sections);
    let mut o = h.header_size;
    for _ in 0..h.sections {
        let magic: [u8; 4] = r.bytes(o, 4)?.try_into().unwrap();
        let size = r.u32(o + 4)? as usize;
        if size < 8 || o + size > r.data.len() {
            return Err(r.err(format!(
                "section {} at {o:#x} has bad size {size:#x}",
                String::from_utf8_lossy(&magic)
            )));
        }
        out.push((magic, o, size));
        o += size;
    }
    Ok(out)
}
