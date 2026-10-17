//! BNTX texture containers (Switch) and Tegra X1 block-linear unswizzling.
//!
//! BFRES model files embed their textures as a BNTX; `find_embedded` locates it. Pixel data is
//! returned linear (row-major, top row first), still in its original format (RGBA8 or BCn), which
//! Apple Silicon GPUs sample directly. The few uncompressed formats the engine has no GPU path
//! for (R8, R8G8, R5G5B5A1: SMM2 UI textures) are expanded to RGBA8 here, with the texture's
//! channel select kept, so they sample the way the GPU would have sampled the original.
//!
//! Array textures (Switch font sheets) keep every layer's mip 0 in [`Texture::layers`].

use crate::error::{Error, Result};

fn bad(detail: impl Into<String>) -> Error {
    Error::Format {
        what: "BNTX",
        detail: detail.into(),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PixelFormat {
    Rgba8,
    Bc1,
    Bc2,
    Bc3,
    Bc4,
    Bc5,
    Bc6h,
    Bc7,
    Other(u8),
}

impl PixelFormat {
    fn from_type(t: u8) -> Self {
        match t {
            0x0B => Self::Rgba8,
            0x1A => Self::Bc1,
            0x1B => Self::Bc2,
            0x1C => Self::Bc3,
            0x1D => Self::Bc4,
            0x1E => Self::Bc5,
            0x1F => Self::Bc6h,
            0x20 => Self::Bc7,
            t => Self::Other(t),
        }
    }
    /// (bytes per element, element width/height in pixels)
    pub fn element(self) -> Option<(usize, usize)> {
        Some(match self {
            Self::Rgba8 => (4, 1),
            Self::Bc1 | Self::Bc4 => (8, 4),
            Self::Bc2 | Self::Bc3 | Self::Bc5 | Self::Bc6h | Self::Bc7 => (16, 4),
            Self::Other(_) => return None,
        })
    }
}

/// Number interpretation of the format (low byte of the BNTX format word).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Variant {
    Unorm,
    Snorm,
    Srgb,
    Other(u8),
}

#[derive(Clone, Debug)]
pub struct Texture {
    pub name: String,
    pub width: usize,
    pub height: usize,
    pub mip_count: usize,
    pub format: PixelFormat,
    pub variant: Variant,
    /// Channel selectors as stored (one byte per output channel R,G,B,A: 0 = zero, 1 = one,
    /// 2 = red, 3 = green, 4 = blue, 5 = alpha).
    pub channel_select: [u8; 4],
    /// Mip 0, unswizzled to linear element rows (layer 0 of an array texture).
    pub data: Vec<u8>,
    /// Mip 0 of every array layer (layer 0 included) when the texture has more than one.
    pub layers: Vec<Vec<u8>>,
}

/// Uncompressed formats that are expanded to RGBA8 on load: format type → bytes per pixel.
fn expanded_bpp(t: u8) -> Option<usize> {
    match t {
        0x02 => Some(1), // R8
        0x05 => Some(2), // R5G5B5A1
        0x09 => Some(2), // R8G8
        _ => None,
    }
}

/// Pixels of an expanded format → RGBA8 (missing channels read as 0 and alpha as 1, as the GPU
/// samples them).
pub fn expand_to_rgba8(t: u8, src: &[u8]) -> Vec<u8> {
    match t {
        0x02 => src.iter().flat_map(|&r| [r, 0, 0, 255]).collect(),
        0x09 => src
            .as_chunks::<2>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[1], 0, 255])
            .collect(),
        _ => src
            .as_chunks::<2>()
            .0
            .iter()
            .flat_map(|p| {
                // R5G5B5A1, red in the low bits [inf: one SMM2 texture, not checked on screen]
                let v = u16::from_le_bytes(*p) as u32;
                let c = |s: u32| (((v >> s) & 31) * 255 / 31) as u8;
                [c(0), c(5), c(10), if v & 0x8000 != 0 { 255 } else { 0 }]
            })
            .collect(),
    }
}

