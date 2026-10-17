//! Wii U (GX2 / R7xx "AddrLib") surface tiling: element address of (x, y) in a tiled surface,
//! surface padding, and tile ↔ linear conversion for single-sample 2D textures (mip 0).
//!
//! Written from the public description of the R6xx/R7xx address library: surfaces are made of
//! 8×8-element *micro tiles* (64 elements, element bits interleaved by bits-per-element); the
//! macro-tiled modes group micro tiles into 32×16-element macro tiles spread over 2 pipes and
//! 4 banks, with a per-surface pipe/bank swizzle and a per-slice bank rotation. Block-compressed
//! formats are addressed in 4×4-pixel blocks (64 or 128 bits per element).
//!
//! Hardware constants (Wii U "Latte"): 2 pipes, 4 banks, 256-byte pipe interleave.

const PIPES: u32 = 2;
const BANKS: u32 = 4;
const PIPE_INTERLEAVE_BYTES: u32 = 256;
const GROUP_BITS: u32 = 8; // log2(pipe interleave bytes)
const PIPE_BITS: u32 = 1;
const BANK_BITS: u32 = 2;
const MICRO_TILE_PIXELS: u32 = 64;

/// GX2 tile modes (`GX2TileMode`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TileMode {
    LinearGeneral = 0,
    LinearAligned = 1,
    Tiled1DThin1 = 2,
    Tiled1DThick = 3,
    Tiled2DThin1 = 4,
    Tiled2DThin2 = 5,
    Tiled2DThin4 = 6,
    Tiled2DThick = 7,
    Tiled2BThin1 = 8,
    Tiled2BThin2 = 9,
    Tiled2BThin4 = 10,
    Tiled2BThick = 11,
    Tiled3DThin1 = 12,
    Tiled3DThick = 13,
    Tiled3BThin1 = 14,
    Tiled3BThick = 15,
}

impl TileMode {
    pub fn from_code(c: u32) -> Option<Self> {
        use TileMode::*;
        Some(match c {
            0 => LinearGeneral,
            1 => LinearAligned,
            2 => Tiled1DThin1,
            3 => Tiled1DThick,
            4 => Tiled2DThin1,
            5 => Tiled2DThin2,
            6 => Tiled2DThin4,
            7 => Tiled2DThick,
            8 => Tiled2BThin1,
            9 => Tiled2BThin2,
            10 => Tiled2BThin4,
            11 => Tiled2BThick,
            12 => Tiled3DThin1,
            13 => Tiled3DThick,
            14 => Tiled3BThin1,
            15 => Tiled3BThick,
            _ => return None,
        })
    }

    fn code(self) -> u32 {
        self as u32
    }

    pub fn is_linear(self) -> bool {
        self.code() <= 1
    }

    pub fn is_macro(self) -> bool {
        self.code() >= 4
    }

    fn thickness(self) -> u32 {
        match self {
            Self::Tiled1DThick
            | Self::Tiled2DThick
            | Self::Tiled2BThick
            | Self::Tiled3DThick
            | Self::Tiled3BThick => 4,
            _ => 1,
        }
    }

    fn is_bank_swapped(self) -> bool {
        matches!(
            self,
            Self::Tiled2BThin1
                | Self::Tiled2BThin2
                | Self::Tiled2BThin4
                | Self::Tiled2BThick
                | Self::Tiled3BThin1
                | Self::Tiled3BThick
        )
    }

    /// Per-slice bank/pipe rotation of the macro-tiled modes.
    fn rotation(self) -> u32 {
        match self.code() {
            4..=11 => PIPES * ((BANKS >> 1) - 1),
            12..=15 => 1,
            _ => 0,
        }
    }

    /// Macro-tile aspect ratio: THIN2 tiles are twice as tall, THIN4 four times.
    fn aspect_ratio(self) -> u32 {
        match self {
            Self::Tiled2DThin2 | Self::Tiled2BThin2 => 2,
            Self::Tiled2DThin4 | Self::Tiled2BThin4 => 4,
            _ => 1,
        }
    }
}

