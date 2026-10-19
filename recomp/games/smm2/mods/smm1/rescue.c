// SMM1's rescue scene (rescue.h), as Block.rpx v208 plays it [obs, research/specs/smm1-ending.md §2]:
//   - DemoPeach at (416, 48): <Style>_Object_peach's peach_model (Normal, Expert, Super Expert) or kinopio_model
//     (Easy), with field_model (the ground) at (0, 48, -1000); the wait_peach / wait_kinopio texture pattern plays
//     in SMW and NSMBU only
//   - demo Mario at (-32, 47): waits 30 frames, walks right 1.5 units a frame ('run', rate 1.125) until x = 384,
//     stands ('wait') 30 frames, then the balloon text runs 360 frames (Out at 300), then 120 more frames (300 in
//     NSMBU with Peach) before the credits are asked for
//   - orthographic camera centred on Mario, clamped so the 384 x 216 view [inf] stays inside x 0-1152, y 0-432
// Positions are course units (16 a block); the view fills the 1280 x 720 layout space.
#include "rescue.h"

#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "bfres.h"
#include "smm1_data.h"

#define VIEW_W 384.0f
#define VIEW_H 216.0f

struct Rescue {
    int style, form;
    bool peach;
    Bfres *obj, *mario;
    const BModel *field, *npc, *mdl;
    const BTexPat *npc_wait, *run, *wait;
    int walk_end;  // the frame Mario reaches x = 384
};

static const char *STYLE[4] = {"M1", "M3", "MW", "WU"};

static Bfres *load(const char *name) {
    size_t n;
    uint8_t *d = smm1_model(name, &n);
    Bfres *b = d ? bfres_load(d, n) : NULL;
    free(d);  // (bfres_load decoded what it keeps)
    if (!b) fprintf(stderr, "[smm1ui] rescue: model %s missing\n", name);
    return b;
}

Rescue *rescue_new(int style, bool peach, int form) {
    Rescue *r = calloc(1, sizeof *r);
    r->style = style < 0 || style > 3 ? 0 : style, r->peach = peach, r->form = form;
    char name[64];
    snprintf(name, sizeof name, "%s_Object_peach", STYLE[r->style]);
    r->obj = load(name);
    snprintf(name, sizeof name, "%s_Player_%sMdl", STYLE[r->style], form ? "SpMario" : "Mario");
    r->mario = load(name);
    if (!r->mario && form) {
        snprintf(name, sizeof name, "%s_Player_MarioMdl", STYLE[r->style]);
        r->mario = load(name);
    }
    if (r->obj) {
        r->field = bfres_model(r->obj, "field_model");
        r->npc = bfres_model(r->obj, peach ? "peach_model" : "kinopio_model");
        if (r->style >= 2) r->npc_wait = bfres_texpat(r->obj, peach ? "wait_peach" : "wait_kinopio");
    }
    if (r->mario) {
        r->mdl = r->mario->nmodels ? &r->mario->models[0] : NULL;
        r->run = bfres_texpat(r->mario, r->style == 3 ? "walk" : "run");
        if (!r->run) r->run = bfres_texpat(r->mario, "walk");
        r->wait = bfres_texpat(r->mario, "wait");
    }
    // walk: x from -32 by 1.5 a frame; the step that would reach 384 sets x = 384
    r->walk_end = 30 + (int)ceilf((384 - 1.5f + 32) / 1.5f);
    return r;
}
void rescue_free(Rescue *r) { free(r); }  // (the models' textures stay in the texture table)

int rescue_text_in(const Rescue *r) { return r->walk_end + 30; }
int rescue_text_out(const Rescue *r) { return rescue_text_in(r) + 300; }
int rescue_length(const Rescue *r) { return rescue_text_in(r) + 360 + (r->style == 3 && r->peach ? 300 : 120); }

static float mario_x(const Rescue *r, int t) {
    if (t < 30) return -32;
    float x = -32 + 1.5f * (float)(t - 30);
    return t >= r->walk_end || x > 384 ? 384 : x;
}

