//! Software BC7 (BPTC) decoding to RGBA8, for GPUs without BC texture support (WebGL2, most
//! mobile GPUs). Written from the public format description (Khronos Data Format Specification,
//! "BPTC Compressed Texture Image Formats"); the partition and anchor tables are part of that
//! specification.

/// Per mode: subsets, partition bits, rotation bits, index-selection bits, colour bits, alpha
/// bits, per-endpoint p-bits, shared p-bits, index bits, secondary index bits.
#[derive(Clone, Copy)]
struct Mode {
    subsets: usize,
    partition_bits: u32,
    rotation_bits: u32,
    index_sel_bits: u32,
    color_bits: u32,
    alpha_bits: u32,
    endpoint_pbits: bool,
    shared_pbits: bool,
    index_bits: u32,
    index2_bits: u32,
}

#[allow(clippy::too_many_arguments)] // one row of the mode table
const fn m(
    subsets: usize,
    pb: u32,
    rb: u32,
    isb: u32,
    cb: u32,
    ab: u32,
    epb: bool,
    spb: bool,
    ib: u32,
    ib2: u32,
) -> Mode {
    Mode {
        subsets,
        partition_bits: pb,
        rotation_bits: rb,
        index_sel_bits: isb,
        color_bits: cb,
        alpha_bits: ab,
        endpoint_pbits: epb,
        shared_pbits: spb,
        index_bits: ib,
        index2_bits: ib2,
    }
}

const MODES: [Mode; 8] = [
    m(3, 4, 0, 0, 4, 0, true, false, 3, 0),
    m(2, 6, 0, 0, 6, 0, false, true, 3, 0),
    m(3, 6, 0, 0, 5, 0, false, false, 2, 0),
    m(2, 6, 0, 0, 7, 0, true, false, 2, 0),
    m(1, 0, 2, 1, 5, 6, false, false, 2, 3),
    m(1, 0, 2, 0, 7, 8, false, false, 2, 2),
    m(1, 0, 0, 0, 7, 7, true, false, 4, 0),
    m(2, 6, 0, 0, 5, 5, true, false, 2, 0),
];

/// Two-subset partitions: bit i set = pixel i belongs to subset 1.
const P2: [u16; 64] = [
    0xCCCC, 0x8888, 0xEEEE, 0xECC8, 0xC880, 0xFEEC, 0xFEC8, 0xEC80, 0xC800, 0xFFEC, 0xFE80, 0xE800,
    0xFFE8, 0xFF00, 0xFFF0, 0xF000, 0xF710, 0x008E, 0x7100, 0x08CE, 0x008C, 0x7310, 0x3100, 0x8CCE,
    0x088C, 0x3110, 0x6666, 0x366C, 0x17E8, 0x0FF0, 0x718E, 0x399C, 0xAAAA, 0xF0F0, 0x5A5A, 0x33CC,
    0x3C3C, 0x55AA, 0x9696, 0xA55A, 0x73CE, 0x13C8, 0x324C, 0x3BDC, 0x6996, 0xC33C, 0x9966, 0x0660,
    0x0272, 0x04E4, 0x4E40, 0x2720, 0xC936, 0x936C, 0x39C6, 0x639C, 0x9336, 0x9CC6, 0x817E, 0xE718,
    0xCCF0, 0x0FCC, 0x7744, 0xEE22,
];

