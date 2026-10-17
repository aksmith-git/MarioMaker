# Decisions

Newest first. Each entry: what was decided, why, and what would change it.

## 2026-10-17 — Native app milestone (Phase 5b) before the 100 Mario Challenge
Owner wants a proper native macOS app before the 100 Mario Challenge work starts. Phase 5b adds the
app bundle, import screen, course library, pause menu, settings, save data and per-course
"playable" marking. Phase 6's course pool is limited to playable courses and grows with Phase 7
(alternative: do Phase 7 first; rejected for now because it delays the mode a lot).

## 2026-10-17 — Course sample from the public mm2_level dataset
TGRCode's API returned 502 and SMMDB served a self-signed certificate. TheGreatRambler's Hugging
Face dataset (26.6M courses, Feb 2022, CC BY-NC-SA 4.0) is read remotely with DuckDB range requests.
It also carries Nintendo's own difficulty bucket and clear rates, useful for Phase 6. Shard 0 is
used first: its uploads predate game v2.0, matching the v1.0.0 dump. Courses stay outside the repo.

## 2026-10-17 — SMM2 extracted with hactool rather than Ryujinx's Extract Data
Same output, but scriptable and repeatable. Keys are read in place from Ryujinx's folder.

## 2026-10-17 — SMM2 RomFS lives on the external SSD
Internal disk had ~1 GB free. `~/smm-work/extracted/smm2-romfs` is a symlink, so paths are unchanged.

## 2026-10-17 — CDecrypt patched to take the common key at runtime
Upstream CDecrypt embeds the Wii U common key. Rule: key material only comes from the owner's
console. The local build reads `$WIIU_COMMON_KEY_FILE` instead. Patch kept outside the repo.

## 2026-10-17 — SMM1 reference build is update v208, not the disc's v16
The disc ships base v16 plus update v208 (in the GI partition). Only v208 contains Super Expert
in the 100 Mario Challenge, so v208's `Block.rpx` and content are the reference for Phase 6.
Base content + v208 overlay = the effective game.

## 2026-10-17 — NSO loading via Ghidra Switch Loader built from source; nx2elf as fallback
Upstream Switch Loader release targets Ghidra 12.1.2; extensions must match Ghidra exactly, and
REA is configured against 12.1.4. Built 1.6.1 @ 5d80d195 against 12.1.4.

## 2026-10-17 — hactoolnet dropped
Its source repository is no longer on GitHub; hactool 1.4.0 + nstool v1.9.2 cover the need.

## 2026-10-17 — Proprietary-content guard checks content, not just extensions
Extension lists miss renamed files. The guard also checks magic bytes (Yaz0, SARC, zstd, BNTX,
BFRES, MSBT, BYML, NSO, XCI, WUX, RPX...), key-file shapes, and a 5 MiB size cap. Runs in CI and
as a pre-commit hook.

## Open
- Licence for the repository (owner's call).
- Engine language/framework: deferred to Phase 4 per plan.
