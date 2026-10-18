//! Cache textures decoded to RGBA8 on the CPU, for GPUs that can't sample BCn (WebGL2, iPhones
//! and older iPads). Both shells use it: the portable shell's upload planner and the Swift app
//! through `mmk_texture_decode_rgba`. Channel selects are left to the caller (Metal applies them
//! as a texture swizzle, the portable shell on the CPU).

use mmk_assets::MtexFormat;
use mmk_formats::{bc7, bcn};

/// Decode `width × height` pixels of `format` to RGBA8, top row first (None: BC6H, which
/// isn't decoded here, or data too short). BC4 gives (r, 0, 0, 255) and BC5 (r, g, 0, 255),
/// as a GPU samples them.
pub fn decode_rgba(format: MtexFormat, data: &[u8], width: u32, height: u32) -> Option<Vec<u8>> {
    let mut rgba = match format {
        MtexFormat::Rgba8 => data.to_vec(),
        MtexFormat::Bc7 => bc7::decode(data, width, height),
        MtexFormat::Bc6h => return None,
        f => {
            // mmk_formats::bcn kinds: 1 BC1 … 5 BC5 (mtex codes are one higher)
            let mut px = bcn::decode(f as u8 - 1, data, width, height);
            if f == MtexFormat::Bc4 {
                // the tool decoder spreads r for viewing
                for p in px.as_chunks_mut::<4>().0 {
                    p[1] = 0;
                    p[2] = 0;
                }
            }
            px
        }
    };
    let n = (width as usize) * (height as usize) * 4;
    if rgba.len() < n {
        return None;
    }
    rgba.truncate(n);
    Some(rgba)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bc1_and_bc4_decode_like_the_gpu() {
        // one BC1 block, every pixel colour 0 = pure red (565: 0xF800)
        let px = decode_rgba(MtexFormat::Bc1, &[0x00, 0xF8, 0, 0, 0, 0, 0, 0], 4, 4).unwrap();
        assert_eq!(px.len(), 64);
        assert_eq!(&px[..4], &[255, 0, 0, 255]);
        // one BC4 block, both endpoints 200: r = 200, g = b = 0
        let px = decode_rgba(MtexFormat::Bc4, &[200, 200, 0, 0, 0, 0, 0, 0], 4, 4).unwrap();
        assert_eq!(&px[..4], &[200, 0, 0, 255]);
    }

    #[test]
    fn short_data_and_bc6h_are_refused() {
        assert!(decode_rgba(MtexFormat::Rgba8, &[1, 2, 3], 1, 1).is_none());
        assert!(decode_rgba(MtexFormat::Bc6h, &[0; 16], 4, 4).is_none());
    }
}
