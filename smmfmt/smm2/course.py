"""SMM2 decrypted course data (0x5BFC0 bytes, little-endian). See docs/formats/smm2-course.md."""

from __future__ import annotations

from ..binstruct import Field as F
from ..binstruct import Record, RecordArray, SlotArray

COURSE_SIZE = 0x5BFC0
HEADER_SIZE = 0x200
AREA_SIZE = 0x2DEE0
AREA_OFFSETS = (0x200, 0x2E0E0)  # main area, sub area


class CourseHeader(Record):
    SIZE = HEADER_SIZE
    FIELDS = (
        F("start_y", 0x00, "B"),
        F("goal_y", 0x01, "B"),
        F("goal_x", 0x02, "h"),
        F("timer", 0x04, "h"),
        F("clear_condition_magnitude", 0x06, "h"),
        F("year", 0x08, "h"),
        F("month", 0x0A, "b"),
        F("day", 0x0B, "b"),
        F("hour", 0x0C, "b"),
        F("minute", 0x0D, "b"),
        F("autoscroll_speed", 0x0E, "B"),
        F("clear_condition_category", 0x0F, "B"),
        F("clear_condition", 0x10, "I"),
        F("game_version", 0x14, "I"),
        F("management_flags", 0x18, "I"),
        F("clear_check_attempts", 0x1C, "I"),
        F("clear_check_time", 0x20, "I"),
        F("creation_id", 0x24, "I"),
        F("upload_id", 0x28, "Q"),
        F("clear_check_game_version", 0x30, "I"),
        F("unk_f0", 0xF0, "B"),
        F("game_style", 0xF1, "ascii", 3),
        F("name", 0xF4, "utf16", 0x21),
        F("description", 0x136, "utf16", 0x65),
    )


class AreaHeader(Record):
    SIZE = 0x48
    FIELDS = (
        F("theme", 0x00, "B"),
        F("autoscroll_type", 0x01, "B"),
        F("boundary_type", 0x02, "B"),
        F("orientation", 0x03, "B"),
        F("liquid_end_height", 0x04, "B"),
        F("liquid_mode", 0x05, "B"),
        F("liquid_speed", 0x06, "B"),
        F("liquid_start_height", 0x07, "B"),
        F("boundary_right", 0x08, "i"),
        F("boundary_top", 0x0C, "i"),
        F("boundary_left", 0x10, "i"),
        F("boundary_bottom", 0x14, "i"),
        F("flags", 0x18, "I"),
        F("object_count", 0x1C, "i"),
        F("sound_effect_count", 0x20, "i"),
        F("snake_block_count", 0x24, "i"),
        F("clear_pipe_count", 0x28, "i"),
        F("piranha_creeper_count", 0x2C, "i"),
        F("exclamation_block_count", 0x30, "i"),
        F("track_block_count", 0x34, "i"),
        F("ground_count", 0x3C, "i"),
        F("track_count", 0x40, "i"),
        F("icicle_count", 0x44, "i"),
    )


class CourseObject(Record):
    """Positions are in 1/160 tile units, centre-anchored; y increases upward."""

    SIZE = 0x20
    FIELDS = (
        F("x", 0x00, "i"),
        F("y", 0x04, "i"),
        F("width", 0x0A, "B"),
        F("height", 0x0B, "B"),
        F("flags", 0x0C, "I"),
        F("child_flags", 0x10, "I"),
        F("extended_data", 0x14, "I"),
        F("id", 0x18, "h"),
        F("child_id", 0x1A, "h"),
        F("link_id", 0x1C, "h"),
        F("sound_effect_id", 0x1E, "h"),
    )

    @property
    def tile_x(self) -> float:
        return (self.x - 80) / 160

    @property
    def tile_y(self) -> float:
        return (self.y - 80) / 160


class SoundEffect(Record):
    SIZE = 4
    FIELDS = (F("id", 0, "B"), F("x", 1, "B"), F("y", 2, "B"))


class GroundTile(Record):
    SIZE = 4
    FIELDS = (F("x", 0, "B"), F("y", 1, "B"), F("id", 2, "B"), F("background_id", 3, "B"))


class Track(Record):
    SIZE = 0x0C
    FIELDS = (
        F("unk_0", 0x0, "H"),
        F("flags", 0x2, "B"),
        F("x", 0x3, "B"),
        F("y", 0x4, "B"),
        F("type", 0x5, "B"),
        F("link_id", 0x6, "H"),
        F("k0", 0x8, "H"),
        F("k1", 0xA, "H"),
    )


class Icicle(Record):
    SIZE = 4
    FIELDS = (F("x", 0, "B"), F("y", 1, "B"), F("type", 2, "B"))


# --- path records: a header followed by a fixed-capacity node array -------------------------

class SnakeNode(Record):
    SIZE = 8
    FIELDS = (F("index", 0, "H"), F("direction", 2, "H"), F("unk_4", 4, "H"))


class ClearPipeNode(Record):
    SIZE = 8
    FIELDS = (
        F("type", 0, "B"), F("index", 1, "B"), F("x", 2, "B"), F("y", 3, "B"),
        F("width", 4, "B"), F("height", 5, "B"), F("unk_6", 6, "B"), F("direction", 7, "B"),
    )


class PathNode(Record):
    SIZE = 4
    FIELDS = (F("unk_0", 0, "B"), F("direction", 1, "B"))


