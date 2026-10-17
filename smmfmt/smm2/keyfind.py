"""Locate the course key table inside the user's own SMM2 executable.

No key material ships with this project. Instead, given the game's main executable (as an ELF,
e.g. from nx2elf) and any one encrypted .bcd from the same dump, this scans the executable's
read-only and data segments for the 64-word window that decrypts the .bcd correctly.

Cheap filter first: the key bytes depend only on 16 table entries chosen by the file's random
state, and decrypting the single AES block at body offset 0xF0 must reveal a game-style code
("M1", "M3", "MW", "WU", "3W") followed by NUL. Survivors are then checked with the full CRC32
and CMAC.

    python -I -m smmfmt.smm2.keyfind --elf main.elf --bcd lesson_000.bcd --out course_table.bin
"""

from __future__ import annotations

import argparse
import struct
import sys
from pathlib import Path

from cryptography.hazmat.primitives.ciphers import Cipher, algorithms, modes

from .crypto import (FOOTER_OFFSET, HEADER_SIZE, TABLE_WORDS, CourseCryptoError, decrypt_bcd,
                     key_from_picks, key_schedule)

STYLE_CODES = (b"M1", b"M3", b"MW", b"WU", b"3W")


def elf_segments(elf: bytes) -> list[tuple[int, bytes]]:
    """(file offset, bytes) of each loadable segment that isn't executable."""
    if elf[:4] != b"\x7fELF" or elf[4] != 2:
        raise ValueError("expected a 64-bit ELF")
    phoff, = struct.unpack_from("<Q", elf, 0x20)
    phentsize, phnum = struct.unpack_from("<HH", elf, 0x36)
    segs = []
    for i in range(phnum):
        p_type, p_flags, p_offset, _vaddr, _paddr, p_filesz = struct.unpack_from("<IIQQQQ", elf, phoff + i * phentsize)
        if p_type == 1 and not p_flags & 1:  # PT_LOAD, not executable
            segs.append((p_offset, elf[p_offset:p_offset + p_filesz]))
    return segs


def find_table(elf: bytes, bcd: bytes) -> tuple[int, bytes]:
    state = struct.unpack_from("<4I", bcd, FOOTER_OFFSET + 16)
    aes_picks = key_schedule(state, 1)[0]
    needed = sorted({idx for idx, _ in aes_picks})
    # Body block 15 holds bytes 0xF0..0xFF; CBC needs ciphertext block 14 as its chaining value.
    c14 = bcd[HEADER_SIZE + 0xE0:HEADER_SIZE + 0xF0]
    c15 = bcd[HEADER_SIZE + 0xF0:HEADER_SIZE + 0x100]

    for seg_off, seg in elf_segments(elf):
        n_words = len(seg) // 4
        words = struct.unpack_from(f"<{n_words}I", seg)
        for start in range(0, n_words - TABLE_WORDS + 1):
            window = words[start:start + TABLE_WORDS]
            if not all(window[i] for i in needed):
                continue
            key = key_from_picks(aes_picks, window)
            block = Cipher(algorithms.AES(key), modes.ECB()).decryptor().update(c15)
            plain = bytes(a ^ b for a, b in zip(block, c14))
            if plain[1:3] in STYLE_CODES and plain[3] == 0:
                table_bytes = seg[start * 4:(start + TABLE_WORDS) * 4]
                try:
                    decrypt_bcd(bcd, window, verify=True)
                except CourseCryptoError:
                    continue
                return seg_off + start * 4, table_bytes
    raise LookupError("course key table not found in this executable")


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--elf", type=Path, required=True)
    ap.add_argument("--bcd", type=Path, required=True, help="any encrypted .bcd from the same dump")
    ap.add_argument("--out", type=Path, required=True)
    args = ap.parse_args()
    offset, table = find_table(args.elf.read_bytes(), args.bcd.read_bytes())
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_bytes(table)
    print(f"found course key table at ELF file offset {offset:#x}; wrote {args.out}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
