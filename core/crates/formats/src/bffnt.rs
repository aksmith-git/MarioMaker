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
        while cmap != 0 && guard < 4096 {
            let first = r.u16(cmap)? as u32;
            let last = r.u16(cmap + 2)? as u32;
            let method = r.u16(cmap + 4)?;
            let next = r.u32(cmap + 8)? as usize;
            let d = cmap + 12;
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

    /// Untile and decode sheet `i`. Glyph coverage ends up in alpha (RGB = 1).
    pub fn decode_sheet(&self, i: usize) -> Result<Image> {
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
