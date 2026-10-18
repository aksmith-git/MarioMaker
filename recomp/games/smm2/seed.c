// SMM2_SEED=<n>: a fixed seed for the game's global random number generator, so a boot is repeatable.
//
// The game's sead::Random objects seed themselves from nn::os::GetSystemTick() (sead::Random::init(), main+0x29a050);
// the first one is the global generator that, among other things, picks the title screen's course
// (Course/title_course_data_NN). With a fixed seed every boot shows the same course, which reference comparisons
// against an emulator need (Ryujinx shows the same course on every boot; SMM2_SEED=4 is the one it showed on
// 2026-10-19). Without the variable the tick is passed through unchanged.
//
// The game makes more generators after the first (each seeded from the tick again). SMM2_SEED_ALL=1 seeds every one of
// them with SMM2_SEED, which is what the Ryujinx trace build does when it patches the tick read in sead::Random::init()
// to the same constant: then both runtimes draw the same numbers (replay comparisons with enemies).
#include <stdlib.h>

#include "../../runtime/hle.h"

#define RANDOM_INIT_RET (MAIN_BASE + 0x29a064ull) // return address of the GetSystemTick call inside sead::Random::init()

HLE_FUNC("_ZN2nn2os13GetSystemTickEv", smm2_get_system_tick) {
    uint64_t t = rt_ticks();
    static int seeded;
    if (c->x[30] == RANDOM_INIT_RET && (!seeded || ENV("SMM2_SEED_ALL"))) {
        const char *s = getenv("SMM2_SEED");
        if (s) {
            t = strtoull(s, NULL, 0);
            if (!seeded) LOG("[smm2] global random seed %llu%s\n", (unsigned long long)t,
                             ENV("SMM2_SEED_ALL") ? " (and every later sead::Random)" : "");
            seeded = 1;
        }
    }
    c->x[0] = t;
}
