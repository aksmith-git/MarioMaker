"""Round-trip tests against the owner's own data. Skipped when the data isn't present (e.g. CI).

Byte-exact means: parse -> write produces the identical bytes, and for .bcd files,
decrypt -> parse -> write -> re-encrypt with the original IV/random state produces the identical file.
"""

import unittest
import zlib

from smmfmt import corpus
from smmfmt.smm1 import Course as Smm1Course
from smmfmt.smm2 import Course, GameStyle
from smmfmt.smm2.crypto import decrypt_bcd, reencrypt

STYLE_BY_DATASET = {0: "M1", 1: "M3", 2: "MW", 3: "WU", 4: "3W"}


class DatasetCourseTests(unittest.TestCase):
    def setUp(self):
        self.courses = list(corpus.dataset_courses())
        if not self.courses:
            self.skipTest("dataset sample not present")

    def test_round_trip_and_metadata(self):
        for meta, data in self.courses:
            with self.subTest(data_id=meta["data_id"]):
                c = Course.from_bytes(data)
                self.assertEqual(c.to_bytes(), data)
                self.assertEqual(c.header.game_style, STYLE_BY_DATASET[meta["gamestyle"]])
                self.assertEqual(c.header.name, meta["name"])
                self.assertEqual(c.header.timer, meta["timer"])
                self.assertEqual(c.main.header.theme, meta["theme"])
                self.assertEqual(c.header.clear_condition_magnitude, meta["clear_condition_magnitude"])
                check_slot_invariants(self, c, meta["data_id"])

    def test_counts_within_capacity_and_ids_known(self):
        for meta, data in self.courses:
            c = Course.from_bytes(data)
            for area in c.areas:
                for o in area.objects:
                    self.assertTrue(0 <= o.id <= 132, (meta["data_id"], o.id))
                    self.assertTrue(o.child_id == -1 or 0 <= o.child_id <= 132, (meta["data_id"], o.child_id))


PATH_ARRAYS = ("snake_blocks", "clear_pipes", "piranha_creepers", "exclamation_blocks", "track_blocks")


def check_slot_invariants(test, c, label):
    """Path arrays: live count == stored count, and every entry sits in the slot named by its index."""
    from smmfmt.smm2.course import AREA_ARRAYS
    counts = {a[0]: a[1] for a in AREA_ARRAYS}
    for area in c.areas:
        for attr in PATH_ARRAYS:
            arr = getattr(area, attr)
            test.assertEqual(len(arr), getattr(area.header, counts[attr]), (label, attr))
            for slot, rec in arr.slots.items():
                test.assertEqual(rec.index, slot, (label, attr))


class BuiltinSmm2Tests(unittest.TestCase):
    def setUp(self):
        self.table = corpus.course_table()
        if self.table is None:
            self.skipTest("course key table not extracted (see smmfmt.smm2.keyfind)")

    def _check_version(self, version):
        files = list(corpus.smm2_builtin_bcds(version))
        if not files:
            self.skipTest(f"SMM2 {version} romfs not present")
        for name, data in files:
            with self.subTest(file=name):
                bcd = decrypt_bcd(data, self.table)  # verifies CRC32 and CMAC
                c = Course.from_bytes(bcd.body)
                self.assertIn(c.header.game_style, GameStyle.ALL)
                check_slot_invariants(self, c, name)
                body = c.to_bytes()
                self.assertEqual(body, bcd.body)
                self.assertEqual(reencrypt(bcd, self.table, body), data)
        return len(files)

    def test_v303(self):
        self.assertEqual(self._check_version("v3.0.3"), 180)

    def test_v100(self):
        self.assertEqual(self._check_version("v1.0.0"), 180)


class BuiltinSmm1Tests(unittest.TestCase):
    def _check(self, version, expected):
        files = list(corpus.smm1_builtin_cdts(version))
        if not files:
            self.skipTest(f"SMM1 {version} content not present")
        self.assertEqual(len(files), expected)
        for name, data in files:
            with self.subTest(file=name):
                self.assertTrue(Smm1Course.stored_crc_ok(data))
                c = Smm1Course.from_bytes(data)
                self.assertIn(c.header.game_style, ("M1", "M3", "MW", "WU"))
                self.assertEqual(c.header.version, 0xB)
                self.assertEqual(c.to_bytes(), data)

    def test_v16(self):
        self._check("v16", 152)

    def test_v208_overrides(self):
        self._check("v208", 4)


if __name__ == "__main__":
    unittest.main()
