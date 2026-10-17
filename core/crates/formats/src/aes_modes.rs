//! AES-128-CBC (no padding) and AES-CMAC (RFC 4493) on top of the `aes` block cipher.

use aes::Aes128;
use aes::cipher::{Array, BlockCipherDecrypt, BlockCipherEncrypt, KeyInit};

fn cipher(key: &[u8; 16]) -> Aes128 {
    Aes128::new(&Array::from(*key))
}

fn xor_into(dst: &mut [u8; 16], src: &[u8]) {
    for (d, s) in dst.iter_mut().zip(src) {
        *d ^= s;
    }
}

/// Decrypt `data` (length a multiple of 16) in CBC mode.
pub fn cbc_decrypt(key: &[u8; 16], iv: &[u8; 16], data: &[u8]) -> Vec<u8> {
    assert_eq!(data.len() % 16, 0);
    let c = cipher(key);
    let mut prev = *iv;
    let mut out = Vec::with_capacity(data.len());
    for chunk in data.as_chunks::<16>().0 {
        let mut block = Array::from(*chunk);
        c.decrypt_block(&mut block);
        let mut plain: [u8; 16] = block.into();
        xor_into(&mut plain, &prev);
        out.extend_from_slice(&plain);
        prev.copy_from_slice(chunk);
    }
    out
}

/// Encrypt `data` (length a multiple of 16) in CBC mode.
pub fn cbc_encrypt(key: &[u8; 16], iv: &[u8; 16], data: &[u8]) -> Vec<u8> {
    assert_eq!(data.len() % 16, 0);
    let c = cipher(key);
    let mut prev = *iv;
    let mut out = Vec::with_capacity(data.len());
    for chunk in data.as_chunks::<16>().0 {
        let mut x = prev;
        xor_into(&mut x, chunk);
        let mut block = Array::from(x);
        c.encrypt_block(&mut block);
        prev = block.into();
        out.extend_from_slice(&prev);
    }
    out
}

fn dbl(b: [u8; 16]) -> [u8; 16] {
    let mut out = [0u8; 16];
    let mut carry = 0u8;
    for i in (0..16).rev() {
        out[i] = (b[i] << 1) | carry;
        carry = b[i] >> 7;
    }
    if b[0] & 0x80 != 0 {
        out[15] ^= 0x87;
    }
    out
}

/// AES-CMAC (RFC 4493).
pub fn cmac(key: &[u8; 16], msg: &[u8]) -> [u8; 16] {
    let c = cipher(key);
    let mut l = Array::from([0u8; 16]);
    c.encrypt_block(&mut l);
    let k1 = dbl(l.into());
    let k2 = dbl(k1);

    let n = msg.len().div_ceil(16).max(1);
    let complete = !msg.is_empty() && msg.len().is_multiple_of(16);
    let mut last = [0u8; 16];
    let tail = &msg[(n - 1) * 16..];
    if complete {
        last.copy_from_slice(tail);
        xor_into(&mut last, &k1);
    } else {
        last[..tail.len()].copy_from_slice(tail);
        last[tail.len()] = 0x80;
        xor_into(&mut last, &k2);
    }
    let mut x = [0u8; 16];
    for chunk in msg.chunks(16).take(n - 1) {
        xor_into(&mut x, chunk);
        let mut b = Array::from(x);
        c.encrypt_block(&mut b);
        x = b.into();
    }
    xor_into(&mut x, &last);
    let mut b = Array::from(x);
    c.encrypt_block(&mut b);
    b.into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    // RFC 4493 section 4 test vectors.
    const KEY: &str = "2b7e151628aed2a6abf7158809cf4f3c";
    const MSG: &str = "6bc1bee22e409f96e93d7e117393172aae2d8a571e03ac9c9eb76fac45af8e5130c81c46a35ce411e5fbc1191a0a52eff69f2445df4f9b17ad2b417be66c3710";

    #[test]
    fn cmac_rfc4493() {
        let key: [u8; 16] = hex(KEY).try_into().unwrap();
        let msg = hex(MSG);
        for (len, expect) in [
            (0, "bb1d6929e95937287fa37d129b756746"),
            (16, "070a16b46b4d4144f79bdd9dd04a287c"),
            (40, "dfa66747de9ae63030ca32611497c827"),
            (64, "51f0bebf7e3b9d92fc49741779363cfe"),
        ] {
            assert_eq!(cmac(&key, &msg[..len]).to_vec(), hex(expect), "len {len}");
        }
    }

    // NIST SP 800-38A F.2.1 CBC-AES128.
    #[test]
    fn cbc_sp800_38a() {
        let key: [u8; 16] = hex(KEY).try_into().unwrap();
        let iv: [u8; 16] = hex("000102030405060708090a0b0c0d0e0f").try_into().unwrap();
        let pt = hex(MSG);
        let ct = hex(
            "7649abac8119b246cee98e9b12e9197d5086cb9b507219ee95db113a917678b273bed6b8e3c1743b7116e69e222295163ff1caa1681fac09120eca307586e1a7",
        );
        assert_eq!(cbc_encrypt(&key, &iv, &pt), ct);
        assert_eq!(cbc_decrypt(&key, &iv, &ct), pt);
    }
}
