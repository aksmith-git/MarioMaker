import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from check_no_proprietary import check_file  # noqa: E402


class CheckFileTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.dir = Path(self.tmp.name)

    def tearDown(self):
        self.tmp.cleanup()

    def write(self, name: str, data: bytes) -> Path:
        p = self.dir / name
        p.write_bytes(data)
        return p

    def test_clean_source_file_passes(self):
        self.assertEqual(check_file(self.write("main.rs", b"fn main() {}\n")), [])

    def test_blocked_extension(self):
        self.assertTrue(check_file(self.write("course.bcd", b"\x00" * 16)))

    def test_blocked_name(self):
        self.assertTrue(check_file(self.write("prod.keys", b"")))

    def test_renamed_sarc_caught_by_magic(self):
        self.assertTrue(check_file(self.write("level.dat", b"SARC\x00\x14\xfe\xff" + b"\x00" * 32)))

    def test_renamed_zstd_caught_by_magic(self):
        self.assertTrue(check_file(self.write("thing.bin", b"\x28\xb5\x2f\xfd" + b"\x00" * 32)))

    def test_xci_magic_at_offset(self):
        self.assertTrue(check_file(self.write("game.img", b"\x00" * 0x100 + b"HEAD" + b"\x00" * 16)))

    def test_wiiu_rpx_detected(self):
        head = b"\x7fELF\x01\x02\x01\xca\xfe" + b"\x00" * 64
        self.assertTrue(check_file(self.write("cemu.elf", head)))

    def test_key_lines_detected(self):
        body = b"header_key = " + b"ab" * 32 + b"\n"
        self.assertTrue(check_file(self.write("notes.txt", body)))

    def test_aarch64_elf_detected(self):
        head = b"\x7fELF\x02\x01\x01" + b"\x00" * 11 + b"\xb7\x00" + b"\x00" * 40
        self.assertTrue(check_file(self.write("main.bin", head)))

    def test_native_binaries_and_spirv_blocked(self):
        self.assertTrue(check_file(self.write("game.dylib", b"\x00")))
        self.assertTrue(check_file(self.write("blob.bin", b"\xcf\xfa\xed\xfe" + b"\x00" * 16)))
        self.assertTrue(check_file(self.write("shader.dat", b"\x03\x02\x23\x07" + b"\x00" * 16)))

    def test_recomp_private_and_generated_paths_blocked(self):
        for sub in ("recomp/private", "recomp/generated"):
            d = self.dir / sub
            d.mkdir(parents=True)
            p = d / "fn.c"
            p.write_bytes(b"int x;\n")
            self.assertTrue(check_file(p), sub)

    def test_disassembly_and_recompiled_code_detected(self):
        asm = "".join(f"7100{i:06x}: ldr x0, [x1, #8]\n" for i in range(300)).encode()
        self.assertTrue(check_file(self.write("dump.txt", asm)))
        gen = "".join(f"ctx->x[{i % 31}] = ctx->x[{(i + 1) % 31}] + 1;\n" for i in range(600)).encode()
        self.assertTrue(check_file(self.write("fn_7100.c", gen)))
        # a normal source file mentioning a few registers in comments passes
        ok = b"// the game keeps the speed in x8 (ldr x8, [x0, #0x10])\nfn main() {}\n"
        self.assertEqual(check_file(self.write("note.rs", ok)), [])

    def test_bare_key_list_detected(self):
        body = b"ab" * 16 + b"\n" + b"cd" * 16 + b"\n"
        self.assertTrue(check_file(self.write("notes.md", body)))

    def test_key_with_trailing_label_detected(self):
        self.assertTrue(check_file(self.write("a.md", b"ab" * 16 + b" common\n\n" + b"cd" * 16 + b" smm  \n")))

    def test_key_filename_detected(self):
        self.assertTrue(check_file(self.write("wii keys from console.txt", b"hello\n")))

    def test_7z_magic_detected(self):
        self.assertTrue(check_file(self.write("update", b"7z\xbc\xaf\x27\x1c\x00\x04" + b"\x00" * 24)))

    def test_hex_in_docs_is_fine(self):
        self.assertEqual(check_file(self.write("doc.md", b"# Notes\n\nThe course hash 0123456789abcdef0123456789abcdef identifies the upload.\n")), [])


if __name__ == "__main__":
    unittest.main()
