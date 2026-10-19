// SMM1's 100 Mario Challenge screens (screens.h), as Block.rpx v208 strings them together
// [obs unless marked: research/specs/smm1-100mario-screens.md].
#include "screens.h"

#include <math.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "smm1_audio.h"

const int RUN_LENGTHS[4] = {8, 16, 16, 6};
static const char *SUFFIX[4] = {"Esy", "Nml", "Exp", "SpExp"};
#define MARIO_PART "L_WorldMapMario_00"
#define KURIBO "L_WorldMapKuribo_00"

static const char *F(const char *fmt, ...) {  // a formatted path (ring of buffers)
    static char buf[16][160];
    static int k;
    char *b = buf[k++ & 15];
    va_list ap;
    va_start(ap, fmt);
    vsnprintf(b, 160, fmt, ap);
    va_end(ap);
    return b;
}

// ---------------------------------------------------------------- the start card (Play_CourseIn100Mario_00)
// T_CourseNum_00 "n/total", T_Zanki_00 the lives, P_MarioSml_i shown for i < lives, Type frame 1 on the run's last
// course; the In by case: a new run's first course AnimA (215) + AnimB (70), the first after Continue or a retry
// AnimB alone (AnimA at its end), any other course nothing (AnimB at its end).
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
    lay_show(c, "L_NF_00", false);  // national flag: our makers have no SMM1 country code
    lay_show(c, "P_NFBase_00", false);
    for (int k = 0; k < 100; k++) {
        if (k < i->lives) lay_visible(c, F("P_MarioSml_%02d", k));
        else lay_show(c, F("P_MarioSml_%02d", k), false);
    }
    lay_hold(c, "Type", i->last ? 1 : 0);
}

// ---------------------------------------------------------------- world map (Nwk_WorldMap{Esy,Nml,Exp,SpExp}_00)
static const char *dot(const WorldMap *m, int i) { return F("L_WorldMapCourse%s_%02d", SUFFIX[m->d], i); }
float wm_dot_frame(const WorldMap *m, int i) { return (float)(i * m->step + m->base); }

WorldMap *wm_new(int d) {
    Lay *l = lay_open(F("Nwk_WorldMap%s_00", SUFFIX[d]));
    if (!l) return NULL;
    WorldMap *m = calloc(1, sizeof *m);
    m->l = l, m->d = d, m->count = RUN_LENGTHS[d];
    // MoveMario: frames 0-48 the jump out of the start castle, then 12 frames a dot from 60; Super Expert's step is
    // (162 - 48) / 6 = 19 from 67
    m->step = d == 3 ? 19 : 12, m->base = d == 3 ? 67 : 60;
    lay_play_loop(l, "Default", 0);
    lay_play_loop(l, "L_Castle_00/Default", 0);
    lay_play_loop(l, "L_Castle_01/Default", 0);
    return m;
}
void wm_free(WorldMap *m) {
    if (!m) return;
    lay_free(m->l);
    free(m);
}
void wm_set_counters(WorldMap *m, int lives, int cleared) {  // the layout's texts: "<lives>", "<cleared>/<total>"
    const char *a[1] = {F("%d", lives)}, *b[1] = {F("%d", cleared)};
    lay_text_args(m->l, "T_RestMarioNum_00", "T_RestMarioNum_00", a, 1);
    lay_text_args(m->l, "T_ClearCourseNum_00", "T_ClearCourseNum_00", b, 1);
}
// a course dot: hidden (In frame 0) or shown (In at its end); Type frame 0 shows its clear stamp, 1 hides it
static void wm_dot(WorldMap *m, int i, bool shown, bool stamp) {
    lay_hold(m->l, F("%s/In", dot(m, i)), shown ? lay_length(m->l, F("%s/In", dot(m, i))) : 0);
    lay_hold(m->l, F("%s/Type", dot(m, i)), stamp ? 0 : 1);
}
static void wm_dot_in(WorldMap *m, int i, int tick) {
    lay_hold(m->l, F("%s/Type", dot(m, i)), 1);
    lay_play(m->l, F("%s/In", dot(m, i)), tick);
}
static void mario_reset(WorldMap *m) {
    static const char *all[] = {"In", "Jump", "TwoJump", "Walk", "Stand", "Go", "Out"};
    for (int i = 0; i < 7; i++) lay_stop(m->l, F(MARIO_PART "/%s", all[i]));
}
static void mario_hide(WorldMap *m) { mario_reset(m), lay_show(m->l, MARIO_PART, false); }
static void mario_stand(WorldMap *m) {
    mario_reset(m), lay_visible(m->l, MARIO_PART);
    lay_hold(m->l, MARIO_PART "/In", lay_length(m->l, MARIO_PART "/In"));
    lay_hold(m->l, MARIO_PART "/Stand", 0);
}
static void mario_play(WorldMap *m, const char *anim, int tick, bool with_in) {
    mario_reset(m), lay_visible(m->l, MARIO_PART);
    if (with_in) lay_play(m->l, MARIO_PART "/In", tick);
    else lay_hold(m->l, MARIO_PART "/In", lay_length(m->l, MARIO_PART "/In"));
    if (!strcmp(anim, "Walk")) lay_play_loop(m->l, MARIO_PART "/Walk", tick);
    else lay_play(m->l, F(MARIO_PART "/%s", anim), tick);
}
// the map before MapIn as the select screen shows it, from the difficulty's continue record
static void wm_pose(WorldMap *m, bool cont, int lives, int cleared) {
    int k = cont ? cleared : 0;
    lay_hold(m->l, "MapIn", 0);
    for (int i = 0; i < m->count; i++) wm_dot(m, i, i < k || (cont && i == k), i < k);
    if (cont) lay_hold_end(m->l, "AnimB");  // Peach on the castle
    else lay_hold(m->l, "AnimB", 0);
    wm_set_counters(m, cont ? lives : 100, cont ? cleared : 0);
    if (cont) {
        lay_hold(m->l, "MoveMario", wm_dot_frame(m, k));
        mario_stand(m);
    } else {
        lay_hold(m->l, "MoveMario", 0);
        mario_hide(m);
    }
    lay_show(m->l, "L_StartBtn_01", cont);  // Continue only with a continue record
}

