// The layout player (lyt.h): a layout's base values, its bound animations at their frames and the caller's
// overrides -> per-pane transform, alpha, visibility -> textured quads (pictures, windows, text, parts).
// A C port of SMCraft's mmk-ui player.rs / text.rs (our own Rust, branch smcraft-archive); the rules and their
// [inf] notes are the same:
//   - pane transform: parent * T(translate + anchor offset) * Rz * Ry * Rx * S(scale); a child sits on its
//     parent's origin point unless its parent-anchor bits (origin >> 4) are set, then at
//     (anchor factor - parent origin factor) * parent size
//   - an "influenced alpha" pane multiplies its alpha into all its descendants (a part pane into its layout)
//   - text: font scale = font size / font cell size; lines advance line_feed * sy + line space
#include "lyt.h"

#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef struct {
    float m[3][4];
} Mtx;
static const Mtx IDENT = {{{1, 0, 0, 0}, {0, 1, 0, 0}, {0, 0, 1, 0}}};
static Mtx mul(const Mtx *a, const Mtx *b) {
    Mtx o;
    for (int r = 0; r < 3; r++)
        for (int c = 0; c < 4; c++) {
            float v = a->m[r][0] * b->m[0][c] + a->m[r][1] * b->m[1][c] + a->m[r][2] * b->m[2][c];
            if (c == 3) v += a->m[r][3];
            o.m[r][c] = v;
        }
    return o;
}
static Mtx scale_m(float sx, float sy) {
    Mtx m = IDENT;
    m.m[0][0] = sx, m.m[1][1] = sy;
    return m;
}
static Mtx srt(const float t[3], const float rd[3], const float s[2]) {
    const float k = 3.14159265358979f / 180;
    float sx = sinf(rd[0] * k), cx = cosf(rd[0] * k), sy = sinf(rd[1] * k), cy = cosf(rd[1] * k);
    float sz = sinf(rd[2] * k), cz = cosf(rd[2] * k);
    float r[3][3] = {{cz * cy, cz * sy * sx - sz * cx, cz * sy * cx + sz * sx},
                     {sz * cy, sz * sy * sx + cz * cx, sz * sy * cx - cz * sx},
                     {-sy, cy * sx, cy * cx}};
    Mtx m;
    for (int i = 0; i < 3; i++) m.m[i][0] = r[i][0] * s[0], m.m[i][1] = r[i][1] * s[1], m.m[i][2] = r[i][2], m.m[i][3] = t[i];
    return m;
}
static void apply(const Mtx *m, float x, float y, float out[2]) {
    out[0] = m->m[0][0] * x + m->m[0][1] * y + m->m[0][3];
    out[1] = m->m[1][0] * x + m->m[1][1] * y + m->m[1][3];
}

typedef struct {
    int anim;
    float frame;
} Bound;
typedef struct {
    bool has_t;
    float t[2];
    float alpha;  // < 0: none
} Ovr;

struct LInst {
    LRes *res;
    Bound *bound;
    int nb, capb;
    int8_t *vis;  // caller visibility per pane: -1 none
    uint32_t **text;
    int *ntext;
    Ovr *ovr;
    LInst **part;
    // from the enclosing part pane's properties
    const LProp **basic;
    const LProp **picovr;
    LRes *owner;
    const LProp **textprop;
};

typedef struct {
    float t[3], r[3], s[2], size[2], alpha;
    bool visible;
    float col[4][4];
} PS;
typedef struct {
    float black[4], white[4];
    LTexSrt srt[3];
    int pattern[3];
} MS;

static void *zalloc(size_t n) { return calloc(1, n ? n : 1); }

static LInst *inst_new(LRes *r);
static LInst *part_instance(LRes *owner, const LPane *p, LRes *sub) {
    if (!sub) return NULL;
    LInst *in = inst_new(sub);
    in->owner = owner;
    for (int k = 0; k < p->prt.nprops; k++) {
        const LProp *pr = &p->prt.props[k];
        int i = lyt_pane_index(sub, pr->name);
        if (i < 0) continue;
        if (pr->basic_usage) in->basic[i] = pr;
        if (pr->pane && pr->pane->kind == LP_PIC) in->picovr[i] = pr;
        if (pr->pane && pr->pane->kind == LP_TXT) in->textprop[i] = pr;
    }
    return in;
}
static LInst *inst_new(LRes *r) {
    LInst *in = zalloc(sizeof *in);
    int n = r->npane;
    in->res = r;
    in->vis = malloc((size_t)(n ? n : 1));
    memset(in->vis, -1, (size_t)(n ? n : 1));
    in->text = zalloc(sizeof(uint32_t *) * (size_t)(n + 1));
    in->ntext = zalloc(sizeof(int) * (size_t)(n + 1));
    in->ovr = zalloc(sizeof(Ovr) * (size_t)(n + 1));
    for (int i = 0; i < n; i++) in->ovr[i].alpha = -1;
    in->part = zalloc(sizeof(LInst *) * (size_t)(n + 1));
    in->basic = zalloc(sizeof(LProp *) * (size_t)(n + 1));
    in->picovr = zalloc(sizeof(LProp *) * (size_t)(n + 1));
    in->textprop = zalloc(sizeof(LProp *) * (size_t)(n + 1));
    for (int i = 0; i < n; i++)
        if (r->pane[i].kind == LP_PRT) in->part[i] = part_instance(r, &r->pane[i], r->part[i]);
    return in;
}
LInst *lyt_inst(LRes *r) { return r ? inst_new(r) : NULL; }
void lyt_inst_free(LInst *in) {
    if (!in) return;
    for (int i = 0; i < in->res->npane; i++) {
        lyt_inst_free(in->part[i]);
        free(in->text[i]);
    }
    free(in->bound), free(in->vis), free(in->text), free(in->ntext), free(in->ovr), free(in->part);
    free(in->basic), free(in->picovr), free(in->textprop);
    free(in);
}
const LRes *lyt_inst_res(const LInst *i) { return i->res; }

