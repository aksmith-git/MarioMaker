"""Locate the owner's local game data and course samples (all outside the repository).

Set SMM_WORK to override the default ~/smm-work. Every helper returns nothing when the data
isn't present, so tests that use it skip cleanly in CI.
"""

from __future__ import annotations

import json
import os
import zlib
from pathlib import Path
from typing import Iterator

WORK = Path(os.environ.get("SMM_WORK", Path.home() / "smm-work"))
SMM2_ROMFS = {"v1.0.0": WORK / "extracted/smm2-romfs", "v3.0.3": WORK / "extracted/smm2-v303-romfs"}
SMM1_COURSE_DIRS = {
    "v16": WORK / "extracted/smm1-disc/GM000500001018DC00000000000100/content/Course",
    "v208": WORK / "extracted/smm1-update-v208/content/Course",
}
DATASET = WORK / "courses/mm2_level"
COURSE_TABLE = WORK / "keys/smm2_course_table.bin"


def _sarc_files(path: Path) -> Iterator[tuple[str, bytes]]:
    import oead  # only needed when reading the game's own archives

    sarc = oead.Sarc(oead.yaz0.decompress(path.read_bytes()))
    for f in sarc.get_files():
        yield f.name, bytes(f.data)


def smm2_builtin_bcds(version: str = "v3.0.3") -> Iterator[tuple[str, bytes]]:
    root = SMM2_ROMFS[version] / "Course"
    if root.is_dir():
        for szs in sorted(root.glob("*.szs")):
            yield from _sarc_files(szs)


def smm1_builtin_cdts(version: str = "v16") -> Iterator[tuple[str, bytes]]:
    root = SMM1_COURSE_DIRS[version]
    if root.is_dir():
        for szs in sorted(root.glob("*.szs")):
            yield from _sarc_files(szs)


def dataset_courses() -> Iterator[tuple[dict, bytes]]:
    index = DATASET / "index.json"
    if index.is_file():
        for meta in json.loads(index.read_text())["courses"]:
            yield meta, zlib.decompress((DATASET / meta["file"]).read_bytes())


def course_table():
    if COURSE_TABLE.is_file():
        from .smm2.crypto import table_from_bytes

        return table_from_bytes(COURSE_TABLE.read_bytes())
    return None