// one model's shapes at a world placement, through the camera, as triangles
static void draw_model(const Bfres *b, const BModel *m, const BTexPat *pat, float frame, float px, float py, float pz,
                       float camx, float camy, bool mirror, LQuads *out) {
    if (!m) return;
    const float kx = 1280 / VIEW_W, ky = 720 / VIEW_H;
    // back to front within the model (the SMB1 sky sits at z -1 behind the ground)
    int order[64], n = m->nshapes < 64 ? m->nshapes : 64;
    float zs[64];
    for (int i = 0; i < n; i++) {
        const BShape *s = &m->shapes[i];
        float z = s->nverts ? s->pos[0][2] : 0;
        if (s->bone >= 0 && s->bone < m->nbones) z += m->bones[s->bone].world[2][3];
        zs[i] = z, order[i] = i;
    }
    for (int i = 1; i < n; i++)
        for (int k = i; k > 0 && zs[order[k]] < zs[order[k - 1]]; k--) {
            int tmp = order[k];
            order[k] = order[k - 1], order[k - 1] = tmp;
        }
    for (int oi = 0; oi < n; oi++) {
        const BShape *s = &m->shapes[order[oi]];
        if (!s->idx || !s->pos || s->mat >= m->nmats) continue;
        const BMat *mt = &m->mats[s->mat];
        const char *texname = pat ? bfres_texpat_at(pat, mt->name, frame) : NULL;
        int tex = texname ? bfres_texture(b, texname) : -1;
        if (tex < 0 && mt->ntex) tex = bfres_texture(b, mt->tex[0]);
        const float (*w)[4] = s->bone >= 0 && s->bone < m->nbones ? (const float (*)[4])m->bones[s->bone].world : NULL;
        for (int i = 0; i + 2 < s->nidx; i += 3) {
            LQuad q;
            memset(&q, 0, sizeof q);
            float P[3][2], U[3][2];
            for (int k = 0; k < 3; k++) {
                const float *v = s->pos[s->idx[i + k] < (uint32_t)s->nverts ? s->idx[i + k] : 0];
                float x = v[0], y = v[1], z = v[2];
                if (w) {
                    float wx = w[0][0] * x + w[0][1] * y + w[0][2] * z + w[0][3];
                    float wy = w[1][0] * x + w[1][1] * y + w[1][2] * z + w[1][3];
                    x = wx, y = wy;
                }
                if (mirror) x = -x;
                P[k][0] = (x + px - camx) * kx, P[k][1] = (y + py - camy) * ky;
                const float *uv = s->uv[s->idx[i + k] < (uint32_t)s->nverts ? s->idx[i + k] : 0];
                U[k][0] = uv[0] * mt->srt[0] + mt->srt[3], U[k][1] = uv[1] * mt->srt[1] + mt->srt[4];
            }
            (void)pz;
            // corners TL, TR, BL = vertices 0, 1, 2; BR = TR + BL - TL (the rasterizer keeps the half s + t < 1)
            for (int k = 0; k < 3; k++) {
                q.pos[k][0] = P[k][0], q.pos[k][1] = P[k][1];
                q.uv0[k][0] = U[k][0], q.uv0[k][1] = U[k][1];
            }
            q.pos[3][0] = P[1][0] + P[2][0] - P[0][0], q.pos[3][1] = P[1][1] + P[2][1] - P[0][1];
            q.uv0[3][0] = U[1][0] + U[2][0] - U[0][0], q.uv0[3][1] = U[1][1] + U[2][1] - U[0][1];
            memset(q.col, 255, sizeof q.col);
            memset(q.black, 0, 4), memset(q.white, 255, 4);
            q.tex0 = tex, q.tex1 = -1;
            q.flags = LQ_TRI | LQ_BLEND_NORMAL | (uint32_t)(mt->wrap_s[0] & 3) << LQ_WRAP_S0 |
                      (uint32_t)(mt->wrap_t[0] & 3) << LQ_WRAP_T0 | LQ_COMB_NONE << LQ_COMB_C | LQ_COMB_NONE << LQ_COMB_A;
            if (out->n == out->cap) out->cap = out->cap ? out->cap * 2 : 256, out->q = realloc(out->q, sizeof(LQuad) * (size_t)out->cap);
            out->q[out->n++] = q;
        }
    }
}

