"""Course codes as shown to players.

SMM2: 9 characters from a 30-symbol alphabet (no vowels, no Z), e.g. "2J5-3K2-Y9G". The code
packs the server data ID with a few check bits.

SMM1: "CCCC-0000-HHHH-HHHH", where the last 8 hex digits are the data ID and CCCC is a checksum
keyed with the game server's access key. We only parse SMM1 codes; generating the checksum is out
of scope.
"""

from __future__ import annotations

import re

ALPHABET = "0123456789BCDFGHJKLMNPQRSTVWXY"
XOR_KEY = 0x1680E07C


class InvalidCourseCode(ValueError):
    pass


def smm2_code_to_data_id(code: str) -> tuple[int, bool]:
    """Return (data_id, is_maker_id)."""
    s = code.replace("-", "").strip().upper()
    if len(s) != 9 or any(c not in ALPHABET for c in s):
        raise InvalidCourseCode(code)
    n = 0
    for c in reversed(s):
        n = n * 30 + ALPHABET.index(c)
    low = n & 0xFFF
    always_one = (n >> 12) & 1
    is_maker = bool((n >> 13) & 1)
    mid = (n >> 14) & 0xFFFFF
    check = (n >> 34) & 0x3F
    always_eight = (n >> 40) & 0xF
    data_id = ((low << 20) | mid) ^ XOR_KEY
    if always_one != 1 or always_eight != 8 or check != (data_id - 31) % 64:
        raise InvalidCourseCode(code)
    return data_id, is_maker


def smm2_data_id_to_code(data_id: int, is_maker: bool = False) -> str:
    x = data_id ^ XOR_KEY
    n = (8 << 40) | (((data_id - 31) % 64) << 34) | ((x & 0xFFFFF) << 14) | (int(is_maker) << 13) | (1 << 12) | (x >> 20)
    chars = []
    for _ in range(9):
        n, d = divmod(n, 30)
        chars.append(ALPHABET[d])
    s = "".join(chars)
    return f"{s[0:3]}-{s[3:6]}-{s[6:9]}"


_SMM1 = re.compile(r"^([0-9A-F]{4})-([0-9A-F]{4})-([0-9A-F]{4})-([0-9A-F]{4})$")


def smm1_code_to_data_id(code: str) -> int:
    m = _SMM1.match(code.strip().upper())
    if not m:
        raise InvalidCourseCode(code)
    return int(m.group(3) + m.group(4), 16)
