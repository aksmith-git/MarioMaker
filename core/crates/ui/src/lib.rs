//! Layout player for the SMM1 presentation (100 Mario Challenge screens, menus).
//!
//! Loads layouts imported by `mmk_import::smm1` (JSON + `.mtex` textures + fonts + messages),
//! applies bound animations at a given frame and flattens the pane tree into a draw list of
//! textured [`Quad`]s (pictures, window frames and text glyphs). Evaluation is deterministic: the
//! same layout, animations and frames always give the same list.
//!
//! Coordinates: layout units (the layout is 1280×720 for full-screen SMM1 screens), origin at the
//! centre, y up. Texture coordinates: (0, 0) = top-left of the texture.

pub mod cache;
pub mod files;
pub mod font;
pub mod player;
mod text;

pub use cache::{ARG_PLACEHOLDER, LayoutRes, TexRef, TextureEntry, UiCache, UiError};
pub use files::{DirFiles, Fetch, MemFiles, UiFiles};
pub use font::FontRes;
pub use player::{LayoutPlayer, PaneGlobal, PaneInfo, PaneOverride};

/// `Quad::tex0/tex1` when there is no texture.
pub const TEXTURE_NONE: u32 = u32::MAX;

/// One textured quad. `#[repr(C)]` and shared with the C ABI (`MmkUiQuad` in mmk.h).
///
/// Shading (done by the renderer): `t = combine(sample(tex0, uv0), sample(tex1, uv1))` (a missing
/// texture samples as 1), `c = mix(black, white, t)` per channel, `out = c * color` (per-vertex
/// colour, pane alpha already folded into its alpha), blended with the quad's blend mode.
/// Colours are 8-bit sRGB-encoded values as authored.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Quad {
    /// Corners: top-left, top-right, bottom-left, bottom-right.
    pub pos: [[f32; 2]; 4],
    pub uv0: [[f32; 2]; 4],
    pub uv1: [[f32; 2]; 4],
    pub color: [[u8; 4]; 4],
    pub black: [u8; 4],
    pub white: [u8; 4],
    pub tex0: u32,
    pub tex1: u32,
    /// See the `FLAG_*` / `SHIFT_*` constants.
    pub flags: u32,
}

/// Wrap mode bits (2 each, 0 clamp, 1 repeat, 2 mirror): s0, t0, s1, t1.
pub const SHIFT_WRAP_S0: u32 = 0;
pub const SHIFT_WRAP_T0: u32 = 2;
pub const SHIFT_WRAP_S1: u32 = 4;
pub const SHIFT_WRAP_T1: u32 = 6;
/// Linear filtering (otherwise nearest).
pub const FLAG_LINEAR: u32 = 1 << 8;
/// tex0/tex1 colour combine mode (4 bits): 0 replace (tex1), 1 modulate, 2 add, 5 subtract,
/// 15 = no second texture (use tex0).
pub const SHIFT_COMBINE_COLOR: u32 = 12;
/// Alpha combine mode, same codes.
pub const SHIFT_COMBINE_ALPHA: u32 = 16;
/// Blend: op (0 off/overwrite, 1 add, 2 subtract, 3 reverse subtract, 4 min, 5 max), source and
/// destination factor (0 zero, 1 one, 2 dst colour, 3 1−dst colour, 4 src alpha, 5 1−src alpha,
/// 6 dst alpha, 7 1−dst alpha, 8 src colour, 9 1−src colour), 4 bits each.
pub const SHIFT_BLEND_OP: u32 = 20;
pub const SHIFT_BLEND_SRC: u32 = 24;
pub const SHIFT_BLEND_DST: u32 = 28;
/// The usual "source over" blend (add, src alpha, 1 − src alpha).
pub const BLEND_NORMAL: u32 =
    (1 << SHIFT_BLEND_OP) | (4 << SHIFT_BLEND_SRC) | (5 << SHIFT_BLEND_DST);
pub const COMBINE_NONE: u32 = 15;

#[cfg(test)]
mod tests {
    #[test]
    fn quad_layout() {
        assert_eq!(size_of::<super::Quad>(), 132);
        assert_eq!(align_of::<super::Quad>(), 4);
    }
}
