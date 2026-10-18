// RECOMP_MOD_AUTOPLAY=1: a minimal autopilot for unattended 100 Mario test runs (evidence, regression checks). It is
// local to the mod and only steers by what the mod already sees: the Endless requests (course list, course start,
// miss, clear, skip). On the map it presses A (Play, and any window); in a course it holds right + run; when a course
// takes longer than RECOMP_MOD_AUTOPLAY_SKIP seconds (default 20, under the 30 s after which the game offers to rate a skipped course), it holds - to skip it. `=jump` also jumps every
// second and a half. It presses buttons through the test-input hook (runtime/hle/input.c: active while LARECOMP_INPUT
// is set, which also navigates the menus up to the Endless Challenge). Not a pad system: the R2 scripted pad replaces
// it for replays.
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

#include "mod.h"

extern uint64_t g_test_buttons, g_test_buttons_until;  // runtime/game_hooks.c (weak), read by runtime/hle/input.c
extern uint64_t renderer_present_count(void);

enum { BTN_A = 1 << 0, BTN_Y = 1 << 3, BTN_MINUS = 1 << 11, BTN_RIGHT = 1 << 14, BTN_DOWN = 1 << 15 };
enum { AP_OFF, AP_MAP, AP_COURSE, AP_AFTER };

static volatile int mode = AP_OFF;
static volatile uint64_t mode_since, course_since;
static pthread_once_t once = PTHREAD_ONCE_INIT;

static void *autopilot(void *arg) {
    (void)arg;
    const char *v = getenv("RECOMP_MOD_AUTOPLAY");
    bool jump = v && !strcmp(v, "jump");
    uint64_t skip_after = (getenv("RECOMP_MOD_AUTOPLAY_SKIP") ? strtoull(getenv("RECOMP_MOD_AUTOPLAY_SKIP"), NULL, 0) : 20) * 60;
    for (;;) {
        usleep(20000);
        uint64_t now = renderer_present_count(), b = 0;
        switch (mode) {
        case AP_MAP:  // after 10 s (windows and the map settle), tap A every 4 s
            if (now - mode_since > 600 && (now - mode_since) % 240 < 8) b = BTN_A;
            break;
        case AP_COURSE:
            if (now - course_since > skip_after) {
                // hold - (about 2 s) until the skip happens, letting go now and then: a hold that began during a
                // transition (a retry's course-in) doesn't count
                if ((now - course_since - skip_after) % 240 < 200) b = BTN_MINUS;
                if (now - course_since == skip_after + 1) LOG("[mod] autoplay: skipping (%llus in the course)\n",
                                                              (unsigned long long)(skip_after / 60));
            } else if (now - mode_since > 120) {
                b = BTN_RIGHT | BTN_Y;
                if (jump && (now - mode_since) % 90 < 20) b |= BTN_A;
            }
            break;
        case AP_AFTER: {  // the goal, then the result screen (Boo / I like it / Quit / Next): make sure of Next
            uint64_t t = now - mode_since;
            if (t >= 480 && t < 488) b = BTN_RIGHT;
            else if (t >= 540 && t < 548) b = BTN_DOWN;
            else if (t >= 600 && t < 608) b = BTN_A;
            else if (t > 700) mode = AP_MAP, mode_since = now;
            break;
        }
        default: break;
        }
        g_test_buttons = b;
        g_test_buttons_until = b ? now + 4 : 0;
    }
    return NULL;
}

static void start(void) {
    pthread_t t;
    pthread_create(&t, NULL, autopilot, NULL);
    pthread_detach(t);
    LOG("[mod] autoplay on\n");
}

// endless.c: what just happened
void autoplay_event(const char *what) {
    if (!getenv("RECOMP_MOD_AUTOPLAY")) return;
    pthread_once(&once, start);
    uint64_t now = renderer_present_count();
    if (!strcmp(what, "list")) {
        if (mode == AP_OFF) mode = AP_MAP, mode_since = now;
    } else if (!strcmp(what, "start")) {
        mode = AP_COURSE, mode_since = course_since = now;
    } else if (!strcmp(what, "miss")) {
        mode = AP_COURSE, mode_since = now;  // (the course's clock goes on: a course it can't clear gets skipped)
    } else if (!strcmp(what, "clear")) {
        mode = AP_AFTER, mode_since = now;
    } else if (!strcmp(what, "skip")) {  // (the next course follows at once)
        mode = AP_MAP, mode_since = now;
    } else if (!strcmp(what, "end")) {
        mode = AP_OFF;
    }
}