// "[Part/…/]Name": the instance and the last name
static LInst *resolve(LInst *in, const char *path, const char **last) {
    char buf[256];
    snprintf(buf, sizeof buf, "%s", path);
    char *s = buf, *slash;
    const char *tail = path;
    while ((slash = strchr(s, '/'))) {
        *slash = 0;
        int i = lyt_pane_index(in->res, s);
        if (i < 0 || !in->part[i]) return NULL;
        in = in->part[i];
        tail += (slash - s) + 1;
        s = slash + 1;
    }
    *last = tail;
    return in;
}

bool lyt_bind(LInst *in, const char *path, float frame) {
    const char *name;
    LInst *p = resolve(in, path, &name);
    if (!p) return false;
    int a = lyt_anim_index(p->res, name);
    if (a < 0) return false;
    for (int k = 0; k < p->nb; k++)
        if (p->bound[k].anim == a) { p->bound[k].frame = frame; return true; }
    if (p->nb == p->capb) p->capb = p->capb ? p->capb * 2 : 8, p->bound = realloc(p->bound, sizeof(Bound) * (size_t)p->capb);
    p->bound[p->nb++] = (Bound){a, frame};
    return true;
}
void lyt_unbind(LInst *in, const char *path) {
    const char *name;
    LInst *p = resolve(in, path, &name);
    if (!p) return;
    int a = lyt_anim_index(p->res, name);
    for (int k = 0; k < p->nb; k++)
        if (p->bound[k].anim == a) {
            memmove(&p->bound[k], &p->bound[k + 1], sizeof(Bound) * (size_t)(p->nb - k - 1));
            p->nb--;
            return;
        }
}
void lyt_unbind_all(LInst *in) {
    in->nb = 0;
    for (int i = 0; i < in->res->npane; i++)
        if (in->part[i]) lyt_unbind_all(in->part[i]);
}
float lyt_anim_length(LInst *in, const char *path) {
    const char *name;
    LInst *p = resolve(in, path, &name);
    if (!p) return -1;
    int a = lyt_anim_index(p->res, name);
    return a < 0 ? -1 : p->res->anim[a].frames;
}
bool lyt_set_text(LInst *in, const char *path, const uint32_t *s, int n) {
    const char *name;
    LInst *p = resolve(in, path, &name);
    if (!p) return false;
    int i = lyt_pane_index(p->res, name);
    if (i < 0 || p->res->pane[i].kind != LP_TXT) return false;
    free(p->text[i]);
    p->text[i] = malloc(sizeof(uint32_t) * (size_t)(n + 1));
    memcpy(p->text[i], s, sizeof(uint32_t) * (size_t)n);
    p->ntext[i] = n;
    return true;
}
bool lyt_clear_text(LInst *in, const char *path) {
    const char *name;
    LInst *p = resolve(in, path, &name);
    if (!p) return false;
    int i = lyt_pane_index(p->res, name);
    if (i < 0) return false;
    free(p->text[i]);
    p->text[i] = NULL;
    return true;
}
uint32_t *lyt_utf8(const char *s, int *n) {
    size_t len = strlen(s);
    uint32_t *o = malloc(sizeof(uint32_t) * (len + 1));
    int k = 0;
    for (size_t i = 0; i < len;) {
        uint8_t c = (uint8_t)s[i];
        uint32_t u;
        int extra;
        if (c < 0x80) u = c, extra = 0;
        else if ((c & 0xE0) == 0xC0) u = c & 0x1F, extra = 1;
        else if ((c & 0xF0) == 0xE0) u = c & 0x0F, extra = 2;
        else if ((c & 0xF8) == 0xF0) u = c & 0x07, extra = 3;
        else { i++; continue; }
        i++;
        for (int e = 0; e < extra && i < len; e++, i++) u = u << 6 | ((uint8_t)s[i] & 0x3F);
        o[k++] = u;
    }
    *n = k;
    return o;
}
bool lyt_set_text8(LInst *in, const char *path, const char *utf8) {
    int n;
    uint32_t *u = lyt_utf8(utf8, &n);
    bool ok = lyt_set_text(in, path, u, n);
    free(u);
    return ok;
}
bool lyt_set_visible(LInst *in, const char *path, int v) {
    const char *name;
    LInst *p = resolve(in, path, &name);
    if (!p) return false;
    int i = lyt_pane_index(p->res, name);
    if (i < 0) return false;
    p->vis[i] = (int8_t)(v < 0 ? -1 : v ? 1 : 0);
    return true;
}
bool lyt_set_pane(LInst *in, const char *path, const float *xy, float alpha) {
    const char *name;
    LInst *p = resolve(in, path, &name);
    if (!p) return false;
    int i = lyt_pane_index(p->res, name);
    if (i < 0) return false;
    p->ovr[i].has_t = xy != NULL;
    if (xy) p->ovr[i].t[0] = xy[0], p->ovr[i].t[1] = xy[1];
    p->ovr[i].alpha = alpha;
    return true;
}

// ---------------------------------------------------------------- evaluation
static void base_mats(const LRes *r, MS *ms) {
    for (int m = 0; m < r->nmat; m++) {
        for (int c = 0; c < 4; c++) ms[m].black[c] = r->mat[m].black[c], ms[m].white[c] = r->mat[m].white[c];
        memcpy(ms[m].srt, r->mat[m].srt, sizeof ms[m].srt);
        ms[m].pattern[0] = ms[m].pattern[1] = ms[m].pattern[2] = -1;
    }
}

