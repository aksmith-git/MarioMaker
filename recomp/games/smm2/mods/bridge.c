// The run side of the 100 Mario bridge (hundred.h): SMM1's screens on top, SMM2's own course play underneath.
//
// How a course starts without SMM2's Endless screens: they are still there, hidden under the flow's screens, and this
// file drives them. SMM2's Endless machinery is what makes a server course playable in SMM2's own play (the run
// manager, the course list and download jobs, the CRC and validity checks, the course-in card, the HUD with the
// lives and the three banked 1-Ups, the skip, the retry after a miss, the goal), and the mod already answers all of its
// requests from our server (endless.c). So:
//   - the Endless menu's page is caught each frame in its Disp state (its `this`), and the difficulty is chosen by
//     calling the game's own tab handler for it on the game thread;
//   - New Game / Play / skip / back are the menu's own buttons, pressed through the test-input hook (the player's own
//     input goes to the flow while it is active);
//   - what happens in the course comes back through the requests the game makes (course start, miss, clear, skip)
//     and goes to the flow as hundred_flow_course_started() / hundred_course_result().
//
// RECOMP_MOD_AUTOPLAY=1 (=jump) also plays the courses (right + run [+ jump], skip after RECOMP_MOD_AUTOPLAY_SKIP s)
// for unattended test runs; RECOMP_MOD_FLOW_TEST=<difficulty> makes the stand-in flow below start a run and play it
// to its end (the evidence runs). Both need LARECOMP_INPUT set (the test-input hook), which also scripts the way to
// the Endless menu.
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

#include "hundred.h"
#include "mod.h"
#include "../../../runtime/net/json.h"

extern uint64_t g_test_buttons, g_test_buttons_until;  // runtime/game_hooks.c (weak), read by runtime/hle/input.c
extern uint64_t renderer_present_count(void);
void endless_fill_status(struct hundred_status *out, int active_d);  // endless.c
void endless_forget_run(int d);

enum { BTN_A = 1 << 0, BTN_B = 1 << 1, BTN_Y = 1 << 3, BTN_PLUS = 1 << 10, BTN_MINUS = 1 << 11, BTN_RIGHT = 1 << 14,
       BTN_DOWN = 1 << 15 };

// ---- driver: one request at a time, pressed on its own thread
enum { DRV_IDLE, DRV_NEW_GAME, DRV_PLAY, DRV_SKIP, DRV_AFTER_CLEAR, DRV_BACK, DRV_SAVE_QUIT, DRV_COURSE };
static volatile int drv = DRV_IDLE;
static volatile uint64_t drv_since, course_since;
static volatile int run_d = -1;          // the flow's run (difficulty), -1 none
static volatile int pending_tab = -1;    // a difficulty to select on the game thread
static volatile uint64_t menu_page, menu_seen;  // the Endless menu page and the frame it was last in Disp
static volatile bool play_queued;      // Play asked for while the result screen is up
static pthread_once_t once = PTHREAD_ONCE_INIT;

static void set_drv(int d) { drv = d, drv_since = renderer_present_count(); }

static void *driver(void *arg) {
    (void)arg;
    const char *ap = getenv("RECOMP_MOD_AUTOPLAY");
    bool jump = ap && !strcmp(ap, "jump");
    uint64_t skip_after =
        (getenv("RECOMP_MOD_AUTOPLAY_SKIP") ? strtoull(getenv("RECOMP_MOD_AUTOPLAY_SKIP"), NULL, 0) : 20) * 60;
    for (;;) {
        usleep(20000);
        uint64_t now = renderer_present_count(), t = now - drv_since, b = 0;
        switch (drv) {
        case DRV_NEW_GAME:  // (the tab is set first, on the game thread) New Game, then the confirm window's Yes
            if (pending_tab >= 0) { drv_since = now; break; }
            if (t >= 90 && t < 98) b = BTN_A;
            else if (t >= 420 && t < 428) b = BTN_A;
            else if (t > 440) set_drv(DRV_PLAY);
            break;
        case DRV_PLAY:  // the map's Play, every 4 s until the course starts (10 s first: the map's walk / telop)
            if (t > 600 && t % 240 < 8) b = BTN_A;
            break;
        case DRV_COURSE:
            if (!ap) break;
            if (now - course_since > skip_after) {
                if ((now - course_since - skip_after) % 240 < 200) b = BTN_MINUS;  // (pulsed: see the retry card)
            } else if (t > 120) {
                b = BTN_RIGHT | BTN_Y;
                if (jump && t % 90 < 20) b |= BTN_A;
            }
            break;
        case DRV_SKIP:  // SMM1's skip, free: hold - (pulsed: a hold begun during a transition doesn't count)
            if (t % 240 < 200) b = BTN_MINUS;
            break;
        case DRV_AFTER_CLEAR:  // the result screen (Boo / I like it / Quit / Next): Next, then back on the map
            if (t >= 480 && t < 488) b = BTN_RIGHT;
            else if (t >= 540 && t < 548) b = BTN_DOWN;
            else if (t >= 600 && t < 608) b = BTN_A;
            else if (t > 700) { set_drv(play_queued ? DRV_PLAY : DRV_IDLE); play_queued = false; }
            break;
        case DRV_BACK:  // B on the Endless menu: back to Course World
            if (t < 8) b = BTN_B;
            else set_drv(DRV_IDLE);
            break;
        case DRV_SAVE_QUIT:  // the map's Quit (+), then the window's Yes
            if (t < 8) b = BTN_PLUS;
            else if (t >= 60 && t < 68) b = BTN_A;
            else if (t > 80) set_drv(DRV_IDLE);
            break;
        default: break;
        }
        g_test_buttons = b;
        g_test_buttons_until = b ? now + 4 : 0;
    }
    return NULL;
}

