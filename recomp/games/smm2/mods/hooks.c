// Host side of the mod hook points (games/smm2/mods/patches.py): called right after the game's own instruction at
// each listed address. With SMM1's rules (the 100 Mario mod, ruleset "hundred"):
//   - lives clamps: where a clamp loads its cap of 99 or compares a lives count with 99, 100 is used instead (SMM1's
//     run starts at 100 lives and can't go above it);
//   - skip during the miss demo: the run flag that makes it cost a life is cleared again (SMM1's skip is free);
//   - retry after a miss: coins and score go back to their values at the start of the course (SMM1 throws away
//     what the failed attempt collected).
// Otherwise nothing changes.
#include "mod.h"

#define RUN_OBJECT 0x7002c56bb0ull  // the Endless run: +0x30 current counters, +0x40 counters at course start

static void lives_cap(Cpu *c, uint64_t addr) {
    uint32_t w = g32(addr);
    if ((w & 0xFFE00000u) == 0x52800000u && ((w >> 5) & 0xFFFF) == 99) {  // movz wd, #99
        c->x[w & 0x1F] = 100;
    } else if ((w & 0xFF80001Fu) == 0x7100001Fu && ((w >> 10) & 0xFFF) == 99) {  // cmp wn, #99
        uint32_t rn = (w >> 5) & 0x1F;
        (void)subs32(c, (uint32_t)c->x[rn], 100);
    }
}

static void retry_rollback(void) {
    uint64_t run = g64(RUN_OBJECT);
    if (!run) return;
    uint64_t cur = g64(run + 0x30), snap = g64(run + 0x40);
    if (!cur || !snap || !g64(cur + 0x10) || !g64(snap + 0x10) || !g64(cur + 0x18) || !g64(snap + 0x18)) return;
    s64(g64(cur + 0x10), g64(g64(snap + 0x10)));  // coins {below 100, total}
    s32(g64(cur + 0x18), g32(g64(snap + 0x18)));  // score
}

void smm2_mod_after(Cpu *c, uint64_t addr) {
    if (!mod_hundred_rules()) return;
    switch (addr - MAIN_ADDR(0)) {
    case 0x15877b0:  // strb w9, [x8, #0x48]: "skipped during the miss demo"
        s8(c->x[8] + 0x48, 0);
        break;
    case 0x16dfb40:  // Endless retry: clears and lives reloaded from the run record
        retry_rollback();
        break;
    default:
        lives_cap(c, addr);
    }
}
