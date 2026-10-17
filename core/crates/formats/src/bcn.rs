//! Software BC1-BC5 decoding to RGBA8 (for inspection tools and tests; the GPU samples BCn
//! directly in the engine).

fn rgb565(c: u16) -> [u8; 3] {
    let r = ((c >> 11) & 31) as u32;
    let g = ((c >> 5) & 63) as u32;
    let b = (c & 31) as u32;
    [
        ((r << 3) | (r >> 2)) as u8,
        ((g << 2) | (g >> 4)) as u8,
        ((b << 3) | (b >> 2)) as u8,
    ]
}

fn color_block(b: &[u8], out: &mut [[u8; 4]; 16], force_four: bool) {
    let c0 = u16::from_le_bytes([b[0], b[1]]);
    let c1 = u16::from_le_bytes([b[2], b[3]]);
    let (p0, p1) = (rgb565(c0), rgb565(c1));
    let mix = |a: u8, b: u8, wa: u32, wb: u32, d: u32| ((a as u32 * wa + b as u32 * wb) / d) as u8;
    let mut pal = [[0u8; 4]; 4];
    pal[0] = [p0[0], p0[1], p0[2], 255];
    pal[1] = [p1[0], p1[1], p1[2], 255];
    if c0 > c1 || force_four {
        for k in 0..3 {
            pal[2][k] = mix(p0[k], p1[k], 2, 1, 3);
            pal[3][k] = mix(p0[k], p1[k], 1, 2, 3);
        }
        pal[2][3] = 255;
        pal[3][3] = 255;
    } else {
        for k in 0..3 {
            pal[2][k] = mix(p0[k], p1[k], 1, 1, 2);
        }
        pal[2][3] = 255;
        pal[3] = [0, 0, 0, 0];
    }
    let bits = u32::from_le_bytes([b[4], b[5], b[6], b[7]]);
    for (i, px) in out.iter_mut().enumerate() {
        *px = pal[((bits >> (2 * i)) & 3) as usize];
    }
}

fn bc4_block(b: &[u8]) -> [u8; 16] {
    let (a0, a1) = (b[0] as u32, b[1] as u32);
    let mut pal = [0u8; 8];
    pal[0] = a0 as u8;
    pal[1] = a1 as u8;
    if a0 > a1 {
        for i in 1..7 {
            pal[i + 1] = (((7 - i) as u32 * a0 + i as u32 * a1) / 7) as u8;
        }
    } else {
        for i in 1..5 {
            pal[i + 1] = (((5 - i) as u32 * a0 + i as u32 * a1) / 5) as u8;
        }
        pal[6] = 0;
        pal[7] = 255;
    }
    let mut bits = 0u64;
    for (i, &x) in b[2..8].iter().enumerate() {
        bits |= (x as u64) << (8 * i);
    }
    let mut out = [0u8; 16];
    for (i, o) in out.iter_mut().enumerate() {
        *o = pal[((bits >> (3 * i)) & 7) as usize];
    }
    out
}

/// Decode `width × height` pixels of BCn `kind` (1-5) blocks into RGBA8. Single-channel
/// formats put R in every channel (alpha 255); BC5 gives R, G, 0, 255.
pub fn decode(kind: u8, data: &[u8], width: u32, height: u32) -> Vec<u8> {
    let (w, h) = (width as usize, height as usize);
    let (bw, bh) = (w.div_ceil(4), h.div_ceil(4));
    let block = if matches!(kind, 1 | 4) { 8 } else { 16 };
    let mut out = vec![0u8; w * h * 4];
    for by in 0..bh {
        for bx in 0..bw {
            let o = (by * bw + bx) * block;
            let Some(b) = data.get(o..o + block) else {
                continue;
            };
            let mut px = [[0u8; 4]; 16];
            match kind {
                1 => color_block(b, &mut px, false),
                2 => {
                    color_block(&b[8..], &mut px, true);
                    for (i, p) in px.iter_mut().enumerate() {
                        let nib = (b[i / 2] >> (4 * (i % 2))) & 15;
                        p[3] = nib * 17;
                    }
                }
                3 => {
                    color_block(&b[8..], &mut px, true);
                    for (p, a) in px.iter_mut().zip(bc4_block(&b[..8])) {
                        p[3] = a;
                    }
                }
                4 => {
                    for (p, r) in px.iter_mut().zip(bc4_block(b)) {
                        *p = [r, r, r, 255];
                    }
                }
                _ => {
                    let r = bc4_block(&b[..8]);
                    let g = bc4_block(&b[8..]);
                    for i in 0..16 {
                        px[i] = [r[i], g[i], 0, 255];
                    }
                }
            }
            for (i, p) in px.iter().enumerate() {
                let (x, y) = (bx * 4 + i % 4, by * 4 + i / 4);
                if x < w && y < h {
                    let d = (y * w + x) * 4;
                    out[d..d + 4].copy_from_slice(p);
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bc1_solid_and_bc4_ramp() {
        // BC1: c0 = pure red, all indices 0
        let red = 0xF800u16.to_le_bytes();
        let b = [red[0], red[1], 0, 0, 0, 0, 0, 0];
        let px = decode(1, &b, 4, 4);
        assert_eq!(&px[..4], &[255, 0, 0, 255]);
        // BC4: endpoints 255 / 0, index 1 everywhere → 0
        let b = [
            255,
            0,
            0b0100_1001,
            0b1001_0010,
            0b0010_0100,
            0b0100_1001,
            0b1001_0010,
            0b0010_0100,
        ];
        let px = decode(4, &b, 4, 4);
        assert_eq!(&px[..4], &[0, 0, 0, 255]);
        // partial blocks are cropped
        assert_eq!(decode(4, &b, 3, 2).len(), 3 * 2 * 4);
    }
}
