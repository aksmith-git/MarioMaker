//! Course file bytes in any accepted form → course data.

use std::io::Read;

use mmk_formats::smm2::{BCD_SIZE, Course, crypto};

/// Course bytes → decrypted course data (0x5BFC0 bytes), and whether they came from an encrypted
/// `.bcd`. Accepts SMM2 course data, optionally zlib-compressed, or a `.bcd`; the latter needs the
/// key table (`keys/course_table.bin` of an imported cache), which `key_table` supplies on demand.
pub fn decode_data(
    bytes: &[u8],
    key_table: impl FnOnce() -> Result<Vec<u8>, String>,
) -> Result<(Vec<u8>, bool), String> {
    let mut data = bytes.to_vec();
    if data.len() > 2 && data[0] == 0x78 {
        let mut out = Vec::new();
        flate2::read::ZlibDecoder::new(&data[..])
            .read_to_end(&mut out)
            .map_err(|e| format!("zlib: {e}"))?;
        data = out;
    }
    let mut encrypted = false;
    if data.len() == BCD_SIZE {
        let raw = key_table()?;
        let table = crypto::table_from_bytes(&raw).map_err(|e| e.to_string())?;
        data = crypto::decrypt_bcd(&data, &table, true)
            .map_err(|e| e.to_string())?
            .body;
        encrypted = true;
    }
    Ok((data, encrypted))
}

/// Course bytes → parsed course (see [`decode_data`]).
pub fn decode(
    bytes: &[u8],
    key_table: impl FnOnce() -> Result<Vec<u8>, String>,
) -> Result<Course, String> {
    let (data, _) = decode_data(bytes, key_table)?;
    Course::parse(&data).map_err(|e| e.to_string())
}

/// Path of the key table inside an asset cache.
pub const KEY_TABLE: &str = "keys/course_table.bin";

/// The message used when a `.bcd` arrives without imported game data.
pub fn no_key_table(detail: impl std::fmt::Display) -> String {
    format!("encrypted course: needs imported game data ({detail})")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn raw_and_zlib_courses_decode() {
        let raw = Course::blank("M1").to_bytes().unwrap();
        let none = || Err("no keys".to_string());
        assert_eq!(decode_data(&raw, none).unwrap(), (raw.clone(), false));
        let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
        z.write_all(&raw).unwrap();
        let packed = z.finish().unwrap();
        assert_eq!(packed[0], 0x78);
        let c = decode(&packed, none).unwrap();
        assert_eq!(c.header.game_style.as_str_lossy(), "M1");
    }

    #[test]
    fn encrypted_course_asks_for_the_key_table() {
        let bcd = vec![0u8; BCD_SIZE];
        let err = decode_data(&bcd, || Err(no_key_table("missing"))).unwrap_err();
        assert!(err.contains("needs imported game data"), "{err}");
    }
}
