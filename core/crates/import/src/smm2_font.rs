//! SMM2 scalable fonts → bitmap fonts for the layout player.
//!
//! SMM2 draws text with TrueType/OpenType fonts combined by BFCPX files (see
//! `mmk_formats::bfcpx`). The layout player draws bitmap fonts (glyph cells on sheets, as SMM1's
//! BFFNT), so the importer rasterises every character the English text needs, once, at the
//! combination's authored size, into the same cache form (`fonts/<Name>.json` + sheets).
//!
//! Choices where the game's own rules aren't known [inf]:
//! - Line height = 1.5 × the authored size and character width = the size. SMM2's text panes are
//!   authored with font sizes whose height is 1.5–1.57 × their width; with these metrics such a
//!   pane scales glyphs evenly, as the game shows them.
//! - The baseline sits so the main Latin font's em box is centred in the line.
//! - Fonts the game takes from the console (`nintendo_*`, the Switch system fonts) are not in the
//!   dump: the standard one is replaced by the combination's other fonts, and the controller
//!   button icons (U+E0E0…) are drawn here as simple shapes with the button's letters.
//! - Glyph borders (FontInfo `_B4` faces) are not drawn yet.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use ab_glyph::{Font as _, FontVec, PxScale, ScaleFont, point};
use mmk_assets::{Mtex, MtexFormat, MtexVariant};
use mmk_formats::bfcpx::Combination;
use mmk_formats::bffnt::{CharWidth, Font};

/// Line height / authored size [inf, see the module notes].
pub const LINE_HEIGHT: f32 = 1.5;
/// Widest sheet written.
const SHEET_MAX: u32 = 2048;
/// Game fonts that stand in for the console's system fonts (`nintendo_udsg-r_std_003` …), which
/// are not part of the game: UD Kakugo is the closest in the dump to the system's UD gothic.
pub const SUBSTITUTES: &[&str] = &["ninP_UDKakugoSmall-DB.bfotf"];

/// A rasterised font: metrics and character map as a BFFNT would give them, plus its sheets
/// (RGBA8, white with the glyph coverage in alpha).
pub struct Baked {
    pub font: Font,
    pub sheets: Vec<Mtex>,
    /// Characters no source font had (drawn with the replacement glyph).
    pub missing: Vec<char>,
    /// Characters drawn from each source font file (or "(button)" for drawn icons).
    pub sources: BTreeMap<String, usize>,
}

/// A controller button icon of the Switch system font, by code point [inf: the community's table
/// of `nintendo_ext_003`]: (letters, round).
pub fn button_icon(c: char) -> Option<(&'static str, bool)> {
    Some(match c as u32 {
        0xE0A0 | 0xE0E0 => ("A", true),
        0xE0A1 | 0xE0E1 => ("B", true),
        0xE0A2 | 0xE0E2 => ("X", true),
        0xE0A3 | 0xE0E3 => ("Y", true),
        0xE0A4 | 0xE0E4 => ("L", false),
        0xE0A5 | 0xE0E5 => ("R", false),
        0xE0A6 | 0xE0E6 => ("ZL", false),
        0xE0A7 | 0xE0E7 => ("ZR", false),
        0xE0E8 => ("SL", false),
        0xE0E9 => ("SR", false),
        0xE0EF | 0xE0B5 => ("+", true),
        0xE0F0 | 0xE0B6 => ("−", true),
        _ => return None,
    })
}

struct Raster {
    /// Coverage, `w`×`h`, row-major.
    px: Vec<u8>,
    w: u32,
    h: u32,
    width: CharWidth,
}

fn px_scale(f: &FontVec, px_per_em: f32) -> PxScale {
    let upem = f.units_per_em().unwrap_or(1000.0);
    PxScale::from(px_per_em * f.height_unscaled() / upem)
}