/// Three-subset partitions, one row of 16 pixel subsets each.
const P3: [[u8; 16]; 64] = [
    [0, 0, 1, 1, 0, 0, 1, 1, 0, 2, 2, 1, 2, 2, 2, 2],
    [0, 0, 0, 1, 0, 0, 1, 1, 2, 2, 1, 1, 2, 2, 2, 1],
    [0, 0, 0, 0, 2, 0, 0, 1, 2, 2, 1, 1, 2, 2, 1, 1],
    [0, 2, 2, 2, 0, 0, 2, 2, 0, 0, 1, 1, 0, 1, 1, 1],
    [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 2, 2, 1, 1, 2, 2],
    [0, 0, 1, 1, 0, 0, 1, 1, 0, 0, 2, 2, 0, 0, 2, 2],
    [0, 0, 2, 2, 0, 0, 2, 2, 1, 1, 1, 1, 1, 1, 1, 1],
    [0, 0, 1, 1, 0, 0, 1, 1, 2, 2, 1, 1, 2, 2, 1, 1],
    [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2],
    [0, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 2, 2, 2, 2],
    [0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 2, 2, 2, 2],
    [0, 0, 1, 2, 0, 0, 1, 2, 0, 0, 1, 2, 0, 0, 1, 2],
    [0, 1, 1, 2, 0, 1, 1, 2, 0, 1, 1, 2, 0, 1, 1, 2],
    [0, 1, 2, 2, 0, 1, 2, 2, 0, 1, 2, 2, 0, 1, 2, 2],
    [0, 0, 1, 1, 0, 1, 1, 2, 1, 1, 2, 2, 1, 2, 2, 2],
    [0, 0, 1, 1, 2, 0, 0, 1, 2, 2, 0, 0, 2, 2, 2, 0],
    [0, 0, 0, 1, 0, 0, 1, 1, 0, 1, 1, 2, 1, 1, 2, 2],
    [0, 1, 1, 1, 0, 0, 1, 1, 2, 0, 0, 1, 2, 2, 0, 0],
    [0, 0, 0, 0, 1, 1, 2, 2, 1, 1, 2, 2, 1, 1, 2, 2],
    [0, 0, 2, 2, 0, 0, 2, 2, 0, 0, 2, 2, 1, 1, 1, 1],
    [0, 1, 1, 1, 0, 1, 1, 1, 0, 2, 2, 2, 0, 2, 2, 2],
    [0, 0, 0, 1, 0, 0, 0, 1, 2, 2, 2, 1, 2, 2, 2, 1],
    [0, 0, 0, 0, 0, 0, 1, 1, 0, 1, 2, 2, 0, 1, 2, 2],
    [0, 0, 0, 0, 1, 1, 0, 0, 2, 2, 1, 0, 2, 2, 1, 0],
    [0, 1, 2, 2, 0, 1, 2, 2, 0, 0, 1, 1, 0, 0, 0, 0],
    [0, 0, 1, 2, 0, 0, 1, 2, 1, 1, 2, 2, 2, 2, 2, 2],
    [0, 1, 1, 0, 1, 2, 2, 1, 1, 2, 2, 1, 0, 1, 1, 0],
    [0, 0, 0, 0, 0, 1, 1, 0, 1, 2, 2, 1, 1, 2, 2, 1],
    [0, 0, 2, 2, 1, 1, 0, 2, 1, 1, 0, 2, 0, 0, 2, 2],
    [0, 1, 1, 0, 0, 1, 1, 0, 2, 0, 0, 2, 2, 2, 2, 2],
    [0, 0, 1, 1, 0, 1, 2, 2, 0, 1, 2, 2, 0, 0, 1, 1],
    [0, 0, 0, 0, 2, 0, 0, 0, 2, 2, 1, 1, 2, 2, 2, 1],
    [0, 0, 0, 0, 0, 0, 0, 2, 1, 1, 2, 2, 1, 2, 2, 2],
    [0, 2, 2, 2, 0, 0, 2, 2, 0, 0, 1, 2, 0, 0, 1, 1],
    [0, 0, 1, 1, 0, 0, 1, 2, 0, 0, 2, 2, 0, 2, 2, 2],
    [0, 1, 2, 0, 0, 1, 2, 0, 0, 1, 2, 0, 0, 1, 2, 0],
    [0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 0, 0, 0, 0],
    [0, 1, 2, 0, 1, 2, 0, 1, 2, 0, 1, 2, 0, 1, 2, 0],
    [0, 1, 2, 0, 2, 0, 1, 2, 1, 2, 0, 1, 0, 1, 2, 0],
    [0, 0, 1, 1, 2, 2, 0, 0, 1, 1, 2, 2, 0, 0, 1, 1],
    [0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 0, 0, 0, 0, 1, 1],
    [0, 1, 0, 1, 0, 1, 0, 1, 2, 2, 2, 2, 2, 2, 2, 2],
    [0, 0, 0, 0, 0, 0, 0, 0, 2, 1, 2, 1, 2, 1, 2, 1],
    [0, 0, 2, 2, 1, 1, 2, 2, 0, 0, 2, 2, 1, 1, 2, 2],
    [0, 0, 2, 2, 0, 0, 1, 1, 0, 0, 2, 2, 0, 0, 1, 1],
    [0, 2, 2, 0, 1, 2, 2, 1, 0, 2, 2, 0, 1, 2, 2, 1],
    [0, 1, 0, 1, 2, 2, 2, 2, 2, 2, 2, 2, 0, 1, 0, 1],
    [0, 0, 0, 0, 2, 1, 2, 1, 2, 1, 2, 1, 2, 1, 2, 1],
    [0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 2, 2, 2, 2],
    [0, 2, 2, 2, 0, 1, 1, 1, 0, 2, 2, 2, 0, 1, 1, 1],
    [0, 0, 0, 2, 1, 1, 1, 2, 0, 0, 0, 2, 1, 1, 1, 2],
    [0, 0, 0, 0, 2, 1, 1, 2, 2, 1, 1, 2, 2, 1, 1, 2],
    [0, 2, 2, 2, 0, 1, 1, 1, 0, 1, 1, 1, 0, 2, 2, 2],
    [0, 0, 0, 2, 1, 1, 1, 2, 1, 1, 1, 2, 0, 0, 0, 2],
    [0, 1, 1, 0, 0, 1, 1, 0, 0, 1, 1, 0, 2, 2, 2, 2],
    [0, 0, 0, 0, 0, 0, 0, 0, 2, 1, 1, 2, 2, 1, 1, 2],
    [0, 1, 1, 0, 0, 1, 1, 0, 2, 2, 2, 2, 2, 2, 2, 2],
    [0, 0, 2, 2, 0, 0, 1, 1, 0, 0, 1, 1, 0, 0, 2, 2],
    [0, 0, 2, 2, 1, 1, 2, 2, 1, 1, 2, 2, 0, 0, 2, 2],
    [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2, 1, 1, 2],
    [0, 0, 0, 2, 0, 0, 0, 1, 0, 0, 0, 2, 0, 0, 0, 1],
    [0, 2, 2, 2, 1, 2, 2, 2, 0, 2, 2, 2, 1, 2, 2, 2],
    [0, 1, 0, 1, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2],
    [0, 1, 1, 1, 2, 0, 1, 1, 2, 2, 0, 1, 2, 2, 2, 0],
];

