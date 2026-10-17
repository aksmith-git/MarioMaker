"""Parity between the Rust `mmk-formats` crate and this Python reference implementation.

Runs the Rust CLI (`mmk dump`) on course files and checks that (a) Rust round-trips them
byte-exactly and (b) every parsed field matches `smmfmt`. Synthetic courses run everywhere
(CI included); the owner's corpus runs locally when present.

The binary is taken from $MMK_BIN, else core/target/debug/mmk; tests skip if it is missing.
"""

import json
import os
import random
import subprocess
import tempfile
import unittest
from pathlib import Path

from smmfmt import corpus, dump
from smmfmt.smm1 import Course as Smm1Course
from smmfmt.smm2 import Course
from smmfmt.smm2.crypto import decrypt_bcd, encrypt_bcd
from tests.test_smm1_course import make_cdt
from tests.test_smm2_course import make_course

ROOT = Path(__file__).resolve().parent.parent
MMK = Path(os.environ.get("MMK_BIN", ROOT / "core/target/debug/mmk"))


def run_mmk(files, table=None):
    cmd = [str(MMK), "dump", *map(str, files)]
    if table:
        cmd += ["--table", str(table)]
    out = subprocess.run(cmd, capture_output=True, text=True)
    return [json.loads(line) for line in out.stdout.splitlines()]


@unittest.skipUnless(MMK.is_file(), "mmk binary not built (cargo build in core/)")
class SyntheticParityTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.dir = Path(self.tmp.name)

    def tearDown(self):
        self.tmp.cleanup()

    def test_smm2_course(self):
        data = make_course().to_bytes()
        p = self.dir / "c.course"
        p.write_bytes(data)
        [res] = run_mmk([p])
        self.assertTrue(res["roundtrip_ok"], res)
        self.assertEqual(res["course"], dump.smm2_course(Course.from_bytes(data)))

    def test_smm2_bcd_with_stand_in_table(self):
        table = tuple(random.Random(99).getrandbits(32) for _ in range(64))
        (self.dir / "t.bin").write_bytes(b"".join(w.to_bytes(4, "little") for w in table))
        body = make_course().to_bytes()
        p = self.dir / "c.bcd"
        p.write_bytes(encrypt_bcd(body, table))
        [res] = run_mmk([p], self.dir / "t.bin")
        self.assertTrue(res["roundtrip_ok"], res)
        self.assertEqual(res["course"], dump.smm2_course(Course.from_bytes(body)))

    def test_smm1_cdt(self):
        data = make_cdt()
        p = self.dir / "c.cdt"
        p.write_bytes(data)
        [res] = run_mmk([p])
        self.assertTrue(res["roundtrip_ok"] and res["crc_ok"], res)
        self.assertEqual(res["course"], dump.smm1_course(Smm1Course.from_bytes(data)))


@unittest.skipUnless(MMK.is_file(), "mmk binary not built (cargo build in core/)")
class CorpusParityTests(unittest.TestCase):
    """Every course in the owner's local corpus: Rust round trip + field-by-field parity."""

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.dir = Path(self.tmp.name)

    def tearDown(self):
        self.tmp.cleanup()

    def _check(self, items, expected_fn, table=None):
        paths = []
        for i, (name, data) in enumerate(items):
            p = self.dir / f"{i:04d}_{Path(name).name}"
            p.write_bytes(data)
            paths.append((p, data))
        results = run_mmk([p for p, _ in paths], table)
        self.assertEqual(len(results), len(paths))
        for (p, data), res in zip(paths, results):
            with self.subTest(file=p.name):
                self.assertTrue(res.get("roundtrip_ok"), res.get("error"))
                self.assertEqual(res["course"], expected_fn(data))
        return len(paths)

    def test_dataset(self):
        items = [(f"{m['data_id']}.course", d) for m, d in corpus.dataset_courses()]
        if not items:
            self.skipTest("dataset sample not present")
        self.assertEqual(self._check(items, lambda d: dump.smm2_course(Course.from_bytes(d))), len(items))

    def test_smm2_builtin(self):
        table = corpus.course_table()
        items = list(corpus.smm2_builtin_bcds("v3.0.3")) + list(corpus.smm2_builtin_bcds("v1.0.0"))
        if table is None or not items:
            self.skipTest("built-in courses or key table not present")
        expect = lambda d: dump.smm2_course(Course.from_bytes(decrypt_bcd(d, table).body))  # noqa: E731
        self.assertEqual(self._check(items, expect, corpus.COURSE_TABLE), 360)

    def test_smm1_builtin(self):
        items = list(corpus.smm1_builtin_cdts("v16")) + list(corpus.smm1_builtin_cdts("v208"))
        if not items:
            self.skipTest("SMM1 content not present")
        self.assertEqual(self._check(items, lambda d: dump.smm1_course(Smm1Course.from_bytes(d))), 156)


if __name__ == "__main__":
    unittest.main()
