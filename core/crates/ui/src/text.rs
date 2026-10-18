//! Text panes → glyph quads.
//!
//! Font scale: `font_size.x / font width`, `font_size.y / font height` (FINF). Each line advances
//! `line_feed · sy + line_space`; each character `advance · sx + char_space`. A glyph cell's top
//! sits `(ascent − baseline) · sy` below the line top. The text block (widest line × line count)
//! is placed in the pane by the text position (packed like a pane origin), and each line is
//! aligned inside the block by the line alignment (or the position's horizontal part).

use mmk_formats::lyt::Text;

use crate::font::FontRes;
use crate::player::Mtx;
use crate::*;

fn factor(code: u8) -> f32 {
    match code & 3 {
        1 => 0.0,
        2 => 1.0,
        _ => 0.5,
    }
}

/// Horizontal / vertical scale of `f` for a text pane's font size.
fn scale(f: &FontRes, size: [f32; 2]) -> (f32, f32) {
    (
        size[0] / f.font.width.max(1) as f32,
        size[1] / f.font.height.max(1) as f32,
    )
}

fn line_width(font: &FontRes, line: &str, size: [f32; 2], char_space: f32) -> f32 {
    let mut w = 0.0;
    let mut n = 0;
    for c in line.chars().filter(|c| !c.is_control()) {
        let f = font.for_char(c);
        w += f.glyph(c).width.char_width as f32 * scale(f, size).0;
        n += 1;
    }
    if n > 1 {
        w += char_space * (n - 1) as f32;
    }
    w
}

/// Width of the widest line of `s` (pane units).
pub(crate) fn text_width(font: &FontRes, s: &str, size: [f32; 2], char_space: f32) -> f32 {
    s.split('\n')
        .map(|l| line_width(font, l, size, char_space))
        .fold(0.0, f32::max)
}

/// Append the glyph quads of `s` laid out in the pane rectangle `rect` (local space).
#[allow(clippy::too_many_arguments)]
pub(crate) fn emit(
    font: &FontRes,
    t: &Text,
    s: &str,
    (l, top, w, h): (f32, f32, f32, f32),
    mtx: &Mtx,
    alpha: f32,
    black: [u8; 4],
    white: [u8; 4],
    blend: u32,
    out: &mut Vec<Quad>,
) {
    let f = &font.font;
    let (_, sy) = scale(font, t.font_size);
    let line_h = f.line_feed as f32 * sy + t.line_space;
    let lines: Vec<&str> = s.split('\n').collect();
    let widths: Vec<f32> = lines
        .iter()
        .map(|ln| line_width(font, ln, t.font_size, t.char_space))
        .collect();
    let block_w = widths.iter().copied().fold(0.0, f32::max);
    let block_h = lines.len() as f32 * line_h - t.line_space;
    let bx = l + (w - block_w) * factor(t.position);
    let by = top - (h - block_h) * factor(t.position >> 2);
    let line_factor = match t.line_alignment {
        1 => 0.0,
        2 => 0.5,
        3 => 1.0,
        _ => factor(t.position),
    };
    let fade = |c: [u8; 4]| [c[0], c[1], c[2], (c[3] as f32 * alpha).round() as u8];
    let flags = blend
        | FLAG_LINEAR
        | COMBINE_NONE << SHIFT_COMBINE_COLOR
        | COMBINE_NONE << SHIFT_COMBINE_ALPHA;
    // the line's baseline sits `ascent` below its top; each glyph cell hangs from the baseline
    let ascent = f.ascent as f32 * sy;
    // with the shadow flag, the shadow pass is drawn first, offset by the shadow offset
    let passes: &[bool] = if t.flags & 1 != 0 {
        &[true, false]
    } else {
        &[false]
    };
    for &shadow in passes {
        let (off, top_c, bottom_c) = if shadow {
            (t.shadow_offset, fade(t.shadow_top), fade(t.shadow_bottom))
        } else {
            ([0.0, 0.0], fade(t.top_color), fade(t.bottom_color))
        };
        for (i, line) in lines.iter().enumerate() {
            let mut pen = bx + (block_w - widths[i]) * line_factor + off[0];
            let line_top = by - i as f32 * line_h + off[1];
            for c in line.chars().filter(|c| !c.is_control()) {
                let gf = font.for_char(c);
                let (sx, gsy) = scale(gf, t.font_size);
                let g = gf.glyph(c);
                let x0 = pen + g.width.left as f32 * sx;
                let x1 = x0 + g.width.glyph_width as f32 * sx;
                let y0 = line_top - ascent + gf.font.baseline as f32 * gsy;
                let y1 = y0 - gf.font.cell_height as f32 * gsy;
                pen += g.width.char_width as f32 * sx + t.char_space;
                if c == ' ' || c == '\u{3000}' || g.width.glyph_width == 0 {
                    continue;
                }
                out.push(Quad {
                    pos: [
                        mtx.apply(x0, y0),
                        mtx.apply(x1, y0),
                        mtx.apply(x0, y1),
                        mtx.apply(x1, y1),
                    ],
                    uv0: [[g.u0, g.v0], [g.u1, g.v0], [g.u0, g.v1], [g.u1, g.v1]],
                    uv1: [[0.0; 2]; 4],
                    color: [top_c, top_c, bottom_c, bottom_c],
                    black,
                    white,
                    tex0: g.texture,
                    tex1: TEXTURE_NONE,
                    flags,
                });
            }
        }
    }
}