/// Swizzle word that addresses slice `slice` of a thin 2D array surface (stored slice by slice)
/// as if it were a single surface: the address library XORs each slice's bank/pipe with
/// `swizzle + slice · rotation`. BFFNT glyph sheets are such slices.
pub fn slice_swizzle(mode: TileMode, swizzle: u32, slice: u32) -> u32 {
    if !mode.is_macro() || mode.thickness() != 1 {
        return swizzle;
    }
    let base = ((swizzle >> 8) & 1) + PIPES * ((swizzle >> 9) & 3);
    let v = (base + slice * mode.rotation()) % (PIPES * BANKS);
    // v = pipe + PIPES·bank → pipe in bit 8, bank in bits 9-10
    v << 8
}

/// Padded size of mip level 0.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SurfaceInfo {
    /// Row pitch in elements.
    pub pitch: u32,
    /// Padded height in elements.
    pub height: u32,
    /// Bytes of the padded surface.
    pub size: usize,
}

/// Padding rules of mip level 0 (single sample, one slice) for `width × height` *elements*
/// (pixels, or 4×4 blocks for BCn) of `bpp` bits.
pub fn surface_info(width: u32, height: u32, bpp: u32, mode: TileMode) -> SurfaceInfo {
    let (pitch_align, height_align) = match mode {
        TileMode::LinearGeneral => (1, 1),
        TileMode::LinearAligned => ((2048 / bpp).max(64), 1),
        TileMode::Tiled1DThin1 | TileMode::Tiled1DThick => {
            let t = mode.thickness();
            ((PIPE_INTERLEAVE_BYTES / bpp / t).max(8), 8)
        }
        _ => {
            let a = mode.aspect_ratio();
            let t = mode.thickness();
            let macro_w = 8 * BANKS / a;
            let macro_h = 8 * PIPES * a;
            let pitch_align = macro_w.max(macro_w * (PIPE_INTERLEAVE_BYTES / bpp / (8 * t)));
            let pitch_align = if mode.is_bank_swapped() {
                pitch_align.max(bank_swapped_width(mode, bpp, width))
            } else {
                pitch_align
            };
            (pitch_align, macro_h)
        }
    };
    let pitch = width.max(1).div_ceil(pitch_align) * pitch_align;
    let height = height.max(1).div_ceil(height_align) * height_align;
    let size = (pitch as usize * height as usize * bpp as usize).div_ceil(8);
    SurfaceInfo {
        pitch,
        height,
        size,
    }
}

fn bank_swapped_width(mode: TileMode, bpp: u32, pitch: u32) -> u32 {
    if !mode.is_bank_swapped() {
        return 0;
    }
    let swap_size = 256u32;
    let row_size = 2048u32;
    let split_size = 2048u32;
    let micro_bytes = MICRO_TILE_PIXELS * mode.thickness() * bpp / 8;
    let factor = mode.aspect_ratio();
    let swap_tiles = (swap_size >> 1) / bpp;
    let swap_width = swap_tiles * 8 * BANKS;
    let height_bytes = (micro_bytes.min(split_size) / mode.thickness()) * 8 * PIPES * factor / 8;
    let swap_max = PIPES * BANKS * row_size / height_bytes.max(1);
    let swap_min = PIPE_INTERLEAVE_BYTES * 8 * BANKS / bpp;
    let mut w = swap_width.clamp(swap_min, swap_max.max(swap_min));
    while w >= 2 * pitch {
        w >>= 1;
    }
    w
}

/// Bit index of element (x, y) inside its 8×8 micro tile (thin modes).
fn pixel_index_in_micro_tile(x: u32, y: u32, bpp: u32) -> u32 {
    let (x0, x1, x2) = (x & 1, (x >> 1) & 1, (x >> 2) & 1);
    let (y0, y1, y2) = (y & 1, (y >> 1) & 1, (y >> 2) & 1);
    let bits = match bpp {
        8 => [x0, x1, x2, y1, y0, y2],
        16 => [x0, x1, x2, y0, y1, y2],
        64 => [x0, y0, x1, x2, y1, y2],
        128 => [y0, x0, x1, x2, y1, y2],
        _ => [x0, x1, y0, x2, y1, y2], // 32 bpp (and others)
    };
    bits.iter().enumerate().fold(0, |acc, (i, b)| acc | b << i)
}

