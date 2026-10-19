// The 100 Mario Challenge as in SMM1: the host-drawn flow between courses (../hundred.h).
//
// SMM1's screens are drawn by the layout player into one 1280 x 720 RGBA image per frame (lyt_raster, on a worker
// thread so the present never waits for it); the renderers put it over the game's frame (runtime/ui/menu.mm,
// game_overlay_image): full screen (the game's frame hidden) for the difficulty select, the map and card, game over
// and the ending; over the course for pause, clear, skip and the card after a miss. While a screen takes input the
// game gets none (runtime/hle/input.c, game_overlay_captured).
//
// The flow ticks once per presented frame (60 Hz). The mod's calls (game threads) are queued and handled at the
// next tick.
//
// Tests: RECOMP_HUNDRED_FLOW_TEST=<screen>[@<frame>] opens a screen with a made-up run when the mod isn't there
// (select, newgame:<d>, resume:<d>:<cleared>, afterclear:<d>:<cleared>, card, pause, clear[:last], miss, skip,
// gameover, ending:<d>[:<courses>]); tools/test_smm1ui --flow renders the same without the game.
#include <math.h>
#include <pthread.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

#include "ending.h"
#include "smm1_data.h"

#define WEAK __attribute__((weak))
#define LOGF(...) fprintf(stderr, "[hundred] " __VA_ARGS__)
#define W 1280
#define H 720

// ================================================================ the mod's side, when it isn't linked: a made-up run
static struct {
    bool used;      // a stub below ran (so the fake course timeline is ticked here)
    int d, lives, cleared, playing, play_tick, outcome;
    bool saved[4];
    int saved_lives[4], saved_cleared[4];
} fake = {.lives = 100};
static void fake_course(int i, struct hundred_credit *c) {
    static const char *names[8] = {"Up the Mountain", "Pipe Maze", "Koopa Troopa Beach", "Ghost House Panic",
                                   "Airship Assault", "Underground Run", "Castle of Lava", "Sky High"};
    static const char *makers[8] = {"Luigi", "Toadette", "Peach", "Daisy", "Wario", "Yoshi", "Rosalina", "Kamek"};
    snprintf(c->name, sizeof c->name, "%s %d", names[i % 8], i + 1);
    snprintf(c->maker, sizeof c->maker, "%s", makers[i % 8]);
}
WEAK bool hundred_run_status(struct hundred_status *o) {
    fake.used = true;
    memset(o, 0, sizeof *o);
    o->difficulty = fake.playing >= 0 ? fake.d : -1;
    o->lives = fake.lives, o->cleared = fake.cleared, o->total = RUN_LENGTHS[fake.d];
    for (int i = 0; i < 4; i++) {
        o->unlocked[i] = true;
        o->can_continue[i] = fake.saved[i];
        o->continue_lives[i] = fake.saved_lives[i], o->continue_cleared[i] = fake.saved_cleared[i];
    }
    fake_course(fake.cleared, &o->current);
    snprintf(o->current_code, sizeof o->current_code, "%s", "B0X-7GH-1JK");
    o->ncredits = fake.cleared;
    for (int i = 0; i < fake.cleared && i < 16; i++) fake_course(i, &o->credits[i]);
    return true;
}
WEAK bool hundred_run_start(int d, bool resume) {
    fake.used = true;
    fake.d = d;
    if (resume && fake.saved[d]) fake.lives = fake.saved_lives[d], fake.cleared = fake.saved_cleared[d];
    else fake.lives = 100, fake.cleared = 0;
    return true;
}
WEAK bool hundred_play_next(void) { fake.used = true; fake.playing = 1, fake.play_tick = 0; return true; }
WEAK void hundred_play_resume(void) { fake.used = true; }
WEAK void hundred_play_retry(void) { fake.used = true; fake.playing = 1, fake.play_tick = 0; }
WEAK void hundred_play_skip(void) { fake.used = true; fake.playing = 3, fake.play_tick = 0; }
WEAK void hundred_run_save_quit(void) {
    fake.used = true;
    fake.saved[fake.d] = true, fake.saved_lives[fake.d] = fake.lives, fake.saved_cleared[fake.d] = fake.cleared;
    fake.playing = 4, fake.play_tick = 0;
}
WEAK void hundred_run_finish(void) { fake.used = true; fake.saved[fake.d] = false, fake.playing = 0; }
WEAK void hundred_flow_exit(void) { fake.used = true; }

