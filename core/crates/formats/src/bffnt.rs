//! BFFNT bitmap fonts (Wii U, version 3): glyph sheets (GX2 textures), per-glyph widths and the
//! character → glyph map.
//!
//! ```text
//! header "FFNT" bom u16 hdr_size u32 version u32 file_size u16 sections u16 pad
//! FINF   u8 type, u8 height, u8 width, u8 ascent, u16 line_feed, u16 alter_char_index,
//!        s8 default_left, u8 default_glyph_width, u8 default_char_width, u8 encoding,
//!        u32 tglp, u32 cwdh, u32 cmap       (offsets point just past a section header)
//! TGLP   u8 cell_w, u8 cell_h, u8 sheet_count, u8 max_char_width, u32 sheet_size,
//!        u16 baseline, u16 sheet_format, u16 cells_per_row, u16 cells_per_column,
//!        u16 sheet_w, u16 sheet_h, u32 sheet_data_offset (absolute)
//! CWDH   u16 first_index, u16 last_index, u32 next; (s8 left, u8 glyph_w, u8 char_w)[n]
//! CMAP   u16 first_code, u16 last_code, u16 method (0 direct, 1 table, 2 scan), pad, u32 next,
//!        direct: u16 first_index · table: u16 index[n] · scan: u16 count, (u16 code, u16 index)[]
//!        (code ranges chain through `next`)
//! ```
//! Cells are laid out on each sheet with a 1-pixel gutter: cell (c, r) starts at
//! (c·(cell_w+1)+1, r·(cell_h+1)+1). Sheets are the slices of one 2D-tiled GX2 array surface
//! (so each has its own bank rotation) and are stored bottom row first.
//!
//! Switch fonts (version 4, little-endian; SMM2's number and picture fonts) keep that layout
//! with two changes, checked on every SMM2 bitmap font:
//! - the sheet data is a complete BNTX file (one texture; sheets are its array layers, or one
//!   BNTX per sheet), stored top row first;
//! - CMAP code ranges are u32 (`u32 first, u32 last, u16 method, pad, u32 next`) and scan
//!   entries are `u32 code, u16 index, pad` after a `u16 count, pad` header.

use std::collections::BTreeMap;

use crate::bflim::{FlimFormat, Image, decode_surface};
use crate::error::Result;
use crate::lyt::{Reader, file_header};

pub const MAGIC: &[u8; 4] = b"FFNT";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CharWidth {
    pub left: i8,
    pub glyph_width: u8,
    pub char_width: u8,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Font {
    pub version: u32,
    pub font_type: u8,
    pub height: u8,
    pub width: u8,
    pub ascent: u8,
    pub line_feed: u16,
    pub alter_char_index: u16,
    pub default_width: CharWidth,
    pub encoding: u8,
    pub cell_width: u8,
    pub cell_height: u8,
    pub max_char_width: u8,
    pub baseline: u16,
    pub sheet_format: u16,
    pub cells_per_row: u16,
    pub cells_per_column: u16,
    pub sheet_width: u16,
    pub sheet_height: u16,
    /// Raw (tiled) sheets.
    pub sheets: Vec<Vec<u8>>,
    /// Glyph index → widths (glyphs without an entry use `default_width`).
    pub widths: BTreeMap<u16, CharWidth>,
    /// Character code → glyph index.
    pub cmap: BTreeMap<u32, u16>,
}

/// Where a glyph lives: sheet number and pixel rectangle of its cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GlyphCell {
    pub sheet: usize,
    pub x: u32,
    pub y: u32,
}

