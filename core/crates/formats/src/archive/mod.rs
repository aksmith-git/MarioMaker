//! Nintendo container formats used by SMM2's RomFS: Yaz0, SARC, zstd.

pub mod sarc;
pub mod yaz0;

use crate::error::{Error, Result};

pub use sarc::Sarc;

/// zstd-compressed data (`.zs` files) → bytes.
pub fn zstd_decompress(data: &[u8]) -> Result<Vec<u8>> {
    use std::io::Read;
    let mut dec = ruzstd::decoding::StreamingDecoder::new(data).map_err(|e| Error::Format {
        what: "zstd",
        detail: e.to_string(),
    })?;
    let mut out = Vec::new();
    dec.read_to_end(&mut out).map_err(|e| Error::Format {
        what: "zstd",
        detail: e.to_string(),
    })?;
    Ok(out)
}

/// Strip any Yaz0 / zstd wrappers (in any order) and return the payload.
pub fn unwrap(mut data: Vec<u8>) -> Result<Vec<u8>> {
    for _ in 0..3 {
        if data.starts_with(b"Yaz0") {
            data = yaz0::decompress(&data)?;
        } else if data.starts_with(&[0x28, 0xB5, 0x2F, 0xFD]) {
            data = zstd_decompress(&data)?;
        } else {
            break;
        }
    }
    Ok(data)
}