// NSMBU: Mario is a skinned 3-D model (body + head) with skeletal animations ('walk' at rate 3.0, 'wait'), turned to
// face right; drawn back to front by triangle depth under the same orthographic camera [inf: the game uses a
// perspective camera here]
typedef struct {
    float z;
    LQuad q;
} Tri;
static int tri_cmp(const void *a, const void *b) {
    float d = ((const Tri *)a)->z - ((const Tri *)b)->z;
    return d < 0 ? -1 : d > 0 ? 1 : 0;
}
static void xform(const float m[3][4], const float v[3], float o[3]) {
    for (int k = 0; k < 3; k++) o[k] = m[k][0] * v[0] + m[k][1] * v[1] + m[k][2] * v[2] + m[k][3];
}
static void add_shapes(const Bfres *b, const BModel *m, float (*bones)[3][4], const float (*root)[4], float px, float py,
                       float camx, float camy, Tri **tris, int *n, int *cap) {
    const float kx = 1280 / VIEW_W, ky = 720 / VIEW_H;
    for (int si = 0; si < m->nshapes; si++) {
        const BShape *s = &m->shapes[si];
        if (!s->idx || !s->pos || s->mat >= m->nmats) continue;
        const BMat *mt = &m->mats[s->mat];
        int tex = mt->ntex ? bfres_texture(b, mt->tex[0]) : -1;
        for (int i = 0; i + 2 < s->nidx; i += 3) {
            float P[3][3], U[3][2];
            for (int k = 0; k < 3; k++) {
                uint32_t vi = s->idx[i + k] < (uint32_t)s->nverts ? s->idx[i + k] : 0;
                int bone = s->skin_count >= 1 && s->idx0 ? bfres_skin_bone(m, s->idx0[vi][0]) : s->bone;
                if (bone < 0 || bone >= m->nbones) bone = 0;
                float a[3], c[3];
                if (s->skin_count >= 2) memcpy(a, s->pos[vi], sizeof a);  // (smooth skin: model space)
                else xform(bones[bone], s->pos[vi], a);
                xform(root, a, c);
                P[k][0] = (c[0] + px - camx) * kx, P[k][1] = (c[1] + py - camy) * ky, P[k][2] = c[2];
                U[k][0] = s->uv[vi][0] * mt->srt[0] + mt->srt[3], U[k][1] = s->uv[vi][1] * mt->srt[1] + mt->srt[4];
            }
            if (*n == *cap) *cap = *cap ? *cap * 2 : 1024, *tris = realloc(*tris, sizeof(Tri) * (size_t)*cap);
            Tri *t = &(*tris)[(*n)++];
            memset(t, 0, sizeof *t);
            t->z = (P[0][2] + P[1][2] + P[2][2]) / 3;
            for (int k = 0; k < 3; k++) {
                t->q.pos[k][0] = P[k][0], t->q.pos[k][1] = P[k][1];
                t->q.uv0[k][0] = U[k][0], t->q.uv0[k][1] = U[k][1];
            }
            t->q.pos[3][0] = P[1][0] + P[2][0] - P[0][0], t->q.pos[3][1] = P[1][1] + P[2][1] - P[0][1];
            t->q.uv0[3][0] = U[1][0] + U[2][0] - U[0][0], t->q.uv0[3][1] = U[1][1] + U[2][1] - U[0][1];
            memset(t->q.col, 255, sizeof t->q.col);
            memset(t->q.white, 255, 4);
            t->q.tex0 = tex, t->q.tex1 = -1;
            t->q.flags = LQ_TRI | LQ_LINEAR | LQ_BLEND_NORMAL | (uint32_t)(mt->wrap_s[0] & 3) << LQ_WRAP_S0 |
                         (uint32_t)(mt->wrap_t[0] & 3) << LQ_WRAP_T0 | LQ_COMB_NONE << LQ_COMB_C | LQ_COMB_NONE << LQ_COMB_A;
        }
    }
}
static void draw_wu_mario(Rescue *r, int t, float mx, float my, float camx, float camy, LQuads *out) {
    const BModel *body = bfres_model(r->mario, r->form ? "MB_model" : "SMB_model");
    const BModel *head = bfres_model(r->mario, r->form ? "MH_model" : "SMH_model");
    if (!body) return;
    bool walking = t >= 30 && t < r->walk_end;
    const BSkelAnim *a = bfres_skelanim(r->mario, walking ? "walk" : "wait");
    float frame = a && a->frames > 0 ? fmodf((float)(walking ? t - 30 : t >= r->walk_end ? t - r->walk_end : 0) * (walking ? 3.0f : 1.0f), (float)a->frames) : 0;
    float (*bones)[3][4] = calloc((size_t)body->nbones + 1, sizeof *bones);
    bfres_pose(body, a, frame, bones);
    static const float root[3][4] = {{0, 0, 1, 0}, {0, 1, 0, 0}, {-1, 0, 0, 0}};  // turn the model (facing +z) to face +x
    Tri *tris = NULL;
    int n = 0, cap = 0;
    add_shapes(r->mario, body, bones, root, mx, my, camx, camy, &tris, &n, &cap);
    if (head) {  // the head on the body's face joint [inf]
        int fb = 0;
        for (int i = 0; i < body->nbones; i++)
            if (!strcmp(body->bones[i].name, "face_1")) fb = i;
        float (*hb)[3][4] = calloc((size_t)head->nbones + 1, sizeof *hb);
        bfres_pose(head, NULL, 0, hb);
        for (int i = 0; i < head->nbones; i++) {
            float o[3][4];
            for (int rr = 0; rr < 3; rr++)
                for (int c = 0; c < 4; c++)
                    o[rr][c] = bones[fb][rr][0] * hb[i][0][c] + bones[fb][rr][1] * hb[i][1][c] + bones[fb][rr][2] * hb[i][2][c] + (c == 3 ? bones[fb][rr][3] : 0);
            memcpy(hb[i], o, sizeof o);
        }
        add_shapes(r->mario, head, hb, root, mx, my, camx, camy, &tris, &n, &cap);
        free(hb);
    }
    qsort(tris, (size_t)n, sizeof(Tri), tri_cmp);
    for (int i = 0; i < n; i++) {
        if (out->n == out->cap) out->cap = out->cap ? out->cap * 2 : 1024, out->q = realloc(out->q, sizeof(LQuad) * (size_t)out->cap);
        out->q[out->n++] = tris[i].q;
    }
    free(tris);
    free(bones);
}