/// Anchor pixel of subset 1 in the two-subset partitions.
const A2: [u8; 64] = [
    15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 2, 8, 2, 2, 8, 8, 15, 2, 8,
    2, 2, 8, 8, 2, 2, 15, 15, 6, 8, 2, 8, 15, 15, 2, 8, 2, 2, 2, 15, 15, 6, 6, 2, 6, 8, 15, 15, 2,
    2, 15, 15, 15, 15, 15, 2, 2, 15,
];

/// Anchor pixels of subsets 1 and 2 in the three-subset partitions.
const A3_1: [u8; 64] = [
    3, 3, 15, 15, 8, 3, 15, 15, 8, 8, 6, 6, 6, 5, 3, 3, 3, 3, 8, 15, 3, 3, 6, 10, 5, 8, 8, 6, 8, 5,
    15, 15, 8, 15, 3, 5, 6, 10, 8, 15, 15, 3, 15, 5, 15, 15, 15, 15, 3, 15, 5, 5, 5, 8, 5, 10, 5,
    10, 8, 13, 15, 12, 3, 3,
];
const A3_2: [u8; 64] = [
    15, 8, 8, 3, 15, 15, 3, 8, 15, 15, 15, 15, 15, 15, 15, 8, 15, 8, 15, 3, 15, 8, 15, 8, 3, 15, 6,
    10, 15, 15, 10, 8, 15, 3, 15, 10, 10, 8, 9, 10, 6, 15, 8, 15, 3, 6, 6, 8, 15, 3, 15, 15, 15,
    15, 15, 15, 15, 15, 15, 15, 3, 15, 15, 8,
];

const W2: [u32; 4] = [0, 21, 43, 64];
const W3: [u32; 8] = [0, 9, 18, 27, 37, 46, 55, 64];
const W4: [u32; 16] = [0, 4, 9, 13, 17, 21, 26, 30, 34, 38, 43, 47, 51, 55, 60, 64];

fn weights(bits: u32) -> &'static [u32] {
    match bits {
        2 => &W2,
        3 => &W3,
        _ => &W4,
    }
}

fn interp(a: u8, b: u8, w: u32) -> u8 {
    (((64 - w) * a as u32 + w * b as u32 + 32) >> 6) as u8
}

/// Little-endian bit reader over one 128-bit block.
struct Bits(u128, u32);