class _PathRecord(Record):
    NODE: type[Record]
    NODE_OFFSET = 4
    NODE_CAPACITY = 0
    COUNT_FIELD = "node_count"

    def __init__(self, raw=None):
        super().__init__(raw)
        start = self.NODE_OFFSET
        node_raw = self._raw[start:start + self.NODE.SIZE * self.NODE_CAPACITY]
        self.nodes = RecordArray(self.NODE, self.NODE_CAPACITY, node_raw, getattr(self, self.COUNT_FIELD))

    def to_bytes(self) -> bytes:
        if self.nodes.changed_length:
            setattr(self, self.COUNT_FIELD, len(self.nodes))
        buf = bytearray(super().to_bytes())
        data = self.nodes.to_bytes()
        buf[self.NODE_OFFSET:self.NODE_OFFSET + len(data)] = data
        return bytes(buf)


class SnakeBlock(_PathRecord):
    SIZE = 0x3C4
    FIELDS = (F("index", 0, "B"), F("node_count", 1, "B"), F("unk_2", 2, "B"))
    NODE, NODE_CAPACITY = SnakeNode, 120


class ClearPipe(_PathRecord):
    SIZE = 0x124
    FIELDS = (F("index", 0, "B"), F("node_count", 1, "B"), F("unk_2", 2, "B"))
    NODE, NODE_CAPACITY = ClearPipeNode, 36


class PiranhaCreeper(_PathRecord):
    SIZE = 0x54
    FIELDS = (F("unk_0", 0, "B"), F("index", 1, "B"), F("node_count", 2, "B"))
    NODE, NODE_CAPACITY = PathNode, 20


class ExclamationBlock(_PathRecord):
    SIZE = 0x2C
    FIELDS = (F("unk_0", 0, "B"), F("index", 1, "B"), F("node_count", 2, "B"))
    NODE, NODE_CAPACITY = PathNode, 10


class TrackBlock(ExclamationBlock):
    pass


# --- areas --------------------------------------------------------------------------------

# attribute, count field in AreaHeader, area-relative offset, capacity, record type, slot-indexed
# Path arrays are slot-indexed: slot i holds the entry whose `index` is i, slots may be sparse,
# and the count is the number of live slots (verified on 730 courses, see tests/test_corpus.py).
AREA_ARRAYS = (
    ("objects", "object_count", 0x48, 2600, CourseObject, False),
    ("sound_effects", "sound_effect_count", 0x14548, 300, SoundEffect, False),
    ("snake_blocks", "snake_block_count", 0x149F8, 5, SnakeBlock, True),
    ("clear_pipes", "clear_pipe_count", 0x15CCC, 200, ClearPipe, True),
    ("piranha_creepers", "piranha_creeper_count", 0x240EC, 10, PiranhaCreeper, True),
    ("exclamation_blocks", "exclamation_block_count", 0x24434, 10, ExclamationBlock, True),
    ("track_blocks", "track_block_count", 0x245EC, 10, TrackBlock, True),
    ("ground", "ground_count", 0x247A4, 4000, GroundTile, False),
    ("tracks", "track_count", 0x28624, 1500, Track, False),
    ("icicles", "icicle_count", 0x2CC74, 300, Icicle, False),
)


class Area:
    def __init__(self, raw: bytes | bytearray | None = None):
        raw = bytes(AREA_SIZE) if raw is None else raw
        if len(raw) != AREA_SIZE:
            raise ValueError(f"area needs {AREA_SIZE:#x} bytes")
        self._raw = bytearray(raw)
        self.header = AreaHeader.from_buffer(raw, 0)
        for attr, count_field, off, cap, cls, slotted in AREA_ARRAYS:
            chunk = raw[off:off + cls.SIZE * cap]
            arr = SlotArray(cls, cap, chunk) if slotted else RecordArray(cls, cap, chunk, getattr(self.header, count_field))
            setattr(self, attr, arr)

    def to_bytes(self) -> bytes:
        buf = bytearray(self._raw)
        for attr, count_field, off, cap, cls, _slotted in AREA_ARRAYS:
            arr = getattr(self, attr)
            if arr.changed_length:
                setattr(self.header, count_field, len(arr))
            data = arr.to_bytes()
            buf[off:off + len(data)] = data
        buf[:AreaHeader.SIZE] = self.header.to_bytes()
        return bytes(buf)


class Course:
    def __init__(self, raw: bytes | bytearray | None = None):
        raw = bytes(COURSE_SIZE) if raw is None else raw
        if len(raw) != COURSE_SIZE:
            raise ValueError(f"course data must be {COURSE_SIZE:#x} bytes, got {len(raw):#x}")
        self.header = CourseHeader.from_buffer(raw, 0)
        self.main = Area(raw[AREA_OFFSETS[0]:AREA_OFFSETS[0] + AREA_SIZE])
        self.sub = Area(raw[AREA_OFFSETS[1]:AREA_OFFSETS[1] + AREA_SIZE])

    @classmethod
    def from_bytes(cls, data: bytes) -> "Course":
        return cls(data)

    @classmethod
    def blank(cls, style: str = "M1") -> "Course":
        c = cls()
        h = c.header
        h.timer, h.game_style, h.unk_f0 = 300, style, 0xFF
        h.start_y, h.goal_y, h.goal_x = 1, 1, 2400
        for area in (c.main, c.sub):
            area.header.boundary_top, area.header.boundary_right = 0x1B0, 0xF00
        return c

    @property
    def areas(self) -> tuple[Area, Area]:
        return self.main, self.sub

    def to_bytes(self) -> bytes:
        out = bytearray(COURSE_SIZE)
        out[:HEADER_SIZE] = self.header.to_bytes()
        for off, area in zip(AREA_OFFSETS, self.areas):
            out[off:off + AREA_SIZE] = area.to_bytes()
        return bytes(out)
