// The run side of the 100 Mario bridge (hundred.h): SMM1's screens on top, SMM2's own course play underneath.
//
// How a course starts without SMM2's Endless screens: they are still there, hidden under the flow's screens, and this
// file drives them. SMM2's Endless machinery is what makes a server course playable in SMM2's own play (the run
// manager, the course list and download jobs, the CRC and validity checks, the course-in card, the HUD with the
// lives and the three banked 1-Ups, the skip, the retry after a miss, the goal), and the mod already answers all of its
// requests from our server (endless.c). So:
//   - the Endless menu's page is caught each frame in its Disp state (its `this`), and the difficulty is chosen by
//     calling the game's own tab handler for it on the game thread;
//   - New Game / Continue / Play / skip / back are the menu's own buttons, pressed through the test-input hook (the
//     player's own input goes to the flow while it is active);
//   - SMM2's pause during a run course stays closed: the play is held in the pause state while the flow shows SMM1's
//     pause, and its choice (resume, Start Over, Save & Quit) leaves that state the way the pause menu's items do;
//   - what happens in the course comes back through the requests the game makes (course start, miss, clear, skip,
//     suspend) and goes to the flow as hundred_flow_course_started() / hundred_course_result().
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
void endless_prepare_run(int d, bool resume);

enum { BTN_A = 1 << 0, BTN_B = 1 << 1, BTN_Y = 1 << 3, BTN_L = 1 << 6, BTN_R = 1 << 7, BTN_PLUS = 1 << 10, BTN_MINUS = 1 << 11, BTN_LEFT = 1 << 12,
       BTN_UP = 1 << 13, BTN_RIGHT = 1 << 14, BTN_DOWN = 1 << 15 };

// ---- driver: one request at a time, pressed on its own thread
enum { DRV_IDLE, DRV_NEW_GAME, DRV_CONTINUE, DRV_PLAY, DRV_SKIP, DRV_AFTER_CLEAR, DRV_RESULT_QUIT, DRV_BACK,
       DRV_SAVE_QUIT, DRV_COURSE, DRV_PAUSE_TEST, DRV_BROKEN };
static volatile int drv = DRV_IDLE;
static volatile uint64_t drv_since, course_since;
static volatile int run_d = -1;          // the flow's run (difficulty), -1 none
static volatile int pending_tab = -1;    // a difficulty to select on the game thread (the tab's handler)
static volatile int tab_target = -1;     // a difficulty to select with L / R
static volatile int tab_presses;
static volatile uint64_t menu_page, menu_seen;  // the Endless menu page and the frame it was last in Disp
static volatile bool play_queued;      // Play asked for while the result screen is up
static volatile bool suspending;       // Save & Quit under way: the menu's return reports it
static volatile bool suspend_posted;   // the game posted its suspend (the confirm window was answered)
static volatile bool leaving_won;      // a won run's last result screen: Quit back to the menu, nothing to report
static volatile bool over_save;        // New Game over a saved run (the menu shows Continue too)
static pthread_once_t once = PTHREAD_ONCE_INIT;

static void set_drv(int d) { drv = d, drv_since = renderer_present_count(); }
static int test_difficulty(void);
static void mod_shot(const char *what, uint64_t after_frames);
static volatile bool started_pending;    // a course start / retry not yet seen playing
static volatile uint64_t started_since;
static void course_started(const char *why);

