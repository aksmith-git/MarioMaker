//! Yaz0 (LZ77 variant) decompression.

use crate::error::{Error, Result};

pub fn decompress(data: &[u8]) -> Result<Vec<u8>> {
    let bad = |d: &str| Error::Format {
        what: "Yaz0",
        detail: d.to_string(),
    };
    if data.len() < 16 || &data[..4] != b"Yaz0" {
        return Err(bad("missing magic"));
    }
    let size = u32::from_be_bytes(data[4..8].try_into().unwrap()) as usize;
    let mut out = Vec::with_capacity(size);
    let mut src = 16;
    let byte = |i: usize| data.get(i).copied().ok_or_else(|| bad("truncated"));
    while out.len() < size {
        let code = byte(src)?;
        src += 1;
        for bit in (0..8).rev() {
            if out.len() >= size {
                break;
            }
            if code & (1 << bit) != 0 {
                out.push(byte(src)?);
                src += 1;
            } else {
                let b1 = byte(src)? as usize;
                let b2 = byte(src + 1)? as usize;
                src += 2;
                let dist = ((b1 & 0xF) << 8 | b2) + 1;
                let len = match b1 >> 4 {
                    0 => {
                        src += 1;
                        byte(src - 1)? as usize + 0x12
                    }
                    n => n + 2,
                };
                if dist > out.len() {
                    return Err(bad("back-reference before start"));
                }
                let start = out.len() - dist;
                for i in 0..len {
                    out.push(out[start + i]);
                }
            }
        }
    }
    out.truncate(size);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_and_backref() {
        // "abcabcabc": 3 literals, then a back-reference of length 6, distance 3.
        let mut d = b"Yaz0".to_vec();
        d.extend_from_slice(&9u32.to_be_bytes());
        d.extend_from_slice(&[0; 8]);
        d.push(0b1110_0000);
        d.extend_from_slice(b"abc");
        d.extend_from_slice(&[(6 - 2) << 4, 3 - 1]);
        assert_eq!(decompress(&d).unwrap(), b"abcabcabc");
    }
}