impl Bits {
    fn read(&mut self, n: u32) -> u32 {
        if n == 0 {
            return 0;
        }
        let v = ((self.0 >> self.1) & ((1u128 << n) - 1)) as u32;
        self.1 += n;
        v
    }
}

/// Subset of pixel `i` in partition `p` of a mode with `subsets` subsets.
fn subset(subsets: usize, p: usize, i: usize) -> usize {
    match subsets {
        2 => ((P2[p] >> i) & 1) as usize,
        3 => P3[p][i] as usize,
        _ => 0,
    }
}

fn is_anchor(subsets: usize, p: usize, i: usize) -> bool {
    i == 0
        || match subsets {
            2 => i == A2[p] as usize,
            3 => i == A3_1[p] as usize || i == A3_2[p] as usize,
            _ => false,
        }
}

/// Decode one 16-byte block into 16 RGBA pixels (row-major within the block).
pub fn decode_block(block: &[u8; 16]) -> [[u8; 4]; 16] {
    let mut bits = Bits(u128::from_le_bytes(*block), 0);
    if block[0] == 0 {
        return [[0; 4]; 16]; // reserved mode: transparent black
    }
    let mode_n = block[0].trailing_zeros();
    let mode = MODES[mode_n as usize];
    bits.read(mode_n + 1);
    let p = bits.read(mode.partition_bits) as usize;
    let rotation = bits.read(mode.rotation_bits);
    let index_sel = bits.read(mode.index_sel_bits);

    let ns = mode.subsets;
    // endpoints[subset*2 + e][channel]
    let mut ep = [[0u32; 4]; 6];
    for c in 0..3 {
        for e in ep.iter_mut().take(ns * 2) {
            e[c] = bits.read(mode.color_bits);
        }
    }
    if mode.alpha_bits > 0 {
        for e in ep.iter_mut().take(ns * 2) {
            e[3] = bits.read(mode.alpha_bits);
        }
    }
    let (mut cbits, mut abits) = (mode.color_bits, mode.alpha_bits);
    if mode.endpoint_pbits || mode.shared_pbits {
        let mut pb = [0u32; 6];
        if mode.endpoint_pbits {
            for v in pb.iter_mut().take(ns * 2) {
                *v = bits.read(1);
            }
        } else {
            for s in 0..ns {
                let v = bits.read(1);
                pb[s * 2] = v;
                pb[s * 2 + 1] = v;
            }
        }
        for (e, &b) in ep.iter_mut().zip(&pb).take(ns * 2) {
            for v in &mut e[..3] {
                *v = *v << 1 | b;
            }
            if mode.alpha_bits > 0 {
                e[3] = e[3] << 1 | b;
            }
        }
        cbits += 1;
        if abits > 0 {
            abits += 1;
        }
    }
    let expand = |v: u32, n: u32| -> u8 {
        if n == 0 {
            255
        } else {
            let v = v << (8 - n);
            (v | v >> n) as u8
        }
    };
    let mut endpoints = [[0u8; 4]; 6];
    for (out, e) in endpoints.iter_mut().zip(&ep).take(ns * 2) {
        for c in 0..3 {
            out[c] = expand(e[c], cbits);
        }
        out[3] = expand(e[3], abits);
    }

    let mut idx = [0u32; 16];
    for (i, v) in idx.iter_mut().enumerate() {
        let n = mode.index_bits - is_anchor(ns, p, i) as u32;
        *v = bits.read(n);
    }
    let mut idx2 = [0u32; 16];
    if mode.index2_bits > 0 {
        for (i, v) in idx2.iter_mut().enumerate() {
            *v = bits.read(mode.index2_bits - (i == 0) as u32);
        }
    }

    let mut out = [[0u8; 4]; 16];
    for (i, px) in out.iter_mut().enumerate() {
        let s = subset(ns, p, i);
        let (e0, e1) = (endpoints[s * 2], endpoints[s * 2 + 1]);
        let (ci, cb, ai, ab) = if mode.index2_bits == 0 {
            (idx[i], mode.index_bits, idx[i], mode.index_bits)
        } else if index_sel == 0 {
            (idx[i], mode.index_bits, idx2[i], mode.index2_bits)
        } else {
            (idx2[i], mode.index2_bits, idx[i], mode.index_bits)
        };
        let (cw, aw) = (weights(cb)[ci as usize], weights(ab)[ai as usize]);
        for c in 0..3 {
            px[c] = interp(e0[c], e1[c], cw);
        }
        px[3] = interp(e0[3], e1[3], aw);
        match rotation {
            1 => px.swap(0, 3),
            2 => px.swap(1, 3),
            3 => px.swap(2, 3),
            _ => {}
        }
    }
    out
}