static void states(const LInst *in, PS *ps, MS *ms) {
    const LRes *r = in->res;
    for (int i = 0; i < r->npane; i++) {
        const LPane *p = &r->pane[i];
        PS *s = &ps[i];
        memcpy(s->t, p->t, sizeof s->t), memcpy(s->r, p->r, sizeof s->r), memcpy(s->s, p->s, sizeof s->s);
        memcpy(s->size, p->size, sizeof s->size);
        s->alpha = p->alpha;
        s->visible = p->flags & 1;
        const uint8_t (*vc)[4] = p->kind == LP_PIC ? p->pic.vc : p->kind == LP_WND ? p->wnd.content.vc : NULL;
        for (int k = 0; k < 4; k++)
            for (int c = 0; c < 4; c++) s->col[k][c] = vc ? vc[k][c] : 255;
        const LProp *b = in->basic[i];
        if (b) {
            uint8_t u = b->basic_usage;
            if (u & 1) s->visible = u & 2;
            if (b->has_basic) {
                if (u & 8) memcpy(s->t, b->t, sizeof s->t);
                if (u & 16) memcpy(s->size, b->size, sizeof s->size);
                if (u & 32) memcpy(s->s, b->s, sizeof s->s);
                if (u & 64) memcpy(s->r, b->r, sizeof s->r);
                if (u & 128) s->alpha = b->alpha;
            }
        }
    }
    base_mats(r, ms);
    for (int k = 0; k < in->nb; k++) {
        const LAnim *a = &r->anim[in->bound[k].anim];
        float f = in->bound[k].frame;
        for (int c = 0; c < a->nc; c++) {
            const LACont *ct = &a->c[c];
            if (ct->target < 0) continue;
            for (int t = 0; t < ct->nt; t++) {
                const LATag *tg = &ct->t[t];
                for (int e = 0; e < tg->ne; e++) {
                    const LAEntry *en = &tg->e[e];
                    float v = lyt_eval(en, f);
                    int tr = en->target, ix = en->index;
                    if (ct->kind == 1) {
                        MS *m = &ms[ct->target];
                        if (!memcmp(tg->magic, "FLMC", 4)) {
                            if (tr < 4) m->black[tr] = v;
                            else if (tr < 8) m->white[tr - 4] = v;
                        } else if (!memcmp(tg->magic, "FLTS", 4) && ix < 3) {
                            LTexSrt *s = &m->srt[ix];
                            if (tr == 0) s->t[0] = v;
                            else if (tr == 1) s->t[1] = v;
                            else if (tr == 2) s->r = v;
                            else if (tr == 3) s->s[0] = v;
                            else if (tr == 4) s->s[1] = v;
                        } else if (!memcmp(tg->magic, "FLTP", 4) && ix < 3) {
                            int ti = v < 0 ? 0 : (int)v;
                            if (ti < a->ntex && a->tex[ti] >= 0) m->pattern[ix] = a->tex[ti];
                        }
                    } else {
                        PS *s = &ps[ct->target];
                        if (!memcmp(tg->magic, "FLPA", 4)) {
                            if (tr <= 2) s->t[tr] = v;
                            else if (tr <= 5) s->r[tr - 3] = v;
                            else if (tr <= 7) s->s[tr - 6] = v;
                            else if (tr <= 9) s->size[tr - 8] = v;
                        } else if (!memcmp(tg->magic, "FLVI", 4))
                            s->visible = v >= 0.5f;
                        else if (!memcmp(tg->magic, "FLVC", 4)) {
                            if (tr < 16) s->col[tr / 4][tr % 4] = v;
                            else if (tr == 16) s->alpha = v;
                        }
                    }
                }
            }
        }
    }
    for (int i = 0; i < r->npane; i++) {
        if (in->ovr[i].has_t) ps[i].t[0] = in->ovr[i].t[0], ps[i].t[1] = in->ovr[i].t[1];
        if (in->ovr[i].alpha >= 0) ps[i].alpha = in->ovr[i].alpha;
    }
}

static float factor(int code) { return (code & 3) == 1 ? 0.0f : (code & 3) == 2 ? 1.0f : 0.5f; }

typedef struct {
    PS *ps;
    MS *ms;
    Mtx *mtx;
    float *alpha, *passed;
    bool *visible;
} Pass;

static void pass_free(Pass *p) { free(p->ps), free(p->ms), free(p->mtx), free(p->alpha), free(p->passed), free(p->visible); }

static void pass(const LInst *in, const Mtx *base, float base_alpha, Pass *P) {
    const LRes *r = in->res;
    int n = r->npane;
    P->ps = zalloc(sizeof(PS) * (size_t)(n + 1));
    P->ms = zalloc(sizeof(MS) * (size_t)(r->nmat + 1));
    P->mtx = zalloc(sizeof(Mtx) * (size_t)(n + 1));
    P->alpha = zalloc(sizeof(float) * (size_t)(n + 1));
    P->passed = zalloc(sizeof(float) * (size_t)(n + 1));
    P->visible = zalloc(sizeof(bool) * (size_t)(n + 1));
    states(in, P->ps, P->ms);
    for (int i = 0; i < n; i++)
        if (in->vis[i] >= 0) P->ps[i].visible = in->vis[i];
    for (int i = 0; i < n; i++) {
        const LPane *p = &r->pane[i];
        const PS *s = &P->ps[i];
        int par = p->parent;
        Mtx pm = par >= 0 ? P->mtx[par] : *base;
        float inh = par >= 0 ? P->passed[par] : base_alpha, an[2] = {0, 0};
        bool pv = par >= 0 ? P->visible[par] : true;
        if (par >= 0 && (p->origin >> 4)) {
            const LPane *pp = &r->pane[par];
            an[0] = (factor(p->origin >> 4) - factor(pp->origin)) * P->ps[par].size[0];
            an[1] = (factor(pp->origin >> 2) - factor(p->origin >> 6)) * P->ps[par].size[1];
        }
        float t[3] = {s->t[0] + an[0], s->t[1] + an[1], s->t[2]};
        Mtx l = srt(t, s->r, s->s);
        P->mtx[i] = mul(&pm, &l);
        P->alpha[i] = s->alpha / 255.0f * inh;
        P->passed[i] = (p->flags & 2) ? P->alpha[i] : inh;
        P->visible[i] = pv && s->visible;
    }
}

bool lyt_pane_info(LInst *in, const char *path, LPaneInfo *out) {
    const char *name;
    LInst *p = resolve(in, path, &name);
    if (!p) return false;
    int i = lyt_pane_index(p->res, name);
    if (i < 0) return false;
    PS *ps = zalloc(sizeof(PS) * (size_t)(p->res->npane + 1));
    MS *ms = zalloc(sizeof(MS) * (size_t)(p->res->nmat + 1));
    states(p, ps, ms);
    memcpy(out->t, ps[i].t, sizeof out->t), memcpy(out->r, ps[i].r, sizeof out->r);
    memcpy(out->s, ps[i].s, sizeof out->s), memcpy(out->size, ps[i].size, sizeof out->size);
    out->alpha = ps[i].alpha;
    out->visible = p->vis[i] >= 0 ? p->vis[i] : ps[i].visible;
    free(ps), free(ms);
    return true;
}