static void *driver(void *arg) {
    (void)arg;
    const char *ap = getenv("RECOMP_MOD_AUTOPLAY");
    bool jump = ap && !strcmp(ap, "jump");
    uint64_t skip_after =
        (getenv("RECOMP_MOD_AUTOPLAY_SKIP") ? strtoull(getenv("RECOMP_MOD_AUTOPLAY_SKIP"), NULL, 0) : 20) * 60;
    for (;;) {
        usleep(20000);
        uint64_t now = renderer_present_count(), t = now - drv_since, b = 0;
        if ((drv == DRV_NEW_GAME || drv == DRV_CONTINUE) && tab_target >= 0 && now - menu_seen > 10) {
            // (not on SMM2's Endless menu yet, e.g. still leaving the last run: wait for it)
            static uint64_t logged;
            if (now - logged > 600) logged = now, LOG("[mod] bridge: waiting for SMM2's Endless menu\n");
            drv_since = now;
            g_test_buttons = 0, g_test_buttons_until = 0;
            continue;
        }
        if ((drv == DRV_NEW_GAME || drv == DRV_CONTINUE) && tab_target >= 0) {
            // the difficulty, the way a player picks it: L / R until the menu's difficulty (main+0x2c3f618) is the
            // one wanted (calling the tab's handler directly can leave the buttons without a focus when Continue
            // goes away); after 12 presses, the handler (menu_disp)
            int cur = (int)g32(MAIN_ADDR(0x2c3f618));
            if (cur == tab_target) {
                MLOG("bridge: difficulty %d on the menu\n", tab_target);
                tab_target = -1, tab_presses = 0, drv_since = now;
                mod_shot("tab", 60), mod_shot("tab", 95);
            } else if (tab_presses >= 12) {
                pending_tab = tab_target, tab_target = -1, tab_presses = 0;
            } else if (t >= 30) {
                b = cur < tab_target ? BTN_R : BTN_L;
                if (t >= 38) tab_presses++, drv_since = now;
            }
            g_test_buttons = b;
            g_test_buttons_until = b ? now + 4 : 0;
            continue;
        }
        switch (drv) {
        case DRV_NEW_GAME:  // (the tab is set first, on the game thread) New Game, then the confirm window's Yes.
                            // With a saved run the focus starts on Continue, right of New Game [obs]
            if (pending_tab >= 0) { drv_since = now; break; }
            // "Previous save data will be overwritten. Continue?" opens on No, Yes is right of it [obs, flow agent]:
            // Right then A, repeated in case the window comes late (on the map Right does nothing and A is Play)
            // (after L / R the focus is on the tab bar [obs]: Down brings it to the buttons; twice, in case the
            // first falls into the tab change)
            if ((t >= 40 && t < 48) || (t >= 70 && t < 78)) b = BTN_DOWN;
            else if (over_save && t >= 100 && t < 108) b = BTN_LEFT;
            else if (t >= 130 && t < 138) b = BTN_A;
            else if (over_save && t >= 300 && t < 780) {
                if ((t - 300) % 120 < 8) b = BTN_RIGHT;
                else if ((t - 300) % 120 >= 40 && (t - 300) % 120 < 48) b = BTN_A;
            } else if (!over_save && t >= 420 && t < 428) b = BTN_A;
            else if (t > (over_save ? 780u : 440u)) set_drv(DRV_PLAY);
            break;
        case DRV_CONTINUE:  // (the tab is set first) Continue: the focused button when a run is saved [obs]
            if (pending_tab >= 0) { drv_since = now; break; }
            if ((t >= 40 && t < 48) || (t >= 70 && t < 78)) b = BTN_DOWN;
            else if (t >= 130 && t < 138) b = BTN_A;
            else if (t > 160) set_drv(DRV_PLAY);
            break;
        case DRV_PLAY:  // the map's Play, every 2 s until the course starts (presses during the map's walk / telop
                        // are ignored by the map)
            if (t > 180 && t % 120 < 8) b = BTN_A;
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
        case DRV_BROKEN:  // the "corrupt" window: A every 2 s for 10 s, then the map's Play
            if (t >= 120 && t % 120 < 8) b = BTN_A;
            else if (t > 600) set_drv(DRV_PLAY);
            break;
        case DRV_PAUSE_TEST:  // (flow test) + in the course: SMM2's pause, which the mod keeps closed
            if (t < 8) b = BTN_PLUS;
            else if (t > 30) set_drv(DRV_COURSE);
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
        case DRV_RESULT_QUIT:  // the result screen's Quit (below Boo), then the confirm window's Yes (right of No
                               // [inf: as the overwrite window]); Right + A again every 2 s until the suspend is posted
            if (t >= 480 && t < 488) b = BTN_DOWN;
            else if (t >= 540 && t < 548) b = BTN_A;
            else if (t >= 700 && t < 1300 && !suspend_posted) {
                if ((t - 700) % 120 < 8) b = BTN_RIGHT;
                else if ((t - 700) % 120 >= 40 && (t - 700) % 120 < 48) b = BTN_A;
            } else if (t >= 1300 || suspend_posted) set_drv(DRV_IDLE);
            break;
        case DRV_BACK:  // B on the Endless menu: back to Course World
            if (t < 8) b = BTN_B;
            else set_drv(DRV_IDLE);
            break;
        case DRV_SAVE_QUIT:  // the map's Quit (+), then the window's Yes (Right + A until the suspend is posted)
            if (t < 8) b = BTN_PLUS;
            else if (t >= 60 && t < 660 && !suspend_posted) {
                if ((t - 60) % 120 < 8) b = BTN_RIGHT;
                else if ((t - 60) % 120 >= 40 && (t - 60) % 120 < 48) b = BTN_A;
            } else if (t >= 660 || suspend_posted) set_drv(DRV_IDLE);
            break;
        default: break;
        }
        if (started_pending && now - started_since > 600) course_started("fallback");
        g_test_buttons = b;
        g_test_buttons_until = b ? now + 4 : 0;
    }
    return NULL;
}

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
    // (SMM2's own Endless Challenge: 100 Mario not offered, or the player chose SMM2's from the 100 Mario select)
    if ((!mod_hundred_offered() || mod_smm2_session()) && test_difficulty() < 0) {
        menu_seen = renderer_present_count();
        guest_call_orig(c, MENU_DISP_EXEC);
        return;
    }
    uint64_t page = c->x[0], now = renderer_present_count();
    bool back = now - menu_seen > 30;  // (first Disp frame after a while: the menu is (back) on screen)
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
    if (back && leaving_won) {  // a won run: SMM2 is back on its menu, ready for the next one
        leaving_won = suspending = false;
        set_drv(DRV_IDLE);
        LOG("[mod] bridge: back on SMM2's Endless menu after the win\n");
        return;
    }
    if (back && suspending) {  // Save & Quit done: the game is back on its Endless menu
        suspending = false;
        int d = run_d;
        run_d = -1;
        set_drv(DRV_IDLE);
        LOG("[mod] bridge: Save & Quit done (difficulty %d)\n", d);
        result(HUNDRED_GIVEUP_SAVE);
        return;
    }
    if (!back || run_d >= 0) return;
    if (test_difficulty() >= 0) {
        LOG("[mod] bridge: Endless menu entered: flow test run\n");
        const char *cont = getenv("RECOMP_MOD_FLOW_TEST_CONTINUE");
        hundred_run_start(test_difficulty(), cont && atoi(cont));
    } else if (!hundred_flow_active()) {
        LOG("[mod] bridge: Endless menu entered: the 100 Mario flow opens\n");
        hundred_flow_open();
    }
}

