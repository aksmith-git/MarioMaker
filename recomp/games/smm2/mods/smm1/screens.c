// SMM1's 100 Mario Challenge screens (screens.h). Port of SMCraft's front/src/smm1.rs.
#include "screens.h"

#include <math.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

const int RUN_LENGTHS[4] = {8, 16, 16, 6};
static const char *SUFFIX[4] = {"Esy", "Nml", "Exp", "SpExp"};
#define MARIO "N_WorldMapMario_00"
#define MARIO_PART "L_WorldMapMario_00"

static const char *F(const char *fmt, ...) {  // a formatted path (ring of buffers)
    static char buf[8][160];
    static int k;
    char *b = buf[k++ & 7];
    va_list ap;
    va_start(ap, fmt);
    vsnprintf(b, 160, fmt, ap);
    va_end(ap);
    return b;
}

// ---------------------------------------------------------------- the start card
// One small Mario per life: the formation is 100 and AnimB sends #99 forward, so the hidden ones are the first [inf].
void card_configure(Lay *c, const CardInfo *i) {
    lay_text(c, "T_CourseName_00", i->name);
    lay_text(c, "T_PlayerName_00", i->maker);
    if (i->code[0]) {
        lay_text(c, "T_IDNumber_00", i->code);
        lay_visible(c, "N_CourseID_00");
    } else
        lay_show(c, "N_CourseID_00", false);
    lay_text(c, "T_Zanki_00", F("%d", i->lives));
    lay_text(c, "T_CourseNum_00", F("%d/%d", i->number, i->total));
    lay_show(c, "L_NF_00", false);  // national flag: no region data
    lay_show(c, "P_NFBase_00", false);
    int shown = i->lives < 100 ? (i->lives < 0 ? 0 : i->lives) : 100;
    for (int k = 0; k < 100; k++) {
        if (k >= 100 - shown) lay_visible(c, F("P_MarioSml_%02d", k));
        else lay_show(c, F("P_MarioSml_%02d", k), false);
    }
}

// ---------------------------------------------------------------- world map
static const char *dot(const WorldMap *m, int i) { return F("L_WorldMapCourse%s_%02d", SUFFIX[m->d], i); }

float wm_stop(const WorldMap *m, int i) {
    if (!m->nstops) return 0;
    return m->stops[i < m->nstops ? i : m->nstops - 1];
}

// Mario's stops: the MoveMario frame at which his pane reaches each course dot (the animation authors one walk along
// all dots)
static void compute_stops(WorldMap *m) {
    float len = lay_length(m->l, "MoveMario");
    m->nstops = 0;
    if (len <= 0) return;
    float dx[16], dy[16];
    int nd = 0;
    for (int i = 0; i < m->count && i < 16; i++) {
        LPaneInfo p;
        if (lay_pane(m->l, dot(m, i), &p)) dx[nd] = p.t[0], dy[nd] = p.t[1], nd++;
    }
    for (float f = 0; f <= len && m->nstops < nd; f += 1) {
        lyt_bind(m->l->in, "MoveMario", f);
        LPaneInfo p;
        if (lay_pane(m->l, MARIO, &p) && fabsf(p.t[0] - dx[m->nstops]) < 1.5f && fabsf(p.t[1] - dy[m->nstops]) < 1.5f)
            m->stops[m->nstops++] = f;
    }
    lay_stop(m->l, "MoveMario");
    while (m->nstops < m->count) m->stops[m->nstops++] = len;
}