// ================================================================ queue (mod -> flow)
enum { Q_OPEN = 1, Q_PAUSE, Q_STARTED, Q_RESULT, Q_ENDING };
static pthread_mutex_t g_q = PTHREAD_MUTEX_INITIALIZER;
static int g_queue[32][2], g_nq;
static struct hundred_credit g_end_credits[16];
static int g_end_n, g_end_d;
static void post(int kind, int arg) {
    pthread_mutex_lock(&g_q);
    if (g_nq < 32) g_queue[g_nq][0] = kind, g_queue[g_nq][1] = arg, g_nq++;
    pthread_mutex_unlock(&g_q);
}

// ================================================================ the flow
enum { S_CLOSED, S_SELECT, S_INTRO, S_WAIT, S_PLAYING, S_GAMEOVER, S_ENDING };
static struct {
    int state, d;
    bool ready, ending_only, ending_done, captured, full;
    Select *sel;
    Intro *intro;
    GameOver *go;
    Ending *end;
    Overlay ov;
    struct hundred_status st;
    unsigned prev;
    int skip_hold, skip_until, clear_act_at, card_retry;
    char test[64];
    long test_at;
} F;

static void status(void) { hundred_run_status(&F.st); }

static CardInfo card_info(void) {
    CardInfo c;
    memset(&c, 0, sizeof c);
    snprintf(c.name, sizeof c.name, "%s", F.st.current.name);
    snprintf(c.maker, sizeof c.maker, "%s", F.st.current.maker);
    snprintf(c.code, sizeof c.code, "%s", F.st.current_code);
    c.lives = F.st.lives, c.number = F.st.cleared + 1, c.total = F.st.total ? F.st.total : RUN_LENGTHS[F.d];
    return c;
}

static void close_scenes(void) {
    select_free(F.sel), F.sel = NULL;
    intro_free(F.intro), F.intro = NULL;
    gameover_free(F.go), F.go = NULL;
    ending_free(F.end), F.end = NULL;
}
static void to_select(void) {
    status();
    Select *s = select_new(F.d, &F.st);
    close_scenes();
    ov_hide(&F.ov);
    F.sel = s;
    F.state = s ? S_SELECT : S_CLOSED;
}
static void to_intro(int entry) {
    status();
    CardInfo c = card_info();
    Intro *in = intro_new(entry, F.d, &c, F.st.cleared, F.sel);
    close_scenes();
    ov_hide(&F.ov);
    F.intro = in;
    F.state = in ? S_INTRO : S_CLOSED;
}
static void to_ending(int d, int lives, int n, const struct hundred_credit *cr) {
    (void)lives;
    Ending *e = ending_new_style(d, n, cr, F.st.style, F.st.form);
    close_scenes();
    ov_hide(&F.ov);
    F.end = e;
    F.state = S_ENDING;
}
static void play_next(void) {
    F.state = S_WAIT;
    if (F.intro) F.intro->hold_card = true;
    if (!hundred_play_next()) to_select();
}

static bool open_data(void) {
    if (!F.ready) F.ready = smm1_data_open();
    return F.ready;
}

static void on_result(int outcome) {
    status();
    switch (outcome) {
    case HUNDRED_CLEAR:
        F.state = S_PLAYING;
        ov_show_clear(&F.ov, F.st.credits[F.st.ncredits > 0 ? F.st.ncredits - 1 : 0].name,
                      F.st.credits[F.st.ncredits > 0 ? F.st.ncredits - 1 : 0].maker, F.st.cleared >= F.st.total);
        F.clear_act_at = -1;
        break;
    case HUNDRED_MISS:
        if (F.st.lives > 0) {
            CardInfo c = card_info();
            F.state = S_PLAYING;
            ov_show_card(&F.ov, &c);
            F.card_retry = 1;
        } else {
            close_scenes();
            ov_hide(&F.ov);
            F.go = gameover_new();
            F.state = F.go ? S_GAMEOVER : S_CLOSED;
            hundred_run_finish();
        }
        break;
    case HUNDRED_SKIP: to_intro(ENTRY_DIRECT); break;
    case HUNDRED_GIVEUP_SAVE:
    case HUNDRED_ERROR:
    default: to_select(); break;
    }
}