// ---- SMM2's pause during a run course -> SMM1's pause (the flow's)
// The Endless play scene's pause state (cPausePlay, index 3 of its machine at scene+0x238) runs this exec each frame
// (the scene's virtual at vtable+0x390: v3.0.3 main+0x1bba050, the Endless play scene's only). With a 100 Mario run
// the pause menu is not opened: the play stays held in that state while the flow shows SMM1's pause, and its choice
// leaves the state the way the menu's own items do: resume -> state 1 (as when the menu closes), Start Over -> 0x13
// cRestartEndlessMode, Save & Quit -> 0x12 cSuspendEndlessMode (as after the confirm window's Yes).
// [obs: 0x7101bba050, 0x7101bbad00, 0x7101bbb350]
#define PAUSE_EXEC MAIN_ADDR(0x1bba050)
#define CHANGE_STATE MAIN_ADDR(0x8b9320)  // (machine, state index)
#define PAUSE_FLAGS MAIN_ADDR(0x2a37ed8)  // ptr: +0x330 bit 0 cleared when the pause closes
#define RUN_OBJECT MAIN_ADDR(0x2c56bb0)   // ptr: the Endless run (+0x49: u16 cleared before a suspend)
enum { PAUSE_NONE, PAUSE_RESUME, PAUSE_RETRY, PAUSE_SAVE_QUIT };
static volatile int pause_cmd;
static volatile bool paused, restart_from_pause;
static void test_pause(void);