// ---------------------------------------------------------------- difficulty select (Nwk_ListTitle_00 over the map)
static const char *TABS[4] = {"L_Sub_00/L_ListSelectBtnSub_10", "L_Sub_00/L_ListSelectBtnSub_00",
                              "L_Sub_00/L_ListSelectBtnSub_01", "L_Sub_00/L_ListSelectBtnSub_08"};

static int open_tabs(const struct hundred_status *st) { return st->unlocked[3] ? 4 : st->unlocked[2] ? 3 : 2; }

static void btn(Lay *l, int tick, const char *part, const char *anim, bool play) {
    static const char *all[6] = {"OnOff", "Valid", "Invalid", "Decide", "Cancel", "DecideLoop"};
    for (int i = 0; i < 6; i++)
        if (strcmp(all[i], anim)) lay_stop(l, F("%s/%s", part, all[i]));
    if (play) lay_play(l, F("%s/%s", part, anim), tick);
    else lay_hold_end(l, F("%s/%s", part, anim));
}
static void update_tabs(Select *s, const struct hundred_status *st) {
    int open = open_tabs(st);
    for (int d = 0; d < open; d++) btn(s->title, s->tick, TABS[d], d == s->d ? "Decide" : "Valid", false);
}
static void update_buttons(Select *s, bool play) {
    WorldMap *m = s->maps[s->d];
    if (!m) return;
    static const char *parts[2] = {"L_StartBtn_01", "L_StartBtn_00"};  // Continue (upper), New Game
    for (int i = 0; i < 2; i++) {
        if (i == s->focus) btn(m->l, s->tick, parts[i], "OnOff", play);  // Select: OnOff forward
        else lay_hold(m->l, F("%s/OnOff", parts[i]), 0);                // UnSelect: back to its start
    }
}
static bool prepare(Select *s, int d, const struct hundred_status *st) {
    if (!s->maps[d] && !(s->maps[d] = wm_new(d))) return false;
    bool cont = st->can_continue[d];
    wm_pose(s->maps[d], cont, st->continue_lives[d], st->continue_cleared[d]);
    return true;
}