static void handle_queue(void) {
    int q[32][2], n;
    pthread_mutex_lock(&g_q);
    memcpy(q, g_queue, sizeof q);
    n = g_nq, g_nq = 0;
    pthread_mutex_unlock(&g_q);
    for (int i = 0; i < n; i++) {
        switch (q[i][0]) {
        case Q_OPEN:
            if (F.state == S_CLOSED && open_data()) F.ending_only = false, to_select();
            break;
        case Q_PAUSE:
            if (F.state == S_PLAYING && F.ov.mode == OV_NONE) {
                status();
                const LMsg *m = NULL;
                char line[96];
                snprintf(line, sizeof line, "100 Mario Challenge (%d/%d)", F.st.cleared + 1, F.st.total);
                (void)m;
                ov_show_pause(&F.ov, line, F.st.current.name, F.st.current.maker);
            }
            break;
        case Q_STARTED:
            if (F.state == S_WAIT) {
                intro_free(F.intro), F.intro = NULL;
                F.state = S_PLAYING;
            }
            break;
        case Q_RESULT:
            if (F.state == S_PLAYING || F.state == S_WAIT) on_result(q[i][1]);
            break;
        case Q_ENDING:
            if (open_data()) {
                F.ending_only = true, F.ending_done = false;
                to_ending(g_end_d, 100, g_end_n, g_end_credits);
            } else
                F.ending_done = true;
            break;
        }
    }
}

// the made-up course timeline (when the mod's functions are the stubs above)
static void fake_tick(void) {
    if (!fake.used || !fake.playing) return;
    fake.play_tick++;
    long play_len = getenv("RECOMP_HUNDRED_FAKE_PLAY") ? atol(getenv("RECOMP_HUNDRED_FAKE_PLAY")) : 240;
    if (fake.playing == 1 && fake.play_tick == 30) post(Q_STARTED, 0);
    if (fake.playing == 1 && fake.play_tick == play_len) {
        fake.playing = 2;
        const char *o = getenv("RECOMP_HUNDRED_FAKE_OUTCOME");
        if (o && !strcmp(o, "miss")) { fake.lives--; post(Q_RESULT, HUNDRED_MISS); }
        else { fake.cleared++; post(Q_RESULT, HUNDRED_CLEAR); }
    }
    if (fake.playing == 3 && fake.play_tick == 2) { fake.cleared++; post(Q_RESULT, HUNDRED_SKIP); }
    if (fake.playing == 4 && fake.play_tick == 2) post(Q_RESULT, HUNDRED_GIVEUP_SAVE);
}

static void step_playing(const UiIn *in) {
    Overlay *o = &F.ov;
    if (o->mode == OV_PAUSE) {
        if (in->pressed & BT_UP) ov_pause_select(o, (o->pause_sel + 2) % 3);
        if (in->pressed & BT_DOWN) ov_pause_select(o, (o->pause_sel + 1) % 3);
        if (in->pressed & (BT_B | BT_PLUS)) { ov_hide_pause(o); hundred_play_resume(); }
        else if (in->pressed & BT_A) {
            int sel = o->pause_sel;
            ov_hide_pause(o);
            if (sel == 0) hundred_play_resume();
            else if (sel == 1) hundred_play_retry();
            else hundred_run_save_quit();
        }
        return;
    }
    if (o->mode == OV_CLEAR) {
        if (F.clear_act_at >= 0) {
            if (o->tick >= F.clear_act_at) {
                bool next = o->clear_focus == 1;
                ov_hide(o);
                if (!next) hundred_run_save_quit();
                else if (F.st.cleared >= F.st.total) {
                    to_ending(F.d, F.st.lives, F.st.ncredits, F.st.credits);
                } else {
                    to_intro(ENTRY_AFTER_CLEAR);
                }
            }
            return;
        }
        if (!ov_clear_ready(o)) return;
        if (in->pressed & BT_LEFT) ov_clear_select(o, 0);
        if (in->pressed & BT_RIGHT) ov_clear_select(o, 1);
        if (in->pressed & BT_A) { ov_press_clear(o); F.clear_act_at = o->tick + 20; }
        return;
    }
    if (o->mode == OV_CARD) {
        if (ov_card_done(o) || (in->pressed & BT_A)) {
            ov_hide(o);
            if (F.card_retry) F.card_retry = 0, hundred_play_retry();
        }
        return;
    }
    // SMM1's skip: hold the skip button through the three countdown steps (3 x 32 frames), then Pass
    if (F.skip_until > 0) {
        if (o->tick >= F.skip_until) { F.skip_until = 0; ov_hide(o); hundred_play_skip(); }
        return;
    }
    if (in->held & BT_MINUS) {
        if (F.skip_hold++ == 0) ov_skip_begin(o);
        if (F.skip_hold == 96) F.skip_until = o->tick + ov_skip_pass(o);
    } else if (F.skip_hold) {
        F.skip_hold = 0;
        ov_skip_cancel(o);
    }
}

