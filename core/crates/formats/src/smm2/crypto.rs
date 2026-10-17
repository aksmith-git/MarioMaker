//! SMM2 on-console course file (`.bcd`) encryption. Layout and key derivation:
//! docs/formats/smm2-course.md §1. The 64-word key table is NOT part of this project: callers
//! pass the one found in the user's own game executable.

use crate::aes_modes::{cbc_decrypt, cbc_encrypt, cmac};
use crate::error::{Error, Result};

pub const BCD_SIZE: usize = 0x5C000;
pub const HEADER_SIZE: usize = 0x10;
pub const BODY_SIZE: usize = 0x5BFC0;
pub const FOOTER_OFFSET: usize = HEADER_SIZE + BODY_SIZE;
pub const MAGIC: &[u8; 4] = b"SCDL";
pub const TABLE_WORDS: usize = 64;

pub type KeyTable = [u32; TABLE_WORDS];

pub fn table_from_bytes(data: &[u8]) -> Result<KeyTable> {
    if data.len() != TABLE_WORDS * 4 {
        return Err(Error::Size {
            what: "key table",
            expected: TABLE_WORDS * 4,
            got: data.len(),
        });
    }
    let mut t = [0u32; TABLE_WORDS];
    for (i, w) in t.iter_mut().enumerate() {
        *w = u32::from_le_bytes(data[i * 4..i * 4 + 4].try_into().unwrap());
    }
    Ok(t)
}

/// xorshift128 as used by the game's random-number class.
#[derive(Clone, Debug)]
pub struct SeadRandom {
    pub s: [u32; 4],
}

impl SeadRandom {
    pub fn next_u32(&mut self) -> u32 {
        let [s0, s1, s2, s3] = self.s;
        let mut t = s0 ^ (s0 << 11);
        t ^= t >> 8;
        t ^= s3 ^ (s3 >> 19);
        self.s = [s1, s2, s3, t];
        t
    }
}

/// For each key, the 16 (table index, shift) picks. Depends only on the random state.
pub fn key_schedule(state: [u32; 4], count: usize) -> Vec<[(usize, u32); 16]> {
    let mut rng = SeadRandom { s: state };
    (0..count)
        .map(|_| {
            let mut picks = [(0usize, 0u32); 16];
            for p in picks.iter_mut() {
                let idx = (rng.next_u32() >> 26) as usize;
                let shift = (rng.next_u32() >> 27) & 24;
                *p = (idx, shift);
            }
            picks
        })
        .collect()
}

pub fn key_from_picks(picks: &[(usize, u32); 16], table: &KeyTable) -> [u8; 16] {
    let mut out = [0u8; 16];
    for w in 0..4 {
        let mut word = 0u32;
        for &(idx, shift) in &picks[w * 4..w * 4 + 4] {
            word = (word << 8) | ((table[idx] >> shift) & 0xFF);
        }
        out[w * 4..w * 4 + 4].copy_from_slice(&word.to_le_bytes());
    }
    out
}