Select *select_new(int d, const struct hundred_status *st) {
    Lay *t = lay_open("Nwk_ListTitle_00");
    if (!t) return NULL;
    Select *s = calloc(1, sizeof *s);
    s->title = t, s->exit_at = -1;
    int open = open_tabs(st);
    s->d = d < open ? d : 0;
    lay_play(t, "In", 0);
    lay_play_loop(t, "Default", 0);
    lay_hold(t, "MapIn", 0);
    lay_hold(t, "L_Sub_00/ListTitleType", 0);              // the 100 Mario list
    lay_hold(t, "L_Sub_00/OpenType", (float)(open - 2));  // 2 / 3 / 4 tabs as they unlock
    if (!prepare(s, s->d, st)) { select_free(s); return NULL; }
    lay_play(s->maps[s->d]->l, "In", 0);  // the map comes in (48)
    s->cont = st->can_continue[s->d];
    s->focus = s->cont ? 0 : 1;  // the cursor on Continue when there is one
    update_tabs(s, st);
    update_buttons(s, false);
    return s;
}
void select_free(Select *s) {
    if (!s) return;
    for (int i = 0; i < 4; i++) wm_free(s->maps[i]);
    wm_free(s->leaving);
    lay_free(s->title);
    free(s);
}
// to a lower difficulty (or Easy) the maps slide with OutR / InR, else OutL / InL (64 frames each)
static void switch_to(Select *s, int d, const struct hundred_status *st) {
    if (d == s->d || !prepare(s, d, st)) return;
    int old = s->d;
    bool r = d == 0 || (d == 1 && old == 2) || (d == 2 && old == 3);
    WorldMap *m = s->maps[old];
    s->maps[old] = NULL;
    if (m) {
        lay_play(m->l, r ? "OutR" : "OutL", s->tick);
        wm_free(s->leaving);
        s->leaving = m, s->leave_until = s->tick + 64;
    }
    s->d = d;
    lay_play(s->maps[d]->l, r ? "InR" : "InL", s->tick);
    s->cont = st->can_continue[d];
    s->focus = s->cont ? 0 : 1;
    update_tabs(s, st);
    update_buttons(s, false);
}
int select_step(Select *s, const UiIn *in, const struct hundred_status *st, bool *resume) {
    s->tick++;
    if (s->leaving && s->tick >= s->leave_until) wm_free(s->leaving), s->leaving = NULL;
    if (s->exit_at >= 0) return s->tick >= s->exit_at ? (s->exit_at = -1, EV_EXIT) : EV_NONE;
    if (!s->maps[s->d]) return EV_NONE;
    unsigned p = in->pressed;
    int open = open_tabs(st);
    if (p & (BT_LEFT | BT_RIGHT | BT_L | BT_R)) {  // the tab group's cursor (and the shoulder shortcuts)
        int d = s->d + ((p & (BT_RIGHT | BT_R)) ? 1 : -1);
        if (d >= 0 && d < open) {
            switch_to(s, d, st);
            smm1_se("SE_PageNext1");
            return EV_TAB;
        }
        return EV_NONE;
    }
    if ((p & BT_UP) && s->cont && s->focus != 0) s->focus = 0, update_buttons(s, true), smm1_se("SE_BongoCursol");
    if ((p & BT_DOWN) && s->focus != 1) s->focus = 1, update_buttons(s, true), smm1_se("SE_BongoCursol");
    if (p & BT_B) {  // the map plays Out (32) and Course World's top comes back
        lay_play(s->maps[s->d]->l, "Out", s->tick);
        lay_play(s->title, "Out", s->tick);
        s->exit_at = s->tick + 32;
        return EV_NONE;
    }
    if (p & BT_A) {
        *resume = s->focus == 0 && s->cont;
        btn(s->maps[s->d]->l, s->tick, *resume ? "L_StartBtn_01" : "L_StartBtn_00", "Decide", true);
        smm1_se("daikettefinal");
        if (s->d >= 2) smm1_se("SI_DaiketeiExpert");
        return EV_START;
    }
    return EV_NONE;
}
void select_draw(Select *s, LQuads *out) {
    if (s->leaving) lay_draw(s->leaving->l, s->tick, out);
    if (s->maps[s->d]) lay_draw(s->maps[s->d]->l, s->tick, out);
    lay_draw(s->title, s->tick, out);
}

// ---------------------------------------------------------------- map demos + the card
enum { P_MAPIN, P_COUNTIN, P_TELOP, P_KURIBO, P_CASTLE, P_ANIMB, P_JUMP, P_DOTIN, P_WALK, P_TWOJUMP, P_GO, P_FADE,
       P_CARD_IN, P_CARD_HOLD, P_CARD_OUT, P_DONE };
#define FADE 30  // the iris wipe to the card [inf: its length]

struct Intro {
    WorldMap *map;
    Lay *title, *card;
    int d, entry, cleared, tick, phase, end, from_dot, to_dot, kuribo_walk, kuribo_stand, card_intro;
    bool had_continue, started, skip;
};