WorldMap *wm_new(int d) {
    Lay *l = lay_open(F("Nwk_WorldMap%s_00", SUFFIX[d]));
    if (!l) return NULL;
    WorldMap *m = calloc(1, sizeof *m);
    m->l = l, m->d = d, m->count = RUN_LENGTHS[d];
    compute_stops(m);
    // resting loops; the beat bounces run on a fixed period here (the game syncs them to the music) [inf]
    lay_play_loop(l, "Default", 0);
    lay_play_loop(l, "Beat_00_00", 0);
    lay_play_loop(l, "Beat_00_01", 0);
    lay_play_loop(l, "L_Castle_00/Default", 0);
    lay_play_loop(l, "L_Castle_01/Default", 0);
    lay_play_loop(l, "L_WorldMapKuribo_00/Default", 0);
    lay_hold(l, MARIO_PART "/Stand", 0);
    return m;
}
void wm_free(WorldMap *m) {
    if (!m) return;
    lay_free(m->l);
    free(m);
}
// dots before n show cleared (Type frame 1: the "clear" disc) [inf]
void wm_set_cleared(WorldMap *m, int n) {
    for (int i = 0; i < m->count; i++) lay_hold(m->l, F("%s/Type", dot(m, i)), i < n ? 1 : 0);
}
void wm_set_counters(WorldMap *m, int lives, int cleared) {
    lay_text(m->l, "T_RestMarioNum_00", F("%d", lives));
    lay_text(m->l, "T_ClearCourseNum_00", F("%d/%d", cleared, m->count));
}
void wm_stand(WorldMap *m) {
    lay_stop(m->l, MARIO_PART "/Walk");
    lay_stop(m->l, MARIO_PART "/Jump");
    lay_hold(m->l, MARIO_PART "/Stand", 0);
}
void wm_place_mario(WorldMap *m, int i) {
    lay_hold(m->l, "MoveMario", i < 0 ? 0 : wm_stop(m, i));
    wm_stand(m);
}
int wm_walk(WorldMap *m, float from, float to, int tick) {
    lay_play_ex(m->l, "MoveMario", tick, from, to, false, 1);
    lay_stop(m->l, MARIO_PART "/Stand");
    if (from < 28) lay_play(m->l, MARIO_PART "/Jump", tick);  // out of the start castle first
    lay_play_loop(m->l, MARIO_PART "/Walk", tick + (from < 28 ? 28 : 0));
    return tick + (int)ceilf(to - from);
}

// ---------------------------------------------------------------- difficulty select
static const char *TABS[4] = {"L_Sub_00/L_ListSelectBtnSub_10", "L_Sub_00/L_ListSelectBtnSub_00",
                              "L_Sub_00/L_ListSelectBtnSub_01", "L_Sub_00/L_ListSelectBtnSub_08"};

static void btn_state(Lay *l, int tick, const char *part, const char *anim, float frame) {
    static const char *all[5] = {"OnOff", "Valid", "Invalid", "Decide", "Cancel"};
    for (int i = 0; i < 5; i++)
        if (strcmp(all[i], anim)) lay_stop(l, F("%s/%s", part, all[i]));
    if (frame >= 0) lay_hold(l, F("%s/%s", part, anim), frame);
    else lay_play(l, F("%s/%s", part, anim), tick);
}

static void update_tabs(Select *s, const struct hundred_status *st) {
    for (int d = 0; d < 4; d++) {
        if (d == s->d) btn_state(s->title, s->tick, TABS[d], "Decide", lay_length(s->title, F("%s/Decide", TABS[d])));
        else btn_state(s->title, s->tick, TABS[d], st->unlocked[d] ? "Valid" : "Invalid", 1);
    }
}
static void update_buttons(Select *s, const struct hundred_status *st) {
    int d = s->d;
    bool open = st->unlocked[d], cont = open && st->can_continue[d];
    if (!cont && s->focus == 0) s->focus = 1;
    WorldMap *m = s->maps[d];
    if (!m) return;
    static const char *parts[2] = {"L_StartBtn_01", "L_StartBtn_00"};  // Continue (upper), New Game
    for (int i = 0; i < 2; i++) {
        bool enabled = open && (i == 1 || cont);
        if (!enabled) btn_state(m->l, s->tick, parts[i], "Invalid", 1);
        else if (i == s->focus) btn_state(m->l, s->tick, parts[i], "OnOff", 3);
        else btn_state(m->l, s->tick, parts[i], "Valid", 1);
    }
}
// a map in its selection state: before MapIn, counters and Mario from the continue record
static void refresh(Select *s, int d, const struct hundred_status *st) {
    WorldMap *m = s->maps[d];
    if (!m) return;
    bool cont = st->can_continue[d];
    lay_hold(m->l, "MapIn", 0);
    wm_set_cleared(m, cont ? st->continue_cleared[d] : 0);
    wm_set_counters(m, cont ? st->continue_lives[d] : 100, cont ? st->continue_cleared[d] : 0);
    if (cont) {
        wm_place_mario(m, st->continue_cleared[d]);
        lay_hold_end(m->l, "AnimB");  // Peach already on the castle
    } else {
        wm_place_mario(m, -1);
        lay_stop(m->l, "AnimB");
    }
    update_buttons(s, st);
}
static bool prepare(Select *s, int d, const struct hundred_status *st) {
    if (!s->maps[d]) {
        s->maps[d] = wm_new(d);
        if (!s->maps[d]) return false;
        refresh(s, d, st);
    }
    return true;
}

