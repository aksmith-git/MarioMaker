"""SMM1 (Wii U) course area file `.cdt` (0x15000 bytes, big-endian). See docs/formats/smm1-course.md.

A course is two of these: `course_data.cdt` (main area) and `course_data_sub.cdt` (sub area).
"""

from __future__ import annotations

import struct
import zlib

from ..binstruct import Field as F
from ..binstruct import Record, RecordArray

CDT_SIZE = 0x15000
CRC_OFFSET = 0x08
CRC_START = 0x10
OBJECTS_OFFSET = 0xF0
OBJECT_CAPACITY = 2600
SOUNDS_OFFSET = 0x145F0
SOUND_CAPACITY = 300
EMPTY_SOUND = bytes.fromhex("FFFF00FFFF000000")


class CourseHeader(Record):
    SIZE = 0xF0
    ENDIAN = ">"
    FIELDS = (
        F("version", 0x00, "Q"),
        F("crc32", 0x08, "I"),
        F("year", 0x10, "H"),
        F("month", 0x12, "B"),
        F("day", 0x13, "B"),
        F("hour", 0x14, "B"),
        F("minute", 0x15, "B"),
        F("official_maker", 0x17, "B"),
        F("is_downloaded", 0x20, "B"),
        F("previously_removed", 0x21, "B"),
        F("physics_version", 0x27, "B"),
        F("name", 0x28, "utf16", 0x21),
        F("game_style", 0x6A, "ascii", 2),
        F("theme", 0x6D, "B"),
        F("uploaded", 0x6E, "B"),
        F("clear_check", 0x6F, "B"),
        F("timer", 0x70, "H"),
        F("autoscroll", 0x72, "B"),
        F("width", 0x74, "I"),
        F("mii_country", 0xD8, "i"),
        F("object_count", 0xEC, "I"),
    )


class CourseObject(Record):
    """x and y are in 1/160 tile units (tile * 160 + 80); z is the draw layer."""

    SIZE = 0x20
    ENDIAN = ">"
    FIELDS = (
        F("x", 0x00, "I"),
        F("z", 0x04, "I"),
        F("y", 0x08, "h"),
        F("width", 0x0A, "b"),
        F("height", 0x0B, "b"),
        F("flags", 0x0C, "I"),
        F("child_flags", 0x10, "I"),
        F("extended_data", 0x14, "I"),
        F("type", 0x18, "b"),
        F("child_type", 0x19, "b"),
        F("link_id", 0x1A, "h"),
        F("effect_index", 0x1C, "h"),
        F("transform_id", 0x1E, "b"),
        F("child_transform_id", 0x1F, "b"),
    )


class SoundEffect(Record):
    """Layout only partly known; the raw 8 bytes are always preserved."""

    SIZE = 8
    ENDIAN = ">"
    FIELDS = (F("type", 0, "B"), F("variation", 2, "B"), F("x", 3, "B"), F("y", 4, "B"))

    @property
    def is_empty(self) -> bool:
        return self.to_bytes() == EMPTY_SOUND


class Course:
    def __init__(self, raw: bytes | bytearray):
        if len(raw) != CDT_SIZE:
            raise ValueError(f".cdt must be {CDT_SIZE:#x} bytes, got {len(raw):#x}")
        self._raw = bytearray(raw)
        self.header = CourseHeader.from_buffer(raw, 0)
        obj_raw = raw[OBJECTS_OFFSET:OBJECTS_OFFSET + CourseObject.SIZE * OBJECT_CAPACITY]
        self.objects = RecordArray(CourseObject, OBJECT_CAPACITY, obj_raw, self.header.object_count)
        snd_raw = raw[SOUNDS_OFFSET:SOUNDS_OFFSET + SoundEffect.SIZE * SOUND_CAPACITY]
        self.sound_effects = RecordArray(SoundEffect, SOUND_CAPACITY, snd_raw, SOUND_CAPACITY)

    @classmethod
    def from_bytes(cls, data: bytes) -> "Course":
        return cls(data)

    @staticmethod
    def stored_crc_ok(data: bytes) -> bool:
        return struct.unpack_from(">I", data, CRC_OFFSET)[0] == zlib.crc32(data[CRC_START:])

    def to_bytes(self) -> bytes:
        buf = bytearray(self._raw)
        if self.objects.changed_length:
            self.header.object_count = len(self.objects)
        data = self.objects.to_bytes()
        buf[OBJECTS_OFFSET:OBJECTS_OFFSET + len(data)] = data
        data = self.sound_effects.to_bytes()
        buf[SOUNDS_OFFSET:SOUNDS_OFFSET + len(data)] = data
        buf[:CourseHeader.SIZE] = self.header.to_bytes()
        struct.pack_into(">I", buf, CRC_OFFSET, zlib.crc32(buf[CRC_START:]))
        self.header.crc32 = struct.unpack_from(">I", buf, CRC_OFFSET)[0]
        return bytes(buf)