static void pane_rect(int origin, const float size[2], float out[4]) {
    out[2] = size[0], out[3] = size[1];
    out[0] = -size[0] * factor(origin);
    out[1] = size[1] * factor(origin >> 2);
}

static bool global_at(LInst *in, const char *path, const Mtx *base, float ba, bool bv, float c[4][2], float *alpha, bool *vis) {
    char first[64];
    const char *slash = strchr(path, '/');
    size_t len = slash ? (size_t)(slash - path) : strlen(path);
    if (len >= sizeof first) return false;
    memcpy(first, path, len), first[len] = 0;
    int i = lyt_pane_index(in->res, first);
    if (i < 0) return false;
    Pass P;
    pass(in, base, ba, &P);
    bool ok = true;
    if (!slash) {
        float rc[4];
        pane_rect(in->res->pane[i].origin, P.ps[i].size, rc);
        float l = rc[0], t = rc[1], w = rc[2], h = rc[3];
        apply(&P.mtx[i], l, t, c[0]), apply(&P.mtx[i], l + w, t, c[1]);
        apply(&P.mtx[i], l, t - h, c[2]), apply(&P.mtx[i], l + w, t - h, c[3]);
        *alpha = P.alpha[i];
        *vis = P.visible[i] && bv;
    } else if (in->part[i]) {
        Mtx sm = scale_m(in->res->pane[i].prt.magnify[0], in->res->pane[i].prt.magnify[1]);
        Mtx m = mul(&P.mtx[i], &sm);
        ok = global_at(in->part[i], slash + 1, &m, P.passed[i], P.visible[i] && bv, c, alpha, vis);
    } else
        ok = false;
    pass_free(&P);
    return ok;
}
bool lyt_pane_global(LInst *in, const char *path, float corners[4][2], float *alpha, bool *visible) {
    float a;
    bool v;
    bool ok = global_at(in, path, &IDENT, 1, true, corners, &a, &v);
    if (alpha) *alpha = a;
    if (visible) *visible = v;
    return ok;
}

const LMsg *lyt_message(LInst *in, const char *label) {
    const LMsg *m = lyt_msbt_get(&in->res->msg, label);
    for (int i = 0; !m && i < in->res->npane; i++)
        if (in->part[i]) m = lyt_message(in->part[i], label);
    return m;
}

// ---------------------------------------------------------------- quads
static uint8_t u8c(float v) { return v <= 0 ? 0 : v >= 255 ? 255 : (uint8_t)lroundf(v); }

static void push(LQuads *o, const LQuad *q) {
    if (o->n == o->cap) o->cap = o->cap ? o->cap * 2 : 256, o->q = realloc(o->q, sizeof(LQuad) * (size_t)o->cap);
    o->q[o->n++] = *q;
}
void lyt_quads_reset(LQuads *q) { q->n = 0; }

typedef struct {
    const LRes *res;
    const MS *ms;
    float pane_size[2];
    bool has_shared;
    uint8_t sh_black[4], sh_white[4];  // a window's "one material for all frames" colours
} Ctx;

static const float UNIT_UV[4][2] = {{0, 0}, {1, 0}, {0, 1}, {1, 1}};

static void apply_srt(float uv[4][2], const LTexSrt *s) {
    const float k = 3.14159265358979f / 180;
    float sn = sinf(s->r * k), cs = cosf(s->r * k);
    for (int i = 0; i < 4; i++) {
        float x = (uv[i][0] - 0.5f) * s->s[0], y = (uv[i][1] - 0.5f) * s->s[1];
        uv[i][0] = cs * x - sn * y + 0.5f + s->t[0];
        uv[i][1] = sn * x + cs * y + 0.5f + s->t[1];
    }
}

static int mat_texture(const Ctx *c, const LMat *m, const MS *ms, int map) {
    if (map < 3 && ms->pattern[map] >= 0) return ms->pattern[map];
    if (map >= m->ntex) return -1;
    int slot = m->tex[map].tex;
    return slot < c->res->ntex ? c->res->tex[slot] : -1;
}

static void mat_uvs(const Ctx *c, const LMat *m, const MS *ms, int map, int nsets, const float (*sets)[4][2],
                    const float local[4][2], const float global[4][2], float out[4][2]) {
    int proj = 0;
    for (int g = 0; g < map && g < m->ngen; g++)
        if (m->gen_src[g] >= 3) proj++;
    int src = map < m->ngen ? m->gen_src[map] : -1;
    if (src >= 0 && src <= 2) {
        if (src < nsets) memcpy(out, sets[src], sizeof(float) * 8);
        else memcpy(out, UNIT_UV, sizeof(float) * 8);
    } else if (src >= 3) {
        const LProj *p = proj < m->nproj ? &m->proj[proj] : NULL;
        float sx = p && p->s[0] != 0 ? p->s[0] : 1, sy = p && p->s[1] != 0 ? p->s[1] : 1;
        float tx = p ? p->t[0] : 0, ty = p ? p->t[1] : 0;
        const float (*pts)[2] = src == 3 ? global : local;
        float w = src == 3 ? c->res->w : c->pane_size[0], h = src == 3 ? c->res->h : c->pane_size[1];
        if (w < 1e-6f) w = 1e-6f;
        if (h < 1e-6f) h = 1e-6f;
        for (int i = 0; i < 4; i++) {
            out[i][0] = 0.5f + (pts[i][0] / w - tx) / sx;
            out[i][1] = 0.5f - (pts[i][1] / h - ty) / sy;
        }
    } else {
        if (map < nsets) memcpy(out, sets[map], sizeof(float) * 8);
        else if (nsets) memcpy(out, sets[0], sizeof(float) * 8);
        else memcpy(out, UNIT_UV, sizeof(float) * 8);
    }
    if (map < m->nsrt) apply_srt(out, &ms->srt[map]);
}