Select *select_new(int d, const struct hundred_status *st) {
    Lay *t = lay_open("Nwk_ListTitle_00");
    if (!t) return NULL;
    Select *s = calloc(1, sizeof *s);
    s->title = t, s->d = d, s->focus = 1;
    lay_play(t, "In", 0);
    lay_play_loop(t, "Default", 0);
    lay_hold(t, "MapIn", 0);
    lay_hold(t, "L_Sub_00/ListTitleType", 0);  // frame 0: the 100 Mario list
    lay_hold(t, "L_Sub_00/OpenType", 2);  // OpenType 0/1/2 shows 2/3/4 tabs; all four are shown, locked ones greyed
    if (!prepare(s, d, st)) { select_free(s); return NULL; }
    lay_play(s->maps[d]->l, "In", 0);
    update_tabs(s, st);
    return s;
}
void select_free(Select *s) {
    if (!s) return;
    for (int i = 0; i < 4; i++) wm_free(s->maps[i]);
    wm_free(s->leaving);
    lay_free(s->title);
    free(s);
}
static void switch_to(Select *s, int d, const struct hundred_status *st) {
    if (d == s->d || !prepare(s, d, st)) return;
    bool right = d > s->d;
    WorldMap *m = s->maps[s->d];
    s->maps[s->d] = NULL;
    if (m) {
        lay_play(m->l, right ? "OutL" : "OutR", s->tick);
        wm_free(s->leaving);
        s->leaving = m;
        s->leave_until = s->tick + 32;
    }
    s->d = d;
    refresh(s, d, st);
    lay_play(s->maps[d]->l, right ? "InR" : "InL", s->tick);
    update_tabs(s, st);
}
int select_step(Select *s, const UiIn *in, const struct hundred_status *st, bool *resume) {
    s->tick++;
    if (s->leaving && s->tick >= s->leave_until) wm_free(s->leaving), s->leaving = NULL;
    if (!s->maps[s->d]) return EV_NONE;
    unsigned p = in->pressed;
    if (p & (BT_LEFT | BT_RIGHT)) {
        int d = p & BT_RIGHT ? (s->d < 3 ? s->d + 1 : 3) : (s->d > 0 ? s->d - 1 : 0);
        switch_to(s, d, st);
        return EV_NONE;
    }
    bool cont = st->unlocked[s->d] && st->can_continue[s->d];
    if ((p & BT_UP) && cont) s->focus = 0, update_buttons(s, st);
    if (p & BT_DOWN) s->focus = 1, update_buttons(s, st);
    if (p & BT_B) return EV_EXIT;
    if (p & BT_A) {
        if (!st->unlocked[s->d]) return EV_LOCKED;
        *resume = s->focus == 0 && cont;
        static const char *slides[4] = {"OutL", "OutR", "InL", "InR"};
        for (int i = 0; i < 4; i++) lay_stop(s->maps[s->d]->l, slides[i]);
        return EV_START;
    }
    return EV_NONE;
}
void select_draw(Select *s, LQuads *out) {
    if (s->leaving) lay_draw(s->leaving->l, s->tick, out);
    if (s->maps[s->d]) lay_draw(s->maps[s->d]->l, s->tick, out);
    lay_draw(s->title, s->tick, out);
}

// ---------------------------------------------------------------- map walk + start card
#define CARD_HOLD 150  // the card's Out animation length: a timer [inf]

static void schedule_card(Intro *s) {
    if (s->card_start < 0) return;
    lay_play(s->card, "AnimA", s->card_start);
    lay_play(s->card, "AnimB", s->card_start + (int)lay_length(s->card, "AnimA"));
}
static int card_end(Intro *s) {
    if (s->card_start < 0) return -1;
    return s->card_start + (int)(lay_length(s->card, "AnimA") + lay_length(s->card, "AnimB")) + CARD_HOLD;
}

