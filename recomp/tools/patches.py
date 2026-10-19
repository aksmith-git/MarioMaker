"""Per-instruction replacements applied by gen.py: guest address -> function(gen, addr) returning C code.

The shared recompiler holds no game patches. A game adds its own in games/<game>/patches.py, which defines
PATCHES = {address: fn}; gen.py --patches <file> loads it. Each patch should check the instruction word it
replaces (so a different build of the game fails loudly instead of being patched in the wrong place) and
describe what it does in words, not as disassembly.

A patch module's addresses assume the module at its MAIN (0x7000000000, runtime/rt.h); given another base (`make
GUEST_ORIGIN=<base>`: runtime/rt.h GUEST_ORIGIN) they are moved with it.
"""
import importlib.util
import os

PATCHES = {}


def load(path, base=None):
    """Merge the PATCHES of a game's patch module into PATCHES (moved to base when the module says its MAIN)."""
    if not path:
        return
    path = os.path.abspath(path)
    spec = importlib.util.spec_from_file_location('game_patches', path)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    shift = base - mod.MAIN if base is not None and hasattr(mod, 'MAIN') else 0
    PATCHES.update({a + shift: fn for a, fn in getattr(mod, 'PATCHES', {}).items()})
