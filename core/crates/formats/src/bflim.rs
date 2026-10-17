//! BFLIM: one layout texture (Wii U). The tiled GX2 image comes first; a 0x28-byte footer ends
//! the file:
//!
//! ```text
//! "FLIM" bom u16 hdr_size(0x14) u32 version u32 file_size u16 sections u16 pad
//! "imag" u32 size(0x10) u16 width u16 height u16 alignment u8 format u8 tile_swizzle u32 data_size
//! ```
//! `tile_swizzle`: bits 0-4 GX2 tile mode, bits 5-7 the surface swizzle (GX2 swizzle bits 8-10).
//!
//! Decoding untiles the surface and returns either RGBA8 pixels or BCn blocks (kept compressed:
//! Apple GPUs sample BC1-BC5 natively), plus the channel mapping the format implies
//! (luminance / alpha formats replicate one channel).

use crate::error::{Error, Result};
use crate::gx2::{self, TileMode};

pub const FOOTER_SIZE: usize = 0x28;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Flim {
    pub width: u32,
    pub height: u32,
    pub alignment: u32,
    /// BFLIM (Cafe) format code, see [`FlimFormat`].
    pub format: u8,
    pub tile_mode: u32,
    /// GX2 surface swizzle word (already shifted to bits 8-10).
    pub swizzle: u32,
    /// Tiled image data.
    pub data: Vec<u8>,
}

/// BFLIM format codes (Wii U).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlimFormat {
    L8,
    A8,
    La4,
    La8,
    Rgb565,
    Rgb5a1,
    Rgba4,
    Rgba8,
    Bc1,
    Bc2,
    Bc3,
    /// BC4 sampled as luminance (R → RGB, alpha 1).
    Bc4L,
    /// BC4 sampled as alpha (RGB 1, R → alpha).
    Bc4A,
    /// BC5 sampled as luminance + alpha.
    Bc5La,
    Rgba8Srgb,
    Bc1Srgb,
    Bc2Srgb,
    Bc3Srgb,
    Rgb565Indirect,
}

impl FlimFormat {
    pub fn from_code(c: u8) -> Option<Self> {
        use FlimFormat::*;
        Some(match c {
            0 => L8,
            1 => A8,
            2 => La4,
            3 => La8,
            5 => Rgb565,
            7 => Rgb5a1,
            8 => Rgba4,
            9 => Rgba8,
            12 => Bc1,
            13 => Bc2,
            14 => Bc3,
            15 => Bc4L,
            16 => Bc4A,
            17 => Bc5La,
            20 => Rgba8Srgb,
            21 => Bc1Srgb,
            22 => Bc2Srgb,
            23 => Bc3Srgb,
            25 => Rgb565Indirect,
            _ => return None,
        })
    }

    /// Bits per GX2 element (a pixel, or a 4×4 block for BCn).
    pub fn element_bits(self) -> u32 {
        use FlimFormat::*;
        match self {
            L8 | A8 | La4 => 8,
            La8 | Rgb565 | Rgb5a1 | Rgba4 | Rgb565Indirect => 16,
            Rgba8 | Rgba8Srgb => 32,
            Bc1 | Bc1Srgb | Bc4L | Bc4A => 64,
            Bc2 | Bc3 | Bc2Srgb | Bc3Srgb | Bc5La => 128,
        }
    }

    pub fn is_block(self) -> bool {
        self.element_bits() >= 64
    }

    pub fn is_srgb(self) -> bool {
        matches!(
            self,
            Self::Rgba8Srgb | Self::Bc1Srgb | Self::Bc2Srgb | Self::Bc3Srgb
        )
    }
}

/// Pixel storage of a decoded texture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PixelKind {
    Rgba8,
    Bc1,
    Bc2,
    Bc3,
    Bc4,
    Bc5,
}

/// A decoded texture: linear rows (pixels, or 4×4 blocks for BCn), top row first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub kind: PixelKind,
    pub srgb: bool,
    /// Where each output channel R, G, B, A comes from: 0 zero, 1 one, 2 R, 3 G, 4 B, 5 A
    /// (the `.mtex` convention).
    pub channel_select: [u8; 4],
    pub data: Vec<u8>,
}

pub const IDENTITY: [u8; 4] = [2, 3, 4, 5];

/// Reverse the 4 pixel rows of a BC4 index block (8 bytes: 2 endpoints + 16 × 3-bit indices).
fn flip_bc4(b: &mut [u8]) {
    let mut bits = 0u64;
    for (i, &x) in b[2..8].iter().enumerate() {
        bits |= (x as u64) << (8 * i);
    }
    let row = |r: u32| (bits >> (12 * r)) & 0xFFF;
    let flipped = row(3) | row(2) << 12 | row(1) << 24 | row(0) << 36;
    for (i, x) in b[2..8].iter_mut().enumerate() {
        *x = (flipped >> (8 * i)) as u8;
    }
}