pub fn derive_keys(state: [u32; 4], table: &KeyTable) -> ([u8; 16], [u8; 16]) {
    let s = key_schedule(state, 2);
    (key_from_picks(&s[0], table), key_from_picks(&s[1], table))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BcdFile {
    pub header: [u8; HEADER_SIZE],
    pub body: Vec<u8>,
    pub iv: [u8; 16],
    pub random_state: [u32; 4],
    pub cmac: [u8; 16],
}

fn read_state(data: &[u8], off: usize) -> [u32; 4] {
    std::array::from_fn(|i| {
        u32::from_le_bytes(data[off + i * 4..off + i * 4 + 4].try_into().unwrap())
    })
}

pub fn decrypt_bcd(data: &[u8], table: &KeyTable, verify: bool) -> Result<BcdFile> {
    if data.len() != BCD_SIZE {
        return Err(Error::Size {
            what: ".bcd",
            expected: BCD_SIZE,
            got: data.len(),
        });
    }
    if &data[0xC..0x10] != MAGIC {
        return Err(Error::Magic {
            what: ".bcd (SCDL)",
        });
    }
    let header: [u8; 16] = data[..16].try_into().unwrap();
    let iv: [u8; 16] = data[FOOTER_OFFSET..FOOTER_OFFSET + 16].try_into().unwrap();
    let random_state = read_state(data, FOOTER_OFFSET + 16);
    let mac: [u8; 16] = data[FOOTER_OFFSET + 32..FOOTER_OFFSET + 48]
        .try_into()
        .unwrap();
    let (aes_key, cmac_key) = derive_keys(random_state, table);
    let body = cbc_decrypt(&aes_key, &iv, &data[HEADER_SIZE..FOOTER_OFFSET]);
    if verify {
        let crc = u32::from_le_bytes(header[8..12].try_into().unwrap());
        if crc32fast::hash(&body) != crc {
            return Err(Error::Crc);
        }
        if cmac(&cmac_key, &body) != mac {
            return Err(Error::Cmac);
        }
    }
    Ok(BcdFile {
        header,
        body,
        iv,
        random_state,
        cmac: mac,
    })
}

/// Encrypt course data with the given header/IV/random state (fresh ones are the caller's job).
pub fn encrypt_bcd(
    body: &[u8],
    table: &KeyTable,
    header: [u8; 16],
    iv: [u8; 16],
    random_state: [u32; 4],
) -> Result<Vec<u8>> {
    if body.len() != BODY_SIZE {
        return Err(Error::Size {
            what: "course body",
            expected: BODY_SIZE,
            got: body.len(),
        });
    }
    let (aes_key, cmac_key) = derive_keys(random_state, table);
    let mut out = Vec::with_capacity(BCD_SIZE);
    let mut hdr = header;
    hdr[8..12].copy_from_slice(&crc32fast::hash(body).to_le_bytes());
    out.extend_from_slice(&hdr);
    out.extend_from_slice(&cbc_encrypt(&aes_key, &iv, body));
    out.extend_from_slice(&iv);
    for w in random_state {
        out.extend_from_slice(&w.to_le_bytes());
    }
    out.extend_from_slice(&cmac(&cmac_key, body));
    Ok(out)
}

/// Default header for a new file: version 1, 0x10, 0, CRC (filled in), "SCDL".
pub fn default_header() -> [u8; 16] {
    let mut h = [0u8; 16];
    h[0..4].copy_from_slice(&1u32.to_le_bytes());
    h[4..6].copy_from_slice(&0x10u16.to_le_bytes());
    h[12..16].copy_from_slice(MAGIC);
    h
}

/// Write a decrypted file back with its original header, IV and random state.
pub fn reencrypt(bcd: &BcdFile, table: &KeyTable, body: Option<&[u8]>) -> Result<Vec<u8>> {
    encrypt_bcd(
        body.unwrap_or(&bcd.body),
        table,
        bcd.header,
        bcd.iv,
        bcd.random_state,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stand_in_table() -> KeyTable {
        // Not the game's table: a deterministic stand-in so crypto can be tested without game data.
        let mut x = 0x1234_5678u32;
        std::array::from_fn(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            x
        })
    }

    #[test]
    #[allow(clippy::identity_op)] // spelled out to match the formula in the docs
    fn xorshift_step() {
        let mut r = SeadRandom { s: [1, 2, 3, 4] };
        let mut t = 1u32 ^ (1 << 11);
        t ^= t >> 8;
        t ^= 4 ^ (4 >> 19);
        assert_eq!(r.next_u32(), t);
        assert_eq!(r.s, [2, 3, 4, t]);
    }

    #[test]
    fn round_trip_and_tamper() {
        let table = stand_in_table();
        let body: Vec<u8> = (0..BODY_SIZE).map(|i| (i * 7 % 251) as u8).collect();
        let data = encrypt_bcd(&body, &table, default_header(), [9; 16], [1, 2, 3, 4]).unwrap();
        assert_eq!(data.len(), BCD_SIZE);
        let dec = decrypt_bcd(&data, &table, true).unwrap();
        assert_eq!(dec.body, body);
        assert_eq!(reencrypt(&dec, &table, None).unwrap(), data);

        let mut bad = data.clone();
        bad[0x1000] ^= 0xFF;
        assert!(decrypt_bcd(&bad, &table, true).is_err());
        let mut wrong = table;
        wrong.iter_mut().for_each(|w| *w ^= 1);
        assert!(decrypt_bcd(&data, &wrong, true).is_err());
    }
}
