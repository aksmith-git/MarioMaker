import struct
import unittest
import zlib

from smmfmt.smm1 import Course, CourseObject
from smmfmt.smm1.course import CDT_SIZE, EMPTY_SOUND, SOUNDS_OFFSET


def make_cdt() -> bytes:
    raw = bytearray(CDT_SIZE)
    struct.pack_into(">Q", raw, 0, 0xB)
    raw[0x6A:0x6C] = b"M1"
    struct.pack_into(">H", raw, 0x70, 300)
    for i in range(300):
        raw[SOUNDS_OFFSET + i * 8:SOUNDS_OFFSET + i * 8 + 8] = EMPTY_SOUND
    c = Course(bytes(raw))
    c.header.name = "SMM1 Test"
    o = CourseObject()
    o.x, o.y, o.z, o.width, o.height = 160 * 4 + 80, 160 * 1 + 80, 0, 1, 1
    o.flags, o.type, o.child_type, o.link_id, o.effect_index = 0x06000840, 0, -1, -1, -1
    c.objects.items.append(o)
    return c.to_bytes()


class Smm1CourseTests(unittest.TestCase):
    def test_build_and_parse(self):
        data = make_cdt()
        self.assertTrue(Course.stored_crc_ok(data))
        c = Course.from_bytes(data)
        self.assertEqual(c.header.name, "SMM1 Test")
        self.assertEqual(c.header.game_style, "M1")
        self.assertEqual(c.header.timer, 300)
        self.assertEqual(c.header.object_count, 1)
        self.assertEqual(c.objects[0].x, 720)
        self.assertTrue(all(s.is_empty for s in c.sound_effects))
        self.assertEqual(c.to_bytes(), data)

    def test_big_endian(self):
        data = make_cdt()
        self.assertEqual(data[0x28:0x2C], "SM".encode("utf-16-be"))
        self.assertEqual(data[0xEC:0xF0], b"\x00\x00\x00\x01")

    def test_crc_refreshed_after_edit(self):
        c = Course.from_bytes(make_cdt())
        c.header.timer = 500
        data = c.to_bytes()
        self.assertEqual(struct.unpack_from(">I", data, 8)[0], zlib.crc32(data[0x10:]))


if __name__ == "__main__":
    unittest.main()
