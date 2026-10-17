"""Crypto tests use a random stand-in table; the real one only ever comes from the user's dump."""

import random
import struct
import unittest

from smmfmt.smm2.crypto import (BCD_SIZE, CourseCryptoError, SeadRandom, decrypt_bcd, encrypt_bcd,
                                reencrypt)
from tests.test_smm2_course import make_course

TABLE = tuple(random.Random(1234).getrandbits(32) for _ in range(64))


class SeadRandomTests(unittest.TestCase):
    def test_xorshift_step(self):
        r = SeadRandom((1, 2, 3, 4))
        t = (1 ^ (1 << 11)) & 0xFFFFFFFF
        t ^= t >> 8
        t ^= 4 ^ (4 >> 19)
        self.assertEqual(r.next(), t)
        self.assertEqual(r.s, [2, 3, 4, t])


class BcdTests(unittest.TestCase):
    def setUp(self):
        self.body = make_course().to_bytes()

    def test_round_trip(self):
        data = encrypt_bcd(self.body, TABLE)
        self.assertEqual(len(data), BCD_SIZE)
        self.assertEqual(data[0xC:0x10], b"SCDL")
        self.assertNotEqual(data[0x10:0x110], self.body[:0x100])
        self.assertEqual(decrypt_bcd(data, TABLE).body, self.body)

    def test_reencrypt_is_byte_identical(self):
        data = encrypt_bcd(self.body, TABLE)
        self.assertEqual(reencrypt(decrypt_bcd(data, TABLE), TABLE), data)

    def test_wrong_table_rejected(self):
        data = encrypt_bcd(self.body, TABLE)
        wrong = tuple((w ^ 1) for w in TABLE)
        with self.assertRaises(CourseCryptoError):
            decrypt_bcd(data, wrong)

    def test_tampered_body_rejected(self):
        data = bytearray(encrypt_bcd(self.body, TABLE))
        data[0x1000] ^= 0xFF
        with self.assertRaises(CourseCryptoError):
            decrypt_bcd(bytes(data), TABLE)

    def test_header_crc(self):
        import zlib
        data = encrypt_bcd(self.body, TABLE)
        self.assertEqual(struct.unpack_from("<I", data, 8)[0], zlib.crc32(self.body))


if __name__ == "__main__":
    unittest.main()