static void flow_test_open(const char *spec);

// one tick: input, the queue, the current screen
static void tick(unsigned buttons) {
    handle_queue();
    fake_tick();
    UiIn in = {buttons & ~F.prev, buttons};
    F.prev = buttons;
    F.ov.tick++;
    bool resume = false;
    switch (F.state) {
    case S_SELECT: {
        int ev = select_step(F.sel, &in, &F.st, &resume);
        if (ev == EV_EXIT) { close_scenes(); F.state = S_CLOSED; hundred_flow_exit(); }
        else if (ev == EV_START) {
            F.d = F.sel->d;
            if (hundred_run_start(F.d, resume)) to_intro(resume ? ENTRY_RESUME : ENTRY_NEW);
        }
        break;
    }
    case S_INTRO:
        if (intro_step(F.intro, &in) == EV_PLAY) play_next();
        break;
    case S_WAIT:
        if (F.intro) intro_step(F.intro, &in);  // (the card holds its last frame)
        break;
    case S_PLAYING: step_playing(&in); break;
    case S_GAMEOVER:
        if (gameover_step(F.go, &in) == EV_BACK) to_select();
        break;
    case S_ENDING:
        if (ending_step(F.end, &in) == EV_DONE) {
            if (F.ending_only) { close_scenes(); F.state = S_CLOSED; F.ending_done = true; }
            else { hundred_run_finish(); to_select(); }
        }
        break;
    default: break;
    }
    F.full = F.state == S_SELECT || F.state == S_INTRO || F.state == S_WAIT || F.state == S_GAMEOVER || F.state == S_ENDING;
    F.captured = F.full || (F.state == S_PLAYING && (F.ov.mode == OV_PAUSE || F.ov.mode == OV_CLEAR || F.ov.mode == OV_CARD));
}

// the quads of this frame and the colour under them; false: nothing to draw
static bool build(LQuads *q, uint8_t bg[4]) {
    lyt_quads_reset(q);
    memset(bg, 0, 4);
    if (F.full) bg[3] = 255;
    switch (F.state) {
    case S_SELECT: select_draw(F.sel, q); break;
    case S_INTRO:
    case S_WAIT:
        if (F.intro) intro_draw(F.intro, q);
        break;
    case S_GAMEOVER: gameover_draw(F.go, q); break;
    case S_ENDING: ending_background(F.end, bg), ending_draw(F.end, q); break;
    case S_PLAYING: ov_draw(&F.ov, q); break;
    default: break;
    }
    return F.full || q->n > 0;
}

// ================================================================ the mod's calls
void hundred_flow_open(void) { post(Q_OPEN, 0); }
bool hundred_flow_active(void) { return F.full; }
bool hundred_flow_overlay(void) { return !F.full && F.captured; }
void hundred_flow_pause(void) { post(Q_PAUSE, 0); }
void hundred_flow_course_started(void) { post(Q_STARTED, 0); }
void hundred_course_result(int outcome) { post(Q_RESULT, outcome); }
void hundred_ending_show(int d, int n, const struct hundred_credit *c) {
    pthread_mutex_lock(&g_q);
    g_end_d = d, g_end_n = n < 0 ? 0 : n > 16 ? 16 : n;
    if (c) memcpy(g_end_credits, c, sizeof(*c) * (size_t)g_end_n);
    pthread_mutex_unlock(&g_q);
    F.ending_done = false;
    post(Q_ENDING, 0);
}
bool hundred_ending_done(void) { return F.ending_done; }

