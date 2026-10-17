"""Fixed-layout binary records that round-trip byte-exactly.

A Record keeps the raw bytes it was parsed from. Writing starts from those bytes and packs the
known fields over them, so unknown, reserved and garbage bytes survive a parse/write cycle and
only fields the caller actually changed alter the output.
"""

from __future__ import annotations

import struct
from typing import Any, ClassVar


class Field:
    """One field at a fixed offset.

    kind is a struct format character ('B', 'h', 'I', 'Q', ...), or 'utf16' / 'ascii' for
    fixed-size strings, where length is the capacity in characters (utf16) or bytes (ascii),
    including the terminator.
    """

    __slots__ = ("name", "offset", "kind", "length")

    def __init__(self, name: str, offset: int, kind: str, length: int = 0):
        self.name, self.offset, self.kind, self.length = name, offset, kind, length

    def _codec(self, endian: str) -> tuple[str, int]:
        if self.kind == "utf16":
            return ("utf-16-le" if endian == "<" else "utf-16-be"), 2
        return "latin-1", 1

    def read(self, buf: bytes | bytearray, endian: str) -> Any:
        if self.kind in ("utf16", "ascii"):
            codec, unit = self._codec(endian)
            raw = bytes(buf[self.offset:self.offset + self.length * unit])
            for i in range(0, len(raw), unit):
                if raw[i:i + unit] == b"\0" * unit:
                    raw = raw[:i]
                    break
            return raw.decode(codec, errors="surrogatepass")
        return struct.unpack_from(endian + self.kind, buf, self.offset)[0]

    def write(self, buf: bytearray, value: Any, endian: str) -> None:
        if self.kind in ("utf16", "ascii"):
            codec, unit = self._codec(endian)
            data = value.encode(codec, errors="surrogatepass")
            cap = self.length * unit
            if len(data) > cap:
                raise ValueError(f"{self.name}: {len(value)} chars exceeds capacity {self.length}")
            if len(data) < cap:
                data += b"\0" * unit  # terminator; bytes after it are left as they were
            buf[self.offset:self.offset + len(data)] = data
            return
        struct.pack_into(endian + self.kind, buf, self.offset, value)


class Record:
    """Base class: subclasses set SIZE, FIELDS and ENDIAN."""

    SIZE: ClassVar[int] = 0
    FIELDS: ClassVar[tuple[Field, ...]] = ()
    ENDIAN: ClassVar[str] = "<"

    def __init__(self, raw: bytes | bytearray | None = None):
        if raw is None:
            raw = bytes(self.SIZE)
        if len(raw) != self.SIZE:
            raise ValueError(f"{type(self).__name__} needs {self.SIZE:#x} bytes, got {len(raw):#x}")
        self._raw = bytearray(raw)
        for f in self.FIELDS:
            setattr(self, f.name, f.read(self._raw, self.ENDIAN))

    @classmethod
    def from_buffer(cls, buf: bytes | bytearray, offset: int):
        return cls(buf[offset:offset + cls.SIZE])

    def to_bytes(self) -> bytes:
        buf = bytearray(self._raw)
        for f in self.FIELDS:
            f.write(buf, getattr(self, f.name), self.ENDIAN)
        return bytes(buf)

    def fields(self) -> dict[str, Any]:
        return {f.name: getattr(self, f.name) for f in self.FIELDS}

    def __eq__(self, other: object) -> bool:
        return type(self) is type(other) and self.to_bytes() == other.to_bytes()  # type: ignore[union-attr]

    def __repr__(self) -> str:
        return f"{type(self).__name__}({', '.join(f'{k}={v!r}' for k, v in self.fields().items())})"


class RecordArray:
    """A fixed-capacity array of records whose live length comes from a separate count.

    Slots past the live length are kept as raw bytes. If the list shrinks, freed slots are zeroed.
    """

    def __init__(self, cls: type[Record], capacity: int, raw: bytes | bytearray, count: int):
        self.cls, self.capacity = cls, capacity
        self._raw = bytearray(raw)
        if len(raw) != cls.SIZE * capacity:
            raise ValueError(f"{cls.__name__}[{capacity}] needs {cls.SIZE * capacity:#x} bytes")
        self._parsed = max(0, min(count, capacity))
        self.items: list[Record] = [cls.from_buffer(raw, i * cls.SIZE) for i in range(self._parsed)]

    def to_bytes(self) -> bytes:
        if len(self.items) > self.capacity:
            raise ValueError(f"{self.cls.__name__}: {len(self.items)} items exceeds capacity {self.capacity}")
        buf = bytearray(self._raw)
        size = self.cls.SIZE
        for i, item in enumerate(self.items):
            buf[i * size:(i + 1) * size] = item.to_bytes()
        for i in range(len(self.items), self._parsed):
            buf[i * size:(i + 1) * size] = bytes(size)
        return bytes(buf)

    @property
    def changed_length(self) -> bool:
        return len(self.items) != self._parsed

    def __len__(self) -> int:
        return len(self.items)

    def __iter__(self):
        return iter(self.items)

    def __getitem__(self, i):
        return self.items[i]


class SlotArray:
    """A fixed-capacity array addressed by slot, where empty slots are all-zero.

    Live entries can be sparse (slot 0, 2, 3 ...), so this holds {slot: record} rather than a list.
    Removing a slot zeroes it; the caller stores len() as the live count.
    """

    def __init__(self, cls: type[Record], capacity: int, raw: bytes | bytearray):
        self.cls, self.capacity = cls, capacity
        if len(raw) != cls.SIZE * capacity:
            raise ValueError(f"{cls.__name__}[{capacity}] needs {cls.SIZE * capacity:#x} bytes")
        self._raw = bytearray(raw)
        size = cls.SIZE
        self.slots: dict[int, Record] = {
            i: cls.from_buffer(raw, i * size) for i in range(capacity) if any(raw[i * size:(i + 1) * size])
        }
        self._original = set(self.slots)

    def to_bytes(self) -> bytes:
        buf = bytearray(self._raw)
        size = self.cls.SIZE
        for i in self._original - set(self.slots):
            buf[i * size:(i + 1) * size] = bytes(size)
        for i, item in self.slots.items():
            if not 0 <= i < self.capacity:
                raise ValueError(f"{self.cls.__name__}: slot {i} outside capacity {self.capacity}")
            buf[i * size:(i + 1) * size] = item.to_bytes()
        return bytes(buf)

    @property
    def changed_length(self) -> bool:
        return set(self.slots) != self._original

    def __len__(self) -> int:
        return len(self.slots)

    def __iter__(self):
        return iter(self.slots[i] for i in sorted(self.slots))

    def __getitem__(self, slot: int):
        return self.slots[slot]
