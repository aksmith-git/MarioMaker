#!/usr/bin/env python3
"""Fetch a reproducible, stratified sample of SMM2 courses from TheGreatRambler's public dataset.

Source: https://huggingface.co/datasets/TheGreatRambler/mm2_level (CC BY-NC-SA 4.0), a snapshot
of 26.6M courses taken from Nintendo's servers in February 2022. Courses are player-made; they
are used locally for parser tests and the 100 Mario Challenge pool and are never committed.

Reads parquet shards remotely with HTTP range requests (DuckDB), so a shard is never written to
disk in full. Output layout:

    <out>/index.json                       metadata for every fetched course
    <out>/<style>/<difficulty>/<data_id>.course.zlib   zlib-compressed course data (0x5BFC0 bytes inflated)

    python -I tools/fetch_courses.py --out ~/smm-work/courses/mm2_level --per-bucket 25
"""

from __future__ import annotations

import argparse
import json
import sys
import zlib
from pathlib import Path

import duckdb

DATASET = "https://huggingface.co/datasets/TheGreatRambler/mm2_level/resolve/main/data/"
# Shard 0 holds the earliest uploads (June-July 2019), all made before game version 2.0.
DEFAULT_SHARDS = ["train-00000-of-00196-7a2d43e1e8287c30.parquet"]

STYLES = {0: "smb1", 1: "smb3", 2: "smw", 3: "nsmbu", 4: "sm3dw"}
DIFFICULTIES = {0: "easy", 1: "normal", 2: "expert", 3: "super_expert"}
COURSE_SIZE = 0x5BFC0

META_COLUMNS = [
    "data_id", "name", "description", "uploaded", "gamestyle", "theme", "difficulty", "tag1", "tag2",
    "game_version", "world_record", "upload_time", "clear_condition", "clear_condition_magnitude", "timer",
    "autoscroll_speed", "clears", "attempts", "clear_rate", "plays", "likes", "boos",
]


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", type=Path, required=True)
    ap.add_argument("--per-bucket", type=int, default=25, help="courses per (style, difficulty)")
    ap.add_argument("--extra-style", default="smb1", help="style to oversample for the vertical slice")
    ap.add_argument("--extra-theme", type=int, default=0, help="theme id to oversample (0 = ground)")
    ap.add_argument("--extra", type=int, default=50)
    ap.add_argument("--min-plays", type=int, default=20, help="skip barely-played courses")
    ap.add_argument("--seed", default="smm-engine")
    ap.add_argument("--shard", action="append", help="parquet shard file name (repeatable)")
    args = ap.parse_args()

    out: Path = args.out.expanduser()
    shards = [DATASET + s for s in (args.shard or DEFAULT_SHARDS)]
    db = duckdb.connect()
    db.sql("SET enable_progress_bar = false; INSTALL httpfs; LOAD httpfs;")
    src = f"read_parquet({shards!r})"
    extra_style = {v: k for k, v in STYLES.items()}[args.extra_style]

    # Deterministic pseudo-random order: hash of (seed, data_id).
    pick_sql = f"""
        WITH ranked AS (
          SELECT data_id, gamestyle, difficulty, theme,
                 row_number() OVER (PARTITION BY gamestyle, difficulty ORDER BY hash('{args.seed}' || data_id)) AS r
          FROM {src} WHERE plays >= {args.min_plays}
        ),
        extra AS (
          SELECT data_id FROM ranked
          WHERE gamestyle = {extra_style} AND theme = {args.extra_theme} AND r > {args.per_bucket}
          ORDER BY hash('{args.seed}-extra' || data_id) LIMIT {args.extra}
        )
        SELECT data_id FROM ranked WHERE r <= {args.per_bucket}
        UNION SELECT data_id FROM extra
    """
    ids = [r[0] for r in db.sql(pick_sql).fetchall()]
    print(f"selected {len(ids)} courses", file=sys.stderr)

    cols = ", ".join(META_COLUMNS)
    rows = db.execute(f"SELECT {cols}, level_data FROM {src} WHERE data_id IN (SELECT unnest(?))", [ids]).fetchall()

    index = []
    for row in rows:
        meta = dict(zip(META_COLUMNS, row[:-1]))
        data = zlib.decompress(row[-1])
        if len(data) != COURSE_SIZE:
            print(f"skip {meta['data_id']}: {len(data):#x} bytes, expected {COURSE_SIZE:#x}", file=sys.stderr)
            continue
        rel = Path(STYLES[meta["gamestyle"]], DIFFICULTIES[meta["difficulty"]], f"{meta['data_id']}.course.zlib")
        (out / rel).parent.mkdir(parents=True, exist_ok=True)
        (out / rel).write_bytes(row[-1])  # keep compressed; validated above
        index.append({**meta, "file": rel.as_posix()})

    index.sort(key=lambda m: m["data_id"])
    (out / "index.json").write_text(json.dumps({
        "source": "huggingface.co/datasets/TheGreatRambler/mm2_level",
        "license": "CC BY-NC-SA 4.0",
        "shards": args.shard or DEFAULT_SHARDS,
        "seed": args.seed,
        "courses": index,
    }, ensure_ascii=False, indent=1))
    print(f"wrote {len(index)} courses to {out}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