impl Font {
    pub fn parse(data: &[u8]) -> Result<Self> {
        let h = file_header(data, MAGIC, "BFFNT")?;
        let r = Reader::new(data, h.be, "BFFNT");
        let fi = h.header_size;
        if r.bytes(fi, 4)? != b"FINF" {
            return Err(r.err("FINF section missing"));
        }
        let b = fi + 8;
        let mut f = Font {
            version: h.version,
            font_type: r.u8(b)?,
            height: r.u8(b + 1)?,
            width: r.u8(b + 2)?,
            ascent: r.u8(b + 3)?,
            line_feed: r.u16(b + 4)?,
            alter_char_index: r.u16(b + 6)?,
            default_width: CharWidth {
                left: r.i8(b + 8)?,
                glyph_width: r.u8(b + 9)?,
                char_width: r.u8(b + 10)?,
            },
            encoding: r.u8(b + 11)?,
            ..Default::default()
        };
        let tglp = r.u32(b + 12)? as usize;
        let mut cwdh = r.u32(b + 16)? as usize;
        let mut cmap = r.u32(b + 20)? as usize;

        // TGLP (offset points past the section header)
        let t = tglp;
        f.cell_width = r.u8(t)?;
        f.cell_height = r.u8(t + 1)?;
        let sheet_count = r.u8(t + 2)? as usize;
        f.max_char_width = r.u8(t + 3)?;
        let sheet_size = r.u32(t + 4)? as usize;
        f.baseline = r.u16(t + 8)?;
        f.sheet_format = r.u16(t + 10)?;
        f.cells_per_row = r.u16(t + 12)?;
        f.cells_per_column = r.u16(t + 14)?;
        f.sheet_width = r.u16(t + 16)?;
        f.sheet_height = r.u16(t + 18)?;
        let data_off = r.u32(t + 20)? as usize;
        for i in 0..sheet_count {
            f.sheets
                .push(r.bytes(data_off + i * sheet_size, sheet_size)?.to_vec());
        }

        let mut guard = 0;
        while cwdh != 0 && guard < 4096 {
            let first = r.u16(cwdh)?;
            let last = r.u16(cwdh + 2)?;
            let next = r.u32(cwdh + 4)? as usize;
            for (k, idx) in (first..=last).enumerate() {
                let e = cwdh + 8 + k * 3;
                f.widths.insert(
                    idx,
                    CharWidth {
                        left: r.i8(e)?,
                        glyph_width: r.u8(e + 1)?,
                        char_width: r.u8(e + 2)?,
                    },
                );
            }
            cwdh = next;
            guard += 1;
        }

        guard = 0;
        let nx = f.is_nx();
        while cmap != 0 && guard < 4096 {
            let (first, last, method, next, d) = if nx {
                (
                    r.u32(cmap)?,
                    r.u32(cmap + 4)?,
                    r.u16(cmap + 8)?,
                    r.u32(cmap + 12)? as usize,
                    cmap + 16,
                )
            } else {
                (
                    r.u16(cmap)? as u32,
                    r.u16(cmap + 2)? as u32,
                    r.u16(cmap + 4)?,
                    r.u32(cmap + 8)? as usize,
                    cmap + 12,
                )
            };
            if last < first || last - first > 0x10_FFFF {
                return Err(r.err(format!("CMAP range {first:#x}..{last:#x}")));
            }
            match method {
                0 => {
                    let base = r.u16(d)?;
                    for code in first..=last {
                        f.cmap.insert(code, base + (code - first) as u16);
                    }
                }
                1 => {
                    for (k, code) in (first..=last).enumerate() {
                        let idx = r.u16(d + k * 2)?;
                        if idx != 0xFFFF {
                            f.cmap.insert(code, idx);
                        }
                    }
                }
                _ if nx => {
                    let n = r.u16(d)? as usize;
                    for k in 0..n {
                        let e = d + 4 + k * 8;
                        let idx = r.u16(e + 4)?;
                        if idx != 0xFFFF {
                            f.cmap.insert(r.u32(e)?, idx);
                        }
                    }
                }
                _ => {
                    let n = r.u16(d)? as usize;
                    for k in 0..n {
                        let e = d + 2 + k * 4;
                        let idx = r.u16(e + 2)?;
                        if idx != 0xFFFF {
                            f.cmap.insert(r.u16(e)? as u32, idx);
                        }
                    }
                }
            }
            cmap = next;
            guard += 1;
        }
        Ok(f)
    }

    /// A Switch font (version 4): BNTX sheets, 32-bit CMAP codes.
    pub fn is_nx(&self) -> bool {
        self.version >= 0x0400_0000
    }

    /// Glyph index for a character (falls back to `alter_char_index`).
    pub fn glyph(&self, c: char) -> u16 {
        self.cmap
            .get(&(c as u32))
            .copied()
            .unwrap_or(self.alter_char_index)
    }

    pub fn has_char(&self, c: char) -> bool {
        self.cmap.contains_key(&(c as u32))
    }

    pub fn char_width(&self, glyph: u16) -> CharWidth {
        self.widths
            .get(&glyph)
            .copied()
            .unwrap_or(self.default_width)
    }

    pub fn cell(&self, glyph: u16) -> GlyphCell {
        let per_row = self.cells_per_row.max(1) as u32;
        let per_sheet = per_row * self.cells_per_column.max(1) as u32;
        let g = glyph as u32;
        let local = g % per_sheet;
        GlyphCell {
            sheet: (g / per_sheet) as usize,
            x: (local % per_row) * (self.cell_width as u32 + 1) + 1,
            y: (local / per_row) * (self.cell_height as u32 + 1) + 1,
        }
    }