// ================================================================ tests
static void flow_test_open(const char *spec) {
    if (!open_data()) return;
    char s[64];
    snprintf(s, sizeof s, "%s", spec);
    char *a = strchr(s, ':'), *b = a ? strchr(a + 1, ':') : NULL;
    if (a) *a++ = 0;
    if (b) *b++ = 0;
    int x = a ? atoi(a) : 1, y = b ? atoi(b) : 0;
    fake.used = true;
    LOGF("test: %s\n", spec);
    if (!strcmp(s, "select")) { post(Q_OPEN, 0); return; }
    fake.d = F.d = x < 0 ? 0 : x > 3 ? 3 : x;
    if (!strcmp(s, "newgame")) { hundred_run_start(F.d, false); to_intro(ENTRY_NEW); }
    else if (!strcmp(s, "resume")) {
        fake.saved[F.d] = true, fake.saved_lives[F.d] = 87, fake.saved_cleared[F.d] = y;
        hundred_run_start(F.d, true);
        to_intro(ENTRY_RESUME);
    } else if (!strcmp(s, "afterclear")) {
        hundred_run_start(F.d, false);
        fake.cleared = y, fake.lives = 95;
        to_intro(ENTRY_AFTER_CLEAR);
    } else if (!strcmp(s, "card")) { hundred_run_start(F.d, false); fake.lives = 42; to_intro(ENTRY_DIRECT); }
    else if (!strcmp(s, "gameover")) { close_scenes(); F.go = gameover_new(); F.state = S_GAMEOVER; }
    else if (!strcmp(s, "ending")) {
        struct hundred_credit cr[16];
        int n = y > 0 ? y : RUN_LENGTHS[F.d];
        char *c3 = b ? strchr(b, ':') : NULL;  // ending:<d>:<n>:<style>[:<form>]
        F.st.style = c3 ? atoi(c3 + 1) : 0;
        char *c4 = c3 ? strchr(c3 + 1, ':') : NULL;
        F.st.form = c4 ? atoi(c4 + 1) : 0;
        for (int i = 0; i < n && i < 16; i++) fake_course(i, &cr[i]);
        F.ending_only = true;
        to_ending(F.d, 100, n, cr);
    } else {
        hundred_run_start(F.d, false);
        fake.cleared = 3, fake.lives = 77;
        status();
        close_scenes();
        F.state = S_PLAYING;
        if (!strcmp(s, "pause")) post(Q_PAUSE, 0);
        else if (!strcmp(s, "clear")) { if (a && !strcmp(a, "last")) fake.cleared = RUN_LENGTHS[F.d] - 1; fake.cleared++; on_result(HUNDRED_CLEAR); }
        else if (!strcmp(s, "miss")) { fake.lives--; on_result(HUNDRED_MISS); }
        else if (!strcmp(s, "skip")) F.skip_hold = 0, F.prev = 0;
    }
}

// ================================================================ raster worker + the renderers' hook
static pthread_mutex_t g_img = PTHREAD_MUTEX_INITIALIZER;
static pthread_cond_t g_cv = PTHREAD_COND_INITIALIZER;
static uint8_t *g_front, *g_back;
static uint64_t g_front_seq;
static bool g_front_full, g_has_front;
static LQuads g_job;
static uint8_t g_job_bg[4];
static bool g_job_full, g_job_pending;

static void raster_to(const LQuads *q, const uint8_t bg[4], uint8_t *px) {
    lyt_clear(px, W, H, bg);
    lyt_raster(q, px, W, H, 1280, 720);
}
static void *worker(void *arg) {
    (void)arg;
    LQuads q = {0};
    for (;;) {
        pthread_mutex_lock(&g_img);
        while (!g_job_pending) pthread_cond_wait(&g_cv, &g_img);
        if (q.cap < g_job.n) q.cap = g_job.n, q.q = realloc(q.q, sizeof(LQuad) * (size_t)q.cap);
        memcpy(q.q, g_job.q, sizeof(LQuad) * (size_t)g_job.n);
        q.n = g_job.n;
        uint8_t bg[4];
        memcpy(bg, g_job_bg, 4);
        bool full = g_job_full;
        g_job_pending = false;
        pthread_mutex_unlock(&g_img);
        raster_to(&q, bg, g_back);
        pthread_mutex_lock(&g_img);
        uint8_t *t = g_front;
        g_front = g_back, g_back = t;
        g_front_seq++, g_front_full = full, g_has_front = true;
        pthread_mutex_unlock(&g_img);
    }
    return NULL;
}

#ifndef SMM1UI_STANDALONE
extern uint64_t input_buttons_now(void);  // runtime/hle/input.c: keyboard, controllers, test scripts
extern uint64_t renderer_present_count(void);

bool game_overlay_captured(void) { return F.captured; }
static LQuads g_last;
static bool g_worker_on;

