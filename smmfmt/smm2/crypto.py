"""SMM2 on-console course file (.bcd) encryption.

Layout (0x5C000 bytes):
    0x00000  16-byte header: u32 version, u16, u16, u32 CRC32 of the decrypted body, "SCDL"
    0x00010  body, AES-128-CBC (0x5BFC0 bytes, no padding)
    0x5BFD0  AES IV (16 bytes)
    0x5BFE0  random state, 4 x u32 little-endian
    0x5BFF0  AES-CMAC of the decrypted body

The AES and CMAC keys are drawn from a 64-word constant table using a xorshift128 generator
seeded from the random state. The table is NOT part of this project: it is read at runtime from
the user's own copy of the game (see keyfind.py) and passed in as `table`.
"""

from __future__ import annotations

import os
import struct
import zlib
from dataclasses import dataclass

from cryptography.hazmat.primitives.ciphers import Cipher, algorithms, modes
from cryptography.hazmat.primitives.cmac import CMAC

BCD_SIZE = 0x5C000
HEADER_SIZE = 0x10
BODY_SIZE = 0x5BFC0
FOOTER_OFFSET = HEADER_SIZE + BODY_SIZE
MAGIC = b"SCDL"
TABLE_WORDS = 64


class CourseCryptoError(ValueError):
    pass


class SeadRandom:
    """xorshift128 as used by the game's random-number class."""

    def __init__(self, state: tuple[int, int, int, int]):
        self.s = list(state)

    def next(self) -> int:
        s0, s1, s2, s3 = self.s
        t = (s0 ^ (s0 << 11)) & 0xFFFFFFFF
        t ^= t >> 8
        t ^= s3 ^ (s3 >> 19)
        self.s = [s1, s2, s3, t]
        return t


def key_schedule(state: tuple[int, int, int, int], count: int = 2) -> list[list[tuple[int, int]]]:
    """For each key, the 16 (table index, bit shift) pairs that select its bytes.

    Depends only on the random state, not on the table, which lets keyfind test many candidate
    tables cheaply.
    """
    rng = SeadRandom(state)
    keys = []
    for _ in range(count):
        picks = []
        for _ in range(16):
            idx = rng.next() >> 26
            shift = (rng.next() >> 27) & 24
            picks.append((idx, shift))
        keys.append(picks)
    return keys


def key_from_picks(picks: list[tuple[int, int]], table) -> bytes:
    out = bytearray()
    for w in range(4):
        word = 0
        for idx, shift in picks[w * 4:(w + 1) * 4]:
            word = (word << 8) | ((table[idx] >> shift) & 0xFF)
        out += struct.pack("<I", word)
    return bytes(out)


def derive_keys(state: tuple[int, int, int, int], table) -> tuple[bytes, bytes]:
    aes_picks, cmac_picks = key_schedule(state, 2)
    return key_from_picks(aes_picks, table), key_from_picks(cmac_picks, table)


def table_from_bytes(data: bytes) -> tuple[int, ...]:
    if len(data) != TABLE_WORDS * 4:
        raise ValueError("key table must be 256 bytes")
    return struct.unpack(f"<{TABLE_WORDS}I", data)


def _cmac(key: bytes, data: bytes) -> bytes:
    c = CMAC(algorithms.AES(key))
    c.update(data)
    return c.finalize()


@dataclass
class BcdFile:
    header: bytes        # 16 bytes as stored (CRC is refreshed on encrypt)
    body: bytes          # decrypted course data, 0x5BFC0 bytes
    iv: bytes
    random_state: tuple[int, int, int, int]
    cmac: bytes


def decrypt_bcd(data: bytes, table, verify: bool = True) -> BcdFile:
    if len(data) != BCD_SIZE:
        raise CourseCryptoError(f".bcd must be {BCD_SIZE:#x} bytes, got {len(data):#x}")
    header = data[:HEADER_SIZE]
    if header[0xC:0x10] != MAGIC:
        raise CourseCryptoError("missing SCDL magic")
    iv = data[FOOTER_OFFSET:FOOTER_OFFSET + 16]
    state = struct.unpack_from("<4I", data, FOOTER_OFFSET + 16)
    mac = data[FOOTER_OFFSET + 32:FOOTER_OFFSET + 48]
    aes_key, cmac_key = derive_keys(state, table)
    dec = Cipher(algorithms.AES(aes_key), modes.CBC(iv)).decryptor()
    body = dec.update(data[HEADER_SIZE:FOOTER_OFFSET]) + dec.finalize()
    if verify:
        crc, = struct.unpack_from("<I", header, 8)
        if zlib.crc32(body) != crc:
            raise CourseCryptoError("CRC32 mismatch (wrong key table or corrupt file)")
        if _cmac(cmac_key, body) != mac:
            raise CourseCryptoError("CMAC mismatch (wrong key table or corrupt file)")
    return BcdFile(header, body, iv, state, mac)


def encrypt_bcd(body: bytes, table, *, header: bytes | None = None, iv: bytes | None = None,
                random_state: tuple[int, int, int, int] | None = None) -> bytes:
    """Encrypt course data. IV and random state default to fresh random values, as the game does."""
    if len(body) != BODY_SIZE:
        raise CourseCryptoError(f"body must be {BODY_SIZE:#x} bytes")
    if header is None:
        header = struct.pack("<IHHI4s", 1, 0x10, 0, 0, MAGIC)
    iv = iv if iv is not None else os.urandom(16)
    if random_state is None:
        random_state = struct.unpack("<4I", os.urandom(16))
    aes_key, cmac_key = derive_keys(random_state, table)
    enc = Cipher(algorithms.AES(aes_key), modes.CBC(iv)).encryptor()
    ct = enc.update(body) + enc.finalize()
    hdr = bytearray(header)
    struct.pack_into("<I", hdr, 8, zlib.crc32(body))
    return bytes(hdr) + ct + iv + struct.pack("<4I", *random_state) + _cmac(cmac_key, body)


def reencrypt(bcd: BcdFile, table, body: bytes | None = None) -> bytes:
    """Write a decrypted file back with its original IV and random state."""
    return encrypt_bcd(bcd.body if body is None else body, table, header=bcd.header, iv=bcd.iv,
                       random_state=bcd.random_state)