    /// Sheet pixel format (low 14 bits of `sheet_format`; bit 15 marks a linear sheet).
    pub fn sheet_flim_format(&self) -> Option<FlimFormat> {
        Some(match self.sheet_format & 0x3FFF {
            0 => FlimFormat::Rgba8,
            3 => FlimFormat::Rgb565,
            5 => FlimFormat::La8,
            6 => FlimFormat::La4,
            8 => FlimFormat::A8,
            9 => FlimFormat::Bc1,
            10 => FlimFormat::Bc2,
            11 => FlimFormat::Bc3,
            12 => FlimFormat::Bc4A,
            13 => FlimFormat::Bc5La,
            _ => return None,
        })
    }

    /// Sheet `i` of a Switch font: the embedded BNTX's texture (layer `i`, or the `i`th BNTX),
    /// top row first, in its stored format with the BNTX channel select.
    fn decode_nx_sheet(&self, i: usize) -> Result<Image> {
        let bad = |detail: String| crate::Error::Format {
            what: "BFFNT",
            detail,
        };
        let parse = |raw: &[u8]| {
            crate::bntx::parse(raw)
                .ok()
                .and_then(|t| t.into_iter().next())
        };
        let (tex, layer) = match self.sheets.get(i).and_then(|raw| parse(raw)) {
            Some(t) => (t, None),
            // one BNTX holding every sheet as an array layer
            None => (
                self.sheets
                    .first()
                    .and_then(|raw| parse(raw))
                    .ok_or_else(|| bad(format!("sheet {i}: no BNTX texture")))?,
                Some(i),
            ),
        };
        let data = match layer {
            Some(l) if !tex.layers.is_empty() => tex
                .layers
                .get(l)
                .cloned()
                .ok_or_else(|| bad(format!("no sheet {i}")))?,
            _ => tex.data.clone(),
        };
        use crate::bflim::PixelKind;
        use crate::bntx::PixelFormat as P;
        let kind = match tex.format {
            P::Rgba8 => PixelKind::Rgba8,
            P::Bc1 => PixelKind::Bc1,
            P::Bc2 => PixelKind::Bc2,
            P::Bc3 => PixelKind::Bc3,
            P::Bc4 => PixelKind::Bc4,
            P::Bc5 => PixelKind::Bc5,
            f => return Err(bad(format!("sheet format {f:?} not supported"))),
        };
        // a one-channel sheet (white + coverage in alpha) is read as luminance too: the font's
        // black → white colours interpolate by it, so shaded glyphs (SMM2's SMB3 / SMW number
        // fonts) keep their dark inner pixels [inf]
        let channel_select = match tex.channel_select {
            [1, 1, 1, c] if (2..=5).contains(&c) => [c, c, c, c],
            s => s,
        };
        let mut img = Image {
            width: tex.width as u32,
            height: tex.height as u32,
            kind,
            srgb: tex.variant == crate::bntx::Variant::Srgb,
            channel_select,
            data,
        };
        // like the Wii U sheets, stored bottom row first (SMM2's number fonts come out upside
        // down and in reverse row order otherwise)
        img.flip_vertical();
        Ok(img)
    }

