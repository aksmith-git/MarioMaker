"""Mod hook points for the generator: host code after single instructions of SMM2 v3.0.3 (merged into
games/smm2/patches.py).

Each entry keeps the game's own instruction and then calls smm2_mod_after(c, address) (games/smm2/mods/hooks.c). The
host side decides, per address and per active mod, whether to change anything (a register, the flags, a store); with
every mod off the instruction behaves exactly as before. So the behaviour of a hook point can change without
recompiling; only adding or removing a point needs the generator again.

Each point is checked against the instruction word it expects, so another build of the game fails loudly instead of
being hooked in the wrong place. Offsets are from the start of `main` (the recomp loads it at 0x7000000000).
"""

MAIN = 0x7000000000

# offset in main -> (expected instruction word, what the host does there when the mod is on)
HOOK_POINTS = {
    # 100 Mario: the in-course lives clamp is 99 (per-frame score / coin / lives update): one point loads the cap
    # the clamps fall back to, the others compare a new lives count with it. With the mod: 100.
    0x16ce764: (0x52800C6E, 'lives cap: cap value'),
    0x16ce810: (0x71018CBF, 'lives cap: compare'),
    0x16ce92c: (0x71018C9F, 'lives cap: compare'),
    0x16ce9ac: (0x71018C9F, 'lives cap: compare'),
    0x16cea5c: (0x71018CBF, 'lives cap: compare'),
    0x16ceaa0: (0x71018D3F, 'lives cap: compare'),
    # 100 Mario: the per-player info the HUD and menus show (lives as a byte, clamped to 99), four players: each
    # clamp compares with 99 and loads 99 as the fallback
    0x16848e8: (0x71018D7F, 'lives shown: compare'),
    0x16848ec: (0x52800C6C, 'lives shown: cap value'),
    0x168494c: (0x71018D7F, 'lives shown: compare'),
    0x1684950: (0x52800C6C, 'lives shown: cap value'),
    0x16849b0: (0x71018D7F, 'lives shown: compare'),
    0x16849b4: (0x52800C6C, 'lives shown: cap value'),
    0x1684a14: (0x71018D5F, 'lives shown: compare'),
    0x1684a18: (0x52800C6B, 'lives shown: cap value'),
    0x1684a8c: (0x52800C6E, 'lives shown: cap value'),
    0x1684abc: (0x71018C1F, 'lives shown: compare'),
    # 100 Mario: the skip page marks "skipped during the miss demo" (Endless run flag +0x48 = 1), which costs a life.
    # SMM1's skip is always free: the host stores 0 again.
    0x15877b0: (0x39012109, 'skip during the miss demo: the run flag store'),
    # 100 Mario: course set-up, Endless retry branch, after clears and lives were reloaded from the run record. SMM1
    # throws away the coins and score of a failed attempt: the host copies them back from the course-start snapshot.
    0x16dfb40: (0x29012509, 'retry: the last lives store'),
    # 100 Mario: Endless banks extra lives as pending 1-Ups (max 3 per course), adding n for an award worth n (a 3-Up
    # Moon, a big coin haul). SMM1 counts every award as one: the host notes the pending count when it is loaded and
    # caps the stored one at that + 1. Two paths (the coin roll-over and the 1-Up items), load and store each.
    0x16ce788: (0xB9403A04, 'pending 1-Ups: load (coins)'),
    0x16ce7f4: (0xB9003A04, 'pending 1-Ups: store (coins)'),
    0x16ce9e0: (0xB9403A04, 'pending 1-Ups: load (items)'),
    0x16cea48: (0xB9003A04, 'pending 1-Ups: store (items)'),
}


def _hook(word):
    def patch(gen, addr):
        have = gen.words[(addr - gen.lo) // 4]
        if have != word:
            raise SystemExit(f'mods/patches.py: {addr:#x} is {have:#010x}, not {word:#010x} (not v3.0.3?)')
        orig = ' '.join(gen.render_single(gen.lift(addr), addr, None, set()))
        return f'{orig} {{ extern void smm2_mod_after(Cpu *, uint64_t); smm2_mod_after(c, {addr:#x}ull); }}'
    return patch


PATCHES = {MAIN + off: _hook(word) for off, (word, _) in HOOK_POINTS.items()}