static void change_state(Cpu *c, uint64_t machine, uint32_t st) {
    uint64_t save[8];
    memcpy(save, c->x, sizeof save);
    c->x[0] = machine, c->x[1] = st;
    guest_call(c, CHANGE_STATE);
    memcpy(c->x, save, sizeof save);
}

static void vcall(Cpu *c, uint64_t obj, uint32_t slot) {
    uint64_t save[8];
    memcpy(save, c->x, sizeof save);
    c->x[0] = obj;
    guest_call(c, g64(g64(obj) + slot));
    memcpy(c->x, save, sizeof save);
}

static void pause_exec(Cpu *c) {
    uint64_t s = c->x[0];
    if (!mod_hundred_rules() || run_d < 0) { guest_call_orig(c, PAUSE_EXEC); return; }
    uint64_t p = g64(s + 0x2e0);
    uint32_t ps = p ? g32(p + 0xa8) - 3 : 99;
    bool ps_on = ps < 4 && ((0xbu >> ps) & 1);
    if (ps_on && g32(p + 0x248) == 5) { guest_call_orig(c, PAUSE_EXEC); return; }  // (the game's own skip path)
    if (g32(s + 0x244) == 0) {  // just paused
        s32(s + 0x280, 1);
        if (ps_on) vcall(c, p, 0xb0);  // (what the pause does first, as the game's own)
        pause_cmd = PAUSE_NONE, paused = true;
        LOG("[mod] bridge: paused (SMM1's pause)\n");
        mod_shot("paused", 60);
        if (g_mod.log) {  // the machine's first states (enter / exec / exit member pointers), for the record
            uint64_t tab = g64(s + 0x268);
            for (int i = 0; i < 3 && tab; i++)
                LOG("[mod]   play state %d: enter %llx/%llx exec %llx/%llx exit %llx/%llx\n", i,
                    (unsigned long long)g64(tab + 0x40 * i + 0x10), (unsigned long long)g64(tab + 0x40 * i + 0x18),
                    (unsigned long long)g64(tab + 0x40 * i + 0x20), (unsigned long long)g64(tab + 0x40 * i + 0x28),
                    (unsigned long long)g64(tab + 0x40 * i + 0x30), (unsigned long long)g64(tab + 0x40 * i + 0x38));
        }
        if (test_difficulty() >= 0) test_pause();
        else hundred_flow_pause();
        return;
    }
    int cmd = pause_cmd;
    if (cmd == PAUSE_NONE) return;
    pause_cmd = PAUSE_NONE, paused = false;
    uint64_t flags = g64(PAUSE_FLAGS);
    switch (cmd) {
    case PAUSE_RESUME:
        if (flags) s32(flags + 0x330, g32(flags + 0x330) & ~1u);
        s32(s + 0x280, 4);
        change_state(c, s + 0x238, 1);
        LOG("[mod] bridge: pause closed, the course goes on\n");
        break;
    case PAUSE_RETRY:
        restart_from_pause = true;
        s32(s + 0x280, 4);
        change_state(c, s + 0x238, 0x13);
        LOG("[mod] bridge: Start Over\n");
        break;
    case PAUSE_SAVE_QUIT:
        suspending = true;
        s8(s + 0x2c0, 1);
        s32(s + 0x280, 4);
        change_state(c, s + 0x238, 0x12);
        if (g64(RUN_OBJECT)) s16(g64(RUN_OBJECT) + 0x49, 0);
        LOG("[mod] bridge: Save & Quit from the pause\n");
        break;
    }
}

