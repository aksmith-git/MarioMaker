// SMM1's 100 Mario Challenge screens on the layout player (screens.c): difficulty select, world map + walk + start
// card, game over, and the overlays over a course (pause, clear, skip, the card after a miss). A port of SMCraft's
// front/src/smm1.rs (our own Rust); choreography the data doesn't pin down is marked [inf] there and here, and
// resolved from Block.rpx v208 where research/specs/smm1-100mario-screens.md has it.
#pragma once
#include "../hundred.h"
#include "lay.h"

enum { BT_A = 1, BT_B = 2, BT_X = 4, BT_Y = 8, BT_L = 0x40, BT_R = 0x80, BT_PLUS = 0x400, BT_MINUS = 0x800,
       BT_LEFT = 0x1000, BT_UP = 0x2000, BT_RIGHT = 0x4000, BT_DOWN = 0x8000 };
typedef struct {
    unsigned pressed, held;
} UiIn;

extern const int RUN_LENGTHS[4];

typedef struct {
    char name[66], maker[22], code[24];
    int lives, number, total;
} CardInfo;
void card_configure(Lay *card, const CardInfo *c);

typedef struct {
    Lay *l;
    int d, count, nstops;
    float stops[16];
} WorldMap;
WorldMap *wm_new(int d);
void wm_free(WorldMap *m);
void wm_set_cleared(WorldMap *m, int n);
void wm_set_counters(WorldMap *m, int lives, int cleared);
void wm_place_mario(WorldMap *m, int dot);  // -1: inside the start castle
void wm_stand(WorldMap *m);
int wm_walk(WorldMap *m, float from, float to, int tick);  // returns the arrival tick
float wm_stop(const WorldMap *m, int i);

enum { EV_NONE, EV_EXIT, EV_START, EV_LOCKED, EV_BACK, EV_PLAY, EV_DONE };

typedef struct {
    Lay *title;
    WorldMap *maps[4], *leaving;
    int leave_until, d, focus, tick;
} Select;
Select *select_new(int d, const struct hundred_status *st);
void select_free(Select *s);
int select_step(Select *s, const UiIn *in, const struct hundred_status *st, bool *resume);
void select_draw(Select *s, LQuads *out);

enum { ENTRY_NEW, ENTRY_RESUME, ENTRY_AFTER_CLEAR, ENTRY_DIRECT };
typedef struct {
    WorldMap *map;
    Lay *title, *card;
    int tick, card_start, out_at, walk_start, walk_end;
    float walk_from, walk_to;
    bool finished, hold_card;  // hold_card: keep the card up after its time (waiting for the course)
} Intro;
// takes the select screen's map and title bar (when the same difficulty) for the MapIn that follows
Intro *intro_new(int entry, int d, const CardInfo *info, int cleared, Select *handoff);
void intro_free(Intro *s);
int intro_step(Intro *s, const UiIn *in);
void intro_draw(Intro *s, LQuads *out);

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
    int tick, mode, mode_start, closing_which, closing_until, clear_focus, pause_sel;
    bool clear_last;
    Lay *pause, *clear, *skip, *card;
} Overlay;
void ov_init(Overlay *o);
void ov_free(Overlay *o);
void ov_draw(Overlay *o, LQuads *out);
void ov_show_pause(Overlay *o, const char *mode_line, const char *course, const char *maker);
void ov_pause_select(Overlay *o, int i);
void ov_hide_pause(Overlay *o);
void ov_show_clear(Overlay *o, const char *course, const char *maker, bool last);
void ov_clear_select(Overlay *o, int i);
bool ov_clear_ready(Overlay *o);
void ov_press_clear(Overlay *o);
void ov_skip_begin(Overlay *o);
void ov_skip_cancel(Overlay *o);
int ov_skip_pass(Overlay *o);
void ov_show_card(Overlay *o, const CardInfo *c);
bool ov_card_done(Overlay *o);
void ov_hide(Overlay *o);