fn pipe_from_coord(x: u32, y: u32) -> u32 {
    ((y >> 3) ^ (x >> 3)) & 1
}

fn bank_from_coord(x: u32, y: u32) -> u32 {
    let bit0 = ((y / (16 * PIPES)) ^ (x >> 3)) & 1;
    let bit1 = ((y / (8 * PIPES)) ^ (x >> 4)) & 1;
    bit0 | (bit1 << 1)
}

/// Byte address of element (x, y) in a micro-tiled (1D) surface.
fn addr_micro(x: u32, y: u32, bpp: u32, pitch: u32) -> usize {
    let micro_bytes = (MICRO_TILE_PIXELS * bpp / 8) as usize;
    let tiles_per_row = (pitch >> 3) as usize;
    let tile = (x >> 3) as usize + (y >> 3) as usize * tiles_per_row;
    let pixel = pixel_index_in_micro_tile(x, y, bpp) as usize;
    tile * micro_bytes + (pixel * bpp as usize) / 8
}

/// Byte address of element (x, y) in a macro-tiled (2D/2B/3D/3B, thin) surface.
fn addr_macro(x: u32, y: u32, bpp: u32, pitch: u32, mode: TileMode, swizzle: u32) -> usize {
    let pixel = pixel_index_in_micro_tile(x, y, bpp) as u64;
    let elem_offset = (pixel * bpp as u64) / 8;

    let pipe_swizzle = (swizzle >> 8) & 1;
    let bank_swizzle = (swizzle >> 9) & 3;
    let mut pipe = pipe_from_coord(x, y);
    let mut bank = bank_from_coord(x, y);
    let mut bank_pipe = pipe + PIPES * bank;
    let swz = pipe_swizzle + PIPES * bank_swizzle;
    // slice 0, sample 0: the per-slice bank rotation term is zero
    bank_pipe ^= swz;
    bank_pipe %= PIPES * BANKS;
    pipe = bank_pipe % PIPES;
    bank = bank_pipe / PIPES;

    let a = mode.aspect_ratio();
    let macro_pitch = 8 * BANKS / a;
    let macro_height = 8 * PIPES * a;
    let macro_tiles_per_row = (pitch / macro_pitch) as u64;
    let macro_bytes =
        (mode.thickness() as u64 * bpp as u64 * macro_height as u64 * macro_pitch as u64)
            .div_ceil(8);
    let mx = (x / macro_pitch) as u64;
    let my = (y / macro_height) as u64;
    let macro_offset = (mx + macro_tiles_per_row * my) * macro_bytes;

    if mode.is_bank_swapped() {
        const ORDER: [u32; 4] = [0, 1, 3, 2];
        let w = bank_swapped_width(mode, bpp, pitch).max(1) as u64;
        let swap_index = macro_pitch as u64 * mx / w;
        bank ^= ORDER[(swap_index & (BANKS as u64 - 1)) as usize];
    }

    let group_mask = (1u64 << GROUP_BITS) - 1;
    let total = elem_offset + (macro_offset >> (BANK_BITS + PIPE_BITS));
    let high = (total & !group_mask) << (BANK_BITS + PIPE_BITS);
    let low = total & group_mask;
    let bank_bits = (bank as u64) << (PIPE_BITS + GROUP_BITS);
    let pipe_bits = (pipe as u64) << GROUP_BITS;
    (bank_bits | pipe_bits | low | high) as usize
}