static const int KURIBO_STAND[4] = {156, 268, 268, 174};  // MoveKuribo frame the Goombas stop walking

static void phase(Intro *s, int p);
static void iris(LQuads *out, float cx, float cy, float r);
static void card_in(Intro *s) {
    int t = s->tick;
    if (s->card_intro == CARD_NEW_RUN) {
        lay_hold(s->card, "AnimA", 0);
        lay_play(s->card, "AnimA", t);
        lay_play(s->card, "AnimB", t + (int)lay_length(s->card, "AnimA"));
        smm1_se("SE_100marioAshioto");
        s->end = t + (int)(lay_length(s->card, "AnimA") + lay_length(s->card, "AnimB"));
    } else if (s->card_intro == CARD_RETRY) {
        lay_hold_end(s->card, "AnimA");
        lay_play(s->card, "AnimB", t);
        s->end = t + (int)lay_length(s->card, "AnimB");
    } else {
        lay_hold_end(s->card, "AnimA");
        lay_hold_end(s->card, "AnimB");
        s->end = t;
    }
}
static void phase(Intro *s, int p) {
    s->phase = p;
    int t = s->tick;
    WorldMap *m = s->map;
    Lay *l = m ? m->l : NULL;
    switch (p) {
    case P_MAPIN:  // the curtains open and the view zooms in (300), with the title bar's MapIn
        lay_play(l, "MapIn", t);
        if (s->title) lay_play(s->title, "MapIn", t);
        smm1_se("M3_StarsGather");
        s->end = t + (int)lay_length(l, "MapIn");
        if (s->entry == ENTRY_NEW) {
            for (int i = 0; i < m->count; i++) wm_dot(m, i, false, false);
            mario_hide(m);
            if (s->had_continue) lay_play(l, "CountOut", t), lay_hold(l, "AnimB", 0);
        }
        break;
    case P_COUNTIN:
        if (s->had_continue) lay_play(l, "CountIn", t), s->end = t + (int)lay_length(l, "CountIn");
        else s->end = t;
        wm_set_counters(m, 100, 0);
        break;
    case P_TELOP:  // the banner (210): the layout's own text ("in trouble!", Expert "again", Super Expert "yet again")
        lay_hold(l, "MoveMario", 0);
        lay_play(l, "Telop", t);
        s->end = t + (int)lay_length(l, "Telop");
        break;
    case P_KURIBO:  // the Goombas carry Peach to the castle
        lay_hold(l, KURIBO "/Stand", 0);
        lay_play(l, "L_Castle_00/Anim", t);
        lay_play(l, "MoveKuribo", t);
        s->kuribo_walk = t + 60, s->kuribo_stand = t + KURIBO_STAND[s->d];
        s->end = t + (int)lay_length(l, "MoveKuribo");
        break;
    case P_CASTLE:
        lay_stop(l, KURIBO "/Walk"), lay_stop(l, KURIBO "/Stand"), lay_stop(l, KURIBO "/Default");
        lay_play(l, "L_Castle_01/Anim", t);
        s->end = t + (int)lay_length(l, "L_Castle_01/Anim");
        break;
    case P_ANIMB:  // Peach appears on the castle: "Help!"
        lay_play(l, "AnimB", t);
        smm1_se("MW_PeachHelp");
        s->end = t + (int)lay_length(l, "AnimB");
        break;
    case P_JUMP:  // Mario jumps (out of the start castle, or in on his dot after a clear)
        if (s->entry == ENTRY_NEW) {
            lay_play(l, "L_Castle_00/Anim", t);
            lay_play_ex(l, "MoveMario", t, 0, 48, false, 1);
        } else
            lay_hold(l, "MoveMario", wm_dot_frame(m, s->from_dot));
        mario_play(m, "Jump", t, true);
        smm1_se("M1_SmallMarioJump");
        s->end = t + (int)lay_length(l, MARIO_PART "/Jump");
        break;
    case P_DOTIN:  // the next dot pops in (42)
        wm_dot_in(m, s->to_dot, t);
        s->end = t + (int)lay_length(l, F("%s/In", dot(m, s->to_dot)));
        break;
    case P_WALK: {  // one step along MoveMario (12 frames, Super Expert 19), then Stand
        float a = s->entry == ENTRY_NEW ? 48 : wm_dot_frame(m, s->from_dot), b = wm_dot_frame(m, s->to_dot);
        lay_play_ex(l, "MoveMario", t, a, b, false, 1);
        mario_play(m, "Walk", t, false);
        smm1_se("M3_MoveMap");
        s->end = t + (int)ceilf(b - a);
        break;
    }
    case P_TWOJUMP:  // Continue: Mario's double hop on his dot (36)
        lay_hold(l, "MoveMario", wm_dot_frame(m, s->from_dot));
        mario_play(m, "TwoJump", t, false);
        s->end = t + (int)lay_length(l, MARIO_PART "/TwoJump");
        break;
    case P_GO: {  // Mario's Go (128, at least 60), then the scene changes
        mario_play(m, "Go", t, false);
        smm1_se("M3_Wipe");
        int g = (int)lay_length(l, MARIO_PART "/Go");
        s->end = t + (g > 60 ? g : 60);
        break;
    }
    case P_FADE: s->end = t + FADE; break;
    case P_CARD_IN: card_in(s); break;
    case P_CARD_HOLD: s->end = -1; break;
    case P_CARD_OUT:  // the card leaves (Out, 150) over the starting course
        lay_play(s->card, "Out", t);
        s->end = t + (int)lay_length(s->card, "Out");
        break;
    default: break;
    }
}
// the phase after p for this entry
static int next_phase(const Intro *s, int p) {
    switch (s->entry) {
    case ENTRY_NEW: return p == P_MAPIN ? P_COUNTIN : p == P_COUNTIN ? P_TELOP : p == P_TELOP ? P_KURIBO : p == P_KURIBO ? P_CASTLE
                         : p == P_CASTLE ? P_ANIMB : p == P_ANIMB ? P_JUMP : p == P_JUMP ? P_DOTIN : p == P_DOTIN ? P_WALK
                         : p == P_WALK ? P_GO : p + 1;
    case ENTRY_RESUME: return p == P_MAPIN ? P_TWOJUMP : p == P_TWOJUMP ? P_GO : p + 1;
    case ENTRY_AFTER_CLEAR: return p == P_JUMP ? P_DOTIN : p == P_DOTIN ? P_WALK : p == P_WALK ? P_GO : p + 1;
    default: return p + 1;
    }
}
// A on the map: every demo jumps to its end pose and Mario's Go plays [obs 0x024f6d30]
static void cancel_demo(Intro *s) {
    WorldMap *m = s->map;
    Lay *l = m->l;
    lay_hold_end(l, "MapIn");
    if (s->title) lay_hold_end(s->title, "MapIn");
    static const char *ends[] = {"Telop", "MoveKuribo", "AnimB", "CountIn"};
    for (int i = 0; i < 4; i++)
        if (lay_playing(l, ends[i])) lay_hold_end(l, ends[i]);
    if (s->entry == ENTRY_NEW) lay_hold_end(l, "AnimB");
    lay_stop(l, KURIBO "/Walk"), lay_stop(l, KURIBO "/Stand");
    wm_dot(m, s->to_dot, true, false);
    lay_hold(l, "MoveMario", wm_dot_frame(m, s->to_dot));
    smm1_se("yr_Sweep_GOAL");
    phase(s, P_GO);
}