/// Reverse the 4 pixel rows of a BC1 colour block (indices: one byte per row).
fn flip_bc1(b: &mut [u8]) {
    b[4..8].reverse();
}

impl Image {
    /// Mirror the image top ↔ bottom (pixel rows, or block rows plus the rows inside each
    /// block for BCn). Used for BFFNT sheets, which are stored bottom row first.
    pub fn flip_vertical(&mut self) {
        let block = matches!(
            self.kind,
            PixelKind::Bc1 | PixelKind::Bc2 | PixelKind::Bc3 | PixelKind::Bc4 | PixelKind::Bc5
        );
        let (cols, rows, elem) = if block {
            let eb = match self.kind {
                PixelKind::Bc1 | PixelKind::Bc4 => 8,
                _ => 16,
            };
            (
                self.width.div_ceil(4) as usize,
                self.height.div_ceil(4) as usize,
                eb,
            )
        } else {
            (self.width as usize, self.height as usize, 4)
        };
        let stride = cols * elem;
        let mut out = Vec::with_capacity(self.data.len());
        for r in (0..rows).rev() {
            out.extend_from_slice(&self.data[r * stride..(r + 1) * stride]);
        }
        if block {
            for b in out.chunks_exact_mut(elem) {
                match self.kind {
                    PixelKind::Bc1 => flip_bc1(b),
                    PixelKind::Bc4 => flip_bc4(b),
                    PixelKind::Bc5 => {
                        flip_bc4(&mut b[..8]);
                        flip_bc4(&mut b[8..]);
                    }
                    PixelKind::Bc3 => {
                        flip_bc4(&mut b[..8]);
                        flip_bc1(&mut b[8..]);
                    }
                    PixelKind::Bc2 => {
                        // explicit alpha: one u16 per row
                        let a: Vec<u8> = b[..8].to_vec();
                        for r in 0..4 {
                            b[r * 2..r * 2 + 2].copy_from_slice(&a[(3 - r) * 2..(3 - r) * 2 + 2]);
                        }
                        flip_bc1(&mut b[8..]);
                    }
                    PixelKind::Rgba8 => {}
                }
            }
        }
        self.data = out;
    }
}

fn bad(detail: impl Into<String>) -> Error {
    Error::Format {
        what: "BFLIM",
        detail: detail.into(),
    }
}

impl Flim {
    pub fn parse(file: &[u8]) -> Result<Self> {
        if file.len() < FOOTER_SIZE {
            return Err(Error::Magic { what: "BFLIM" });
        }
        let f = &file[file.len() - FOOTER_SIZE..];
        if &f[..4] != b"FLIM" || &f[0x14..0x18] != b"imag" {
            return Err(Error::Magic { what: "BFLIM" });
        }
        let be = f[4..6] == [0xFE, 0xFF];
        let r = crate::lyt::Reader::new(f, be, "BFLIM");
        let tile = r.u8(0x23)?;
        let size = r.u32(0x24)? as usize;
        if size > file.len() - FOOTER_SIZE {
            return Err(bad(format!("image size {size:#x} exceeds file")));
        }
        Ok(Self {
            width: r.u16(0x1C)? as u32,
            height: r.u16(0x1E)? as u32,
            alignment: r.u16(0x20)? as u32,
            format: r.u8(0x22)?,
            tile_mode: (tile & 0x1F) as u32,
            swizzle: ((tile >> 5) as u32) << 8,
            data: file[..size].to_vec(),
        })
    }

    pub fn decode(&self) -> Result<Image> {
        let fmt = FlimFormat::from_code(self.format)
            .ok_or_else(|| bad(format!("format {} not supported", self.format)))?;
        decode_surface(
            &self.data,
            self.width,
            self.height,
            fmt,
            self.tile_mode,
            self.swizzle,
        )
    }
}

