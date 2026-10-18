//! How a cache texture (`Mtex`) goes to the GPU: as is when the GPU samples its format (RGBA8
//! always, BCn when the adapter has BC support), otherwise decoded to RGBA8 on the CPU.
//! Channel selects (the Switch's texture swizzle) are applied on the CPU, since WebGPU has no
//! texture-view swizzle.

use mmk_assets::{Mtex, MtexFormat, MtexVariant};
use wgpu::TextureFormat;

/// Data ready for `Queue::write_texture`.
pub struct Upload<'a> {
    pub format: TextureFormat,
    pub width: u32,
    pub height: u32,
    pub data: std::borrow::Cow<'a, [u8]>,
    /// Bytes per row of blocks (or of pixels for RGBA8).
    pub bytes_per_row: u32,
    /// Rows of blocks (or of pixels).
    pub rows: u32,
}

const IDENTITY: [u8; 4] = [2, 3, 4, 5];

/// The GPU format for a BCn texture, with its block size in bytes.
fn bc_format(t: &Mtex) -> Option<(TextureFormat, u32)> {
    use TextureFormat as F;
    let srgb = t.variant == MtexVariant::Srgb;
    let snorm = t.variant == MtexVariant::Snorm;
    Some(match t.format {
        MtexFormat::Rgba8 => return None,
        MtexFormat::Bc1 => (
            if srgb {
                F::Bc1RgbaUnormSrgb
            } else {
                F::Bc1RgbaUnorm
            },
            8,
        ),
        MtexFormat::Bc2 => (
            if srgb {
                F::Bc2RgbaUnormSrgb
            } else {
                F::Bc2RgbaUnorm
            },
            16,
        ),
        MtexFormat::Bc3 => (
            if srgb {
                F::Bc3RgbaUnormSrgb
            } else {
                F::Bc3RgbaUnorm
            },
            16,
        ),
        MtexFormat::Bc4 => (if snorm { F::Bc4RSnorm } else { F::Bc4RUnorm }, 8),
        MtexFormat::Bc5 => (if snorm { F::Bc5RgSnorm } else { F::Bc5RgUnorm }, 16),
        MtexFormat::Bc6h => (F::Bc6hRgbUfloat, 16),
        MtexFormat::Bc7 => (
            if srgb {
                F::Bc7RgbaUnormSrgb
            } else {
                F::Bc7RgbaUnorm
            },
            16,
        ),
    })
}

/// Apply a channel select (0 zero, 1 one, 2 R, 3 G, 4 B, 5 A) to RGBA8 pixels.
pub fn swizzle(rgba: &mut [u8], sel: [u8; 4]) {
    if sel == IDENTITY {
        return;
    }
    for px in rgba.as_chunks_mut::<4>().0 {
        let src = *px;
        for (o, &s) in px.iter_mut().zip(&sel) {
            *o = match s {
                0 => 0,
                1 => 255,
                2..=5 => src[(s - 2) as usize],
                _ => src[3],
            };
        }
    }
}

/// Decode any supported texture to RGBA8 (None: BC6H, not decodable here), channel select
/// applied.
pub fn decode_rgba(t: &Mtex) -> Option<Vec<u8>> {
    let mut rgba = mmk_host::texture::decode_rgba(t.format, &t.data, t.width, t.height)?;
    swizzle(&mut rgba, t.channel_select);
    Some(rgba)
}

fn rgba_format(t: &Mtex) -> TextureFormat {
    // BC4/BC5 decode to linear data; everything else follows the texture's variant
    if t.variant == MtexVariant::Srgb && !matches!(t.format, MtexFormat::Bc4 | MtexFormat::Bc5) {
        TextureFormat::Rgba8UnormSrgb
    } else {
        TextureFormat::Rgba8Unorm
    }
}

/// Plan the upload of `t`. `bc` = the device has `TEXTURE_COMPRESSION_BC`. None when the
/// texture can't be used (empty, or BC6H without GPU support).
pub fn plan(t: &Mtex, bc: bool) -> Option<Upload<'_>> {
    if t.width == 0 || t.height == 0 {
        return None;
    }
    let (w, h) = (t.width, t.height);
    if t.format == MtexFormat::Rgba8 && t.channel_select == IDENTITY {
        let n = (w * h * 4) as usize;
        return (t.data.len() >= n).then(|| Upload {
            format: rgba_format(t),
            width: w,
            height: h,
            data: std::borrow::Cow::Borrowed(&t.data[..n]),
            bytes_per_row: w * 4,
            rows: h,
        });
    }
    if let Some((format, block)) = bc_format(t) {
        // the GPU path needs whole blocks and no swizzle
        let whole = w % 4 == 0 && h % 4 == 0;
        let (bw, bh) = (w.div_ceil(4), h.div_ceil(4));
        let n = (bw * bh * block) as usize;
        if bc && whole && t.channel_select == IDENTITY && t.data.len() >= n {
            return Some(Upload {
                format,
                width: w,
                height: h,
                data: std::borrow::Cow::Borrowed(&t.data[..n]),
                bytes_per_row: bw * block,
                rows: bh,
            });
        }
    }
    let rgba = decode_rgba(t)?;
    Some(Upload {
        format: rgba_format(t),
        width: w,
        height: h,
        data: std::borrow::Cow::Owned(rgba),
        bytes_per_row: w * 4,
        rows: h,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tex(format: MtexFormat, w: u32, h: u32, data: Vec<u8>) -> Mtex {
        Mtex {
            format,
            variant: MtexVariant::Srgb,
            width: w,
            height: h,
            channel_select: IDENTITY,
            data,
        }
    }

    #[test]
    fn rgba_goes_as_is_and_bc_falls_back_without_support() {
        let t = tex(MtexFormat::Rgba8, 1, 1, vec![1, 2, 3, 4]);
        let u = plan(&t, false).unwrap();
        assert_eq!(u.format, TextureFormat::Rgba8UnormSrgb);
        assert!(matches!(u.data, std::borrow::Cow::Borrowed(_)));

        // one BC1 block, all pixels colour 0 = pure red (565: 0xF800)
        let bc1 = tex(MtexFormat::Bc1, 4, 4, vec![0x00, 0xF8, 0, 0, 0, 0, 0, 0]);
        assert_eq!(
            plan(&bc1, true).unwrap().format,
            TextureFormat::Bc1RgbaUnormSrgb
        );
        let u = plan(&bc1, false).unwrap();
        assert_eq!(u.format, TextureFormat::Rgba8UnormSrgb);
        assert_eq!(&u.data[..4], &[255, 0, 0, 255]);
        assert_eq!(u.data.len(), 64);
    }

    #[test]
    fn swizzle_and_partial_blocks_decode_on_the_cpu() {
        let mut t = tex(MtexFormat::Rgba8, 1, 1, vec![10, 20, 30, 40]);
        t.channel_select = [5, 2, 1, 0]; // A, R, one, zero
        assert_eq!(&plan(&t, true).unwrap().data[..], &[40, 10, 255, 0]);
        // 2×2 BC1 (not whole blocks): decoded even with BC support
        let bc1 = tex(MtexFormat::Bc1, 2, 2, vec![0x00, 0xF8, 0, 0, 0, 0, 0, 0]);
        let u = plan(&bc1, true).unwrap();
        assert_eq!(
            (u.format, u.data.len()),
            (TextureFormat::Rgba8UnormSrgb, 16)
        );
        assert!(plan(&tex(MtexFormat::Bc6h, 4, 4, vec![0; 16]), false).is_none());
    }
}
