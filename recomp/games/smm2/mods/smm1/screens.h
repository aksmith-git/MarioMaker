// SMM1's 100 Mario Challenge screens on the layout player (screens.c): difficulty select, the world map demos and
// the start card, game over, and the overlays over a course (pause, clear, skip, the card after a miss).
// Choreography from Block.rpx v208 (research/specs/smm1-100mario-screens.md, private): the map's demo state machine
// (MapIn, Telop, the Goombas, AnimB, Mario's jump / walk / Go), the card's In / hold / Out, the select screen's
// tabs and buttons. Started from SMCraft's front/src/smm1.rs (our own Rust); what the binary didn't settle is [inf].
#pragma once
#include "../hundred.h"
#include "lay.h"

enum { BT_A = 1, BT_B = 2, BT_X = 4, BT_Y = 8, BT_L = 0x40, BT_R = 0x80, BT_ZL = 0x100, BT_ZR = 0x200,
       BT_PLUS = 0x400, BT_MINUS = 0x800, BT_LEFT = 0x1000, BT_UP = 0x2000, BT_RIGHT = 0x4000, BT_DOWN = 0x8000 };
typedef struct {
    unsigned pressed, held;
} UiIn;

extern const int RUN_LENGTHS[4];

enum { CARD_STATIC, CARD_NEW_RUN, CARD_RETRY };  // the card's In: none (AnimB at its end), AnimA + AnimB, AnimB
typedef struct {
    char name[66], maker[22], code[24];
    int lives, number, total, intro;
    bool last;  // the run's last course (Type frame 1)
} CardInfo;
void card_configure(Lay *card, const CardInfo *c);

typedef struct {
    Lay *l;
    int d, count, step, base;  // MoveMario frame of dot i: i * step + base
} WorldMap;
WorldMap *wm_new(int d);
void wm_free(WorldMap *m);
void wm_set_counters(WorldMap *m, int lives, int cleared);
float wm_dot_frame(const WorldMap *m, int dot);

enum { EV_NONE, EV_EXIT, EV_START, EV_LOCKED, EV_BACK, EV_PLAY, EV_DONE, EV_FETCH, EV_TAB, EV_FOCUS };

typedef struct {
    Lay *title;
    WorldMap *maps[4], *leaving;
    int leave_until, d, focus, tick, exit_at, bgm_track;
    bool cont;  // this difficulty has a Continue
} Select;
Select *select_new(int d, const struct hundred_status *st);
void select_free(Select *s);
// EV_START (*resume set), EV_EXIT (after the map's Out), EV_TAB (difficulty changed: s->d), EV_FOCUS
int select_step(Select *s, const UiIn *in, const struct hundred_status *st, bool *resume);
void select_draw(Select *s, LQuads *out);

enum { ENTRY_NEW, ENTRY_RESUME, ENTRY_AFTER_CLEAR, ENTRY_DIRECT };
typedef struct Intro Intro;
// takes the select screen's map and title bar (same difficulty) for the MapIn that follows
Intro *intro_new(int entry, int d, const CardInfo *info, int cleared, bool had_continue, Select *handoff);
void intro_free(Intro *s);
void intro_card(Intro *s, const CardInfo *info);  // the course's details as they become known
void intro_started(Intro *s);                     // the course has started: the card may go (Out)
bool intro_full(const Intro *s);                  // covers the game (false during the card's Out)
int intro_step(Intro *s, const UiIn *in);         // EV_DONE when the card's Out has finished
void intro_draw(Intro *s, LQuads *out);
Lay *intro_map_layout(Intro *s);  // the map being shown (NULL once the card is up)
int intro_tick(const Intro *s);

typedef struct {
    Lay *l;
    int tick;
    bool done;
} GameOver;
GameOver *gameover_new(void);
void gameover_free(GameOver *g);
int gameover_step(GameOver *g, const UiIn *in);
void gameover_draw(GameOver *g, LQuads *out);

enum { OV_NONE, OV_PAUSE, OV_CLEAR, OV_CARD, OV_SKIP };
typedef struct {
    int tick, mode, mode_start, closing_which, closing_until, clear_focus, pause_sel, card_out_at;
    bool clear_last;
    Lay *pause, *clear, *skip, *card;
} Overlay;
void ov_init(Overlay *o);
void ov_free(Overlay *o);
void ov_draw(Overlay *o, LQuads *out);
void ov_show_pause(Overlay *o, int cleared, int total, const char *course, const char *maker);
void ov_pause_select(Overlay *o, int i);
void ov_hide_pause(Overlay *o);
void ov_show_clear(Overlay *o, const char *course, const char *maker, bool last);
void ov_clear_select(Overlay *o, int i);
bool ov_clear_ready(Overlay *o);
void ov_press_clear(Overlay *o);
void ov_skip_begin(Overlay *o);
void ov_skip_cancel(Overlay *o);
bool ov_skip_counted(Overlay *o);  // In + the three countdowns are over: the skip fires
int ov_skip_pass(Overlay *o);
void ov_show_card(Overlay *o, const CardInfo *c);  // after a miss: AnimB, held until the retry starts
void ov_card_started(Overlay *o);                 // then OutMiss
bool ov_card_out(Overlay *o);                     // OutMiss is playing (the game shows through)
bool ov_card_done(Overlay *o);
void ov_hide(Overlay *o);