void rescue_draw(Rescue *r, int t, LQuads *out, uint8_t bg[4]) {
    bg[0] = bg[1] = bg[2] = 0, bg[3] = 255;  // what is behind the ground model is [?]: black
    float mx = mario_x(r, t), my = 47;
    float camx = fminf(fmaxf(mx, VIEW_W / 2), 1152 - VIEW_W / 2), camy = fminf(fmaxf(my, VIEW_H / 2), 432 - VIEW_H / 2);
    // back to front: the ground model (z -1000), Peach / Toad, Mario
    if (r->field) draw_model(r->obj, r->field, NULL, 0, 0, 48, -1000, camx, camy, false, out);
    if (r->npc) draw_model(r->obj, r->npc, r->npc_wait, (float)t, 416, 48, 0, camx, camy, false, out);
    if (r->style == 3 && r->mario) draw_wu_mario(r, t, mx, my, camx, camy, out);
    else if (r->mdl) {
        bool walking = t >= 30 && t < r->walk_end;
        const BTexPat *p = walking ? r->run : r->wait;
        float frame = walking ? fmodf((float)(t - 30) * (r->style == 3 ? 3.0f : 1.125f), (float)(p && p->frames > 0 ? p->frames : 1)) : 0;
        draw_model(r->mario, r->mdl, p, frame, mx, my, 0, camx, camy, false, out);
    }
}
