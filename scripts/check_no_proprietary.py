#!/usr/bin/env python3
"""Fail if any tracked (or staged) file looks like proprietary game content.

Checks these things for every file git knows about:
  1. extension   - container/asset/executable/key formats used by the Wii U and Switch, and
                   native binaries / translated shaders (the recomp builds those locally)
  2. magic bytes - catches renamed files (e.g. a SARC saved as .dat, an AArch64 ELF, a Mach-O)
  3. key lines   - lines shaped like Switch prod.keys / title.keys entries
  4. recomp      - nothing under recomp/private/ or recomp/generated/ (derived from the dump),
                   and no text that looks like AArch64 disassembly or recompiled game code

Usage:
  check_no_proprietary.py            # check every tracked file (CI)
  check_no_proprietary.py --staged   # check files staged for commit (pre-commit hook)
  check_no_proprietary.py FILE...    # check specific files

Images (game art, captures) are blocked everywhere except two places: assets/placeholder/
(original placeholder art) and docs/screenshots/ (a few captures of our own builds for the
README, owner's call 2026-10-19: PNG or JPEG, at most 1 MiB each and 12 files).
"""

from __future__ import annotations

import re
import subprocess
import sys
from pathlib import Path

BLOCKED_EXTENSIONS = {
    # disc / package / container images
    ".wux", ".wud", ".xci", ".xcz", ".nsp", ".nsz", ".nca", ".ncz", ".tik", ".cert",
    # executables
    ".nso", ".nro", ".npdm", ".rpx", ".rpl",
    # archives and compression
    ".sarc", ".szs", ".pack", ".zs", ".yaz0", ".lz",
    # models, textures, effects
    ".bfres", ".bntx", ".gtx", ".bflim", ".bclim", ".ptcl", ".bfsha", ".bea",
    # data
    ".byml", ".byaml", ".bcsv", ".msbt", ".msbp", ".aamp", ".bgyml",
    # audio
    ".bfsar", ".bfstm", ".bfwav", ".bwav", ".bars", ".bfgrp", ".bfwar", ".bfseq", ".bfbnk",
    # SMM course files
    ".bcd",
    # keys
    ".keys",
    # opaque archives (could hold anything)
    ".7z", ".rar",
    # native binaries and translated shaders: the recomp generates these locally from the dump
    ".elf", ".o", ".obj", ".a", ".lib", ".so", ".dylib", ".dll", ".exe", ".spv", ".nss", ".kip",
}
BLOCKED_NAMES = {"prod.keys", "title.keys", "dev.keys", "otp.bin", "seeprom.bin", "common.key", "game.key"}