/// Untile a GX2 surface of `fmt` and convert it to an [`Image`]. Shared with the font sheets.
pub fn decode_surface(
    data: &[u8],
    width: u32,
    height: u32,
    fmt: FlimFormat,
    tile_mode: u32,
    swizzle: u32,
) -> Result<Image> {
    let mode = TileMode::from_code(tile_mode)
        .ok_or_else(|| bad(format!("tile mode {tile_mode} not supported")))?;
    let (ew, eh) = if fmt.is_block() {
        (width.div_ceil(4), height.div_ceil(4))
    } else {
        (width, height)
    };
    let lin = gx2::untile(data, ew, eh, fmt.element_bits(), mode, swizzle)
        .map_err(|e| bad(e.to_string()))?;
    use FlimFormat::*;
    let block = |kind, sel| Image {
        width,
        height,
        kind,
        srgb: fmt.is_srgb(),
        channel_select: sel,
        data: lin.clone(),
    };
    let rgba = |px: Vec<u8>| Image {
        width,
        height,
        kind: PixelKind::Rgba8,
        srgb: fmt.is_srgb(),
        channel_select: IDENTITY,
        data: px,
    };
    let expand5 = |v: u16| ((v << 3) | (v >> 2)) as u8;
    let expand6 = |v: u16| ((v << 2) | (v >> 4)) as u8;
    let expand4 = |v: u16| (v * 17) as u8;
    Ok(match fmt {
        Bc1 | Bc1Srgb => block(PixelKind::Bc1, IDENTITY),
        Bc2 | Bc2Srgb => block(PixelKind::Bc2, IDENTITY),
        Bc3 | Bc3Srgb => block(PixelKind::Bc3, IDENTITY),
        Bc4L => block(PixelKind::Bc4, [2, 2, 2, 1]),
        Bc4A => block(PixelKind::Bc4, [1, 1, 1, 2]),
        Bc5La => block(PixelKind::Bc5, [2, 2, 2, 3]),
        Rgba8 | Rgba8Srgb => rgba(lin),
        L8 => rgba(lin.iter().flat_map(|&l| [l, l, l, 255]).collect()),
        A8 => rgba(lin.iter().flat_map(|&a| [255, 255, 255, a]).collect()),
        La4 => rgba(
            lin.iter()
                .flat_map(|&b| {
                    let (l, a) = ((b >> 4) * 17, (b & 15) * 17);
                    [l, l, l, a]
                })
                .collect(),
        ),
        La8 => rgba(
            lin.as_chunks::<2>()
                .0
                .iter()
                .flat_map(|&[l, a]| [l, l, l, a])
                .collect(),
        ),
        Rgb565 | Rgb565Indirect => rgba(
            lin.as_chunks::<2>()
                .0
                .iter()
                .flat_map(|&c| {
                    let v = u16::from_le_bytes(c);
                    [
                        expand5(v & 31),
                        expand6((v >> 5) & 63),
                        expand5(v >> 11),
                        255,
                    ]
                })
                .collect(),
        ),
        Rgb5a1 => rgba(
            lin.as_chunks::<2>()
                .0
                .iter()
                .flat_map(|&c| {
                    let v = u16::from_le_bytes(c);
                    [
                        expand5(v & 31),
                        expand5((v >> 5) & 31),
                        expand5((v >> 10) & 31),
                        if v >> 15 != 0 { 255 } else { 0 },
                    ]
                })
                .collect(),
        ),
        Rgba4 => rgba(
            lin.as_chunks::<2>()
                .0
                .iter()
                .flat_map(|&c| {
                    let v = u16::from_le_bytes(c);
                    [
                        expand4(v & 15),
                        expand4((v >> 4) & 15),
                        expand4((v >> 8) & 15),
                        expand4(v >> 12),
                    ]
                })
                .collect(),
        ),
    })
}

