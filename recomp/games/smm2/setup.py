"""Lay out the player's own Super Mario Maker 2 v3.0.3 files for the recomp.

    setup.py --base-romfs <dir> --update-exefs <dir> --update-romfs <dir> --out <dir>

<out>/exefs: the update's main, sdk, rtld and main.npdm (copied; checked against the v3.0.3 build ids)
<out>/romfs: the base game's RomFS with the update's files over it, as a tree of symbolic links (nothing copied;
             runtime/services/fs.c serves it without building an image)

Nothing here reads or writes keys; the files must already be extracted (exefs and romfs folders).
"""
import argparse
import os
import shutil
import struct
import sys

# NSO build ids (module ids) of the v3.0.3 update: the recomp's addresses, patches and findings are for these.
# (Identifiers only: the same ids name community patch files.)
BUILD_IDS = {
    'main': 'c2dc405ac414c37c8b1c50219c7a0f0c',
    'sdk': 'fcf93a41b08c5193a1410d262602dc621617c56f',
}
EXEFS_FILES = ('main', 'sdk', 'rtld', 'main.npdm')


def build_id(path):
    with open(path, 'rb') as f:
        head = f.read(0x60)
    if head[:4] != b'NSO0':
        return None
    return head[0x40:0x60].rstrip(b'\0').hex()


def copy_exefs(src, dst):
    os.makedirs(dst, exist_ok=True)
    for name in EXEFS_FILES:
        p = os.path.join(src, name)
        if not os.path.isfile(p):
            sys.exit(f'setup: {p} is missing (extract the v3.0.3 update\'s exefs there)')
        want = BUILD_IDS.get(name)
        if want:
            got = build_id(p)
            if got is None:
                sys.exit(f'setup: {p} is not an NSO')
            if not got.startswith(want):
                sys.exit(f'setup: {p} is not the v3.0.3 {name} (build id {got[:16]}..., want {want[:16]}...)')
        q = os.path.join(dst, name)
        if not (os.path.exists(q) and os.path.getsize(q) == os.path.getsize(p)
                and os.path.getmtime(q) >= os.path.getmtime(p)):
            shutil.copy2(p, q)
    print(f'setup: exefs ok ({", ".join(EXEFS_FILES)})')


def place(target, link):
    """A symlink; where the OS won't make one (Windows without Developer Mode), a hard link (same drive), else a copy."""
    try:
        os.symlink(target, link)
        return
    except OSError:
        pass
    try:
        os.link(target, link)
    except OSError:
        shutil.copy2(target, link)


def link_tree(src, dst, counts):
    """Symlink every file of src into dst (dst's existing links are replaced: later trees win)."""
    for root, dirs, files in os.walk(src, followlinks=True):
        rel = os.path.relpath(root, src)
        out = os.path.join(dst, rel) if rel != '.' else dst
        os.makedirs(out, exist_ok=True)
        dirs[:] = [d for d in dirs if not d.startswith('.')]
        for f in files:
            if f.startswith('.'):
                continue
            target = os.path.realpath(os.path.join(root, f))
            link = os.path.join(out, f)
            if os.path.islink(link):
                if os.readlink(link) == target:
                    counts['same'] += 1
                    continue
                os.remove(link)
                counts['replaced'] += 1
            elif os.path.exists(link):  # (a hard link or copy from place(): replaced the same way)
                if os.path.samefile(link, target):
                    counts['same'] += 1
                    continue
                os.remove(link)
                counts['replaced'] += 1
            place(target, link)
            counts['linked'] += 1


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--base-romfs', required=True)
    ap.add_argument('--update-exefs', required=True)
    ap.add_argument('--update-romfs', required=True)
    ap.add_argument('--out', required=True)
    a = ap.parse_args()
    for d in (a.base_romfs, a.update_exefs, a.update_romfs):
        if not os.path.isdir(d):
            sys.exit(f'setup: {d} is not a folder (pass DUMP_BASE_ROMFS / DUMP_UPDATE_EXEFS / DUMP_UPDATE_ROMFS)')
    copy_exefs(a.update_exefs, os.path.join(a.out, 'exefs'))
    romfs = os.path.join(a.out, 'romfs')
    counts = {'linked': 0, 'replaced': 0, 'same': 0}
    link_tree(a.base_romfs, romfs, counts)
    base = dict(counts)
    link_tree(a.update_romfs, romfs, counts)
    n = sum(1 for _, _, fs in os.walk(romfs) for _ in fs)
    print(f'setup: romfs ok: {n} files ({base["linked"] + base["same"]} from the base game, '
          f'{counts["replaced"] - base["replaced"]} replaced by the update)')


if __name__ == '__main__':
    main()