static uint32_t blend_flags(const LMat *m) {
    if (!m->has_blend) return LQ_BLEND_NORMAL;
    return ((uint32_t)(m->blend_op & 15) << LQ_BLEND_OP) | ((uint32_t)(m->blend_src & 15) << LQ_BLEND_SRC) |
           ((uint32_t)(m->blend_dst & 15) << LQ_BLEND_DST);
}

// one quad of material `mat` over rect (l, t, w, h) in the pane's space
static void quad(const Ctx *c, int mat, int nsets, const float (*sets)[4][2], const float rc[4], const Mtx *mtx,
                 const uint8_t col[4][4], const float (*uv_override)[2], LQuads *out) {
    float l = rc[0], t = rc[1], w = rc[2], h = rc[3];
    if (w == 0 || h == 0 || mat >= c->res->nmat) return;
    const LMat *m = &c->res->mat[mat];
    const MS *ms = &c->ms[mat];
    LQuad q;
    memset(&q, 0, sizeof q);
    float local[4][2] = {{l, t}, {l + w, t}, {l, t - h}, {l + w, t - h}};
    for (int i = 0; i < 4; i++) apply(mtx, local[i][0], local[i][1], q.pos[i]);
    if (c->has_shared) memcpy(q.black, c->sh_black, 4), memcpy(q.white, c->sh_white, 4);
    else
        for (int k = 0; k < 4; k++) q.black[k] = u8c(ms->black[k]), q.white[k] = u8c(ms->white[k]);
    if (m->texture_only && !c->has_shared) memset(q.black, 0, 4), memset(q.white, 255, 4);
    q.tex0 = mat_texture(c, m, ms, 0);
    q.tex1 = m->ntex > 1 ? mat_texture(c, m, ms, 1) : -1;
    uint32_t fl = blend_flags(m);
    if (m->ntex > 0) {
        fl |= (uint32_t)(m->tex[0].wrap_s & 3) << LQ_WRAP_S0 | (uint32_t)(m->tex[0].wrap_t & 3) << LQ_WRAP_T0;
        if (m->tex[0].mag_f == 1 || m->tex[0].min_f == 1) fl |= LQ_LINEAR;
    }
    if (m->ntex > 1) fl |= (uint32_t)(m->tex[1].wrap_s & 3) << LQ_WRAP_S1 | (uint32_t)(m->tex[1].wrap_t & 3) << LQ_WRAP_T1;
    uint32_t cc = LQ_COMB_NONE, ca = LQ_COMB_NONE;
    if (q.tex1 >= 0) {
        if (m->ntev > 0) cc = m->tev_c[0] & 15, ca = m->tev_a[0] & 15;
        else cc = ca = 1;
    }
    fl |= cc << LQ_COMB_C | ca << LQ_COMB_A;
    if (m->ntex == 0) {
        q.tex0 = q.tex1 = -1;
        memcpy(q.uv0, UNIT_UV, sizeof q.uv0), memcpy(q.uv1, UNIT_UV, sizeof q.uv1);
    } else {
        mat_uvs(c, m, ms, 0, nsets, sets, (const float (*)[2])local, (const float (*)[2])q.pos, q.uv0);
        if (q.tex1 >= 0) mat_uvs(c, m, ms, 1, nsets, sets, (const float (*)[2])local, (const float (*)[2])q.pos, q.uv1);
        else memcpy(q.uv1, UNIT_UV, sizeof q.uv1);
        if (uv_override) memcpy(q.uv0, uv_override, sizeof q.uv0);
    }
    memcpy(q.col, col, sizeof q.col);
    q.flags = fl;
    push(out, &q);
}

static void vcolors(const float c[4][4], float alpha, uint8_t out[4][4]) {
    for (int k = 0; k < 4; k++) {
        for (int i = 0; i < 3; i++) out[k][i] = u8c(c[k][i]);
        out[k][3] = u8c(c[k][3] * alpha);
    }
}

static void flip_uv(const float in[4][2], int flip, float out[4][2]) {
    static const int map[6][4] = {{0, 1, 2, 3}, {1, 0, 3, 2}, {2, 3, 0, 1}, {2, 0, 3, 1}, {3, 2, 1, 0}, {1, 3, 0, 2}};
    const int *m = map[flip >= 0 && flip <= 5 ? flip : 0];
    for (int i = 0; i < 4; i++) out[i][0] = in[m[i]][0], out[i][1] = in[m[i]][1];
}
static void uv_rect(float u0, float v0, float u1, float v1, float out[4][2]) {
    out[0][0] = u0, out[0][1] = v0, out[1][0] = u1, out[1][1] = v0, out[2][0] = u0, out[2][1] = v1, out[3][0] = u1, out[3][1] = v1;
}

static void tex_size(const Ctx *c, int mat, float *w, float *h) {
    *w = *h = 1;
    if (mat >= c->res->nmat) return;
    const LTex *t = lyt_tex(mat_texture(c, &c->res->mat[mat], &c->ms[mat], 0));
    if (t) *w = t->w > 0 ? (float)t->w : 1, *h = t->h > 0 ? (float)t->h : 1;
}

static void content_quad(Ctx *c, const LWnd *w, const float rc[4], const Mtx *mtx, const uint8_t col[4][4], LQuads *out) {
    bool texonly = w->content.mat < c->res->nmat && c->res->mat[w->content.mat].texture_only;
    bool saved = c->has_shared;
    if (!texonly) c->has_shared = false;
    quad(c, w->content.mat, w->content.nuv, (const float (*)[4][2])w->content.uv, rc, mtx, col, NULL, out);
    c->has_shared = saved;
}