Intro *intro_new(int entry, int d, const CardInfo *info, int cleared, Select *handoff) {
    Lay *card = lay_open("Play_CourseIn100Mario_00");
    if (!card) return NULL;
    Intro *s = calloc(1, sizeof *s);
    s->card = card, s->card_start = s->out_at = s->walk_start = s->walk_end = -1;
    card_configure(card, info);
    if (entry == ENTRY_DIRECT) {
        s->card_start = 0;
        schedule_card(s);
        return s;
    }
    if ((entry == ENTRY_NEW || entry == ENTRY_RESUME) && handoff && handoff->d == d && handoff->maps[d]) {
        s->map = handoff->maps[d], handoff->maps[d] = NULL;
        s->title = handoff->title, handoff->title = NULL;
    } else
        s->map = wm_new(d);
    WorldMap *m = s->map;
    if (!m) {
        s->card_start = 0;
        schedule_card(s);
        return s;
    }
    wm_set_counters(m, info->lives, cleared);
    Lay *l = m->l;
    if (entry == ENTRY_NEW) {
        lay_play(l, "MapIn", 0);
        if (s->title) lay_play(s->title, "MapIn", 0);
        wm_set_cleared(m, 0);
        wm_place_mario(m, -1);
        lay_stop(l, "AnimB");
        // the Goombas carry Peach to the castle under the banner, she appears on it, then Mario jumps out of the
        // start castle and walks to the first dot [inf]
        lay_play(l, "Telop", 200);
        lay_play(l, "MoveKuribo", 200);
        float tl = lay_length(l, "Telop"), mk = lay_length(l, "MoveKuribo");
        int b = 200 + (int)(tl > mk ? tl : mk);
        lay_play(l, "AnimB", b);
        s->walk_start = b + 20, s->walk_from = 0, s->walk_to = wm_stop(m, 0);
    } else if (entry == ENTRY_RESUME) {
        lay_play(l, "MapIn", 0);
        if (s->title) lay_play(s->title, "MapIn", 0);
        wm_set_cleared(m, cleared);
        wm_place_mario(m, cleared);
        lay_hold_end(l, "AnimB");
        s->out_at = 230;
    } else {  // after a clear
        lay_hold_end(l, "MapIn");
        lay_play(l, "In", 0);
        lay_hold_end(l, "AnimB");
        wm_set_cleared(m, cleared);
        if (cleared > 0) lay_play(l, F("%s/Clear", dot(m, cleared - 1)), 40);
        int prev = cleared > 0 ? cleared - 1 : 0;
        wm_place_mario(m, prev);
        s->walk_start = 70, s->walk_from = wm_stop(m, prev), s->walk_to = wm_stop(m, cleared);
    }
    return s;
}
void intro_free(Intro *s) {
    if (!s) return;
    wm_free(s->map);
    lay_free(s->title);
    lay_free(s->card);
    free(s);
}
int intro_step(Intro *s, const UiIn *in) {
    s->tick++;
    if (s->finished) return EV_NONE;
    int t = s->tick;
    if (s->map) {
        if (s->walk_start == t) s->walk_end = wm_walk(s->map, s->walk_from, s->walk_to, t);
        if (s->walk_end == t) {
            wm_stand(s->map);
            lay_hold(s->map->l, "MoveMario", s->walk_to);
            s->out_at = t + 30;
        }
        if (s->out_at == t && s->card_start < 0) {
            lay_play(s->map->l, "Out", t);
            s->card_start = t + lay_ticks(s->map->l, "Out", 1);
            schedule_card(s);
        }
    }
    if (in->pressed & BT_A) {  // A skips ahead (the game skips these with A since 1.20)
        if (s->card_start >= 0 && t >= s->card_start) { s->finished = true; return EV_PLAY; }
        s->card_start = t;
        schedule_card(s);
        return EV_NONE;
    }
    int e = card_end(s);
    if (e >= 0 && t >= e) { s->finished = true; return EV_PLAY; }
    return EV_NONE;
}
void intro_draw(Intro *s, LQuads *out) {
    int t = s->tick;
    if (s->card_start >= 0 && t >= s->card_start) {
        lay_draw(s->card, t, out);
        return;
    }
    if (s->map) lay_draw(s->map->l, t, out);
    if (s->title) lay_draw(s->title, t, out);
}