// The play state (index 1 of the same machine; exec = the Endless scene's virtual at vtable+0x350, main+0x1bb9440)
// runs while the course plays: its first frame after a course start (or a retry) is when the course is on screen
// and playing, and the flow hears hundred_flow_course_started() then. (Fallback: 10 s after the start request.)
#define PLAY_EXEC MAIN_ADDR(0x1bb9440)

static void course_started(const char *why) {
    if (!started_pending) return;
    started_pending = false;
    MLOG("bridge: course on screen (%s)\n", why);
    mod_shot("playing", 120);
    if (test_difficulty() < 0) hundred_flow_course_started();
}

static void play_exec(Cpu *c) {
    if (started_pending && g32(c->x[0] + 0x244) == 0) course_started("play state");
    guest_call_orig(c, PLAY_EXEC);
}

bool bridge_take_start_over(void) {  // endless.c: the life being posted is a Start Over
    bool v = restart_from_pause;
    restart_from_pause = false;
    return v;
}

// ---- RECOMP_MOD_SHOTS=<name>: window shots at the run's moments (tests and evidence), build/<name>_<n>_<what>.png
extern char g_window_shot_request[256];  // runtime/game_hooks.c (weak), saved by the renderer
struct shot { uint64_t at; char name[48]; };
static void *shot_later(void *arg) {
    struct shot *s = arg;
    while (renderer_present_count() < s->at) usleep(5000);
    static int n;
    snprintf(g_window_shot_request, sizeof g_window_shot_request, "build/%s_%03d_%s.png", getenv("RECOMP_MOD_SHOTS"),
             ++n, s->name);
    free(s);
    return NULL;
}
static void mod_shot(const char *what, uint64_t after_frames) {
    if (!getenv("RECOMP_MOD_SHOTS")) return;
    struct shot *s = malloc(sizeof *s);
    s->at = renderer_present_count() + after_frames;
    snprintf(s->name, sizeof s->name, "%s", what);
    pthread_t th;
    pthread_create(&th, NULL, shot_later, s);
    pthread_detach(th);
}

// ---- events from endless.c (the game's own requests)
void bridge_event(const char *what, uint32_t d) {
    pthread_once(&once, driver_start);
    if (!strcmp(what, "start") || !strcmp(what, "retry")) mod_shot(what, 300), mod_shot(what, 900);
    else if (!strcmp(what, "miss")) mod_shot(what, 20);
    else if (!strcmp(what, "clear")) mod_shot(what, 30), mod_shot("result", 420);
    if (!strcmp(what, "start")) {
        drv = DRV_COURSE, drv_since = course_since = renderer_present_count();
        started_since = renderer_present_count(), started_pending = true;
    } else if (!strcmp(what, "miss")) {
        drv = DRV_COURSE, drv_since = renderer_present_count();  // (SMM2 retries the course by itself)
        result(HUNDRED_MISS);
    } else if (!strcmp(what, "retry")) {  // (hooks.c: the course is set up again after a miss / Start Over)
        drv = DRV_COURSE, drv_since = renderer_present_count();  // (the autoplay's skip time counts on)
        LOG("[mod] bridge: the course again (retry)\n");
        started_since = renderer_present_count(), started_pending = true;
    } else if (!strcmp(what, "clear")) {
        set_drv(DRV_AFTER_CLEAR);
        result(HUNDRED_CLEAR);
    } else if (!strcmp(what, "clear_last")) {  // the run is won: Quit on the result screen, back to SMM2's menu
        leaving_won = true, suspend_posted = false;
        set_drv(DRV_RESULT_QUIT);
        result(HUNDRED_CLEAR);
    } else if (!strcmp(what, "skip")) {
        set_drv(DRV_IDLE);  // (SMM2's Endless starts the next course at once)
        result(HUNDRED_SKIP);
    } else if (!strcmp(what, "new")) {
        run_d = (int)d;
        mod_shot("map", 200);
    } else if (!strcmp(what, "broken")) {  // "The course data is corrupt.": OK, then the run goes on (Play)
        LOG("[mod] bridge: a broken course: the window is answered, the next course comes\n");
        set_drv(DRV_BROKEN);
        result(HUNDRED_SKIP);
    } else if (!strcmp(what, "suspend")) {
        suspend_posted = true;
        if (!leaving_won) suspending = true;
    }
}

