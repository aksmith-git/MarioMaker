import unittest

from smmfmt.smm2 import Course, CourseObject, ObjectId, Theme
from smmfmt.smm2.course import AREA_OFFSETS, COURSE_SIZE, SnakeBlock, SnakeNode


def make_course() -> Course:
    c = Course.blank("M1")
    c.header.name = "Test Course ✓"
    c.header.description = "Synthetic course built by the test suite"
    c.main.header.theme = Theme.UNDERGROUND
    for i in range(3):
        o = CourseObject()
        o.x, o.y, o.width, o.height = 160 * (10 + i) + 80, 160 * 2 + 80, 1, 1
        o.flags, o.id, o.child_id, o.link_id, o.sound_effect_id = 0x06000040, ObjectId.GOOMBA, -1, -1, -1
        c.main.objects.items.append(o)
    snake = SnakeBlock()
    snake.index, snake.unk_2 = 1, 1
    for d in (2, 2, 4):
        n = SnakeNode()
        n.index, n.direction, n.unk_4 = 1, d, 0x100
        snake.nodes.items.append(n)
    c.main.snake_blocks.slots[1] = snake
    return c


class Smm2CourseTests(unittest.TestCase):
    def test_size_and_layout_constants(self):
        self.assertEqual(AREA_OFFSETS[1] + 0x2DEE0, COURSE_SIZE)

    def test_blank_round_trip(self):
        data = Course.blank().to_bytes()
        self.assertEqual(len(data), COURSE_SIZE)
        self.assertEqual(Course.from_bytes(data).to_bytes(), data)

    def test_build_parse_round_trip(self):
        data = make_course().to_bytes()
        c = Course.from_bytes(data)
        self.assertEqual(c.header.name, "Test Course ✓")
        self.assertEqual(c.header.game_style, "M1")
        self.assertEqual(c.main.header.theme, Theme.UNDERGROUND)
        self.assertEqual(c.main.header.object_count, 3)
        self.assertEqual([o.id for o in c.main.objects], [ObjectId.GOOMBA] * 3)
        self.assertEqual(c.main.objects[1].tile_x, 11)
        self.assertEqual(c.main.header.snake_block_count, 1)
        self.assertEqual(c.main.snake_blocks[1].node_count, 3)
        self.assertEqual([n.direction for n in c.main.snake_blocks[1].nodes], [2, 2, 4])
        self.assertEqual(c.to_bytes(), data)

    def test_name_offset_and_encoding(self):
        data = make_course().to_bytes()
        self.assertEqual(data[0xF1:0xF3], b"M1")
        self.assertEqual(data[0xF4:0xF4 + 8], "Test".encode("utf-16-le"))

    def test_removing_objects_zeroes_slots_and_updates_count(self):
        c = Course.from_bytes(make_course().to_bytes())
        del c.main.objects.items[1:]
        data = c.to_bytes()
        again = Course.from_bytes(data)
        self.assertEqual(again.main.header.object_count, 1)
        slot1 = AREA_OFFSETS[0] + 0x48 + 0x20
        self.assertEqual(data[slot1:slot1 + 0x20], bytes(0x20))

    def test_unknown_bytes_survive(self):
        raw = bytearray(make_course().to_bytes())
        raw[0x50:0x54] = b"\xde\xad\xbe\xef"  # inside the reserved header region
        raw[AREA_OFFSETS[0] + 0x2D124] = 0x5A  # area reserved tail
        self.assertEqual(Course.from_bytes(bytes(raw)).to_bytes(), bytes(raw))

    def test_sparse_slots_round_trip(self):
        c = make_course()
        pipe_slots = (0, 2, 5)
        from smmfmt.smm2.course import ClearPipe
        for i in pipe_slots:
            p = ClearPipe()
            p.index, p.unk_2 = i, 1
            c.main.clear_pipes.slots[i] = p
        data = c.to_bytes()
        again = Course.from_bytes(data)
        self.assertEqual(sorted(again.main.clear_pipes.slots), list(pipe_slots))
        self.assertEqual(again.main.header.clear_pipe_count, 3)
        del again.main.clear_pipes.slots[2]
        self.assertEqual(sorted(Course.from_bytes(again.to_bytes()).main.clear_pipes.slots), [0, 5])

    def test_name_too_long(self):
        c = Course.blank()
        c.header.name = "x" * 34
        with self.assertRaises(ValueError):
            c.to_bytes()


if __name__ == "__main__":
    unittest.main()
