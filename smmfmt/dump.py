"""Course → plain JSON-able dict, in the same shape as the Rust `mmk dump` command.

Used by the Rust/Python parity tests. Strings are decoded lossily (unpaired surrogates become
U+FFFD) on both sides so the comparison is well defined.
"""

from __future__ import annotations

from .binstruct import Record, RecordArray, SlotArray


def _lossy(v):
    if isinstance(v, str):
        return v.encode("utf-16-le", "surrogatepass").decode("utf-16-le", "replace")
    return v


def record(r: Record) -> dict:
    d = {k: _lossy(v) for k, v in r.fields().items()}
    nodes = getattr(r, "nodes", None)
    if isinstance(nodes, RecordArray):
        d["nodes"] = [record(n) for n in nodes]
    return d


def _array(a):
    if isinstance(a, SlotArray):
        return {str(i): record(a.slots[i]) for i in sorted(a.slots)}
    return [record(x) for x in a]


def smm2_course(c) -> dict:
    from .smm2.course import AREA_ARRAYS

    def area(a):
        out = {"header": record(a.header)}
        for attr, *_ in AREA_ARRAYS:
            out[attr] = _array(getattr(a, attr))
        return out

    return {"game": "SMM2", "header": record(c.header), "areas": [area(c.main), area(c.sub)]}


def smm1_course(c) -> dict:
    return {"game": "SMM1", "header": record(c.header), "objects": _array(c.objects),
            "sound_effects": _array(c.sound_effects)}