static int test_difficulty(void);
static void test_result(int outcome);
static void result(int outcome) {
    if (test_difficulty() >= 0) test_result(outcome);
    else hundred_course_result(outcome);
}

static void driver_start(void) {
    pthread_t th;
    pthread_create(&th, NULL, driver, NULL);
    pthread_detach(th);
}

// ---- the Endless menu page, caught in its Disp state (v3.0.3: page state machine registered at main+0x1a23f50)
#define MENU_DISP_EXEC MAIN_ADDR(0x1a24bf0)
#define MENU_TAB0 MAIN_ADDR(0x1a22260)  // the four tab handlers, 0xc0 apart (Easy .. Super Expert)

static void menu_disp(Cpu *c) {
    if (!mod_hundred_rules()) { guest_call_orig(c, MENU_DISP_EXEC); return; }
    uint64_t page = c->x[0], now = renderer_present_count();
    bool entered = now - menu_seen > 30 && run_d < 0;
    menu_page = page, menu_seen = now;
    if (pending_tab >= 0) {  // choose the difficulty the way the tab buttons do
        int d = pending_tab;
        uint64_t save[8];
        memcpy(save, c->x, sizeof save);
        c->x[0] = g64(page + 0x48);  // (the tab buttons' handlers get the page's owner [obs: their this = page+0x48])
        if (c->x[0]) guest_call(c, MENU_TAB0 + 0xc0ull * (uint64_t)d);
        memcpy(c->x, save, sizeof save);
        pending_tab = -1;
        MLOG("bridge: difficulty %d selected\n", d);
    }
    guest_call_orig(c, MENU_DISP_EXEC);
    if (entered && test_difficulty() >= 0) {
        LOG("[mod] bridge: Endless menu entered: flow test run\n");
        hundred_run_start(test_difficulty(), false);
    } else if (entered && !hundred_flow_active()) {
        LOG("[mod] bridge: Endless menu entered: the 100 Mario flow opens\n");
        hundred_flow_open();
    }
}

// ---- events from endless.c (the game's own requests)
void bridge_event(const char *what, uint32_t d) {
    pthread_once(&once, driver_start);
    if (!strcmp(what, "start")) {
        drv = DRV_COURSE, drv_since = course_since = renderer_present_count();
        if (test_difficulty() < 0) hundred_flow_course_started();
    } else if (!strcmp(what, "miss")) {
        drv = DRV_COURSE, drv_since = renderer_present_count();  // (SMM2 retries the course by itself)
        result(HUNDRED_MISS);
    } else if (!strcmp(what, "clear")) {
        set_drv(DRV_AFTER_CLEAR);
        result(HUNDRED_CLEAR);
    } else if (!strcmp(what, "skip")) {
        set_drv(DRV_IDLE);  // (SMM2's Endless starts the next course at once)
        result(HUNDRED_SKIP);
    } else if (!strcmp(what, "new")) {
        run_d = (int)d;
    }
}

// ---- the run side's API (hundred.h)
bool hundred_run_status(struct hundred_status *out) {
    endless_fill_status(out, run_d);
    return true;
}

