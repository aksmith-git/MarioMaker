"""A one-course title screen (test mod): the game picks its title-screen course at random from ten; this makes all
ten the same one, so the title screen is the same on every boot (frame comparisons against a reference runtime).

    titlemod.py --romfs <game romfs> --course N --out <mod folder>/romfs [--out <another runtime's mod romfs> ...]

Writes Course/title_course_data_00..09.szs as links to the game's own title_course_data_<N>.szs (--copy: copies).
Use the folder with RECOMP_MOD_DIR=<mod folder> (runtime/mods.c) and, for Ryujinx, as a LayeredFS mod
(<root>/mods/contents/01009b90006dc000/<name>/romfs).
"""
import argparse
import os
import shutil
import sys


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--romfs', required=True)
    ap.add_argument('--course', type=int, required=True)
    ap.add_argument('--out', action='append', required=True)
    ap.add_argument('--copy', action='store_true', help="copies instead of links (Ryujinx's LayeredFS needs them)")
    a = ap.parse_args()
    src = os.path.realpath(os.path.join(a.romfs, 'Course', f'title_course_data_{a.course:02d}.szs'))
    if not os.path.isfile(src):
        sys.exit(f'titlemod: {src} not found')
    for out in a.out:
        d = os.path.join(out, 'Course')
        os.makedirs(d, exist_ok=True)
        for i in range(10):
            p = os.path.join(d, f'title_course_data_{i:02d}.szs')
            if os.path.lexists(p):
                os.remove(p)
            shutil.copyfile(src, p) if a.copy else os.symlink(src, p)
        print(f'titlemod: {d}: all ten title courses -> title_course_data_{a.course:02d}')


if __name__ == '__main__':
    main()
