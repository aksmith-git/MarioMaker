// Weak stand-ins for SMM1's ending scene (ending.h): log the call, report done after 3 seconds. The real scene
// (games/smm2/mods/ending/) replaces them.
#include <time.h>

#include "ending.h"
#include "mod.h"

static double t_done;

static double now_s(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return ts.tv_sec + ts.tv_nsec / 1e9;
}

__attribute__((weak)) void hundred_ending_show(int difficulty, int n, const struct hundred_credit *credits) {
    static const char *const names[4] = {"Easy", "Normal", "Expert", "Super Expert"};
    LOG("[mod] ending (stand-in): 100 Mario Challenge %s won, %d courses\n", names[difficulty & 3], n);
    for (int i = 0; i < n; i++) LOG("[mod]   %2d. %s  (%s)\n", i + 1, credits[i].name, credits[i].maker);
    t_done = now_s() + 3;
}

__attribute__((weak)) bool hundred_ending_done(void) { return now_s() >= t_done; }
