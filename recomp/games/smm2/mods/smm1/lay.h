// One playing SMM1 layout with a small timeline (lay.c): animations scheduled on the owner's 60 Hz tick counter and
// bound at their frames before drawing. A port of SMCraft's front::layout::Layout (our own Rust).
#pragma once
#include "lyt.h"

typedef struct {
    char name[64];
    int start;
    float from, to;  // to < -0.5: the animation's end
    bool looping;
    float speed;
} LayTrack;

typedef struct Lay {
    LInst *in;
    LRes *res;
    LayTrack tr[48];
    int ntr;
} Lay;

Lay *lay_open(const char *layout);  // NULL if the dump lacks it
void lay_free(Lay *l);
float lay_length(Lay *l, const char *anim);  // frames, -1 if none
int lay_ticks(Lay *l, const char *anim, int min);
bool lay_has(Lay *l, const char *anim);
void lay_play_ex(Lay *l, const char *anim, int tick, float from, float to, bool looping, float speed);
void lay_play(Lay *l, const char *anim, int tick);       // from its start at tick, holding the last frame
void lay_play_loop(Lay *l, const char *anim, int tick);  // looping
void lay_hold(Lay *l, const char *anim, float frame);    // a fixed frame
void lay_hold_end(Lay *l, const char *anim);
void lay_stop(Lay *l, const char *anim);
bool lay_playing(Lay *l, const char *anim);
bool lay_finished(Lay *l, const char *anim, int tick);
void lay_apply(Lay *l, int tick);
void lay_draw(Lay *l, int tick, LQuads *out);
void lay_text(Lay *l, const char *pane, const char *utf8);
void lay_text32(Lay *l, const char *pane, const uint32_t *s, int n);
bool lay_text_from(Lay *l, const char *pane, const char *label);  // this layout's message `label`
// message `label` with its value tags filled in order from args
bool lay_text_args(Lay *l, const char *pane, const char *label, const char *const *args, int nargs);
void lay_show(Lay *l, const char *pane, bool v);
void lay_visible(Lay *l, const char *pane);  // back to the animations
bool lay_pane(Lay *l, const char *pane, LPaneInfo *out);
// centre / size of a pane's rectangle in layout space (apply the timeline first)
bool lay_rect(Lay *l, const char *pane, float center[2], float size[2], bool *visible);