/// Decode `width × height` pixels of BC7 blocks into RGBA8 (top row first).
pub fn decode(data: &[u8], width: u32, height: u32) -> Vec<u8> {
    let (w, h) = (width as usize, height as usize);
    let (bw, bh) = (w.div_ceil(4), h.div_ceil(4));
    let mut out = vec![0u8; w * h * 4];
    for by in 0..bh {
        for bx in 0..bw {
            let o = (by * bw + bx) * 16;
            let Some(b) = data.get(o..o + 16) else {
                continue;
            };
            let px = decode_block(b.try_into().unwrap());
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

    /// Write `v` into `n` bits at `pos` of a little-endian 128-bit block.
    fn put(block: &mut u128, pos: &mut u32, n: u32, v: u32) {
        *block |= (v as u128 & ((1u128 << n) - 1)) << *pos;
        *pos += n;
    }

    #[test]
    fn tables_are_consistent() {
        for p in 0..64 {
            // pixel 0 is always the anchor of subset 0; the other anchors lie in their subsets
            assert_eq!(P2[p] & 1, 0, "P2[{p}]");
            assert_eq!(subset(2, p, A2[p] as usize), 1, "A2[{p}]");
            assert_eq!(P3[p][0], 0, "P3[{p}]");
            assert_eq!(P3[p][A3_1[p] as usize], 1, "A3_1[{p}]");
            assert_eq!(P3[p][A3_2[p] as usize], 2, "A3_2[{p}]");
            // every subset is used
            for s in 0..3 {
                assert!(P3[p].contains(&s), "P3[{p}] lacks subset {s}");
            }
        }
    }

    #[test]
    fn mode6_solid_block() {
        // mode 6: one subset, 7-bit colour + alpha, p-bits, 4-bit indices; both endpoints equal
        let mut b = 0u128;
        let mut pos = 0;
        put(&mut b, &mut pos, 7, 1 << 6); // mode 6
        for v in [0x40u32, 0x40, 0x20, 0x20, 0x7F, 0x7F, 0x7F, 0x7F] {
            put(&mut b, &mut pos, 7, v); // R0 R1 G0 G1 B0 B1 A0 A1
        }
        put(&mut b, &mut pos, 1, 1);
        put(&mut b, &mut pos, 1, 1);
        let px = decode_block(&b.to_le_bytes());
        // (0x40 << 1 | 1) = 0x81, (0x20 << 1 | 1) = 0x41, 0xFF
        assert!(
            px.iter().all(|p| *p == [0x81, 0x41, 0xFF, 0xFF]),
            "{:?}",
            px[0]
        );
    }

    #[test]
    fn mode5_gradient_and_rotation() {
        // mode 5: rotation 0, colour 7 bits, alpha 8 bits, 2-bit colour and alpha indices
        let mut b = 0u128;
        let mut pos = 0;
        put(&mut b, &mut pos, 6, 1 << 5);
        put(&mut b, &mut pos, 2, 1); // rotation 1: swap R and A
        for v in [0u32, 0x7F, 0, 0x7F, 0, 0x7F] {
            put(&mut b, &mut pos, 7, v);
        }
        put(&mut b, &mut pos, 8, 0x10);
        put(&mut b, &mut pos, 8, 0x10);
        // colour indices: pixel 0 (anchor, 1 bit) = 0, the rest 3; alpha all 0
        put(&mut b, &mut pos, 1, 0);
        for _ in 1..16 {
            put(&mut b, &mut pos, 2, 3);
        }
        let px = decode_block(&b.to_le_bytes());
        // after rotation R holds alpha (0x10) and A holds the red channel
        assert_eq!(px[0], [0x10, 0, 0, 0]);
        assert_eq!(px[1], [0x10, 0xFF, 0xFF, 0xFF]);
    }

    #[test]
    fn reserved_mode_is_transparent() {
        assert_eq!(decode_block(&[0; 16]), [[0; 4]; 16]);
        assert_eq!(decode(&[0; 16], 2, 2), vec![0; 16]);
    }
}