MAGICS = [
    (0, b"Yaz0", "Yaz0 compressed data"),
    (0, b"Yaz1", "Yaz1 compressed data"),
    (0, b"SARC", "SARC archive"),
    (0, b"\x28\xb5\x2f\xfd", "zstd frame (SMM2 .zs)"),
    (0, b"BNTX", "BNTX texture"),
    (0, b"FRES", "BFRES model"),
    (0, b"Gfx2", "GTX texture"),
    (0, b"FSAR", "BFSAR sound archive"),
    (0, b"FSTM", "BFSTM stream"),
    (0, b"FWAV", "BFWAV wave"),
    (0, b"BWAV", "BWAV wave"),
    (0, b"BARS", "BARS audio"),
    (0, b"MsgStdBn", "MSBT text"),
    (0, b"MsgPrjBn", "MSBP project"),
    (0, b"BY\x00", "BYML (big-endian)"),
    (0, b"YB\x02", "BYML (little-endian)"),
    (0, b"YB\x03", "BYML (little-endian)"),
    (0, b"YB\x04", "BYML (little-endian)"),
    (0, b"YB\x07", "BYML (little-endian)"),
    (0, b"NSO0", "Switch NSO executable"),
    (0, b"NRO0", "Switch NRO executable"),
    (0, b"PFS0", "Switch PFS0/NSP container"),
    (0, b"HFS0", "Switch HFS0 container"),
    (0, b"META", "Switch NPDM"),
    (0, b"WUX0", "Wii U WUX image"),
    (0x100, b"HEAD", "Switch XCI cartridge image"),
    (0, b"7z\xbc\xaf\x27\x1c", "7-Zip archive"),
    (0, b"Rar!\x1a\x07", "RAR archive"),
    (0x10, b"NRO0", "Switch NRO executable"),
    (0, b"\xcf\xfa\xed\xfe", "Mach-O binary"),
    (0, b"\xca\xfe\xba\xbe", "Mach-O universal binary"),
    (0, b"MZ", "Windows PE binary"),
    (0, b"\x03\x02\x23\x07", "SPIR-V shader"),
    # per-frame player traces (recomp games/smm2/trace.c, the Ryujinx trace build): snapshots of game memory
    (0, b"TRC1", "player state trace (game memory)"),
]
# Paths that hold files derived from the dump (recompiled code, translated shaders, extracted data)
BLOCKED_PATH_PARTS = ("/recomp/private/", "/recomp/generated/", "/recomp/build/")
# AArch64 disassembly lines ("71000123c0: f9400420  ldr x0, [x1, #8]" or "ldr x0, [x1]")
ASM_LINE = re.compile(
    r"\b(ldr|str|ldp|stp|ldrb|strb|ldrh|strh|adrp|movz|movk|blr|cbz|cbnz|tbz|tbnz|ldaxr|stlxr)"
    r"\s+[xwqsdv][0-9]{1,2}\b",
    re.IGNORECASE,
)
# recompiler output: guest-register accesses in generated C/C++ (ctx->x[3], X(3), r.x[3] ...)
RECOMP_LINE = re.compile(r"(\b\w+->[xwvq]\[\d+\]|\bX\(\d+\)|\b(ctx|cpu|st)\.[xwvq]\[\d+\])")
ASM_LIMIT = 200
RECOMP_LIMIT = 500
KEY_LINE = re.compile(
    r"^\s*(header_key|master_key_[0-9a-f]{2}|titlekek_[0-9a-f]{2}|key_area_key_\w+|package2_key_\w+"
    r"|[0-9a-f]{32})\s*=\s*[0-9a-f]{32,64}\s*$",
    re.IGNORECASE | re.MULTILINE,
)
# "<key>", "label: <key>", "<key> label" - lines that are just a key and a short label
BARE_KEY_LINE = re.compile(
    r"^(?:[\w .:-]{0,40}[:=\s])?\s*(?:[0-9a-f]{32}|[0-9a-f]{64})\s*(?:[#;]?[\w .:-]{0,40})?$", re.IGNORECASE
)
KEY_FILENAME = re.compile(r"keys?\b|otp|seeprom", re.IGNORECASE)
MAX_BYTES = 5 * 1024 * 1024
IMAGE_EXTENSIONS = {".png", ".jpg", ".jpeg", ".gif", ".webp", ".bmp", ".tga", ".dds", ".tif", ".tiff", ".ktx", ".astc"}
SCREENSHOT_DIR = "/docs/screenshots/"
SCREENSHOT_TYPES = {".png", ".jpg", ".jpeg"}
SCREENSHOT_MAX_BYTES = 1024 * 1024
SCREENSHOT_MAX_FILES = 12
ALLOW_LARGE_PREFIXES = ()  # nothing in the repo should be this large


def wiiu_rpx(head: bytes) -> bool:
    # Wii U RPX/RPL: ELF, 32-bit, big-endian, OS/ABI 0xCA 0xFE
    return head[:4] == b"\x7fELF" and head[4] == 1 and head[5] == 2 and head[7:9] == b"\xca\xfe"


def elf_aarch64(head: bytes) -> bool:
    # ELF, little-endian, e_machine 0xB7 (AArch64): a Switch game binary or a host build product
    return head[:4] == b"\x7fELF" and len(head) > 19 and head[18:20] == b"\xb7\x00"


