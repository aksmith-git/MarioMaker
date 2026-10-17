//! Locate the course key table in the user's own executable (port of `smmfmt.smm2.keyfind`).
//!
//! Scans read-only and data segments for the 64-word window that decrypts a sample `.bcd` from
//! the same dump: a one-block test (the game-style code at body offset 0xF1) followed by the full
//! CRC32 + CMAC check. No key material ships with the project.

use aes::Aes128;
use aes::cipher::{Array, BlockCipherDecrypt, KeyInit};

use super::crypto::{
    FOOTER_OFFSET, HEADER_SIZE, KeyTable, TABLE_WORDS, decrypt_bcd, key_from_picks, key_schedule,
};
use crate::nso::{Nso, SegmentKind};

const STYLE_CODES: [&[u8; 2]; 5] = [b"M1", b"M3", b"MW", b"WU", b"3W"];

/// Search `region` (4-byte aligned words) for the table. Returns (byte offset in region, table).
pub fn find_in(region: &[u8], bcd: &[u8]) -> Option<(usize, KeyTable)> {
    if bcd.len() < FOOTER_OFFSET + 32 {
        return None;
    }
    let state: [u32; 4] = std::array::from_fn(|i| {
        let o = FOOTER_OFFSET + 16 + i * 4;
        u32::from_le_bytes(bcd[o..o + 4].try_into().unwrap())
    });
    let picks = key_schedule(state, 1)[0];
    let c14 = &bcd[HEADER_SIZE + 0xE0..HEADER_SIZE + 0xF0];
    let c15: [u8; 16] = bcd[HEADER_SIZE + 0xF0..HEADER_SIZE + 0x100]
        .try_into()
        .unwrap();
    let words: Vec<u32> = region
        .as_chunks::<4>()
        .0
        .iter()
        .map(|w| u32::from_le_bytes(*w))
        .collect();
    if words.len() < TABLE_WORDS {
        return None;
    }
    for start in 0..=words.len() - TABLE_WORDS {
        let window: &[u32; TABLE_WORDS] = words[start..start + TABLE_WORDS].try_into().unwrap();
        if picks.iter().any(|&(idx, _)| window[idx] == 0) {
            continue;
        }
        let key = key_from_picks(&picks, window);
        let mut block = Array::from(c15);
        Aes128::new(&Array::from(key)).decrypt_block(&mut block);
        let plain: [u8; 16] = block.into();
        let p: Vec<u8> = plain.iter().zip(c14).map(|(a, b)| a ^ b).collect();
        if STYLE_CODES.iter().any(|s| &p[1..3] == *s)
            && p[3] == 0
            && decrypt_bcd(bcd, window, true).is_ok()
        {
            return Some((start * 4, *window));
        }
    }
    None
}

/// Search an NSO's read-only and data segments.
pub fn find_in_nso(nso: &Nso, bcd: &[u8]) -> Option<KeyTable> {
    [SegmentKind::Ro, SegmentKind::Data]
        .into_iter()
        .filter_map(|k| nso.segment(k))
        .find_map(|seg| find_in(seg, bcd).map(|(_, t)| t))
}