/// Build a BFLIM file (synthetic fixtures and tests): tiles `linear` and appends the footer.
pub fn build(
    linear: &[u8],
    width: u32,
    height: u32,
    format: u8,
    tile_mode: u32,
    swizzle_bits: u8,
) -> Vec<u8> {
    let fmt = FlimFormat::from_code(format).expect("known format");
    let (ew, eh) = if fmt.is_block() {
        (width.div_ceil(4), height.div_ceil(4))
    } else {
        (width, height)
    };
    let mode = TileMode::from_code(tile_mode).expect("tile mode");
    let mut out = gx2::tile(
        linear,
        ew,
        eh,
        fmt.element_bits(),
        mode,
        (swizzle_bits as u32) << 8,
    )
    .expect("tile");
    let size = out.len() as u32;
    let total = out.len() + FOOTER_SIZE;
    out.extend_from_slice(b"FLIM\xFE\xFF\x00\x14\x02\x02\x00\x00");
    out.extend_from_slice(&(total as u32).to_be_bytes());
    out.extend_from_slice(&[0, 1, 0, 0]);
    out.extend_from_slice(b"imag");
    out.extend_from_slice(&0x10u32.to_be_bytes());
    out.extend_from_slice(&(width as u16).to_be_bytes());
    out.extend_from_slice(&(height as u16).to_be_bytes());
    out.extend_from_slice(&0x2000u16.to_be_bytes());
    out.push(format);
    out.push((tile_mode as u8) | (swizzle_bits << 5));
    out.extend_from_slice(&size.to_be_bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgba8_round_trip_through_tiling() {
        let (w, h) = (40u32, 20u32);
        let px: Vec<u8> = (0..w * h * 4).map(|i| (i % 253) as u8).collect();
        for (mode, swz) in [(4, 5), (2, 0), (4, 0), (1, 0)] {
            let file = build(&px, w, h, 9, mode, swz);
            let f = Flim::parse(&file).unwrap();
            assert_eq!((f.width, f.height, f.tile_mode), (w, h, mode));
            let img = f.decode().unwrap();
            assert_eq!(img.kind, PixelKind::Rgba8);
            assert_eq!(img.data, px);
        }
    }

    #[test]
    fn luminance_alpha_expand() {
        let (w, h) = (8, 8);
        let la: Vec<u8> = (0..w * h).flat_map(|i| [i as u8, 255 - i as u8]).collect();
        let img = Flim::parse(&build(&la, w, h, 3, 4, 0))
            .unwrap()
            .decode()
            .unwrap();
        assert_eq!(&img.data[4..8], &[1, 1, 1, 254]);
        let a8: Vec<u8> = (0..w * h).map(|i| i as u8).collect();
        let img = Flim::parse(&build(&a8, w, h, 1, 2, 0))
            .unwrap()
            .decode()
            .unwrap();
        assert_eq!(&img.data[8..12], &[255, 255, 255, 2]);
    }

    #[test]
    fn block_formats_keep_blocks_and_map_channels() {
        let (w, h) = (30u32, 18u32); // 8 × 5 blocks
        let blocks: Vec<u8> = (0..8 * 5 * 8).map(|i| i as u8).collect();
        let img = Flim::parse(&build(&blocks, w, h, 16, 4, 3))
            .unwrap()
            .decode()
            .unwrap();
        assert_eq!(img.kind, PixelKind::Bc4);
        assert_eq!(img.channel_select, [1, 1, 1, 2]);
        assert_eq!(img.data, blocks);
        let b5: Vec<u8> = (0..8 * 5 * 16).map(|i| (i * 3) as u8).collect();
        let img = Flim::parse(&build(&b5, w, h, 17, 2, 0))
            .unwrap()
            .decode()
            .unwrap();
        assert_eq!(
            (img.kind, img.channel_select),
            (PixelKind::Bc5, [2, 2, 2, 3])
        );
        assert_eq!(img.data, b5);
        let srgb = Flim::parse(&build(&b5, w, h, 23, 4, 0)).unwrap();
        assert!(srgb.decode().unwrap().srgb);
    }

    #[test]
    fn vertical_flip_matches_decoded_flip() {
        let (w, h) = (8u32, 12u32);
        for (kind, code, eb) in [
            (PixelKind::Bc1, 1u8, 8usize),
            (PixelKind::Bc2, 2, 16),
            (PixelKind::Bc3, 3, 16),
            (PixelKind::Bc4, 4, 8),
            (PixelKind::Bc5, 5, 16),
        ] {
            let n = (w as usize / 4) * (h as usize / 4) * eb;
            let data: Vec<u8> = (0..n).map(|i| (i * 37 + 11) as u8).collect();
            let mut img = Image {
                width: w,
                height: h,
                kind,
                srgb: false,
                channel_select: IDENTITY,
                data: data.clone(),
            };
            let before = crate::bcn::decode(code, &img.data, w, h);
            img.flip_vertical();
            let after = crate::bcn::decode(code, &img.data, w, h);
            let row = (w * 4) as usize;
            let mirrored: Vec<u8> = before.chunks_exact(row).rev().flatten().copied().collect();
            assert_eq!(after, mirrored, "{kind:?}");
            img.flip_vertical();
            assert_eq!(img.data, data);
        }
        let mut rgba = Image {
            width: 1,
            height: 2,
            kind: PixelKind::Rgba8,
            srgb: false,
            channel_select: IDENTITY,
            data: vec![1, 2, 3, 4, 5, 6, 7, 8],
        };
        rgba.flip_vertical();
        assert_eq!(rgba.data, [5, 6, 7, 8, 1, 2, 3, 4]);
    }

    #[test]
    fn rejects_bad_files() {
        assert!(Flim::parse(b"short").is_err());
        let mut f = build(&[0; 256], 8, 8, 9, 2, 0);
        let n = f.len();
        f[n - 6] = 99; // unknown format
        assert!(Flim::parse(&f).unwrap().decode().is_err());
    }
}