// ---- the run side's API (hundred.h)
bool hundred_run_status(struct hundred_status *out) {
    endless_fill_status(out, run_d);
    return true;
}

bool hundred_run_start(int difficulty, bool resume_saved) {
    if (difficulty < 0 || difficulty > 3 || (!mod_hundred_rules() && test_difficulty() < 0)) return false;
    struct hundred_status st;
    endless_fill_status(&st, -1);
    if (!st.unlocked[difficulty]) return false;
    if (resume_saved && !st.can_continue[difficulty]) resume_saved = false;
    over_save = !resume_saved && st.can_continue[difficulty];
    pthread_once(&once, driver_start);
    endless_prepare_run(difficulty, resume_saved);
    run_d = difficulty;
    tab_target = difficulty, tab_presses = 0;
    suspending = paused = false;
    set_drv(resume_saved ? DRV_CONTINUE : DRV_NEW_GAME);
    mod_shot("menu", 60), mod_shot("menu", 330);
    LOG("[mod] bridge: run %s, difficulty %d\n", resume_saved ? "continued" : "start", difficulty);
    return true;
}

bool hundred_play_next(void) {
    if (run_d < 0) return false;
    if (drv == DRV_AFTER_CLEAR) play_queued = true;
    else if (drv == DRV_IDLE) set_drv(DRV_PLAY);
    return true;
}

void hundred_play_resume(void) {
    if (paused) pause_cmd = PAUSE_RESUME;
}
void hundred_play_retry(void) {  // Start Over from the pause (after a miss SMM2's Endless retries by itself)
    if (paused) pause_cmd = PAUSE_RETRY;
}
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

// Save & Quit: from the pause, the pause's own suspend; after a clear, the result screen's Quit; else the map's
// Quit. The game posts the suspend itself; back on its menu the flow gets HUNDRED_GIVEUP_SAVE.
void hundred_run_save_quit(void) {
    if (run_d < 0) return;
    if (paused) { pause_cmd = PAUSE_SAVE_QUIT; return; }
    suspending = true, suspend_posted = false;
    set_drv(drv == DRV_AFTER_CLEAR ? DRV_RESULT_QUIT : DRV_SAVE_QUIT);
}

void hundred_run_finish(void) {
    LOG("[mod] bridge: run finished (difficulty %d)\n", run_d);
    post_later("finish");
    endless_forget_run(run_d);
    run_d = -1;
    if (drv != DRV_RESULT_QUIT) set_drv(DRV_IDLE);  // (after a win the way back to the menu goes on)
}

// B on SMM1's difficulty select: SMM2's own Endless Challenge, whose menu is underneath (both challenges in one
// build). Leaving that menu (B there) ends it: the next visit opens 100 Mario again.
void hundred_flow_exit(void) {
    set_drv(DRV_IDLE);
    mod_set_smm2_session(true);
}