def check_file(path: Path) -> list[str]:
    problems: list[str] = []
    name = path.name.lower()
    posix = "/" + path.as_posix().lstrip("/")
    if any(part in posix for part in BLOCKED_PATH_PARTS):
        problems.append("file is under a recomp folder for dump-derived output (private/generated/build)")
    if name in BLOCKED_NAMES or path.suffix.lower() in BLOCKED_EXTENSIONS:
        problems.append(f"blocked file type '{path.suffix or name}'")
    suffix = path.suffix.lower()
    if suffix in IMAGE_EXTENSIONS:
        if SCREENSHOT_DIR in posix:
            if suffix not in SCREENSHOT_TYPES:
                problems.append(f"screenshots must be PNG or JPEG, not '{suffix}'")
            elif path.is_file() and path.stat().st_size > SCREENSHOT_MAX_BYTES:
                problems.append(f"screenshot is over {SCREENSHOT_MAX_BYTES // 1024} KiB")
        elif "/assets/placeholder/" not in posix:
            problems.append("images are only allowed in docs/screenshots/ (captures) and assets/placeholder/")
    if not path.is_file():
        return problems
    size = path.stat().st_size
    if size > MAX_BYTES and not str(path).startswith(ALLOW_LARGE_PREFIXES):
        problems.append(f"file is {size // 1024} KiB (limit {MAX_BYTES // 1024} KiB)")
    with path.open("rb") as fh:
        head = fh.read(0x110)
    for offset, magic, label in MAGICS:
        if head[offset:offset + len(magic)] == magic:
            problems.append(f"content looks like {label}")
    if len(head) >= 9 and wiiu_rpx(head):
        problems.append("content looks like a Wii U RPX/RPL executable")
    if elf_aarch64(head):
        problems.append("content looks like an AArch64 ELF (game or host binary)")
    if size <= MAX_BYTES:
        try:
            text = path.read_text(encoding="utf-8")
        except (UnicodeDecodeError, OSError):
            text = ""
        if KEY_LINE.search(text):
            problems.append("contains lines shaped like console key entries")
        if len(ASM_LINE.findall(text)) > ASM_LIMIT:
            problems.append("content looks like AArch64 disassembly")
        if len(RECOMP_LINE.findall(text)) > RECOMP_LIMIT:
            problems.append("content looks like recompiled game code (guest-register accesses)")
        lines = [ln.strip() for ln in text.splitlines() if ln.strip()]
        if lines and all(BARE_KEY_LINE.match(ln) for ln in lines):
            problems.append("file is a list of bare 128/256-bit hex keys")
    if KEY_FILENAME.search(name) and path.suffix.lower() in {"", ".txt", ".bin", ".dat"}:
        problems.append("filename looks like a key dump")
    return problems


def git_files(staged: bool) -> list[Path]:
    cmd = ["git", "diff", "--cached", "--name-only", "--diff-filter=ACMR", "-z"] if staged else ["git", "ls-files", "-z"]
    out = subprocess.run(cmd, check=True, capture_output=True).stdout
    return [Path(p) for p in out.decode().split("\0") if p]


def main(argv: list[str]) -> int:
    if argv and argv[0] == "--staged":
        files = git_files(staged=True)
    elif argv:
        files = [Path(a) for a in argv]
    else:
        files = git_files(staged=False)
    failures = 0
    for f in files:
        # the guard and its tests necessarily mention blocked names/magics
        if f.as_posix() in {"scripts/check_no_proprietary.py", "scripts/test_check_no_proprietary.py"}:
            continue
        for problem in check_file(f):
            print(f"PROPRIETARY? {f}: {problem}")
            failures += 1
    shots = {f.as_posix() for f in files if SCREENSHOT_DIR in "/" + f.as_posix().lstrip("/")}
    try:
        tracked = subprocess.run(["git", "ls-files", "docs/screenshots"], capture_output=True, text=True).stdout
        shots |= {ln for ln in tracked.splitlines() if ln and Path(ln).exists()}
    except OSError:
        pass
    if len(shots) > SCREENSHOT_MAX_FILES:
        print(f"PROPRIETARY? docs/screenshots: {len(shots)} files (limit {SCREENSHOT_MAX_FILES})")
        failures += 1
    if failures:
        print(f"\n{failures} problem(s). Nintendo assets, binaries, text, audio and keys must never be committed.")
        return 1
    print(f"ok: {len(files)} file(s) checked, nothing proprietary found")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
