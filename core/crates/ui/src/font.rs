//! Font metrics for text layout (from the importer's `fonts/<name>.json`, or a parsed BFFNT).

use mmk_formats::bffnt::{CharWidth, Font};
use serde_json::Value;

use crate::cache::UiError;

/// Metrics + character map of one bitmap font, and the texture ids of its glyph sheets.
#[derive(Clone, Debug, Default)]
pub struct FontRes {
    pub name: String,
    /// Parsed metrics (sheets are not kept here; they are textures in the cache).
    pub font: Font,
    pub sheet_textures: Vec<u32>,
    /// Font used for characters this one lacks (e.g. kana/kanji in user course names).
    pub fallback: Option<std::sync::Arc<FontRes>>,
}

/// A glyph ready to place: texture, pixel rectangle on the sheet, horizontal metrics.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Glyph {
    pub texture: u32,
    pub u0: f32,
    pub v0: f32,
    pub u1: f32,
    pub v1: f32,
    pub width: CharWidth,
}

impl FontRes {
    pub fn from_font(name: &str, mut font: Font, sheet_textures: Vec<u32>) -> Self {
        font.sheets.clear();
        Self {
            name: name.to_string(),
            font,
            sheet_textures,
            fallback: None,
        }
    }

    pub fn from_json(name: &str, v: &Value, sheet_textures: Vec<u32>) -> Result<Self, UiError> {
        let bad = || UiError::Invalid(format!("font {name}: bad JSON"));
        let n = |k: &str| v.get(k).and_then(Value::as_u64).ok_or_else(bad);
        let pair = |k: &str| -> Result<(u64, u64), UiError> {
            let a = v.get(k).and_then(Value::as_array).ok_or_else(bad)?;
            Ok((
                a.first().and_then(Value::as_u64).ok_or_else(bad)?,
                a.get(1).and_then(Value::as_u64).ok_or_else(bad)?,
            ))
        };
        let cw = |a: &[Value]| -> Option<CharWidth> {
            Some(CharWidth {
                left: a.first()?.as_i64()? as i8,
                glyph_width: a.get(1)?.as_u64()? as u8,
                char_width: a.get(2)?.as_u64()? as u8,
            })
        };
        let mut font = Font {
            height: n("height")? as u8,
            width: n("width")? as u8,
            ascent: n("ascent")? as u8,
            line_feed: n("line_feed")? as u16,
            alter_char_index: n("alter_char_index")? as u16,
            default_width: cw(v
                .get("default_width")
                .and_then(Value::as_array)
                .ok_or_else(bad)?)
            .ok_or_else(bad)?,
            baseline: n("baseline")? as u16,
            max_char_width: n("max_char_width")? as u8,
            cells_per_row: n("cells_per_row")? as u16,
            cells_per_column: n("cells_per_column")? as u16,
            sheet_format: n("sheet_format")? as u16,
            ..Default::default()
        };
        let (cw0, ch0) = pair("cell")?;
        font.cell_width = cw0 as u8;
        font.cell_height = ch0 as u8;
        let (sw, sh) = pair("sheet_size")?;
        font.sheet_width = sw as u16;
        font.sheet_height = sh as u16;
        for e in v.get("widths").and_then(Value::as_array).ok_or_else(bad)? {
            let a = e.as_array().ok_or_else(bad)?;
            let g = a.first().and_then(Value::as_u64).ok_or_else(bad)? as u16;
            font.widths.insert(g, cw(&a[1..]).ok_or_else(bad)?);
        }
        for e in v.get("cmap").and_then(Value::as_array).ok_or_else(bad)? {
            let a = e.as_array().ok_or_else(bad)?;
            font.cmap.insert(
                a.first().and_then(Value::as_u64).ok_or_else(bad)? as u32,
                a.get(1).and_then(Value::as_u64).ok_or_else(bad)? as u16,
            );
        }
        Ok(Self {
            name: name.to_string(),
            font,
            sheet_textures,
            fallback: None,
        })
    }

    pub fn has_char(&self, c: char) -> bool {
        self.font.has_char(c)
    }

    /// The font that draws `c`: this one, else the fallback if it has the character, else this
    /// one (its replacement glyph).
    pub fn for_char(&self, c: char) -> &FontRes {
        match &self.fallback {
            Some(f) if !self.has_char(c) && f.has_char(c) => f,
            _ => self,
        }
    }

    /// Glyph for `c` (the font's replacement glyph when it has none).
    pub fn glyph(&self, c: char) -> Glyph {
        let g = self.font.glyph(c);
        let cell = self.font.cell(g);
        let width = self.font.char_width(g);
        let (sw, sh) = (
            self.font.sheet_width.max(1) as f32,
            self.font.sheet_height.max(1) as f32,
        );
        let x0 = cell.x as f32;
        let y0 = cell.y as f32;
        Glyph {
            texture: self
                .sheet_textures
                .get(cell.sheet)
                .copied()
                .unwrap_or(crate::TEXTURE_NONE),
            u0: x0 / sw,
            v0: y0 / sh,
            u1: (x0 + width.glyph_width as f32) / sw,
            v1: (y0 + self.font.cell_height as f32) / sh,
            width,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_and_binary_agree() {
        let font = Font::parse(&mmk_formats::fixtures::sample_font()).unwrap();
        let a = FontRes::from_font("F", font.clone(), vec![7]);
        let json = serde_json::json!({
            "height": font.height, "width": font.width, "ascent": font.ascent,
            "line_feed": font.line_feed, "alter_char_index": font.alter_char_index,
            "default_width": [font.default_width.left, font.default_width.glyph_width, font.default_width.char_width],
            "cell": [font.cell_width, font.cell_height], "baseline": font.baseline,
            "max_char_width": font.max_char_width, "cells_per_row": font.cells_per_row,
            "cells_per_column": font.cells_per_column, "sheet_size": [font.sheet_width, font.sheet_height],
            "sheet_format": font.sheet_format,
            "widths": font.widths.iter().map(|(g, c)| serde_json::json!([g, c.left, c.glyph_width, c.char_width])).collect::<Vec<_>>(),
            "cmap": font.cmap.iter().map(|(c, g)| serde_json::json!([c, g])).collect::<Vec<_>>(),
        });
        let b = FontRes::from_json("F", &json, vec![7]).unwrap();
        for c in ['A', 'B', '0', '2', '!', '?'] {
            assert_eq!(a.glyph(c), b.glyph(c));
        }
        let g = a.glyph('B'); // glyph 1: second cell of the first row
        assert_eq!(g.texture, 7);
        assert_eq!(g.u0, 9.0 / 64.0);
        assert_eq!(g.width.glyph_width, 6);
    }
}