/// Rasterise `c` from `f` at `px_per_em`, baseline `baseline` px below the cell top, clipped to
/// `cell_h` rows.
fn raster(f: &FontVec, c: char, px_per_em: f32, baseline: f32, cell_h: u32) -> Option<Raster> {
    let id = f.glyph_id(c);
    if id.0 == 0 {
        return None;
    }
    let scale = px_scale(f, px_per_em);
    let advance = f.as_scaled(scale).h_advance(id);
    let glyph = id.with_scale_and_position(scale, point(0.0, baseline));
    let Some(outline) = f.outline_glyph(glyph) else {
        // blank glyph (space): advance only
        return Some(Raster {
            px: Vec::new(),
            w: 0,
            h: cell_h,
            width: CharWidth {
                left: 0,
                glyph_width: 0,
                char_width: advance.round().clamp(0.0, 255.0) as u8,
            },
        });
    };
    let b = outline.px_bounds();
    let left = b.min.x.floor();
    let w = (b.max.x.ceil() - left).max(1.0) as u32;
    let mut px = vec![0u8; (w * cell_h) as usize];
    outline.draw(|x, y, cov| {
        let gx = x as i64 + (b.min.x - left).round() as i64;
        let gy = y as i64 + b.min.y.round() as i64;
        if gx >= 0 && (gx as u32) < w && gy >= 0 && (gy as u32) < cell_h {
            let i = (gy as u32 * w + gx as u32) as usize;
            px[i] = px[i].max((cov * 255.0).round().clamp(0.0, 255.0) as u8);
        }
    });
    Some(Raster {
        px,
        w,
        h: cell_h,
        width: CharWidth {
            left: left.clamp(-128.0, 127.0) as i8,
            glyph_width: w.min(255) as u8,
            char_width: advance.round().clamp(0.0, 255.0) as u8,
        },
    })
}

/// A button icon: a filled circle (A, B, X, Y, +, −) or rounded rectangle (L, R, ZL, ZR…) with
/// the letters cut out, drawn with `letters_font` (4× supersampled).
fn button_raster(
    letters: &str,
    round: bool,
    letters_font: &FontVec,
    px_per_em: f32,
    baseline: f32,
    cell_h: u32,
) -> Raster {
    const SS: u32 = 4;
    let em = px_per_em;
    let h = (em * 0.86).round().max(4.0);
    let w = if round {
        h
    } else {
        (h * (0.62 + 0.38 * letters.chars().count() as f32)).round()
    };
    let (wi, hi) = (w as u32, h as u32);
    let top = (baseline - em * 0.78).round().max(0.0);
    // shape coverage
    let mut cov = vec![0f32; (wi * cell_h) as usize];
    let r = if round { h / 2.0 } else { h * 0.3 };
    for y in 0..hi {
        for x in 0..wi {
            let mut inside = 0;
            for sy in 0..SS {
                for sx in 0..SS {
                    let px = x as f32 + (sx as f32 + 0.5) / SS as f32;
                    let py = y as f32 + (sy as f32 + 0.5) / SS as f32;
                    // rounded rectangle: distance to the inner rectangle ≤ r
                    let dx = (px - px.clamp(r, w - r)).abs();
                    let dy = (py - py.clamp(r, h - r)).abs();
                    if dx * dx + dy * dy <= r * r {
                        inside += 1;
                    }
                }
            }
            let gy = y + top as u32;
            if gy < cell_h {
                cov[(gy * wi + x) as usize] = inside as f32 / (SS * SS) as f32;
            }
        }
    }
    // letters, centred in the shape
    let scale = px_scale(
        letters_font,
        em * if letters.len() > 1 { 0.5 } else { 0.62 },
    );
    let sf = letters_font.as_scaled(scale);
    let ids: Vec<_> = letters.chars().map(|c| letters_font.glyph_id(c)).collect();
    let text_w: f32 = ids.iter().map(|&id| sf.h_advance(id)).sum();
    let cap = sf.ascent() * 0.72;
    let mut pen = (w - text_w) / 2.0;
    let base = top + (h + cap) / 2.0;
    for id in ids {
        let g = id.with_scale_and_position(scale, point(pen, base));
        pen += sf.h_advance(id);
        if let Some(o) = letters_font.outline_glyph(g) {
            let b = o.px_bounds();
            o.draw(|x, y, c| {
                let gx = x as i64 + b.min.x.round() as i64;
                let gy = y as i64 + b.min.y.round() as i64;
                if gx >= 0 && (gx as u32) < wi && gy >= 0 && (gy as u32) < cell_h {
                    let i = (gy as u32 * wi + gx as u32) as usize;
                    cov[i] = (cov[i] - c).max(0.0);
                }
            });
        }
    }
    Raster {
        px: cov.iter().map(|v| (v * 255.0).round() as u8).collect(),
        w: wi,
        h: cell_h,
        width: CharWidth {
            left: (em * 0.06).round() as i8,
            glyph_width: wi.min(255) as u8,
            char_width: (w + em * 0.12).round().min(255.0) as u8,
        },
    }
}