static void window(Ctx *c, const LWnd *w, const float r[4], const Mtx *mtx, const uint8_t ccol[4][4], float alpha, LQuads *out) {
    float l = r[0], t = r[1], ww = r[2], wh = r[3];
    float fl = w->fsz[0], fr = w->fsz[1], ft = w->fsz[2], fb = w->fsz[3];
    uint8_t fcol[4][4];
    if (w->flags & 2) memcpy(fcol, ccol, sizeof fcol);
    else
        for (int k = 0; k < 4; k++) fcol[k][0] = fcol[k][1] = fcol[k][2] = 255, fcol[k][3] = u8c(255 * alpha);
    c->has_shared = false;
    if ((w->flags & 1) && w->nframes > 0 && w->fmat[0] < c->res->nmat) {
        const MS *ms = &c->ms[w->fmat[0]];
        c->has_shared = true;
        for (int k = 0; k < 4; k++) c->sh_black[k] = u8c(ms->black[k]), c->sh_white[k] = u8c(ms->white[k]);
    }
    int kind = (w->flags >> 2) & 3;
    bool draw_content = !(w->flags & 0x10) && kind != 2;
    float il = w->infl[0], ir = w->infl[1], it = w->infl[2], ib = w->infl[3];
    float uv[4][2], fuv[4][2];
#define FRAME(i) ((i) < w->nframes ? (i) : 0)
    if (kind == 0) {
        if (draw_content) {
            float rc[4] = {l + fl - il, t - ft + it, ww - fl - fr + il + ir, wh - ft - fb + it + ib};
            content_quad(c, w, rc, mtx, ccol, out);
        }
        if (w->nframes == 1) {
            int m = w->fmat[0], fp = w->fflip[0];
            float iw = ww - fl - fr, ih = wh - ft - fb;
            struct { float rc[4], u[4]; } parts[8] = {
                {{l, t, fl, ft}, {0, 0, 1, 1}}, {{l + ww - fr, t, fr, ft}, {1, 0, 0, 1}},
                {{l, t - wh + fb, fl, fb}, {0, 1, 1, 0}}, {{l + ww - fr, t - wh + fb, fr, fb}, {1, 1, 0, 0}},
                {{l + fl, t, iw, ft}, {1, 0, 1, 1}}, {{l + fl, t - wh + fb, iw, fb}, {1, 1, 1, 0}},
                {{l, t - ft, fl, ih}, {0, 1, 1, 1}}, {{l + ww - fr, t - ft, fr, ih}, {1, 1, 0, 1}}};
            for (int k = 0; k < 8; k++) {
                uv_rect(parts[k].u[0], parts[k].u[1], parts[k].u[2], parts[k].u[3], uv);
                flip_uv((const float (*)[2])uv, fp, fuv);
                quad(c, m, 0, NULL, parts[k].rc, mtx, (const uint8_t (*)[4])fcol, (const float (*)[2])fuv, out);
            }
        } else if (w->nframes == 4) {
            float tw0, th0, tw1, th1, tw2, th2, tw3, th3;
            tex_size(c, w->fmat[0], &tw0, &th0), tex_size(c, w->fmat[1], &tw1, &th1);
            tex_size(c, w->fmat[2], &tw2, &th2), tex_size(c, w->fmat[3], &tw3, &th3);
            struct { float rc[4], u[4]; } q4[4] = {
                {{l, t, ww - fr, ft}, {0, 0, (ww - fr) / tw0, 1}},
                {{l + ww - fr, t, fr, wh - fb}, {0, 0, 1, (wh - fb) / th1}},
                {{l, t - ft, fl, wh - ft}, {0, 1 - (wh - ft) / th2, 1, 1}},
                {{l + fl, t - wh + fb, ww - fl, fb}, {1 - (ww - fl) / tw3, 0, 1, 1}}};
            for (int k = 0; k < 4; k++) {
                uv_rect(q4[k].u[0], q4[k].u[1], q4[k].u[2], q4[k].u[3], uv);
                flip_uv((const float (*)[2])uv, w->fflip[k], fuv);
                quad(c, w->fmat[k], 0, NULL, q4[k].rc, mtx, (const uint8_t (*)[4])fcol, (const float (*)[2])fuv, out);
            }
        } else if (w->nframes == 8) {
            float iw = ww - fl - fr, ih = wh - ft - fb;
            float rects[8][4] = {{l, t, fl, ft}, {l + ww - fr, t, fr, ft}, {l, t - wh + fb, fl, fb},
                                 {l + ww - fr, t - wh + fb, fr, fb}, {l + fl, t, iw, ft}, {l + fl, t - wh + fb, iw, fb},
                                 {l, t - ft, fl, ih}, {l + ww - fr, t - ft, fr, ih}};
            for (int k = 0; k < 8; k++) {
                flip_uv(UNIT_UV, w->fflip[k], fuv);
                quad(c, w->fmat[k], 0, NULL, rects[k], mtx, (const uint8_t (*)[4])fcol, (const float (*)[2])fuv, out);
            }
        }
    } else {
        int lm = w->fmat[0], lf = w->fflip[0], rm, rf;
        if (w->nframes > 1) rm = w->fmat[1], rf = w->fflip[1];
        else rm = lm, rf = lf == 1 ? 0 : 1;
        if (draw_content) {
            float rc[4] = {l + fl - il, t + it, ww - fl - fr + il + ir, wh + it + ib};
            content_quad(c, w, rc, mtx, ccol, out);
            float lr[4] = {l, t, fl, wh}, rr[4] = {l + ww - fr, t, fr, wh};
            flip_uv(UNIT_UV, lf, fuv);
            quad(c, lm, 0, NULL, lr, mtx, (const uint8_t (*)[4])fcol, (const float (*)[2])fuv, out);
            flip_uv(UNIT_UV, rf, fuv);
            quad(c, rm, 0, NULL, rr, mtx, (const uint8_t (*)[4])fcol, (const float (*)[2])fuv, out);
        } else {
            float tw, th;
            tex_size(c, lm, &tw, &th);
            float lw = ww - fr, lr[4] = {l, t, lw, wh}, rr[4] = {l + lw, t, fr, wh};
            uv_rect(0, 0, lw / tw, 1, uv);
            flip_uv((const float (*)[2])uv, lf, fuv);
            quad(c, lm, 0, NULL, lr, mtx, (const uint8_t (*)[4])fcol, (const float (*)[2])fuv, out);
            flip_uv(UNIT_UV, rf, fuv);
            quad(c, rm, 0, NULL, rr, mtx, (const uint8_t (*)[4])fcol, (const float (*)[2])fuv, out);
        }
    }
#undef FRAME
    c->has_shared = false;
}