// Once per presented frame (renderers, runtime/ui/menu.mm): tick the flow, queue this frame's image, return the
// newest finished one (RGBA8 1280 x 720; *full: opaque, the game's frame hidden), or NULL for none. *seq changes
// with the image. The pointer stays valid until the next call.
const uint8_t *game_overlay_image(int *w, int *h, int *full, uint64_t *seq) {
    static bool test_read;
    if (!test_read) {
        test_read = true;
        const char *t = getenv("RECOMP_HUNDRED_FLOW_TEST");
        if (t && *t) {
            snprintf(F.test, sizeof F.test, "%s", t);
            char *at = strchr(F.test, '@');
            F.test_at = at ? atol(at + 1) : 600;
            if (at) *at = 0;
        }
    }
    if (F.test[0] && (long)renderer_present_count() >= F.test_at) {
        char spec[64];
        snprintf(spec, sizeof spec, "%s", F.test);
        F.test[0] = 0;
        flow_test_open(spec);
    }
    if (F.state == S_CLOSED && !g_nq) {
        g_has_front = false;
        F.full = F.captured = false;
        return NULL;
    }
    unsigned b = (unsigned)input_buttons_now();
    // the left stick as the d-pad on these screens
    tick(b);
    LQuads *q = &g_last;
    uint8_t bg[4];
    bool any = build(q, bg);
    pthread_mutex_lock(&g_img);
    if (!g_worker_on) {
        g_front = calloc(1, (size_t)W * H * 4), g_back = calloc(1, (size_t)W * H * 4);
        pthread_t th;
        g_worker_on = pthread_create(&th, NULL, worker, NULL) == 0;
    }
    if (any) {
        if (g_job.cap < q->n) g_job.cap = q->n, g_job.q = realloc(g_job.q, sizeof(LQuad) * (size_t)g_job.cap);
        memcpy(g_job.q, q->q, sizeof(LQuad) * (size_t)q->n);
        g_job.n = q->n;
        memcpy(g_job_bg, bg, 4);
        g_job_full = F.full;
        g_job_pending = true;
        pthread_cond_signal(&g_cv);
    } else
        g_has_front = false;
    const uint8_t *img = any && g_has_front ? g_front : NULL;
    *w = W, *h = H, *full = g_front_full, *seq = g_front_seq;
    pthread_mutex_unlock(&g_img);
    return img;
}
#endif

// ================================================================ standalone renders (tools/test_smm1ui --flow)
#ifdef SMM1UI_STANDALONE
extern int write_png(const char *path, const uint8_t *rgba, int w, int h);
// --flow <out_%04d.png> <screen> <first> <last> [step] [inputs: tick:BUTTON[:len],...]
int flow_test_main(int argc, char **argv) {
    if (argc < 5) { fprintf(stderr, "--flow <out_%%04d.png> <screen> <first> <last> [step] [tick:A[:len],...]\n"); return 2; }
    const char *pat = argv[1];
    long first = atol(argv[3]), last = atol(argv[4]), step = argc > 5 ? atol(argv[5]) : 1;
    const char *inputs = argc > 6 ? argv[6] : "";
    if (step < 1) step = 1;
    flow_test_open(argv[2]);
    uint8_t *px = malloc((size_t)W * H * 4);
    LQuads q = {0};
    for (long t = 1; t <= last; t++) {
        unsigned b = 0;
        char buf[1024];
        snprintf(buf, sizeof buf, "%s", inputs);
        for (char *tok = strtok(buf, ","); tok; tok = strtok(NULL, ",")) {
            long at = 0, len = 1;
            char name[16] = "";
            if (sscanf(tok, "%ld:%15[A-Z]:%ld", &at, name, &len) < 2) continue;
            if (t < at || t >= at + len) continue;
            static const char *nm[16] = {"A", "B", "X", "Y", "LS", "RS", "L", "R", "ZL", "ZR", "PLUS", "MINUS", "LEFT", "UP", "RIGHT", "DOWN"};
            for (int i = 0; i < 16; i++)
                if (!strcmp(name, nm[i])) b |= 1u << i;
        }
        tick(b);
        if (t >= first && (t - first) % step == 0) {
            uint8_t bg[4];
            bool any = build(&q, bg);
            if (!any) bg[0] = bg[1] = bg[2] = 60, bg[3] = 255;  // (the game would show here)
            if (bg[3] == 0) bg[0] = 60, bg[1] = 90, bg[2] = 60, bg[3] = 255;
            struct timespec t0, t1;
            clock_gettime(CLOCK_MONOTONIC, &t0);
            raster_to(&q, bg, px);
            clock_gettime(CLOCK_MONOTONIC, &t1);
            double ms = (t1.tv_sec - t0.tv_sec) * 1e3 + (t1.tv_nsec - t0.tv_nsec) / 1e6;
            char path[512];
            snprintf(path, sizeof path, pat, (int)t);
            write_png(path, px, W, H);
            fprintf(stderr, "frame %ld: state %d, %d quads, raster %.1f ms -> %s\n", t, F.state, q.n, ms, path);
        }
    }
    free(px);
    return 0;
}
#endif
