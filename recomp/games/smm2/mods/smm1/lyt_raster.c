// CPU rasterizer for the layout player's quads (lyt.h), so the SMM1 screens draw the same on Metal and Vulkan: the
// result is one RGBA8 image the renderers put over the game. Shading as SMCraft's UI shader did:
//   t = combine(sample(tex0, uv0), sample(tex1, uv1))   (a missing texture samples as 1)
//   c = mix(black, white, t) * vertex colour, blended into the target with the quad's blend mode.
// Composed in gamma space with 8-bit colours as authored (the Wii U frame was not linear-blended) [inf].
// Layout panes are affine images of rectangles, so every quad is a parallelogram: each pixel centre is mapped back
// to (s, t) in the unit square and the corner attributes interpolate bilinearly there.
#include "lyt.h"

#include <math.h>
#include <pthread.h>
#include <string.h>

typedef struct {
    const LQuads *q;
    uint8_t *px;
    int w, h, y0, y1;
    float lw, lh;
} Job;

static inline int wrapi(int i, int n, int mode) {
    if (mode == 1) { i %= n; return i < 0 ? i + n : i; }
    if (mode == 2) {
        int p = 2 * n;
        i %= p;
        if (i < 0) i += p;
        return i < n ? i : p - 1 - i;
    }
    return i < 0 ? 0 : i >= n ? n - 1 : i;
}

static inline void sample(const LTex *t, float u, float v, int ws, int wt, bool linear, float out[4]) {
    if (!t || !t->px || t->w <= 0 || t->h <= 0) { out[0] = out[1] = out[2] = out[3] = 1; return; }
    if (ws == 0) u = u < 0 ? 0 : u > 1 ? 1 : u;
    if (wt == 0) v = v < 0 ? 0 : v > 1 ? 1 : v;
    const float k = 1.0f / 255;
    if (!linear) {
        int x = wrapi((int)floorf(u * t->w), t->w, ws), y = wrapi((int)floorf(v * t->h), t->h, wt);
        const uint8_t *p = t->px + ((size_t)y * t->w + x) * 4;
        out[0] = p[0] * k, out[1] = p[1] * k, out[2] = p[2] * k, out[3] = p[3] * k;
        return;
    }
    float fx = u * t->w - 0.5f, fy = v * t->h - 0.5f;
    int x0 = (int)floorf(fx), y0 = (int)floorf(fy);
    float ax = fx - x0, ay = fy - y0;
    int xa = wrapi(x0, t->w, ws), xb = wrapi(x0 + 1, t->w, ws), ya = wrapi(y0, t->h, wt), yb = wrapi(y0 + 1, t->h, wt);
    const uint8_t *p00 = t->px + ((size_t)ya * t->w + xa) * 4, *p10 = t->px + ((size_t)ya * t->w + xb) * 4;
    const uint8_t *p01 = t->px + ((size_t)yb * t->w + xa) * 4, *p11 = t->px + ((size_t)yb * t->w + xb) * 4;
    float w00 = (1 - ax) * (1 - ay), w10 = ax * (1 - ay), w01 = (1 - ax) * ay, w11 = ax * ay;
    for (int c = 0; c < 4; c++) out[c] = (p00[c] * w00 + p10[c] * w10 + p01[c] * w01 + p11[c] * w11) * k;
}

static inline float comb(float a, float b, int m) {
    switch (m) {
    case 0: return b;
    case 1: return a * b;
    case 2: return a + b > 1 ? 1 : a + b;
    case 5: return a - b < 0 ? 0 : a - b;
    default: return a;
    }
}

static inline float fac(int f, const float s[4], const float d[4], int c) {
    switch (f) {
    case 0: return 0;
    case 1: return 1;
    case 2: return d[c];
    case 3: return 1 - d[c];
    case 4: return s[3];
    case 5: return 1 - s[3];
    case 6: return d[3];
    case 7: return 1 - d[3];
    case 8: return s[c];
    case 9: return 1 - s[c];
    default: return 1;
    }
}

