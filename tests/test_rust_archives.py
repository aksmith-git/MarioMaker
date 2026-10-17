"""Rust archive/BYML readers vs oead, on the owner's own RomFS (skipped when absent).

Compares `mmk sarc` (entry names, sizes, CRC32s) and `mmk byml` (decoded values) with oead for
Yaz0- and zstd-wrapped archives and every BYML in Pack/Static.pack.
"""

import json
import os
import subprocess
import tempfile
import unittest
import zlib
from pathlib import Path

from smmfmt import corpus

ROOT = Path(__file__).resolve().parent.parent
MMK = Path(os.environ.get("MMK_BIN", ROOT / "core/target/debug/mmk"))
ROMFS = corpus.SMM2_ROMFS["v3.0.3"]

try:
    import oead
except ImportError:  # CI has no oead and no game data
    oead = None


def plain(v):
    """oead BYML value → plain Python (int/float/str/bool/None/list/dict)."""
    if isinstance(v, (dict, oead.byml.Hash)):
        return {str(k): plain(x) for k, x in v.items()}
    if isinstance(v, (list, oead.byml.Array)):
        return [plain(x) for x in v]
    if isinstance(v, (bool, str)) or v is None:
        return v
    if isinstance(v, (oead.F32, float)):
        return float(v)
    if isinstance(v, (bytes, memoryview)):
        return {"binary": bytes(v).hex()}
    return int(v)


def unwrap(data: bytes) -> bytes:
    for _ in range(3):
        if data[:4] == b"Yaz0":
            data = bytes(oead.yaz0.decompress(data))
        elif data[:4] == b"\x28\xb5\x2f\xfd":
            import zstandard
            data = zstandard.ZstdDecompressor().decompress(data, max_output_size=512 << 20)
        else:
            break
    return data


@unittest.skipUnless(oead and MMK.is_file() and ROMFS.is_dir(), "needs oead, mmk and the owner's RomFS")
class ArchiveParityTests(unittest.TestCase):
    def test_sarc_listings(self):
        files = [ROMFS / "Pack/Static.pack", ROMFS / "Pack/M1_Model.pack", ROMFS / "Course/lesson_000.szs"]
        files += sorted((ROMFS / "Layout").glob("*.szs"))[:5]  # Yaz0-wrapped
        files += sorted((ROMFS / "Model").glob("M1_*.zs"))[:20]  # zstd-wrapped
        checked = 0
        for f in files:
            if not f.exists():
                continue
            with self.subTest(file=f.name):
                out = subprocess.run([str(MMK), "sarc", str(f)], capture_output=True, text=True, check=True).stdout
                rust = [json.loads(line) for line in out.splitlines()]
                ref = [{"name": x.name, "size": len(x.data), "crc32": zlib.crc32(bytes(x.data))}
                       for x in oead.Sarc(unwrap(f.read_bytes())).get_files()]
                self.assertEqual(rust, ref)
                checked += 1
        self.assertGreater(checked, 10)

    def test_all_static_pack_byml(self):
        pack = oead.Sarc((ROMFS / "Pack/Static.pack").read_bytes())
        bymls = [f for f in pack.get_files() if f.name.endswith(".byml")]
        self.assertGreater(len(bymls), 100)
        with tempfile.TemporaryDirectory() as tmp:
            for f in bymls:
                with self.subTest(file=f.name):
                    p = Path(tmp) / "x.byml"
                    p.write_bytes(bytes(f.data))
                    res = subprocess.run([str(MMK), "byml", str(p)], capture_output=True, text=True)
                    try:
                        ref = plain(oead.byml.from_binary(bytes(f.data)))
                    except oead.InvalidDataError:
                        # e.g. Param/MyWorldNgc/*Filter.byml are encrypted, not BYML: both must reject
                        self.assertNotEqual(res.returncode, 0)
                        continue
                    self.assertEqual(res.returncode, 0, res.stderr)
                    self.assertEqual(json.loads(res.stdout), ref)


if __name__ == "__main__":
    unittest.main()
