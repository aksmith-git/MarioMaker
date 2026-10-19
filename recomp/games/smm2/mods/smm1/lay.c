// A playing layout and its timeline (lay.h).
#include "lay.h"

#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

Lay *lay_open(const char *layout) {
    LRes *r = lyt_load(layout);
    if (!r) return NULL;
    Lay *l = calloc(1, sizeof *l);
    l->res = r;
    l->in = lyt_inst(r);
    return l;
}
void lay_free(Lay *l) {
    if (!l) return;
    lyt_inst_free(l->in);
    free(l);
}
float lay_length(Lay *l, const char *anim) { return lyt_anim_length(l->in, anim); }
int lay_ticks(Lay *l, const char *anim, int min) {
    int t = (int)lay_length(l, anim);
    return t > min ? t : min;
}
bool lay_has(Lay *l, const char *anim) { return lay_length(l, anim) >= 0; }

void lay_play_ex(Lay *l, const char *anim, int tick, float from, float to, bool looping, float speed) {
    if (!lay_has(l, anim)) return;
    LayTrack t = {.start = tick, .from = from, .to = to, .looping = looping, .speed = speed};
    snprintf(t.name, sizeof t.name, "%s", anim);
    for (int i = 0; i < l->ntr; i++)
        if (!strcmp(l->tr[i].name, anim)) { l->tr[i] = t; return; }
    if (l->ntr < 48) l->tr[l->ntr++] = t;
}
void lay_play(Lay *l, const char *anim, int tick) { lay_play_ex(l, anim, tick, 0, -1, false, 1); }
void lay_play_loop(Lay *l, const char *anim, int tick) { lay_play_ex(l, anim, tick, 0, -1, true, 1); }
void lay_hold(Lay *l, const char *anim, float frame) { lay_play_ex(l, anim, 0, frame, frame, false, 1); }
void lay_hold_end(Lay *l, const char *anim) { lay_hold(l, anim, lay_length(l, anim)); }
void lay_stop(Lay *l, const char *anim) {
    for (int i = 0; i < l->ntr; i++)
        if (!strcmp(l->tr[i].name, anim)) {
            memmove(&l->tr[i], &l->tr[i + 1], sizeof(LayTrack) * (size_t)(l->ntr - i - 1));
            l->ntr--;
            break;
        }
    lyt_unbind(l->in, anim);
}
bool lay_playing(Lay *l, const char *anim) {
    for (int i = 0; i < l->ntr; i++)
        if (!strcmp(l->tr[i].name, anim)) return true;
    return false;
}
bool lay_finished(Lay *l, const char *anim, int tick) {
    for (int i = 0; i < l->ntr; i++) {
        LayTrack *t = &l->tr[i];
        if (strcmp(t->name, anim)) continue;
        if (t->looping) return false;
        float end = t->to < -0.5f ? lay_length(l, anim) : t->to;
        return (tick - t->start) * t->speed + t->from >= end;
    }
    return false;
}
static float frame_of(const LayTrack *t, float length, int tick) {
    int dt = tick - t->start;
    float f = t->from + (dt > 0 ? dt : 0) * t->speed;
    if (t->looping) return length > 0 ? fmodf(f, length) : 0;
    float end = t->to < -0.5f ? length : t->to;
    return t->speed >= 0 ? (f < end ? f : end) : (f > end ? f : end);
}
void lay_apply(Lay *l, int tick) {
    for (int i = 0; i < l->ntr; i++) lyt_bind(l->in, l->tr[i].name, frame_of(&l->tr[i], lay_length(l, l->tr[i].name), tick));
}
void lay_draw(Lay *l, int tick, LQuads *out) {
    if (!l) return;
    lay_apply(l, tick);
    lyt_draw(l->in, out);
}
void lay_text(Lay *l, const char *pane, const char *utf8) { lyt_set_text8(l->in, pane, utf8); }
void lay_text32(Lay *l, const char *pane, const uint32_t *s, int n) { lyt_set_text(l->in, pane, s, n); }
bool lay_text_from(Lay *l, const char *pane, const char *label) {
    const LMsg *m = lyt_message(l->in, label);
    if (!m) return false;
    lyt_set_text(l->in, pane, m->text, m->n);
    return true;
}
bool lay_text_args(Lay *l, const char *pane, const char *label, const char *const *args, int nargs) {
    const LMsg *m = lyt_message(l->in, label);
    if (!m) return false;
    uint32_t out[512];
    int n = 0, a = 0;
    for (int i = 0; i < m->n && n < 500; i++) {
        if (m->text[i] != LYT_ARG) { out[n++] = m->text[i]; continue; }
        if (a < nargs && args[a]) {
            int k;
            uint32_t *u = lyt_utf8(args[a], &k);
            for (int j = 0; j < k && n < 500; j++) out[n++] = u[j];
            free(u);
        }
        a++;
    }
    lyt_set_text(l->in, pane, out, n);
    return true;
}
void lay_show(Lay *l, const char *pane, bool v) { lyt_set_visible(l->in, pane, v ? 1 : 0); }
void lay_visible(Lay *l, const char *pane) { lyt_set_visible(l->in, pane, -1); }
bool lay_pane(Lay *l, const char *pane, LPaneInfo *out) { return lyt_pane_info(l->in, pane, out); }
bool lay_rect(Lay *l, const char *pane, float center[2], float size[2], bool *visible) {
    float c[4][2], a;
    bool v;
    if (!lyt_pane_global(l->in, pane, c, &a, &v)) return false;
    float lo[2] = {c[0][0], c[0][1]}, hi[2] = {c[0][0], c[0][1]};
    for (int i = 1; i < 4; i++)
        for (int k = 0; k < 2; k++) lo[k] = fminf(lo[k], c[i][k]), hi[k] = fmaxf(hi[k], c[i][k]);
    for (int k = 0; k < 2; k++) center[k] = (lo[k] + hi[k]) / 2, size[k] = hi[k] - lo[k];
    if (visible) *visible = v;
    return true;
}