/// Rasterise `chars` for combination `comb` from the decrypted source fonts (`fonts`, by file
/// name). Characters without a source glyph are left out (the alternate glyph draws them).
pub fn bake(comb: &Combination, fonts: &HashMap<String, FontVec>, chars: &BTreeSet<char>) -> Baked {
    let size = comb.size.max(1.0);
    let line_h = (size * LINE_HEIGHT).round().clamp(1.0, 255.0) as u32;
    let present = |i: usize| fonts.get(&comb.fonts[i].file);
    // the main Latin font gives the vertical metrics
    let main = comb
        .candidates('A' as u32)
        .find(|&i| present(i).is_some_and(|f| f.glyph_id('A').0 != 0));
    let main_font = main
        .and_then(present)
        .or_else(|| SUBSTITUTES.iter().find_map(|f| fonts.get(*f)));
    let (asc, desc) = match main_font {
        Some(f) => {
            let upem = f.units_per_em().unwrap_or(1000.0);
            (
                f.ascent_unscaled() / upem * size,
                -f.descent_unscaled() / upem * size,
            )
        }
        None => (size * 0.8, size * 0.2),
    };
    let baseline = ((line_h as f32 - (asc + desc)) / 2.0 + asc).round();
    let letters_font = main_font;

    let mut rasters: Vec<(char, Raster)> = Vec::new();
    let mut missing = Vec::new();
    let mut sources: BTreeMap<String, usize> = BTreeMap::new();
    let mut wanted: BTreeSet<char> = chars.clone();
    if let Some(c) = char::from_u32(comb.alternate_char) {
        wanted.insert(c);
    }
    wanted.insert('?');
    for &c in &wanted {
        let mut got = None;
        // the entries that list the character, then (last resort) every entry, then the
        // substitutes for the console's own fonts
        let order: Vec<(String, f32, i16)> = comb
            .candidates(c as u32)
            .chain((0..comb.fonts.len()).rev())
            .map(|i| {
                let s = &comb.fonts[i];
                (s.file.clone(), s.scale, s.baseline_shift)
            })
            .chain(SUBSTITUTES.iter().map(|f| (f.to_string(), 1.0, 0)))
            .collect();
        for (file, scale, shift) in order {
            let Some(f) = fonts.get(&file) else {
                continue;
            };
            let em = size * if scale > 0.0 { scale } else { 1.0 };
            if let Some(r) = raster(f, c, em, baseline - shift as f32, line_h) {
                *sources.entry(file).or_default() += 1;
                got = Some(r);
                break;
            }
        }
        if got.is_none()
            && let (Some((letters, round)), Some(lf)) = (button_icon(c), letters_font)
        {
            *sources.entry("(button)".into()).or_default() += 1;
            got = Some(button_raster(letters, round, lf, size, baseline, line_h));
        }
        match got {
            Some(r) => rasters.push((c, r)),
            None if c as u32 == comb.alternate_char => {}
            None => missing.push(c),
        }
    }

    // glyph cells on sheets, 1-px gutters (the BFFNT layout the player expects)
    let cell_w = rasters.iter().map(|(_, r)| r.w).max().unwrap_or(1).max(1);
    let cell_h = line_h;
    let per_row = ((SHEET_MAX - 1) / (cell_w + 1)).max(1);
    let per_col = ((SHEET_MAX - 1) / (cell_h + 1)).max(1);
    let per_sheet = (per_row * per_col) as usize;
    let sheet_count = rasters.len().div_ceil(per_sheet).max(1);
    let rows_used = |n: usize| (n as u32).div_ceil(per_row).min(per_col);
    let sheet_w = per_row * (cell_w + 1) + 1;
    let mut sheets = Vec::with_capacity(sheet_count);
    let mut font = Font {
        version: 0x0401_0000,
        font_type: 1,
        height: line_h as u8,
        width: size.round().clamp(1.0, 255.0) as u8,
        ascent: baseline.clamp(0.0, 255.0) as u8,
        line_feed: line_h as u16,
        encoding: 1,
        cell_width: cell_w.min(255) as u8,
        cell_height: cell_h as u8,
        baseline: baseline.clamp(0.0, 255.0) as u16,
        sheet_format: 0,
        cells_per_row: per_row as u16,
        cells_per_column: per_col as u16,
        ..Default::default()
    };
    let rows_max = rows_used(rasters.len().min(per_sheet));
    let sheet_h = rows_max * (cell_h + 1) + 1;
    font.sheet_width = sheet_w as u16;
    font.sheet_height = sheet_h as u16;
    for s in 0..sheet_count {
        // white, coverage in alpha
        let mut data = [255u8, 255, 255, 0].repeat((sheet_w * sheet_h) as usize);
        for (k, (_, r)) in rasters
            .iter()
            .enumerate()
            .skip(s * per_sheet)
            .take(per_sheet)
        {
            let local = (k - s * per_sheet) as u32;
            let x0 = (local % per_row) * (cell_w + 1) + 1;
            let y0 = (local / per_row) * (cell_h + 1) + 1;
            for y in 0..r.h.min(cell_h) {
                for x in 0..r.w {
                    let a = r.px.get((y * r.w + x) as usize).copied().unwrap_or(0);
                    data[(((y0 + y) * sheet_w + x0 + x) * 4 + 3) as usize] = a;
                }
            }
        }
        sheets.push(Mtex {
            format: MtexFormat::Rgba8,
            variant: MtexVariant::Unorm,
            width: sheet_w,
            height: sheet_h,
            channel_select: [2, 3, 4, 5],
            data,
        });
    }
    let mut max_char = 0u8;
    for (g, (c, r)) in rasters.iter().enumerate() {
        font.cmap.insert(*c as u32, g as u16);
        font.widths.insert(g as u16, r.width);
        max_char = max_char.max(r.width.char_width);
    }
    font.max_char_width = max_char;
    let alter = char::from_u32(comb.alternate_char)
        .and_then(|c| font.cmap.get(&(c as u32)).copied())
        .or_else(|| font.cmap.get(&('?' as u32)).copied())
        .unwrap_or(0);
    font.alter_char_index = alter;
    font.default_width = font.widths.get(&alter).copied().unwrap_or_default();
    Baked {
        font,
        sheets,
        missing,
        sources,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn button_icons_cover_the_face_buttons() {
        assert_eq!(button_icon('\u{E0E0}'), Some(("A", true)));
        assert_eq!(button_icon('\u{E0E6}'), Some(("ZL", false)));
        assert_eq!(button_icon('A'), None);
    }

    #[test]
    fn bake_without_source_fonts_keeps_only_drawable_glyphs() {
        // no font files: nothing can be drawn, every character is reported missing
        let comb = Combination {
            size: 40.0,
            alternate_char: 0xE06B,
            fonts: vec![mmk_formats::bfcpx::SubFont {
                file: "absent.bfttf".into(),
                scale: 1.0,
                ..Default::default()
            }],
            ..Default::default()
        };
        let chars: BTreeSet<char> = "Hi".chars().collect();
        let b = bake(&comb, &HashMap::new(), &chars);
        assert_eq!(b.font.height, 60);
        assert_eq!(b.font.width, 40);
        assert!(b.missing.contains(&'H') && b.missing.contains(&'?'));
        assert!(b.font.cmap.is_empty());
        assert_eq!(b.sheets.len(), 1);
    }
}
