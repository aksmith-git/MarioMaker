// Host side of the mod hook points (games/smm2/mods/patches.py): called right after the game's own instruction at
// each listed address. With SMM1's rules (the 100 Mario mod, ruleset "hundred"):
//   - lives clamps: where a clamp loads its cap of 99 or compares a lives count with 99, 100 is used instead (SMM1's
//     run starts at 100 lives and can't go above it);
//   - skip during the miss demo: the run flag that makes it cost a life is cleared again (SMM1's skip is free);
//   - retry after a miss: coins and score go back to their values at the start of the course (SMM1 throws away
//     what the failed attempt collected);
//   - extra lives: an award worth several lives (3-Up Moon) banks one pending 1-Up, as in SMM1 (the cap of 3 per
//     course and the pay-out on clear are SMM2's own and the same as SMM1's).
// Otherwise nothing changes.
#include "mod.h"

#define RUN_OBJECT MAIN_ADDR(0x2c56bb0) // the Endless run: +0x30 current counters, +0x40 counters at course start

void bridge_event(const char *what, uint32_t d);  // bridge.c

static __thread uint32_t pending_before;  // pending 1-Ups before an award (the game's main thread)

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
    LOG("[mod] retry: coins %u (total %u) -> %u (total %u), score %u -> %u\n", g32(g64(cur + 0x10)),
        g32(g64(cur + 0x10) + 4), g32(g64(snap + 0x10)), g32(g64(snap + 0x10) + 4), g32(g64(cur + 0x18)),
        g32(g64(snap + 0x18)));
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
        bridge_event("retry", 0);
        break;
    case 0x16ce788: case 0x16ce9e0:  // pending 1-Ups loaded (w4) before an award is added
        pending_before = (uint32_t)c->x[4];
        break;
    case 0x16ce7f4: case 0x16cea48:  // ... and stored: an award counts as one
        if ((uint32_t)c->x[4] != pending_before)
            LOG("[mod] 1-Up banked (%s): pending %u -> %u\n", addr - MAIN_ADDR(0) == 0x16ce7f4 ? "coins" : "item",
                pending_before, (uint32_t)c->x[4]);
        if ((uint32_t)c->x[4] > pending_before + 1) {
            LOG("[mod] award: pending 1-Ups %u -> %u, banked as %u (SMM1: one per award)\n", pending_before,
                (uint32_t)c->x[4], pending_before + 1);
            c->x[4] = pending_before + 1;
            s32(c->x[16] + 0x38, pending_before + 1);
        }
        break;
    default:
        lives_cap(c, addr);
    }
}