/// Byte address of element (x, y) of mip 0 / slice 0.
pub fn element_address(
    x: u32,
    y: u32,
    bpp: u32,
    pitch: u32,
    mode: TileMode,
    swizzle: u32,
) -> usize {
    match mode {
        TileMode::LinearGeneral | TileMode::LinearAligned => {
            (y as usize * pitch as usize + x as usize) * bpp as usize / 8
        }
        TileMode::Tiled1DThin1 | TileMode::Tiled1DThick => addr_micro(x, y, bpp, pitch),
        _ => addr_macro(x, y, bpp, pitch, mode, swizzle),
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum Gx2Error {
    #[error("tile mode {0} not supported")]
    TileMode(u32),
    #[error("bits per element {0} not supported")]
    Bpp(u32),
    #[error("tiled data is {got} bytes, surface needs {need}")]
    Size { got: usize, need: usize },
}

fn check(bpp: u32, mode: TileMode) -> Result<(), Gx2Error> {
    if !matches!(bpp, 8 | 16 | 32 | 64 | 128) {
        return Err(Gx2Error::Bpp(bpp));
    }
    if mode.thickness() != 1 {
        return Err(Gx2Error::TileMode(mode as u32));
    }
    Ok(())
}

/// Tiled surface → linear rows (`width × height` elements, tightly packed).
/// `swizzle` is the GX2 surface swizzle word (pipe swizzle in bit 8, bank swizzle in bits 9-10).
pub fn untile(
    data: &[u8],
    width: u32,
    height: u32,
    bpp: u32,
    mode: TileMode,
    swizzle: u32,
) -> Result<Vec<u8>, Gx2Error> {
    check(bpp, mode)?;
    let info = surface_info(width, height, bpp, mode);
    let eb = (bpp / 8) as usize;
    let mut out = vec![0u8; width as usize * height as usize * eb];
    for y in 0..height {
        for x in 0..width {
            let src = element_address(x, y, bpp, info.pitch, mode, swizzle);
            let dst = (y as usize * width as usize + x as usize) * eb;
            let s = data.get(src..src + eb).ok_or(Gx2Error::Size {
                got: data.len(),
                need: src + eb,
            })?;
            out[dst..dst + eb].copy_from_slice(s);
        }
    }
    Ok(out)
}

/// Linear rows → tiled surface of `surface_info(..).size` bytes (inverse of [`untile`]).
pub fn tile(
    linear: &[u8],
    width: u32,
    height: u32,
    bpp: u32,
    mode: TileMode,
    swizzle: u32,
) -> Result<Vec<u8>, Gx2Error> {
    check(bpp, mode)?;
    let info = surface_info(width, height, bpp, mode);
    let eb = (bpp / 8) as usize;
    let need = width as usize * height as usize * eb;
    if linear.len() < need {
        return Err(Gx2Error::Size {
            got: linear.len(),
            need,
        });
    }
    let mut out = vec![0u8; info.size];
    for y in 0..height {
        for x in 0..width {
            let dst = element_address(x, y, bpp, info.pitch, mode, swizzle);
            let src = (y as usize * width as usize + x as usize) * eb;
            out[dst..dst + eb].copy_from_slice(&linear[src..src + eb]);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pattern(n: usize) -> Vec<u8> {
        (0..n).map(|i| (i * 7 + i / 251) as u8).collect()
    }

    #[test]
    fn padding_rules() {
        // values that the Wii U layout textures use (checked against their stored sizes)
        let s = |w, h, bpp, m| surface_info(w, h, bpp, m).size;
        assert_eq!(s(8, 8, 8, TileMode::Tiled1DThin1), 256);
        assert_eq!(s(256, 256, 8, TileMode::Tiled2DThin1), 65536);
        assert_eq!(s(28, 20, 8, TileMode::Tiled2DThin1), 4096);
        assert_eq!(s(24, 50, 16, TileMode::Tiled2DThin1), 8192);
        assert_eq!(s(74, 74, 32, TileMode::Tiled2DThin1), 30720);
        assert_eq!(s(2, 2, 64, TileMode::Tiled1DThin1), 512);
        assert_eq!(s(43, 20, 64, TileMode::Tiled2DThin1), 16384);
        assert_eq!(s(13, 13, 128, TileMode::Tiled1DThin1), 4096);
        assert_eq!(s(45, 23, 128, TileMode::Tiled2DThin1), 32768);
    }

    #[test]
    fn micro_tile_index_is_a_permutation() {
        for bpp in [8, 16, 32, 64, 128] {
            let mut seen = [false; 64];
            for y in 0..8 {
                for x in 0..8 {
                    let i = pixel_index_in_micro_tile(x, y, bpp) as usize;
                    assert!(!seen[i], "bpp {bpp}: duplicate index {i}");
                    seen[i] = true;
                }
            }
        }
    }

    #[test]
    fn addresses_are_unique_and_in_bounds() {
        for (mode, swz) in [
            (TileMode::Tiled1DThin1, 0),
            (TileMode::Tiled2DThin1, 0),
            (TileMode::Tiled2DThin1, 5 << 8),
            (TileMode::Tiled2DThin2, 0),
            (TileMode::Tiled2DThin4, 3 << 8),
            (TileMode::Tiled2BThin1, 0),
        ] {
            for bpp in [8, 16, 32, 64, 128] {
                let (w, h) = (70, 40);
                let info = surface_info(w, h, bpp, mode);
                let eb = (bpp / 8) as usize;
                let mut used = vec![false; info.size / eb];
                for y in 0..info.height {
                    for x in 0..info.pitch {
                        let a = element_address(x, y, bpp, info.pitch, mode, swz);
                        assert_eq!(a % eb, 0);
                        assert!(a < info.size, "{mode:?} bpp {bpp}: ({x},{y}) → {a:#x}");
                        assert!(!used[a / eb], "{mode:?} bpp {bpp}: ({x},{y}) collides");
                        used[a / eb] = true;
                    }
                }
            }
        }
    }

    #[test]
    fn tile_then_untile_round_trips() {
        for mode in [
            TileMode::LinearGeneral,
            TileMode::LinearAligned,
            TileMode::Tiled1DThin1,
            TileMode::Tiled2DThin1,
            TileMode::Tiled2BThin1,
        ] {
            for bpp in [8, 16, 32, 64, 128] {
                for swz in [0, 1 << 8, 6 << 8] {
                    let (w, h) = (45, 23);
                    let lin = pattern((w * h * bpp / 8) as usize);
                    let tiled = tile(&lin, w, h, bpp, mode, swz).unwrap();
                    assert_eq!(tiled.len(), surface_info(w, h, bpp, mode).size);
                    assert_eq!(untile(&tiled, w, h, bpp, mode, swz).unwrap(), lin);
                }
            }
        }
    }

    #[test]
    fn macro_tiling_interleaves_pipes_every_256_bytes() {
        // 32 bpp: one micro tile (8×8 × 4 B) = 256 B = one pipe-interleave group. The eight
        // micro tiles of a 32×16 macro tile land on the eight (pipe, bank) slots: pipe =
        // bit 3 of x ^ y, bank bits from x/8, x/16, y/16, y/32.
        let info = surface_info(64, 32, 32, TileMode::Tiled2DThin1);
        let a = |x, y| element_address(x, y, 32, info.pitch, TileMode::Tiled2DThin1, 0);
        assert_eq!(a(0, 0), 0);
        assert_eq!(a(1, 0), 4);
        assert_eq!(a(8, 0), 512 + 256); // bank 1, pipe 1
        assert_eq!(a(0, 8), 256); // bank 0, pipe 1
        assert_eq!(a(16, 0), 1024); // bank 2, pipe 0
        // a swizzle only permutes pipes/banks
        let b = element_address(0, 0, 32, info.pitch, TileMode::Tiled2DThin1, 1 << 8);
        assert_eq!(b, 256);
    }

    #[test]
    fn slice_rotation() {
        // 2D thin: rotation 2 per slice = +1 bank per slice
        assert_eq!(slice_swizzle(TileMode::Tiled2DThin1, 0, 0), 0);
        assert_eq!(slice_swizzle(TileMode::Tiled2DThin1, 0, 1), 1 << 9);
        assert_eq!(slice_swizzle(TileMode::Tiled2DThin1, 0, 3), 3 << 9);
        assert_eq!(slice_swizzle(TileMode::Tiled2DThin1, 0, 4), 0);
        assert_eq!(
            slice_swizzle(TileMode::Tiled2DThin1, 1 << 8, 1),
            (1 << 8) | (1 << 9)
        );
        assert_eq!(slice_swizzle(TileMode::Tiled1DThin1, 0, 5), 0);
    }

    #[test]
    fn errors() {
        assert_eq!(
            untile(&[], 4, 4, 24, TileMode::Tiled1DThin1, 0),
            Err(Gx2Error::Bpp(24))
        );
        assert!(untile(&[0; 4], 8, 8, 32, TileMode::Tiled2DThin1, 0).is_err());
        assert_eq!(
            tile(&[0; 4], 8, 8, 32, TileMode::Tiled1DThick, 0),
            Err(Gx2Error::TileMode(3))
        );
    }
}