Intro *intro_new(int entry, int d, const CardInfo *info, int cleared, bool had_continue, Select *handoff) {
    Lay *card = lay_open("Play_CourseIn100Mario_00");
    if (!card) return NULL;
    Intro *s = calloc(1, sizeof *s);
    s->card = card, s->d = d, s->entry = entry, s->cleared = cleared, s->had_continue = had_continue;
    s->card_intro = info->intro;
    card_configure(card, info);
    if (entry == ENTRY_DIRECT) {
        phase(s, P_CARD_IN);
        return s;
    }
    if ((entry == ENTRY_NEW || entry == ENTRY_RESUME) && handoff && handoff->d == d && handoff->maps[d]) {
        s->map = handoff->maps[d], handoff->maps[d] = NULL;
        s->title = handoff->title, handoff->title = NULL;
    } else
        s->map = wm_new(d);
    WorldMap *m = s->map;
    if (!m) {
        phase(s, P_CARD_IN);
        return s;
    }
    if (entry == ENTRY_NEW) {
        s->from_dot = 0, s->to_dot = 0;
        phase(s, P_MAPIN);
    } else if (entry == ENTRY_RESUME) {
        s->from_dot = s->to_dot = cleared;
        wm_set_counters(m, info->lives, cleared);
        phase(s, P_MAPIN);
    } else {  // after a clear (NetWorldMapScene): the map at its MapIn end, Mario jumps in on the cleared dot
        int c = cleared > 0 ? cleared : 1;
        s->from_dot = c - 1, s->to_dot = c < m->count ? c : m->count - 1;
        lay_hold_end(m->l, "MapIn");
        lay_hold_end(m->l, "AnimB");
        for (int i = 0; i < m->count; i++) wm_dot(m, i, i < c, i < c);
        wm_set_counters(m, info->lives, cleared);
        mario_hide(m);
        phase(s, P_JUMP);
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
void intro_card(Intro *s, const CardInfo *info) {
    CardInfo c = *info;
    c.intro = s->card_intro;
    card_configure(s->card, &c);
    if (s->map) wm_set_counters(s->map, info->lives, s->cleared);
}
void intro_started(Intro *s) {
    s->started = true;
    if (s->phase == P_CARD_HOLD) phase(s, P_CARD_OUT);
}
bool intro_full(const Intro *s) { return s->phase < P_CARD_OUT; }
Lay *intro_map_layout(Intro *s) { return s->map && s->phase < P_CARD_IN ? s->map->l : NULL; }
int intro_tick(const Intro *s) { return s->tick; }

int intro_step(Intro *s, const UiIn *in) {
    s->tick++;
    int t = s->tick;
    if (s->phase == P_DONE) return EV_NONE;
    if (s->map && s->phase == P_KURIBO) {  // the Goombas walk from MoveKuribo frame 60, stand again at 156 / 268 / 174
        if (t == s->kuribo_walk) {
            lay_play(s->map->l, KURIBO "/Default", t);
            lay_play_loop(s->map->l, KURIBO "/Walk", t + (int)lay_length(s->map->l, KURIBO "/Default"));
            smm1_se(s->d == 0 ? "SE_KuriboAshiotoShort" : "SE_KuriboAshioto");
        }
        if (t == s->kuribo_stand) lay_stop(s->map->l, KURIBO "/Walk"), lay_stop(s->map->l, KURIBO "/Default"), lay_hold(s->map->l, KURIBO "/Stand", 0);
    }
    if (in->pressed & BT_A) {
        if (s->map && s->phase < P_GO && s->phase != P_MAPIN) cancel_demo(s);
        else if (s->map && s->phase == P_MAPIN) {  // during MapIn: MapIn to its end, the following demos cancel too
            cancel_demo(s);
        } else if (s->phase >= P_CARD_IN && s->phase <= P_CARD_OUT && s->started) {
            s->phase = P_DONE;  // (once the course is there A cuts the card short)
            return EV_DONE;
        }
    }
    if (s->end >= 0 && t >= s->end) {
        if (s->phase == P_WALK && s->map) mario_stand(s->map), lay_hold(s->map->l, "MoveMario", wm_dot_frame(s->map, s->to_dot));
        if (s->phase == P_CARD_IN) {
            phase(s, s->started ? P_CARD_OUT : P_CARD_HOLD);
        } else if (s->phase == P_CARD_OUT) {
            s->phase = P_DONE;
            return EV_DONE;
        } else {
            phase(s, next_phase(s, s->phase));
            if (s->phase == P_CARD_IN) return EV_FETCH;  // the card is up: the course may come
        }
    }
    return EV_NONE;
}
void intro_draw(Intro *s, LQuads *out) {
    int t = s->tick;
    if (s->phase >= P_CARD_IN) {
        lay_draw(s->card, t, out);
        return;
    }
    if (s->map) lay_draw(s->map->l, t, out);
    if (s->title) lay_draw(s->title, t, out);
    if (s->phase == P_FADE && s->map) {  // the iris closes on Mario (fader type 7, centred on him [obs]; its speed [inf])
        float c[2] = {0, 0}, sz[2];
        bool v;
        lay_rect(s->map->l, MARIO_PART, c, sz, &v);
        float k = (float)(s->end - t) / FADE, r = 800 * (k < 0 ? 0 : k);
        iris(out, c[0], c[1], r);
    }
}

// black over everything but a circle of radius r around (cx, cy) (layout units): a generated mask texture (alpha 0
// inside the circle, 1 outside) on the circle's square, black rectangles around it
static int iris_texture(void) {
    static int tex = -2;
    if (tex != -2) return tex;
    enum { N = 256 };
    uint8_t *px = malloc(N * N * 4);
    for (int y = 0; y < N; y++)
        for (int x = 0; x < N; x++) {
            float dx = (x + 0.5f) / N - 0.5f, dy = (y + 0.5f) / N - 0.5f, d = sqrtf(dx * dx + dy * dy) * 2;
            float a = (d - 0.99f) / 0.01f;
            uint8_t *p = px + ((size_t)y * N + x) * 4;
            p[0] = p[1] = p[2] = 255, p[3] = (uint8_t)(a <= 0 ? 0 : a >= 1 ? 255 : a * 255);
        }
    return tex = lyt_tex_add(N, N, px);
}
static void black_quad(LQuads *out, float x0, float y0, float x1, float y1, int tex) {
    if (x1 <= x0 || y0 <= y1) return;
    LQuad q;
    memset(&q, 0, sizeof q);
    float p[4][2] = {{x0, y0}, {x1, y0}, {x0, y1}, {x1, y1}}, uv[4][2] = {{0, 0}, {1, 0}, {0, 1}, {1, 1}};
    memcpy(q.pos, p, sizeof p), memcpy(q.uv0, uv, sizeof uv);
    memset(q.col, 255, sizeof q.col);
    q.black[3] = 0, q.white[3] = 255;  // (black, by the texture's alpha)
    q.tex0 = tex, q.tex1 = -1;
    q.flags = LQ_LINEAR | LQ_BLEND_NORMAL | LQ_COMB_NONE << LQ_COMB_C | LQ_COMB_NONE << LQ_COMB_A;
    if (out->n == out->cap) out->cap = out->cap ? out->cap * 2 : 256, out->q = realloc(out->q, sizeof(LQuad) * (size_t)out->cap);
    out->q[out->n++] = q;
}
static void iris(LQuads *out, float cx, float cy, float r) {
    const float L = -640, R = 640, T = 360, B = -360;
    black_quad(out, L, T, R, cy + r, -1);           // above
    black_quad(out, L, cy - r, R, B, -1);           // below
    black_quad(out, L, cy + r, cx - r, cy - r, -1);  // left
    black_quad(out, cx + r, cy + r, R, cy - r, -1);  // right
    if (r > 0) black_quad(out, cx - r, cy + r, cx + r, cy - r, iris_texture());
}

// ---------------------------------------------------------------- game over (Cmn_GameOver_00, OneTime 320)
GameOver *gameover_new(void) {
    Lay *l = lay_open("Cmn_GameOver_00");
    if (!l) return NULL;
    GameOver *g = calloc(1, sizeof *g);
    g->l = l;
    lay_play(l, "OneTime", 0);
    smm1_bgm("M1_GameOver", false);
    return g;
}
void gameover_free(GameOver *g) {
    if (!g) return;
    lay_free(g->l);
    free(g);
}
int gameover_step(GameOver *g, const UiIn *in) {
    (void)in;  // (a one-shot layout: it runs to its end)
    g->tick++;
    if (g->done) return EV_NONE;
    if (g->tick >= lay_ticks(g->l, "OneTime", 60)) {
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
    o->clear_focus = 1, o->card_out_at = -1;
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
// pause type 2 (a 100 Mario course): Type frame 0, TypeCourseInfo frame 1; the title "100 Mario Challenge (n/total)"
void ov_show_pause(Overlay *o, int cleared, int total, const char *course, const char *maker) {
    if (!o->pause && (o->pause = lay_open("Cmn_PauseMenu_00"))) {
        Lay *p = o->pause;
        lay_hold(p, "Type", 0);
        lay_hold(p, "TypeCourseInfo", 1);
        lay_text_from(p, "L_ExitBtn_00/T_Btn_00", "T_ExitBtn_01");  // "Save & Quit"
        static const char *hide[4] = {"P_YeahCountIcon_00", "P_PlayerCountIcon_00", "L_NationalFlag_00", "L_OptionBtn_00"};
        for (int i = 0; i < 4; i++) lay_show(p, hide[i], false);
    }
    if (!o->pause) return;
    const char *args[2] = {NULL, NULL};
    char a[16], b[16];
    snprintf(a, sizeof a, "%d", cleared + 1), snprintf(b, sizeof b, "%d", total);
    args[0] = a, args[1] = b;
    lay_text_args(o->pause, "T_Mode_00", "T_Mode100Mario_00", args, 2);
    lay_text(o->pause, "T_CourseName_00", course);
    lay_text(o->pause, "T_MiiName_00", maker);
    lay_stop(o->pause, "Out");
    lay_play(o->pause, "In", o->tick);
    smm1_se("M1_Pose");
    o->closing_which = 0;
    set_mode(o, OV_PAUSE);
    ov_pause_select(o, 0);
}
void ov_pause_select(Overlay *o, int i) {
    if (!o->pause) return;
    if (o->pause_sel != i) smm1_se("SE_BongoCursol");
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
// the course clear: ModeType frame 0 (Save & Quit + Next); ClearType frame 1 on the run's last course (Next alone)
void ov_show_clear(Overlay *o, const char *course, const char *maker, bool last) {
    if (!o->clear) o->clear = lay_open("Play_CourseClear_00");
    Lay *c = o->clear;
    if (!c) return;
    o->clear_last = last;
    lay_hold(c, "ModeType", 0);
    lay_hold(c, "ClearType", last ? 1 : 0);
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
    int f = o->clear_last ? 1 : (i > 1 ? 1 : i < 0 ? 0 : i);
    if (f != o->clear_focus) smm1_se("SE_nwk_select");
    o->clear_focus = f;
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
    smm1_se("SE_nwk_decide");
}
// the skip: In (16), then CountDown_00 / 01 / 02 (32 each); at the end of the third the skip fires, then Pass (32)
void ov_skip_begin(Overlay *o) {
    if (!o->skip) o->skip = lay_open("Play_SlidePassIconKey_00");
    Lay *s = o->skip;
    if (!s) return;
    o->closing_which = 0;
    static const char *stop[5] = {"Out", "Pass", "CountDown_00", "CountDown_01", "CountDown_02"};
    for (int i = 0; i < 5; i++) lay_stop(s, stop[i]);
    int t = o->tick, in = (int)lay_length(s, "In");
    lay_play_loop(s, "Default", t);
    lay_play(s, "In", t);
    lay_play(s, "CountDown_00", t + in);
    lay_play(s, "CountDown_01", t + in + 32);
    lay_play(s, "CountDown_02", t + in + 64);
    set_mode(o, OV_SKIP);
}
bool ov_skip_counted(Overlay *o) {
    if (o->mode != OV_SKIP || !o->skip) return false;
    int since = o->tick - o->mode_start, in = (int)lay_length(o->skip, "In");
    if (since == in || since == in + 32 || since == in + 64) smm1_se(since == in + 64 ? "yr_Sys_Cursor_ON_02" : "yr_Sys_Cursor_ON_01");
    return since >= in + 96;
}
void ov_skip_cancel(Overlay *o) {
    if (o->mode != OV_SKIP || !o->skip) return;
    lay_play(o->skip, "Out", o->tick);
    smm1_se("yr_Sys_Cursor_OFF_01");
    o->closing_which = W_SKIP, o->closing_until = o->tick + lay_ticks(o->skip, "Out", 1);
    set_mode(o, OV_NONE);
}
int ov_skip_pass(Overlay *o) {
    if (!o->skip) return 1;
    lay_play(o->skip, "Pass", o->tick);
    return lay_ticks(o->skip, "Pass", 1);
}
// the card after a miss: AnimA at its end, AnimB (70), held until the retry has started, then OutMiss (60)
void ov_show_card(Overlay *o, const CardInfo *c) {
    if (!o->card) o->card = lay_open("Play_CourseIn100Mario_00");
    if (!o->card) return;
    card_configure(o->card, c);
    lay_stop(o->card, "OutMiss");
    lay_hold_end(o->card, "AnimA");
    lay_play(o->card, "AnimB", o->tick);
    smm1_se("M1_SmallMarioJump");
    o->card_out_at = -1;
    set_mode(o, OV_CARD);
}
void ov_card_started(Overlay *o) {
    if (o->mode != OV_CARD || !o->card || o->card_out_at >= 0) return;
    int b = o->mode_start + (int)lay_length(o->card, "AnimB");
    o->card_out_at = o->tick > b ? o->tick : b;
    lay_play(o->card, "OutMiss", o->card_out_at);
}
bool ov_card_out(Overlay *o) { return o->mode == OV_CARD && o->card_out_at >= 0 && o->tick >= o->card_out_at; }
bool ov_card_done(Overlay *o) {
    if (o->mode != OV_CARD || !o->card) return true;
    return o->card_out_at >= 0 && o->tick >= o->card_out_at + (int)lay_length(o->card, "OutMiss");
}
void ov_hide(Overlay *o) {
    o->closing_which = 0;
    set_mode(o, OV_NONE);
}