// ---------------------------------------------------------------- game over
GameOver *gameover_new(void) {
    Lay *l = lay_open("Cmn_GameOver_00");
    if (!l) return NULL;
    GameOver *g = calloc(1, sizeof *g);
    g->l = l;
    lay_play(l, "OneTime", 0);
    return g;
}
void gameover_free(GameOver *g) {
    if (!g) return;
    lay_free(g->l);
    free(g);
}
int gameover_step(GameOver *g, const UiIn *in) {
    g->tick++;
    if (g->done) return EV_NONE;
    if (g->tick >= lay_ticks(g->l, "OneTime", 60) || (g->tick > 60 && (in->pressed & (BT_A | BT_B)))) {
        g->done = true;
        return EV_BACK;
    }
    return EV_NONE;
}
void gameover_draw(GameOver *g, LQuads *out) { lay_draw(g->l, g->tick, out); }

// ---------------------------------------------------------------- overlays over a course
static const char *PAUSE_PARTS[3] = {"L_CloseBtn_00", "L_RetryBtn_00", "L_ExitBtn_00"};
enum { W_PAUSE = 1, W_SKIP = 2 };

void ov_init(Overlay *o) {
    memset(o, 0, sizeof *o);
    o->clear_focus = 1;
}
void ov_free(Overlay *o) {
    lay_free(o->pause), lay_free(o->clear), lay_free(o->skip), lay_free(o->card);
    memset(o, 0, sizeof *o);
}
static void set_mode(Overlay *o, int m) { o->mode = m, o->mode_start = o->tick; }
void ov_draw(Overlay *o, LQuads *out) {
    int t = o->tick;
    if (o->closing_which && t < o->closing_until) lay_draw(o->closing_which == W_PAUSE ? o->pause : o->skip, t, out);
    Lay *l = o->mode == OV_PAUSE ? o->pause : o->mode == OV_CLEAR ? o->clear : o->mode == OV_CARD ? o->card
           : o->mode == OV_SKIP ? o->skip : NULL;
    if (l) lay_draw(l, t, out);
}
void ov_show_pause(Overlay *o, const char *mode_line, const char *course, const char *maker) {
    if (!o->pause && (o->pause = lay_open("Cmn_PauseMenu_00"))) {
        Lay *p = o->pause;
        // Type 6: the two play buttons without the network ones; the exit button reads "Save & Quit" [inf]
        lay_hold(p, "Type", 6);
        lay_text_from(p, "L_ExitBtn_00/T_Btn_00", "T_ExitBtn_01");
        lay_hold(p, "TypeCourseInfo", 2);
        static const char *hide[5] = {"N_NetworkBtn_00", "P_YeahCountIcon_00", "P_PlayerCountIcon_00", "L_NationalFlag_00",
                                      "L_OptionBtn_00"};
        for (int i = 0; i < 5; i++) lay_show(p, hide[i], false);
    }
    if (!o->pause) return;
    lay_text(o->pause, "T_Mode_00", mode_line);
    lay_text(o->pause, "T_CourseName_00", course);
    lay_text(o->pause, "T_MiiName_00", maker);
    lay_stop(o->pause, "Out");
    lay_play(o->pause, "In", o->tick);
    o->closing_which = 0;
    set_mode(o, OV_PAUSE);
    ov_pause_select(o, 0);
}
void ov_pause_select(Overlay *o, int i) {
    if (!o->pause) return;
    o->pause_sel = i;
    for (int k = 0; k < 3; k++) {
        if (k == i) lay_play(o->pause, F("%s/OnOff", PAUSE_PARTS[k]), o->tick);
        else lay_hold(o->pause, F("%s/OnOff", PAUSE_PARTS[k]), 0);
    }
}
void ov_hide_pause(Overlay *o) {
    if (o->mode != OV_PAUSE || !o->pause) return;
    lay_stop(o->pause, "In");
    lay_play(o->pause, "Out", o->tick);
    o->closing_which = W_PAUSE, o->closing_until = o->tick + lay_ticks(o->pause, "Out", 1);
    set_mode(o, OV_NONE);
}
void ov_show_clear(Overlay *o, const char *course, const char *maker, bool last) {
    if (!o->clear) o->clear = lay_open("Play_CourseClear_00");
    Lay *c = o->clear;
    if (!c) return;
    o->clear_last = last;
    // 100 Mario variant: Save & Quit + Next; ClearType centres Next alone on the run's last course [inf]
    if (last) lay_hold(c, "ClearType", 1);
    else lay_hold(c, "ModeType", 0);
    static const char *hide[4] = {"N_MiiverseComment_01", "N_RcmText_00", "N_RcmCourseBtn_02", "N_CommentGoodBtn_00"};
    for (int i = 0; i < 4; i++) lay_show(c, hide[i], false);
    lay_show(c, "N_Player_00/L_NF_00", false);
    lay_show(c, "L_NF_00", false);
    lay_text(c, "T_CourseName_00", course);
    lay_text(c, "T_PlayerName_00", maker);
    lay_text(c, "T_Time_02", "--’--”"), lay_text(c, "T_Time_03", "---");
    lay_text(c, "T_Time_00", "--’--”"), lay_text(c, "T_Time_01", "---");
    lay_play(c, "In", o->tick);
    lay_play_loop(c, "Default", o->tick);
    set_mode(o, OV_CLEAR);
    ov_clear_select(o, 1);
}
void ov_clear_select(Overlay *o, int i) {
    o->clear_focus = o->clear_last ? 1 : (i > 1 ? 1 : i);
    if (!o->clear) return;
    static const char *parts[2] = {"L_ExitBtn_00", "L_NextBtn_00"};
    for (int k = 0; k < 2; k++) {
        if (k == o->clear_focus) lay_play(o->clear, F("%s/OnOff", parts[k]), o->tick);
        else lay_hold(o->clear, F("%s/OnOff", parts[k]), 0);
    }
}
bool ov_clear_ready(Overlay *o) {
    int n = o->clear ? (int)lay_length(o->clear, "In") : 0;
    return o->mode == OV_CLEAR && o->tick - o->mode_start >= n / 2;
}
void ov_press_clear(Overlay *o) {
    if (o->clear) lay_play(o->clear, F("%s/Decide", o->clear_focus == 0 ? "L_ExitBtn_00" : "L_NextBtn_00"), o->tick);
}
void ov_skip_begin(Overlay *o) {
    if (!o->skip) o->skip = lay_open("Play_SlidePassIconKey_00");
    Lay *s = o->skip;
    if (!s) return;
    o->closing_which = 0;
    static const char *stop[5] = {"Out", "Pass", "CountDown_00", "CountDown_01", "CountDown_02"};
    for (int i = 0; i < 5; i++) lay_stop(s, stop[i]);
    lay_play_loop(s, "Default", o->tick);
    lay_play(s, "In", o->tick);
    lay_play(s, "CountDown_00", o->tick);
    lay_play(s, "CountDown_01", o->tick + 32);
    lay_play(s, "CountDown_02", o->tick + 64);
    set_mode(o, OV_SKIP);
}
void ov_skip_cancel(Overlay *o) {
    if (o->mode != OV_SKIP || !o->skip) return;
    lay_play(o->skip, "Out", o->tick);
    o->closing_which = W_SKIP, o->closing_until = o->tick + lay_ticks(o->skip, "Out", 1);
    set_mode(o, OV_NONE);
}
int ov_skip_pass(Overlay *o) {
    if (!o->skip) return 1;
    lay_play(o->skip, "Pass", o->tick);
    return lay_ticks(o->skip, "Pass", 1);
}
void ov_show_card(Overlay *o, const CardInfo *c) {
    if (!o->card) o->card = lay_open("Play_CourseIn100Mario_00");
    if (!o->card) return;
    card_configure(o->card, c);
    lay_hold_end(o->card, "AnimA");
    lay_play(o->card, "AnimB", o->tick);
    lay_play(o->card, "OutMiss", o->tick + (int)lay_length(o->card, "AnimB"));
    set_mode(o, OV_CARD);
}
bool ov_card_done(Overlay *o) {
    if (o->mode != OV_CARD || !o->card) return true;
    float om = lay_length(o->card, "OutMiss");
    return o->tick - o->mode_start >= (int)(lay_length(o->card, "AnimB") + (om > 1 ? om : 1));
}
void ov_hide(Overlay *o) {
    o->closing_which = 0;
    set_mode(o, OV_NONE);
}