// The Endless menu's Disappear state (index 5 of its machine; exec main+0x1a24d30): the menu closes towards Course
// World (a course start goes through CourseIn / InFade instead).
#define MENU_DISAPPEAR_EXEC MAIN_ADDR(0x1a24d30)
static void menu_disappear(Cpu *c) {
    if (mod_smm2_session() && g32(c->x[0] + 0x244) == 0) mod_set_smm2_session(false);
    guest_call_orig(c, MENU_DISAPPEAR_EXEC);
}

// ---- RECOMP_MOD_FLOW_TEST=<difficulty>: an unattended run in place of the flow (evidence, regression checks). The
// flow's screens are left out (hundred_flow_open is not called); the run starts on menu entry (Continue with
// RECOMP_MOD_FLOW_TEST_CONTINUE=1), Play follows every clear, and the win plays the ending (hundred_ending_show) and
// finishes the run. RECOMP_MOD_FLOW_TEST_PAUSE=<script> pauses in the courses and answers the pause with the script's
// letters in turn (r resume, o Start Over, q Save & Quit), each 8 s into a course.
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
    static int quits;  // RECOMP_MOD_FLOW_TEST_QUIT_AFTER_CLEAR=<n>: Save & Quit on the first n result screens
    const char *qa = getenv("RECOMP_MOD_FLOW_TEST_QUIT_AFTER_CLEAR");
    if (qa && quits < atoi(qa) && !(st.total && st.cleared >= st.total)) {
        quits++;
        LOG("[mod] flow test: Save & Quit on the result screen\n");
        hundred_run_save_quit();
        return;
    }
    if (st.total && st.cleared >= st.total) {
        LOG("[mod] flow test: 100 Mario Challenge won\n");
        pthread_t th;
        pthread_create(&th, NULL, test_win, NULL);
        pthread_detach(th);
    } else {
        hundred_play_next();
    }
}

static int test_pause_step;
static void *test_pause_answer(void *arg) {
    int what = (int)(intptr_t)arg;
    usleep(3000000);
    struct hundred_status st;
    hundred_run_status(&st);
    LOG("[mod] flow test: pause answered '%c' (lives %d, course %s)\n", what, st.lives, st.current_code);
    if (what == 'o') hundred_play_retry();
    else if (what == 'q') hundred_run_save_quit();
    else hundred_play_resume();
    return NULL;
}
static void test_pause(void) {
    const char *script = getenv("RECOMP_MOD_FLOW_TEST_PAUSE");
    int what = script && script[test_pause_step] ? script[test_pause_step++] : 'r';
    pthread_t th;
    pthread_create(&th, NULL, test_pause_answer, (void *)(intptr_t)what);
    pthread_detach(th);
}

static void *test_pauser(void *arg) {  // presses + 8 s into each course while the script has letters
    (void)arg;
    const char *script = getenv("RECOMP_MOD_FLOW_TEST_PAUSE");
    uint64_t last = 0;
    for (;;) {
        usleep(100000);
        if (!script || !script[test_pause_step]) return NULL;
        uint64_t now = renderer_present_count();
        if (drv == DRV_COURSE && course_since != last && now - course_since > 480) {
            last = course_since;
            set_drv(DRV_PAUSE_TEST);
        }
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
    hle_override(PAUSE_EXEC, pause_exec, "mod: Endless pause (bridge)");
    hle_override(PLAY_EXEC, play_exec, "mod: Endless play state (bridge)");
    hle_override(MENU_DISAPPEAR_EXEC, menu_disappear, "mod: Endless menu Disappear (bridge)");
    if (getenv("RECOMP_MOD_TABPROBE")) {
        hle_override(MENU_TAB0, tab_probe, "probe");
        hle_override(MENU_TAB0 + 0xc0, tab_probe, "probe");
    }
    if (test_difficulty() >= 0 && getenv("RECOMP_MOD_FLOW_TEST_PAUSE")) {
        pthread_t th;
        pthread_create(&th, NULL, test_pauser, NULL);
        pthread_detach(th);
    }
}