struct R<'a>(&'a [u8]);
impl R<'_> {
    fn u16(&self, o: usize) -> Result<u16> {
        Ok(u16::from_le_bytes(
            self.0
                .get(o..o + 2)
                .ok_or_else(|| bad("truncated"))?
                .try_into()
                .unwrap(),
        ))
    }
    fn u32(&self, o: usize) -> Result<u32> {
        Ok(u32::from_le_bytes(
            self.0
                .get(o..o + 4)
                .ok_or_else(|| bad("truncated"))?
                .try_into()
                .unwrap(),
        ))
    }
    fn u64(&self, o: usize) -> Result<usize> {
        Ok(u64::from_le_bytes(
            self.0
                .get(o..o + 8)
                .ok_or_else(|| bad("truncated"))?
                .try_into()
                .unwrap(),
        ) as usize)
    }
    fn name(&self, o: usize) -> Result<String> {
        let len = self.u16(o)? as usize;
        let raw = self
            .0
            .get(o + 2..o + 2 + len)
            .ok_or_else(|| bad("name outside file"))?;
        Ok(String::from_utf8_lossy(raw).into_owned())
    }
}

/// Byte range of a BNTX embedded in a larger file (e.g. a BFRES).
pub fn find_embedded(data: &[u8]) -> Option<&[u8]> {
    let start = data.windows(4).position(|w| w == b"BNTX")?;
    let size = u32::from_le_bytes(data.get(start + 0x1C..start + 0x20)?.try_into().ok()?) as usize;
    data.get(start..start + size)
}

pub fn parse(bntx: &[u8]) -> Result<Vec<Texture>> {
    let r = R(bntx);
    if bntx.get(..4) != Some(b"BNTX") || bntx.get(0x20..0x24) != Some(b"NX  ") {
        return Err(bad("missing BNTX/NX magic"));
    }
    let count = r.u32(0x24)? as usize;
    let info_ptrs = r.u64(0x28)?;
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let t = r.u64(info_ptrs + i * 8)?;
        if bntx.get(t..t + 4) != Some(b"BRTI") {
            return Err(bad("missing BRTI"));
        }
        let mip_count = r.u16(t + 0x16)? as usize;
        let fmt = r.u32(t + 0x1C)?;
        let width = r.u32(t + 0x24)? as usize;
        let height = r.u32(t + 0x28)? as usize;
        let layer_count = (r.u32(t + 0x30)? as usize).max(1);
        let block_height_log2 = (r.u32(t + 0x34)? & 7) as usize;
        let image_size = r.u32(t + 0x50)? as usize;
        let channel_select = r.u32(t + 0x58)?.to_le_bytes();
        let name = r.name(r.u64(t + 0x60)?)?;
        let mip0 = r.u64(r.u64(t + 0x70)?)?;
        let type_code = (fmt >> 8) as u8;
        let expand_bpp = expanded_bpp(type_code);
        let mut format = PixelFormat::from_type(type_code);
        let variant = match fmt & 0xFF {
            1 => Variant::Unorm,
            2 => Variant::Snorm,
            6 => Variant::Srgb,
            v => Variant::Other(v as u8),
        };
        // formats the engine doesn't draw (e.g. two-channel normal maps, 0x902) are skipped
        let Some((bpe, ew)) = format.element().or(expand_bpp.map(|b| (b, 1))) else {
            continue;
        };
        let (w_el, h_el) = (width.div_ceil(ew), height.div_ceil(ew));
        // array layers follow each other, each with its whole mip chain
        let stride = image_size / layer_count;
        let mut layers = Vec::with_capacity(layer_count);
        for layer in 0..layer_count {
            let start = (mip0 + layer * stride).min(bntx.len());
            let px = unswizzle(&bntx[start..], w_el, h_el, bpe, 1 << block_height_log2)
                .ok_or_else(|| bad(format!("{name}: image data truncated")))?;
            layers.push(match expand_bpp {
                Some(_) => expand_to_rgba8(type_code, &px),
                None => px,
            });
        }
        if expand_bpp.is_some() {
            format = PixelFormat::Rgba8;
        }
        let data = layers[0].clone();
        out.push(Texture {
            name,
            width,
            height,
            mip_count,
            format,
            variant,
            channel_select,
            data,
            layers: if layer_count > 1 { layers } else { Vec::new() },
        });
    }
    Ok(out)
}