// ---------------------------------------------------------------- text
static const LFont *for_char(const LFont *f, uint32_t ch) {
    if (f->fallback && !lyt_font_has(f, ch) && lyt_font_has(f->fallback, ch)) return f->fallback;
    return f;
}
static uint16_t glyph_of(const LFont *f, uint32_t ch) { return lyt_font_has(f, ch) ? f->cmap[ch] : f->alter; }
static LCharW cw_of(const LFont *f, uint16_t g) { return g < f->nglyphs ? f->widths[g] : f->defw; }
static bool is_control(uint32_t c) { return c < 0x20 || (c >= 0x7F && c < 0xA0); }

static float line_width(const LFont *font, const uint32_t *s, int n, const float size[2], float cspace) {
    float w = 0;
    int k = 0;
    for (int i = 0; i < n; i++) {
        if (is_control(s[i])) continue;
        const LFont *f = for_char(font, s[i]);
        w += cw_of(f, glyph_of(f, s[i])).char_w * (size[0] / (f->width ? f->width : 1));
        k++;
    }
    if (k > 1) w += cspace * (float)(k - 1);
    return w;
}

static void text_emit(const LFont *font, const LTxt *t, const uint32_t *s, int n, const float rc[4], const Mtx *mtx,
                      float alpha, const uint8_t black[4], const uint8_t white[4], uint32_t blend, LQuads *out) {
    float l = rc[0], top = rc[1], w = rc[2], h = rc[3];
    float sy = t->fsize[1] / (font->height ? font->height : 1);
    float line_h = font->line_feed * sy + t->lspace;
    int nlines = 1;
    for (int i = 0; i < n; i++) nlines += s[i] == '\n';
    int *starts = malloc(sizeof(int) * (size_t)(nlines + 1)), *lens = malloc(sizeof(int) * (size_t)(nlines + 1));
    float *widths = malloc(sizeof(float) * (size_t)(nlines + 1)), block_w = 0;
    for (int i = 0, ln = 0, st = 0; i <= n; i++)
        if (i == n || s[i] == '\n') {
            starts[ln] = st, lens[ln] = i - st;
            widths[ln] = line_width(font, s + st, i - st, t->fsize, t->cspace);
            if (widths[ln] > block_w) block_w = widths[ln];
            ln++, st = i + 1;
        }
    float block_h = nlines * line_h - t->lspace;
    float bx = l + (w - block_w) * factor(t->pos), by = top - (h - block_h) * factor(t->pos >> 2);
    float lf = t->lalign == 1 ? 0 : t->lalign == 2 ? 0.5f : t->lalign == 3 ? 1 : factor(t->pos);
    uint32_t flags = blend | LQ_LINEAR | LQ_COMB_NONE << LQ_COMB_C | LQ_COMB_NONE << LQ_COMB_A;
    float ascent = font->ascent * sy;
    for (int pass_i = (t->flags & 1) ? 0 : 1; pass_i < 2; pass_i++) {
        bool shadow = pass_i == 0;
        float off[2] = {shadow ? t->sh_off[0] : 0, shadow ? t->sh_off[1] : 0};
        uint8_t tc[4], bc[4];
        memcpy(tc, shadow ? t->sh_top : t->top, 4), memcpy(bc, shadow ? t->sh_bot : t->bot, 4);
        tc[3] = (uint8_t)lroundf(tc[3] * alpha), bc[3] = (uint8_t)lroundf(bc[3] * alpha);
        for (int ln = 0; ln < nlines; ln++) {
            float pen = bx + (block_w - widths[ln]) * lf + off[0], line_top = by - ln * line_h + off[1];
            for (int i = starts[ln]; i < starts[ln] + lens[ln]; i++) {
                uint32_t ch = s[i];
                if (is_control(ch)) continue;
                const LFont *gf = for_char(font, ch);
                float sx = t->fsize[0] / (gf->width ? gf->width : 1), gsy = t->fsize[1] / (gf->height ? gf->height : 1);
                uint16_t g = glyph_of(gf, ch);
                LCharW cw = cw_of(gf, g);
                float x0 = pen + cw.left * sx, x1 = x0 + cw.glyph_w * sx;
                float y0 = line_top - ascent + gf->baseline * gsy, y1 = y0 - gf->cell_h * gsy;
                pen += cw.char_w * sx + t->cspace;
                if (ch == ' ' || ch == 0x3000 || cw.glyph_w == 0) continue;
                int per_row = gf->cells_row ? gf->cells_row : 1, per_sheet = per_row * (gf->cells_col ? gf->cells_col : 1);
                int sheet = g / per_sheet, local = g % per_sheet;
                float cx = (float)((local % per_row) * (gf->cell_w + 1) + 1), cy = (float)((local / per_row) * (gf->cell_h + 1) + 1);
                float sw = gf->sheet_w ? gf->sheet_w : 1, sh = gf->sheet_h ? gf->sheet_h : 1;
                float u0 = cx / sw, v0 = cy / sh, u1 = (cx + cw.glyph_w) / sw, v1 = (cy + gf->cell_h) / sh;
                LQuad q;
                memset(&q, 0, sizeof q);
                apply(mtx, x0, y0, q.pos[0]), apply(mtx, x1, y0, q.pos[1]), apply(mtx, x0, y1, q.pos[2]), apply(mtx, x1, y1, q.pos[3]);
                q.uv0[0][0] = u0, q.uv0[0][1] = v0, q.uv0[1][0] = u1, q.uv0[1][1] = v0;
                q.uv0[2][0] = u0, q.uv0[2][1] = v1, q.uv0[3][0] = u1, q.uv0[3][1] = v1;
                memcpy(q.col[0], tc, 4), memcpy(q.col[1], tc, 4), memcpy(q.col[2], bc, 4), memcpy(q.col[3], bc, 4);
                memcpy(q.black, black, 4), memcpy(q.white, white, 4);
                q.tex0 = lyt_font_sheet((LFont *)gf, sheet);
                q.tex1 = -1;
                q.flags = flags;
                push(out, &q);
            }
        }
    }
    free(starts), free(lens), free(widths);
}

typedef struct Chain {
    const LMsbt *tables[8];  // enclosing layouts' message tables, outermost first
    int n;
} Chain;

