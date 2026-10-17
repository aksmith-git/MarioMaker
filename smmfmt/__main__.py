"""Inspect a course file.

    python -m smmfmt course.bcd --table ~/smm-work/keys/smm2_course_table.bin
    python -m smmfmt 3000004.course.zlib
    python -m smmfmt course_data.cdt
"""

from __future__ import annotations

import argparse
import json
import sys
import zlib
from collections import Counter
from pathlib import Path


def summarise_smm2(data: bytes) -> dict:
    from .smm2 import Course, ObjectId, Theme

    c = Course.from_bytes(data)
    h = c.header
    out = {"game": "SMM2", "name": h.name, "description": h.description, "style": h.game_style,
           "timer": h.timer, "clear_condition": f"{h.clear_condition:#010x}", "areas": []}
    for label, area in zip(("main", "sub"), c.areas):
        ids = Counter(ObjectId(o.id).name if o.id in ObjectId._value2member_map_ else o.id for o in area.objects)
        out["areas"].append({
            "area": label, "theme": Theme(area.header.theme).name if area.header.theme < 10 else area.header.theme,
            "orientation": "vertical" if area.header.orientation else "horizontal",
            "objects": len(area.objects), "ground": len(area.ground), "tracks": len(area.tracks),
            "clear_pipes": len(area.clear_pipes), "object_ids": dict(ids.most_common()),
        })
    return out


def summarise_smm1(data: bytes) -> dict:
    from .smm1 import Course

    c = Course.from_bytes(data)
    return {"game": "SMM1", "name": c.header.name, "style": c.header.game_style, "theme": c.header.theme,
            "timer": c.header.timer, "crc_ok": Course.stored_crc_ok(data), "objects": len(c.objects),
            "object_types": dict(Counter(o.type for o in c.objects).most_common())}


def main() -> int:
    ap = argparse.ArgumentParser(description="Summarise an SMM1 or SMM2 course file as JSON")
    ap.add_argument("path", type=Path)
    ap.add_argument("--table", type=Path, help="SMM2 course key table (needed for .bcd)")
    args = ap.parse_args()
    data = args.path.read_bytes()
    if data[:2] in (b"\x78\x9c", b"\x78\xda", b"\x78\x01"):
        data = zlib.decompress(data)
    from .smm1.course import CDT_SIZE
    from .smm2.course import COURSE_SIZE
    from .smm2.crypto import BCD_SIZE, decrypt_bcd, table_from_bytes

    if len(data) == BCD_SIZE:
        if not args.table:
            ap.error(".bcd files need --table")
        data = decrypt_bcd(data, table_from_bytes(args.table.read_bytes())).body
    if len(data) == COURSE_SIZE:
        result = summarise_smm2(data)
    elif len(data) == CDT_SIZE:
        result = summarise_smm1(data)
    else:
        ap.error(f"unrecognised size {len(data):#x}")
    json.dump(result, sys.stdout, ensure_ascii=False, indent=1)
    print()
    return 0


if __name__ == "__main__":
    sys.exit(main())