/// Tegra X1 block-linear → linear. `width`/`height` in elements (pixels or 4×4 blocks),
/// `bpe` bytes per element, `block_height` in GOBs (power of two).
pub fn unswizzle(
    src: &[u8],
    width: usize,
    height: usize,
    bpe: usize,
    block_height: usize,
) -> Option<Vec<u8>> {
    let mut out = vec![0u8; width * height * bpe];
    let gobs_per_row = (width * bpe).div_ceil(64);
    for y in 0..height {
        for x in 0..width {
            let xb = x * bpe;
            let gob = (y / (8 * block_height)) * 512 * block_height * gobs_per_row
                + (xb / 64) * 512 * block_height
                + (y % (8 * block_height) / 8) * 512;
            let addr = gob
                + (xb % 64) / 32 * 256
                + (y % 8) / 2 * 64
                + (xb % 32) / 16 * 32
                + (y % 2) * 16
                + xb % 16;
            let dst = (y * width + x) * bpe;
            out[dst..dst + bpe].copy_from_slice(src.get(addr..addr + bpe)?);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_synthetic_textures() {
        use crate::fixtures::sample_bntx;
        // RGBA8 sRGB, 20×10 (not a whole GOB), two array layers
        let l0: Vec<u8> = (0..20 * 10 * 4).map(|i| (i % 253) as u8).collect();
        let l1: Vec<u8> = l0.iter().map(|v| v ^ 0xFF).collect();
        let b = sample_bntx(
            "Tex^u",
            0x0B06,
            20,
            10,
            4,
            [2, 3, 4, 5],
            &[l0.clone(), l1.clone()],
        );
        let t = &parse(&b).unwrap()[0];
        assert_eq!((t.name.as_str(), t.width, t.height), ("Tex^u", 20, 10));
        assert_eq!((t.format, t.variant), (PixelFormat::Rgba8, Variant::Srgb));
        assert_eq!(t.data, l0);
        assert_eq!(t.layers, [l0, l1]);
        assert_eq!(
            find_embedded(&[b"pad!".as_slice(), &b].concat()).unwrap(),
            &b[..]
        );
    }

    #[test]
    fn expands_r8_rg8_and_rgb5a1() {
        use crate::fixtures::sample_bntx;
        let b = sample_bntx(
            "Mask^r",
            0x0201,
            4,
            2,
            1,
            [1, 1, 1, 2],
            &[vec![0, 10, 20, 30, 40, 50, 60, 70]],
        );
        let t = &parse(&b).unwrap()[0];
        assert_eq!(t.format, PixelFormat::Rgba8);
        assert_eq!(&t.data[4..8], &[10, 0, 0, 255]);
        assert_eq!(t.channel_select, [1, 1, 1, 2]);
        assert!(t.layers.is_empty());
        assert_eq!(expand_to_rgba8(0x09, &[1, 2]), [1, 2, 0, 255]);
        // R5G5B5A1: red 31, green 0, blue 16, alpha 1
        let v: u16 = 31 | (16 << 10) | 0x8000;
        assert_eq!(expand_to_rgba8(0x05, &v.to_le_bytes()), [255, 0, 131, 255]);
    }

    #[test]
    fn unswizzle_first_gob_layout() {
        // 16x8 RGBA8 = one GOB (64 bytes x 8 rows). Fill the source with each byte's own address
        // and check a few well-known positions of the GOB layout.
        let src: Vec<u8> = (0..512).map(|i| (i % 251) as u8).collect();
        let out = unswizzle(&src, 16, 8, 4, 1).unwrap();
        let at = |x: usize, y: usize| out[(y * 16 + x) * 4];
        assert_eq!(at(0, 0), 0); // byte 0
        assert_eq!(at(4, 0), 32); // x_bytes 16 -> second 16-byte column of a 32-byte sector
        assert_eq!(at(0, 1), 16); // row 1 -> +16
        assert_eq!(at(0, 2), 64); // row 2 -> +64
        assert_eq!(at(8, 0), (256 % 251) as u8); // x_bytes 32 -> +256
    }
}