bool hundred_run_start(int difficulty, bool resume_saved) {
    if (difficulty < 0 || difficulty > 3 || !mod_hundred_rules()) return false;
    struct hundred_status st;
    endless_fill_status(&st, -1);
    if (!st.unlocked[difficulty]) return false;
    pthread_once(&once, driver_start);
    run_d = difficulty;
    pending_tab = difficulty;
    if (resume_saved) LOG("[mod] bridge: Continue is not driven yet; New Game instead\n");
    set_drv(DRV_NEW_GAME);
    LOG("[mod] bridge: run start, difficulty %d\n", difficulty);
    return true;
}

bool hundred_play_next(void) {
    if (run_d < 0) return false;
    if (drv == DRV_AFTER_CLEAR) play_queued = true;
    else if (drv == DRV_IDLE) set_drv(DRV_PLAY);
    return true;
}

void hundred_play_resume(void) {}  // (SMM2's pause is not intercepted yet: nothing to resume)
void hundred_play_retry(void) {}   // (SMM2's Endless retries the course after a miss by itself)
void hundred_play_skip(void) { set_drv(DRV_SKIP); }

static void *post_async(void *path) {
    Json *j = srv_call("POST", (const char *)path, "{}", NULL);
    json_free(j);
    free(path);
    return NULL;
}

static void post_later(const char *what) {
    static const char *const diff[4] = {"easy", "normal", "expert", "super_expert"};
    if (run_d < 0) return;
    char *path = malloc(80);
    snprintf(path, 80, "/api/endless/%s/%s", diff[run_d], what);
    pthread_t th;
    pthread_create(&th, NULL, post_async, path);
    pthread_detach(th);
}

void hundred_run_save_quit(void) { set_drv(DRV_SAVE_QUIT); }  // (the game posts the suspend itself)

void hundred_run_finish(void) {
    LOG("[mod] bridge: run finished (difficulty %d)\n", run_d);
    post_later("finish");
    endless_forget_run(run_d);
    run_d = -1;
    set_drv(DRV_IDLE);
}

void hundred_flow_exit(void) { set_drv(DRV_BACK); }

// ---- RECOMP_MOD_FLOW_TEST=<difficulty>: an unattended run in place of the flow (evidence, regression checks). The
// flow's screens are left out (hundred_flow_open is not called); the run starts on menu entry, Play follows every
// clear, and the win plays the ending (hundred_ending_show) and finishes the run.
static int flow_test = -2;

static int test_difficulty(void) {
    if (flow_test == -2) flow_test = getenv("RECOMP_MOD_FLOW_TEST") ? atoi(getenv("RECOMP_MOD_FLOW_TEST")) : -1;
    return flow_test;
}

static void *test_win(void *arg) {
    (void)arg;
    struct hundred_status st;
    hundred_run_status(&st);
    hundred_ending_show(st.difficulty, st.ncredits, st.credits);
    for (int i = 0; i < 30 * 60 * 50 && !hundred_ending_done(); i++) usleep(20000);
    hundred_run_finish();
    return NULL;
}

static void test_result(int outcome) {
    static const char *const names[] = {"clear", "miss", "skip", "save & quit", "error"};
    struct hundred_status st;
    hundred_run_status(&st);
    LOG("[mod] flow test: result %s, lives %d, cleared %d / %d\n", names[outcome], st.lives, st.cleared, st.total);
    if (outcome != HUNDRED_CLEAR) return;
    if (st.total && st.cleared >= st.total) {
        LOG("[mod] flow test: 100 Mario Challenge won\n");
        pthread_t th;
        pthread_create(&th, NULL, test_win, NULL);
        pthread_detach(th);
    } else {
        hundred_play_next();
    }
}

static void tab_probe(Cpu *c) {
    uint64_t x0 = c->x[0], pc = c->pc;
    long off = -1;
    for (long o = 0; menu_page && o < 0x800; o += 8)
        if (g64(menu_page + (uint64_t)o) == x0) { off = o; break; }
    LOG("[mod] tab handler %llx: this %llx, menu page %llx, found at page+%#lx\n", (unsigned long long)pc,
        (unsigned long long)x0, (unsigned long long)menu_page, off);
    guest_call_orig(c, pc);
}

void bridge_register(void) {
    hle_override(MENU_DISP_EXEC, menu_disp, "mod: Endless menu Disp (bridge)");
    if (getenv("RECOMP_MOD_TABPROBE")) {
        hle_override(MENU_TAB0, tab_probe, "probe");
        hle_override(MENU_TAB0 + 0xc0, tab_probe, "probe");
    }
}