static const uint32_t *text_for(const LInst *in, int i, const Chain *outer, int *n) {
    const LPane *p = &in->res->pane[i];
    if (in->text[i]) { *n = in->ntext[i]; return in->text[i]; }
    const LProp *tp = in->textprop[i];
    if (tp) {  // a message id given by the enclosing layout's part properties (searched from the screen inwards)
        const char *id = tp->pane->txt.text_id[0] ? tp->pane->txt.text_id : tp->pane->name;
        for (int k = 0; k < outer->n; k++) {
            const LMsg *m = lyt_msbt_get(outer->tables[k], id);
            if (m) { *n = m->n; return m->text; }
        }
        const LMsg *m = lyt_msbt_get(&in->res->msg, id);
        if (m) { *n = m->n; return m->text; }
        *n = tp->pane->txt.ntext;
        return tp->pane->txt.text;
    }
    const char *label = p->txt.text_id[0] ? p->txt.text_id : p->name;
    const LMsg *m = lyt_msbt_get(&in->res->msg, label);
    for (int k = outer->n - 1; !m && k >= 0; k--) m = lyt_msbt_get(outer->tables[k], label);
    if (m) { *n = m->n; return m->text; }
    *n = p->txt.ntext;
    return p->txt.text;
}

static void draw_at(const LInst *in, const Mtx *base, float base_alpha, const Chain *outer, LQuads *out) {
    const LRes *r = in->res;
    Pass P;
    pass(in, base, base_alpha, &P);
    Ctx c = {.res = r, .ms = P.ms};
    for (int i = 0; i < r->npane; i++) {
        if (!P.visible[i]) continue;
        const LPane *p = &r->pane[i];
        const PS *s = &P.ps[i];
        float rc[4];
        pane_rect(p->origin, s->size, rc);
        c.pane_size[0] = s->size[0], c.pane_size[1] = s->size[1];
        uint8_t col[4][4];
        switch (p->kind) {
        case LP_PIC: {
            const LProp *ov = in->picovr[i];
            if (ov && in->owner && ov->mat_usage == 1) {  // the enclosing layout's colours on the part's own picture
                const LMat *om = ov->pane->pic.mat < in->owner->nmat ? &in->owner->mat[ov->pane->pic.mat] : NULL;
                if (om) c.has_shared = true, memcpy(c.sh_black, om->black, 4), memcpy(c.sh_white, om->white, 4);
                vcolors((const float (*)[4])s->col, P.alpha[i], col);
                quad(&c, p->pic.mat, p->pic.nuv, (const float (*)[4][2])p->pic.uv, rc, &P.mtx[i], (const uint8_t (*)[4])col, NULL, out);
                c.has_shared = false;
            } else if (ov && in->owner) {  // the enclosing layout's picture (material, coordinates, colours)
                MS *oms = zalloc(sizeof(MS) * (size_t)(in->owner->nmat + 1));
                base_mats(in->owner, oms);
                Ctx oc = {.res = in->owner, .ms = oms, .pane_size = {s->size[0], s->size[1]}};
                float fc[4][4];
                for (int k = 0; k < 4; k++)
                    for (int q = 0; q < 4; q++) fc[k][q] = ov->pane->pic.vc[k][q];
                vcolors((const float (*)[4])fc, P.alpha[i], col);
                quad(&oc, ov->pane->pic.mat, ov->pane->pic.nuv, (const float (*)[4][2])ov->pane->pic.uv, rc, &P.mtx[i],
                     (const uint8_t (*)[4])col, NULL, out);
                free(oms);
            } else {
                vcolors((const float (*)[4])s->col, P.alpha[i], col);
                quad(&c, p->pic.mat, p->pic.nuv, (const float (*)[4][2])p->pic.uv, rc, &P.mtx[i], (const uint8_t (*)[4])col, NULL, out);
            }
            break;
        }
        case LP_WND:
            vcolors((const float (*)[4])s->col, P.alpha[i], col);
            window(&c, &p->wnd, rc, &P.mtx[i], (const uint8_t (*)[4])col, P.alpha[i], out);
            break;
        case LP_TXT: {
            const LFont *f = p->txt.font < r->nfont ? r->font[p->txt.font] : NULL;
            if (!f) break;
            int n;
            const uint32_t *str = text_for(in, i, outer, &n);
            uint8_t black[4] = {0, 0, 0, 0}, white[4] = {255, 255, 255, 255};
            uint32_t blend = LQ_BLEND_NORMAL;
            if (p->txt.mat < r->nmat) {
                for (int k = 0; k < 4; k++) black[k] = u8c(P.ms[p->txt.mat].black[k]), white[k] = u8c(P.ms[p->txt.mat].white[k]);
                blend = blend_flags(&r->mat[p->txt.mat]);
            }
            text_emit(f, &p->txt, str, n, rc, &P.mtx[i], P.alpha[i], black, white, blend, out);
            break;
        }
        case LP_PRT:
            if (in->part[i]) {
                Mtx sm = scale_m(p->prt.magnify[0], p->prt.magnify[1]);
                Mtx m = mul(&P.mtx[i], &sm);
                Chain ch = *outer;
                if (ch.n < 8) ch.tables[ch.n++] = &r->msg;
                draw_at(in->part[i], &m, P.passed[i], &ch, out);
            }
            break;
        default:
            break;
        }
    }
    pass_free(&P);
}

void lyt_draw(LInst *in, LQuads *out) {
    Chain ch = {.n = 0};
    draw_at(in, &IDENT, 1, &ch, out);
}

float lyt_text_width(LInst *in, const char *path) {
    const char *name;
    LInst *p = resolve(in, path, &name);
    if (!p) return 0;
    int i = lyt_pane_index(p->res, name);
    if (i < 0 || p->res->pane[i].kind != LP_TXT) return 0;
    const LTxt *t = &p->res->pane[i].txt;
    const LFont *f = t->font < p->res->nfont ? p->res->font[t->font] : NULL;
    if (!f) return 0;
    Chain ch = {.n = 0};
    int n;
    const uint32_t *s = text_for(p, i, &ch, &n);
    float best = 0;
    for (int a = 0, st = 0; a <= n; a++)
        if (a == n || s[a] == '\n') {
            float w = line_width(f, s + st, a - st, t->fsize, t->cspace);
            if (w > best) best = w;
            st = a + 1;
        }
    return best;
}
