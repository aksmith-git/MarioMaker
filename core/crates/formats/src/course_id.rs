//! Course codes as shown to players. See `smmfmt/course_id.py` and docs/formats/.

use crate::error::{Error, Result};

const ALPHABET: &[u8; 30] = b"0123456789BCDFGHJKLMNPQRSTVWXY";
const XOR_KEY: u64 = 0x1680_E07C;

/// SMM2 9-character code → (data_id, is_maker_id).
pub fn smm2_code_to_data_id(code: &str) -> Result<(u32, bool)> {
    let s: Vec<u8> = code
        .trim()
        .bytes()
        .filter(|&b| b != b'-')
        .map(|b| b.to_ascii_uppercase())
        .collect();
    let bad = || Error::InvalidCode(code.to_string());
    if s.len() != 9 {
        return Err(bad());
    }
    let mut n: u64 = 0;
    for &c in s.iter().rev() {
        let d = ALPHABET.iter().position(|&a| a == c).ok_or_else(bad)? as u64;
        n = n * 30 + d;
    }
    let low = n & 0xFFF;
    let always_one = (n >> 12) & 1;
    let is_maker = (n >> 13) & 1 == 1;
    let mid = (n >> 14) & 0xFFFFF;
    let check = (n >> 34) & 0x3F;
    let always_eight = (n >> 40) & 0xF;
    let data_id = ((low << 20) | mid) ^ XOR_KEY;
    if always_one != 1 || always_eight != 8 || check != (data_id.wrapping_sub(31)) % 64 {
        return Err(bad());
    }
    Ok((data_id as u32, is_maker))
}

pub fn smm2_data_id_to_code(data_id: u32, is_maker: bool) -> String {
    let id = data_id as u64;
    let x = id ^ XOR_KEY;
    // (data_id - 31) % 64 with Python semantics (non-negative result)
    let check = (id as i64 - 31).rem_euclid(64) as u64;
    let mut n = (8u64 << 40)
        | (check << 34)
        | ((x & 0xFFFFF) << 14)
        | ((is_maker as u64) << 13)
        | (1 << 12)
        | (x >> 20);
    let mut chars = Vec::with_capacity(9);
    for _ in 0..9 {
        chars.push(ALPHABET[(n % 30) as usize] as char);
        n /= 30;
    }
    let s: String = chars.into_iter().collect();
    format!("{}-{}-{}", &s[0..3], &s[3..6], &s[6..9])
}

/// SMM1 code `CCCC-0000-HHHH-HHHH` → data id (last 8 hex digits). The checksum is not verified.
pub fn smm1_code_to_data_id(code: &str) -> Result<u32> {
    let parts: Vec<&str> = code.trim().split('-').collect();
    let ok = parts.len() == 4
        && parts
            .iter()
            .all(|p| p.len() == 4 && p.chars().all(|c| c.is_ascii_hexdigit()));
    if !ok {
        return Err(Error::InvalidCode(code.to_string()));
    }
    u32::from_str_radix(&format!("{}{}", parts[2], parts[3]), 16)
        .map_err(|_| Error::InvalidCode(code.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_code() {
        assert_eq!(
            smm2_code_to_data_id("2J5-3K2-Y9G").unwrap(),
            (6635842, false)
        );
        assert_eq!(smm2_data_id_to_code(6635842, false), "2J5-3K2-Y9G");
    }

    #[test]
    fn round_trip_range() {
        for id in (3_000_000..3_000_500).chain([31, 63, 64, 0xFFFFF, 50_000_000]) {
            for maker in [false, true] {
                assert_eq!(
                    smm2_code_to_data_id(&smm2_data_id_to_code(id, maker)).unwrap(),
                    (id, maker)
                );
            }
        }
    }

    #[test]
    fn rejects() {
        assert!(smm2_code_to_data_id("2J5-3K2-Y9H").is_err());
        assert!(smm2_code_to_data_id("AAA-AAA-AAA").is_err());
        assert_eq!(
            smm1_code_to_data_id("1A2B-0000-0012-3456").unwrap(),
            0x0012_3456
        );
        assert!(smm1_code_to_data_id("not-a-code").is_err());
    }
}
