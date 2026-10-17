#!/usr/bin/env python3
"""Inventory an extracted game directory by real file type.

Walks a directory, identifies each file by magic bytes (unwrapping Yaz0 and zstd), opens SARC
archives one level deep, and prints a Markdown report: per top-level folder, how many files of
each type and their total size, plus the type mix inside archives.

Needs `oead` and `zstandard` (see docs/TOOLCHAIN.md). Output describes the user's own dump and
belongs in research/, not in the repository.

    python -I tools/inventory.py ~/smm-work/extracted/smm1-disc/GM.../content > research/inventory/smm1.md
"""

from __future__ import annotations

import argparse
import collections
import sys
from pathlib import Path

import oead
import zstandard

MAGICS = [
    (b"SARC", "SARC"), (b"BNTX", "BNTX"), (b"FRES", "BFRES"), (b"Gfx2", "GTX"), (b"FLIM", "BFLIM"),
    (b"FLYT", "BFLYT"), (b"FLAN", "BFLAN"), (b"FSAR", "BFSAR"), (b"FSTM", "BFSTM"), (b"FWAV", "BFWAV"),
    (b"BWAV", "BWAV"), (b"BARS", "BARS"), (b"MsgStdBn", "MSBT"), (b"MsgPrjBn", "MSBP"),
    (b"BY\x00", "BYML"), (b"YB", "BYML"), (b"AAMP", "AAMP"), (b"EFTB", "PTCL"), (b"VFXB", "PTCL"),
    (b"SPBD", "PTCL"), (b"FFNT", "BFFNT"), (b"FGRP", "BFGRP"), (b"FWAR", "BFWAR"), (b"FSEQ", "BFSEQ"),
    (b"FBNK", "BFBNK"), (b"FWSD", "BFWSD"), (b"FSTP", "BFSTP"), (b"\x89PNG", "PNG"), (b"\xff\xd8\xff", "JPEG"),
    (b"<?xml", "XML"), (b"\x7fELF", "ELF/RPX"), (b"NSO0", "NSO"), (b"BNSH", "BNSH"), (b"BFSH", "BFSHA"),
    (b"FSHA", "BFSHA"), (b"Yaz0", "Yaz0"), (b"\x28\xb5\x2f\xfd", "zstd"),
]

ZSTD = zstandard.ZstdDecompressor()


def sniff(data: bytes) -> str:
    for magic, name in MAGICS:
        if data.startswith(magic):
            return name
    if len(data) >= 0x20 and data[:4] in (b"PACK",):
        return "PACK"
    try:
        data[:256].decode("ascii")
        return "text?"
    except UnicodeDecodeError:
        return "unknown"


def unwrap(data: bytes) -> tuple[str, bytes]:
    """Return (wrapper chain, innermost payload)."""
    chain = []
    for _ in range(3):
        kind = sniff(data)
        if kind == "Yaz0":
            chain.append("Yaz0")
            data = bytes(oead.yaz0.decompress(data))
        elif kind == "zstd":
            chain.append("zstd")
            data = ZSTD.decompress(data, max_output_size=512 * 1024 * 1024)
        else:
            break
    return "+".join(chain), data


def describe(path: Path, inner: collections.Counter) -> tuple[str, int]:
    raw = path.read_bytes()
    try:
        wrap, data = unwrap(raw)
    except Exception as exc:  # corrupt or unusual wrapper; report rather than abort
        return f"unwrap-error({type(exc).__name__})", len(raw)
    kind = sniff(data)
    if kind == "SARC":
        try:
            for f in oead.Sarc(data).get_files():
                fwrap, fdata = unwrap(bytes(f.data))
                ftype = sniff(fdata)
                inner[(Path(f.name).suffix.lower() or "(none)", (fwrap + ">" if fwrap else "") + ftype)] += 1
        except Exception:
            kind = "SARC(unreadable)"
    return (f"{wrap}>{kind}" if wrap else kind), len(raw)


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("root", type=Path)
    ap.add_argument("--title", default=None)
    args = ap.parse_args()
    root: Path = args.root.expanduser()

    per_dir: dict[str, collections.Counter] = collections.defaultdict(collections.Counter)
    per_dir_bytes: dict[tuple[str, str], int] = collections.Counter()
    per_dir_ext: dict[str, collections.Counter] = collections.defaultdict(collections.Counter)
    inner: dict[str, collections.Counter] = collections.defaultdict(collections.Counter)

    files = sorted(p for p in root.rglob("*") if p.is_file() and p.name != ".DS_Store")
    for p in files:
        rel = p.relative_to(root)
        top = rel.parts[0] if len(rel.parts) > 1 else "."
        kind, size = describe(p, inner[top])
        per_dir[top][kind] += 1
        per_dir_bytes[(top, kind)] += size
        per_dir_ext[top][p.suffix.lower() or "(none)"] += 1

    print(f"# Inventory: {args.title or root.name}\n")
    print(f"Root: `{root}`  \nFiles: {len(files)}  \nTotal: {sum(per_dir_bytes.values()) / 2**20:.1f} MiB\n")
    for top in sorted(per_dir):
        total = sum(v for (t, _), v in per_dir_bytes.items() if t == top)
        print(f"## `{top}/` - {sum(per_dir[top].values())} files, {total / 2**20:.1f} MiB\n")
        print("Extensions: " + ", ".join(f"`{e}` x{n}" for e, n in per_dir_ext[top].most_common()) + "\n")
        print("| detected type | files | MiB |\n|---|---:|---:|")
        for kind, n in per_dir[top].most_common():
            print(f"| {kind} | {n} | {per_dir_bytes[(top, kind)] / 2**20:.2f} |")
        if inner[top]:
            print("\nInside archives (extension, detected type):\n")
            print("| ext | type | count |\n|---|---|---:|")
            for (ext, kind), n in inner[top].most_common(25):
                print(f"| `{ext}` | {kind} | {n} |")
        print()
    return 0


if __name__ == "__main__":
    sys.exit(main())
