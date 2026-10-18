"""Copy the system data the game reads from the player's own firmware (optional): shared fonts and data archives.

    fonts.py --firmware <registered NCA folder> --keys <prod.keys> --data <data root> [--hactool PATH]

Writes <data root>/fonts/*.bfttf (the console's shared fonts, nn::pl: runtime/hle/pl.c substitutes a host font
without them, which looks different) and <data root>/sysdata/<title id>/ (the RomFS of system data archives the game
opens by data id, e.g. the Mii model data; runtime/services/fs.c serves them). The firmware is the NCA folder of a
system dump, e.g. Ryujinx's bis/system/Contents/registered. Keys are only handed to hactool and read in place;
hactool's output is never shown (it can echo key values), only whether each step worked.
"""
import argparse
import os
import shutil
import subprocess
import sys
import tempfile

FONT_TITLES = {
    '0100000000000810',  # NintendoExtension (button glyphs)
    '0100000000000811',  # Standard (Latin + Japanese)
    '0100000000000812',  # Korean
    '0100000000000813',  # Chinese (traditional)
    '0100000000000814',  # Chinese (simplified, + extension)
}
DATA_TITLES = {
    '0100000000000802',  # MiiModel: Mii face parts (nn::mii renders the players' Miis)
    '0100000000000806',  # NgWord (nn::ngc: text filter)
    '010000000000080e',  # TimeZoneBinary
    '0100000000000823',  # NgWord2
}


def run(cmd):
    return subprocess.run(cmd, capture_output=True, text=True, errors='replace')


def nca_path(entry):
    # Ryujinx keeps each NCA as a folder <id>.nca/00; a plain dump has files
    return os.path.join(entry, '00') if os.path.isdir(entry) else entry


def title_id(hactool, keys, nca):
    out = run([hactool, '-k', keys, '-t', 'nca', nca]).stdout
    for line in out.splitlines():
        if line.strip().startswith('Title ID:'):
            return line.split(':', 1)[1].strip().lower()
    return None


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--firmware', required=True)
    ap.add_argument('--keys', required=True)
    ap.add_argument('--data', required=True)
    ap.add_argument('--hactool', default=shutil.which('hactool') or os.path.expanduser('~/smm-work/tools/bin/hactool'))
    a = ap.parse_args()
    if not os.path.exists(a.hactool):
        sys.exit('sysdata: hactool not found (pass --hactool)')
    want = FONT_TITLES | DATA_TITLES
    found = {}
    for name in sorted(os.listdir(a.firmware)):
        if not name.endswith('.nca'):
            continue
        p = nca_path(os.path.join(a.firmware, name))
        if os.path.getsize(p) < 0x8000:  # (data archives are larger: skip tiny NCAs quickly)
            continue
        tid = title_id(a.hactool, a.keys, p)
        if tid in want:
            found[tid] = p
        if len(found) == len(want):
            break
    fonts = os.path.join(a.data, 'fonts')
    os.makedirs(fonts, exist_ok=True)
    nf = nd = 0
    for tid, p in sorted(found.items()):
        if tid in FONT_TITLES:
            with tempfile.TemporaryDirectory() as tmp:
                run([a.hactool, '-k', a.keys, '-t', 'nca', '--romfsdir', tmp, p])
                for f in os.listdir(tmp):
                    if f.endswith('.bfttf'):
                        shutil.copy(os.path.join(tmp, f), os.path.join(fonts, f))
                        nf += 1
        else:
            out = os.path.join(a.data, 'sysdata', tid)
            if os.path.isdir(out):
                shutil.rmtree(out)
            os.makedirs(out)
            run([a.hactool, '-k', a.keys, '-t', 'nca', '--romfsdir', out, p])
            nd += 1 if os.listdir(out) else 0
    missing = sorted(want - set(found))
    print(f'sysdata: {nf} shared font files, {nd} data archives'
          + (f' (not in this firmware: {", ".join(missing)})' if missing else ''))
    if 'nintendo_udsg-r_std_003.bfttf' not in os.listdir(fonts):
        sys.exit('sysdata: the standard font was not found')


if __name__ == '__main__':
    main()