    /// Untile and decode sheet `i`. Glyph coverage ends up in alpha (RGB = 1).
    pub fn decode_sheet(&self, i: usize) -> Result<Image> {
        if self.is_nx() {
            return self.decode_nx_sheet(i);
        }
        let fmt = self
            .sheet_flim_format()
            .ok_or_else(|| crate::Error::Format {
                what: "BFFNT",
                detail: format!("sheet format {:#x} not supported", self.sheet_format),
            })?;
        let tile_mode = if self.sheet_format & 0x8000 != 0 {
            0
        } else {
            4
        };
        // the sheets are the slices of one 2D array surface: each has its own bank rotation
        let swizzle = crate::gx2::TileMode::from_code(tile_mode)
            .map(|m| crate::gx2::slice_swizzle(m, 0, i as u32))
            .unwrap_or(0);
        let raw = self.sheets.get(i).ok_or_else(|| crate::Error::Format {
            what: "BFFNT",
            detail: format!("no sheet {i}"),
        })?;
        let mut img = decode_surface(
            raw,
            self.sheet_width as u32,
            self.sheet_height as u32,
            fmt,
            tile_mode,
            swizzle,
        )?;
        // sheets are stored bottom row first; the cache keeps textures top row first
        img.flip_vertical();
        Ok(img)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::sample_font;

    /// A Switch (v4) font: one BNTX sheet (BC4-like channel select on RGBA8 data), glyphs for
    /// '0' (index 0) and 'é' (index 1, a code above 0xFF) through a v4 scan CMAP.
    fn sample_font_nx() -> Vec<u8> {
        use crate::fixtures::{W, sample_bntx};
        let px: Vec<u8> = (0..16 * 8 * 4).map(|i| (i % 251) as u8).collect();
        let sheet = sample_bntx("Font", 0x0B01, 16, 8, 4, [1, 1, 1, 2], &[px]);
        let mut w = W::le();
        w.bytes(b"FFNT")
            .u16(0xFEFF)
            .u16(0x14)
            .u32(0x0401_0000)
            .u32(0)
            .u16(4)
            .u16(0);
        w.bytes(b"FINF").u32(0x20);
        w.u8(1)
            .u8(8)
            .u8(6)
            .u8(7)
            .u16(9)
            .u16(0)
            .u8(0)
            .u8(5)
            .u8(6)
            .u8(1);
        let ptrs = w.len();
        w.u32(0).u32(0).u32(0);
        let tglp = w.len();
        w.bytes(b"TGLP").u32(0x20);
        w.u8(6)
            .u8(7)
            .u8(1)
            .u8(6)
            .u32(sheet.len() as u32)
            .u16(6)
            .u16(12);
        w.u16(2).u16(1).u16(16).u16(8);
        let data_ptr = w.len();
        w.u32(0);
        let cwdh = w.len();
        w.bytes(b"CWDH").u32(8 + 8 + 6).u16(0).u16(1).u32(0);
        w.u8(0).u8(5).u8(6).u8(1).u8(4).u8(5).pad_to(4);
        let cmap = w.len();
        w.bytes(b"CMAP").u32(8 + 16 + 4 + 16);
        w.u32(0).u32(0xFFFF).u16(2).u16(0).u32(0);
        w.u16(2).u16(0);
        w.u32('0' as u32)
            .u16(0)
            .u16(0)
            .u32('é' as u32)
            .u16(1)
            .u16(0);
        w.pad_to(0x80);
        let data = w.len();
        w.bytes(&sheet);
        let total = w.len() as u32;
        w.put_u32(0x0C, total);
        w.put_u32(ptrs, (tglp + 8) as u32);
        w.put_u32(ptrs + 4, (cwdh + 8) as u32);
        w.put_u32(ptrs + 8, (cmap + 8) as u32);
        w.put_u32(data_ptr, data as u32);
        w.0
    }

    #[test]
    fn parses_switch_font() {
        let f = Font::parse(&sample_font_nx()).unwrap();
        assert!(f.is_nx());
        assert_eq!((f.height, f.ascent, f.baseline), (8, 7, 6));
        assert_eq!(f.glyph('0'), 0);
        assert_eq!(f.glyph('é'), 1);
        assert_eq!(f.char_width(1).char_width, 5);
        let img = f.decode_sheet(0).unwrap();
        assert_eq!(
            (img.width, img.height, img.kind),
            (16, 8, crate::bflim::PixelKind::Rgba8)
        );
        assert_eq!(img.channel_select, [2, 2, 2, 2]);
        // stored bottom row first: output pixel (1, 0) is source pixel (1, 7), bytes 452..456
        assert_eq!(&img.data[4..8], &[201, 202, 203, 204]);
    }

    #[test]
    fn parses_synthetic_font() {
        let f = Font::parse(&sample_font()).unwrap();
        assert_eq!((f.height, f.width, f.ascent, f.line_feed), (9, 7, 8, 10));
        assert_eq!((f.cell_width, f.cell_height, f.baseline), (7, 9, 8));
        assert_eq!(f.glyph('A'), 0);
        assert_eq!(f.glyph('B'), 1);
        assert_eq!(f.glyph('1'), 3);
        assert_eq!(f.glyph('!'), 4);
        assert_eq!(f.glyph('?'), 0); // alter char
        assert!(!f.has_char('?'));
        assert_eq!(
            f.char_width(4),
            CharWidth {
                left: 4,
                glyph_width: 9,
                char_width: 10
            }
        );
        assert_eq!(f.char_width(99), f.default_width);
        // cell 9 = row 1, column 1 on an 8-per-row sheet with 1-px gutters
        assert_eq!(
            f.cell(9),
            GlyphCell {
                sheet: 0,
                x: 9,
                y: 11
            }
        );
        let img = f.decode_sheet(0).unwrap();
        assert_eq!((img.width, img.height), (64, 32));
        // stored bottom row first: output row 0 is stored row 31 (pixel 31·64+1)
        assert_eq!(&img.data[4..8], &[255, 255, 255, (1985 % 251) as u8]);
    }
}