static void raster_quad(const LQuad *q, const Job *j) {
    float P[4][2];
    for (int i = 0; i < 4; i++) {
        P[i][0] = (q->pos[i][0] / j->lw + 0.5f) * j->w;
        P[i][1] = (0.5f - q->pos[i][1] / j->lh) * j->h;
    }
    float e1x = P[1][0] - P[0][0], e1y = P[1][1] - P[0][1], e2x = P[2][0] - P[0][0], e2y = P[2][1] - P[0][1];
    float det = e1x * e2y - e1y * e2x;
    if (fabsf(det) < 1e-6f) return;
    float minx = P[0][0], maxx = P[0][0], miny = P[0][1], maxy = P[0][1];
    for (int i = 1; i < 4; i++) {
        minx = fminf(minx, P[i][0]), maxx = fmaxf(maxx, P[i][0]);
        miny = fminf(miny, P[i][1]), maxy = fmaxf(maxy, P[i][1]);
    }
    int x0 = (int)floorf(minx), x1 = (int)ceilf(maxx), y0 = (int)floorf(miny), y1 = (int)ceilf(maxy);
    if (x0 < 0) x0 = 0;
    if (x1 > j->w) x1 = j->w;
    if (y0 < j->y0) y0 = j->y0;
    if (y1 > j->y1) y1 = j->y1;
    if (x0 >= x1 || y0 >= y1) return;
    float inv = 1.0f / det, dsdx = e2y * inv, dtdx = -e1y * inv;
    const LTex *t0 = lyt_tex(q->tex0), *t1 = lyt_tex(q->tex1);
    uint32_t fl = q->flags;
    int ws0 = fl >> LQ_WRAP_S0 & 3, wt0 = fl >> LQ_WRAP_T0 & 3, ws1 = fl >> LQ_WRAP_S1 & 3, wt1 = fl >> LQ_WRAP_T1 & 3;
    bool lin = fl & LQ_LINEAR;
    int cc = fl >> LQ_COMB_C & 15, ca = fl >> LQ_COMB_A & 15;
    int op = fl >> LQ_BLEND_OP & 15, bs = fl >> LQ_BLEND_SRC & 15, bd = fl >> LQ_BLEND_DST & 15;
    bool two = q->tex1 >= 0 && cc != (int)LQ_COMB_NONE;
    const float k = 1.0f / 255;
    float blk[4], dw[4], col[4][4];
    for (int c = 0; c < 4; c++) blk[c] = q->black[c] * k, dw[c] = (q->white[c] - q->black[c]) * k;
    for (int v = 0; v < 4; v++)
        for (int c = 0; c < 4; c++) col[v][c] = q->col[v][c] * k;
    bool flat = true;  // one vertex colour: skip the per-pixel colour interpolation
    for (int v = 1; v < 4; v++) flat = flat && !memcmp(q->col[v], q->col[0], 4);
    bool untextured = q->tex0 < 0 && q->tex1 < 0;
    bool normal = op == 1 && bs == 4 && bd == 5, tri = fl & LQ_TRI;
    // uv(s, t) = a + b s + c t when the corners' coordinates are affine (the cross term is ~0)
    float au = q->uv0[0][0], bu = q->uv0[1][0] - au, cu = q->uv0[2][0] - au;
    float av = q->uv0[0][1], bv = q->uv0[1][1] - av, cv = q->uv0[2][1] - av;
    bool affine = tri || (fabsf(q->uv0[3][0] - q->uv0[1][0] - q->uv0[2][0] + au) < 1e-5f &&
                          fabsf(q->uv0[3][1] - q->uv0[1][1] - q->uv0[2][1] + av) < 1e-5f);
    bool fast = affine && normal && flat && !two;
    for (int y = y0; y < y1; y++) {
        float dy = y + 0.5f - P[0][1], dx = x0 + 0.5f - P[0][0];
        float s = (dx * e2y - dy * e2x) * inv, t = (e1x * dy - e1y * dx) * inv;
        // the pixels of this row inside the parallelogram: 0 <= s0 + i*dsdx < 1 and 0 <= t0 + i*dtdx < 1
        float lo = 0, hi = (float)(x1 - x0);
        for (int pass = 0; pass < 2; pass++) {
            float v0 = pass ? t : s, dv = pass ? dtdx : dsdx;
            if (fabsf(dv) < 1e-12f) {
                if (v0 < 0 || v0 >= 1) hi = -1;
                continue;
            }
            float a = (0 - v0) / dv, b = (1 - v0) / dv;
            if (a > b) { float tmp = a; a = b; b = tmp; }
            if (a > lo) lo = a;
            if (b < hi) hi = b;
        }
        if (hi < lo) continue;
        int i0 = (int)floorf(lo) - 1, i1 = (int)ceilf(hi) + 1;
        if (i0 < 0) i0 = 0;
        if (i1 > x1 - x0) i1 = x1 - x0;
        s += dsdx * i0, t += dtdx * i0;
        uint8_t *row = j->px + ((size_t)y * j->w + x0 + i0) * 4;
        if (fast) {  // one texture, affine coordinates, flat colour, source over: incremental, integer where it can
            float u = au + bu * s + cu * t, v = av + bv * s + cv * t, dudx = bu * dsdx + cu * dtdx, dvdx = bv * dsdx + cv * dtdx;
            for (int x = x0 + i0; x < x0 + i1; x++, s += dsdx, t += dtdx, u += dudx, v += dvdx, row += 4) {
                if (s < 0 || s >= 1 || t < 0 || t >= 1 || (tri && s + t >= 1)) continue;
                float tx[4];
                if (t0) sample(t0, u, v, ws0, wt0, lin, tx);
                else tx[0] = tx[1] = tx[2] = tx[3] = 1;
                float sa = (blk[3] + dw[3] * tx[3]) * col[0][3];
                if (sa <= 0.5f / 255) continue;
                if (sa > 1) sa = 1;
                float ia = 1 - sa;
                for (int c = 0; c < 3; c++) {
                    float vv = (blk[c] + dw[c] * tx[c]) * col[0][c] * sa * 255 + row[c] * ia;
                    row[c] = (uint8_t)(vv < 0 ? 0 : vv > 255 ? 255 : vv + 0.5f);
                }
                float va = sa * sa * 255 + row[3] * ia;
                row[3] = (uint8_t)(va > 255 ? 255 : va + 0.5f);
            }
            continue;
        }
        for (int x = x0 + i0; x < x0 + i1; x++, s += dsdx, t += dtdx, row += 4) {
            if (s < 0 || s >= 1 || t < 0 || t >= 1 || (tri && s + t >= 1)) continue;
            float a0 = (1 - s) * (1 - t), a1 = s * (1 - t), a2 = (1 - s) * t, a3 = s * t;
            float tx[4] = {1, 1, 1, 1};
            if (!untextured) {
                float u = q->uv0[0][0] * a0 + q->uv0[1][0] * a1 + q->uv0[2][0] * a2 + q->uv0[3][0] * a3;
                float vv = q->uv0[0][1] * a0 + q->uv0[1][1] * a1 + q->uv0[2][1] * a2 + q->uv0[3][1] * a3;
                if (q->tex0 >= 0) sample(t0, u, vv, ws0, wt0, lin, tx);
                if (two) {
                    float u1 = q->uv1[0][0] * a0 + q->uv1[1][0] * a1 + q->uv1[2][0] * a2 + q->uv1[3][0] * a3;
                    float v1 = q->uv1[0][1] * a0 + q->uv1[1][1] * a1 + q->uv1[2][1] * a2 + q->uv1[3][1] * a3;
                    float b[4];
                    sample(t1, u1, v1, ws1, wt1, lin, b);
                    tx[0] = comb(tx[0], b[0], cc), tx[1] = comb(tx[1], b[1], cc), tx[2] = comb(tx[2], b[2], cc);
                    tx[3] = comb(tx[3], b[3], ca);
                }
            }
            float src[4];
            for (int c = 0; c < 4; c++) {
                float vc = flat ? col[0][c] : col[0][c] * a0 + col[1][c] * a1 + col[2][c] * a2 + col[3][c] * a3;
                src[c] = (blk[c] + dw[c] * tx[c]) * vc;
            }
            if (normal) {  // source over: the common case
                float sa = src[3] < 0 ? 0 : src[3] > 1 ? 1 : src[3];
                if (sa <= 0.5f / 255) continue;
                float ia = 1 - sa;
                for (int c = 0; c < 3; c++) {
                    float v = src[c] * sa * 255 + row[c] * ia;
                    row[c] = (uint8_t)(v < 0 ? 0 : v > 255 ? 255 : v + 0.5f);
                }
                float va = sa * sa * 255 + row[3] * ia;
                row[3] = (uint8_t)(va > 255 ? 255 : va + 0.5f);
                continue;
            }
            float dst[4] = {row[0] * k, row[1] * k, row[2] * k, row[3] * k}, o[4];
            if (op == 0) {
                memcpy(o, src, sizeof o);
            } else {
                for (int c = 0; c < 4; c++) {
                    float fs = src[c] * fac(bs, src, dst, c), fd = dst[c] * fac(bd, src, dst, c);
                    switch (op) {
                    case 2: o[c] = fs - fd; break;
                    case 3: o[c] = fd - fs; break;
                    case 4: o[c] = src[c] < dst[c] ? src[c] : dst[c]; break;
                    case 5: o[c] = src[c] > dst[c] ? src[c] : dst[c]; break;
                    default: o[c] = fs + fd; break;
                    }
                }
            }
            for (int c = 0; c < 4; c++) {
                float v = o[c] < 0 ? 0 : o[c] > 1 ? 1 : o[c];
                row[c] = (uint8_t)(v * 255 + 0.5f);
            }
        }
    }
}

static void *band(void *arg) {
    const Job *j = arg;
    for (int i = 0; i < j->q->n; i++) raster_quad(&j->q->q[i], j);
    return NULL;
}

void lyt_raster(const LQuads *q, uint8_t *rgba, int w, int h, float lw, float lh) {
    enum { BANDS = 6 };
    Job jobs[BANDS];
    pthread_t th[BANDS];
    bool started[BANDS] = {0};
    for (int b = 0; b < BANDS; b++) {
        jobs[b] = (Job){q, rgba, w, h, h * b / BANDS, h * (b + 1) / BANDS, lw, lh};
        if (b > 0) started[b] = pthread_create(&th[b], NULL, band, &jobs[b]) == 0;
    }
    band(&jobs[0]);
    for (int b = 1; b < BANDS; b++) {
        if (started[b]) pthread_join(th[b], NULL);
        else band(&jobs[b]);
    }
}

void lyt_clear(uint8_t *rgba, int w, int h, const uint8_t c[4]) {
    for (size_t i = 0; i < (size_t)w * h; i++) memcpy(rgba + i * 4, c, 4);
}
