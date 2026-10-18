"""A one-course title screen for Ryujinx reference captures: the game picks its title-screen course from ten
(Course/title_course_data_00..09.szs); this makes all ten the same one, so Ryujinx shows course N on every boot.

    titlemod.py --romfs <game romfs> --course N --out <mod folder>/romfs --copy

Use the folder as a Ryujinx LayeredFS mod (<root>/mods/contents/01009b90006dc000/<name>/romfs; copies, not links).
The recomp picks its course with SMM2_SEED instead (games/smm2/seed.c: 4 -> 00, 2 -> 01, 1 -> 02, 6 -> 05,
3 -> 06, 10 -> 07; 08 and 09 only come up once the save unlocks them): a course other than the slot's own ends
both runtimes while the title scene loads (the game's behaviour: Ryujinx does it too when made to pick that slot;
docs/smm2-bringup.md, open issues), so with this mod Ryujinx works only while it happens to pick slot N.
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
