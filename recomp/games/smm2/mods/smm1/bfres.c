// Wii U BFRES reader (bfres.h). Layout from public descriptions of FRES version 3 (Wii U): every offset is relative
// to the field holding it; index groups are `u32 size, i32 count`, then count + 1 nodes of 16 bytes
// (`u32 search, u16 left, u16 right, rel name, rel data`, the first node the root).
#include "bfres.h"

#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "lyt.h"

typedef struct {
    const uint8_t *d;
    size_t n;
} B;
static uint32_t u32(const B *b, size_t o) {
    return o + 4 <= b->n ? (uint32_t)b->d[o] << 24 | (uint32_t)b->d[o + 1] << 16 | (uint32_t)b->d[o + 2] << 8 | b->d[o + 3] : 0;
}
static uint16_t u16(const B *b, size_t o) { return o + 2 <= b->n ? (uint16_t)(b->d[o] << 8 | b->d[o + 1]) : 0; }
static uint8_t u8(const B *b, size_t o) { return o < b->n ? b->d[o] : 0; }
static float f32(const B *b, size_t o) {
    uint32_t u = u32(b, o);
    float f;
    memcpy(&f, &u, 4);
    return f;
}
static size_t rel(const B *b, size_t o) {  // 0 = none
    int32_t v = (int32_t)u32(b, o);
    return v ? (size_t)((int64_t)o + v) : 0;
}
static void str(const B *b, size_t o, char *out, size_t cap) {
    size_t k = 0;
    while (o && k + 1 < cap && o + k < b->n && b->d[o + k]) out[k] = (char)b->d[o + k], k++;
    out[k] = 0;
}
static void *zalloc(size_t n) { return calloc(1, n ? n : 1); }

// entries of an index group: data offsets (and names)
static int group(const B *b, size_t g, size_t *data, char (*names)[32], int cap) {
    if (!g) return 0;
    int n = (int32_t)u32(b, g + 4);
    if (n > cap) n = cap;
    for (int i = 0; i < n; i++) {
        size_t node = g + 8 + 16 * (size_t)(i + 1);
        data[i] = rel(b, node + 12);
        if (names) str(b, rel(b, node + 8), names[i], 32);
    }
    return n;
}

// ---------------------------------------------------------------- textures
static int gx2_to_cafe(uint32_t f) {
    switch (f) {
    case 0x001: return 0;
    case 0x007: return 3;
    case 0x008: return 5;
    case 0x00a: return 7;
    case 0x00b: return 8;
    case 0x01a: return 9;
    case 0x41a: return 20;
    case 0x031: return 12;
    case 0x431: return 21;
    case 0x032: return 13;
    case 0x432: return 22;
    case 0x033: return 14;
    case 0x433: return 23;
    case 0x034: return 15;
    case 0x035: return 17;
    default: return -1;
    }
}
static int ftex(const B *b, size_t o, char *name) {
    if (memcmp(b->d + o, "FTEX", 4)) return -1;
    uint32_t w = u32(b, o + 8), h = u32(b, o + 12), fmt = u32(b, o + 24), size = u32(b, o + 36);
    uint32_t tile = u32(b, o + 52), swz = u32(b, o + 56);
    uint8_t sel[4] = {u8(b, o + 0x88), u8(b, o + 0x89), u8(b, o + 0x8A), u8(b, o + 0x8B)};
    size_t nm = rel(b, o + 0xA8), data = rel(b, o + 0xB0);
    if (nm) str(b, nm, name, 32);
    int cafe = gx2_to_cafe(fmt);
    if (cafe < 0 || !data || data + size > b->n) {
        fprintf(stderr, "[smm1ui] bfres texture %s: format %#x not decoded\n", name, fmt);
        return -1;
    }
    uint8_t *px = lyt_surface_raw(b->d + data, size, w, h, cafe, tile, swz);
    if (!px) return -1;
    if (cafe == 0) sel[0] = sel[1] = sel[2] = 0;  // (L8 is decoded as RGB = L)
    if (sel[0] != 0 || sel[1] != 1 || sel[2] != 2 || sel[3] != 3)
        for (size_t i = 0; i < (size_t)w * h; i++) {
            uint8_t *p = px + i * 4, c[4] = {p[0], p[1], p[2], p[3]};
            for (int k = 0; k < 4; k++) p[k] = sel[k] < 4 ? c[sel[k]] : sel[k] == 4 ? 0 : 255;
        }
    return lyt_tex_add((int)w, (int)h, px);
}

// ---------------------------------------------------------------- models
static void euler_matrix(const float s[3], const float r[3], const float t[3], float m[3][4]) {
    float cx = cosf(r[0]), sx = sinf(r[0]), cy = cosf(r[1]), sy = sinf(r[1]), cz = cosf(r[2]), sz = sinf(r[2]);
    float R[3][3] = {{cy * cz, sx * sy * cz - cx * sz, cx * sy * cz + sx * sz},
                     {cy * sz, sx * sy * sz + cx * cz, cx * sy * sz - sx * cz},
                     {-sy, sx * cy, cx * cy}};
    for (int i = 0; i < 3; i++) m[i][0] = R[i][0] * s[0], m[i][1] = R[i][1] * s[1], m[i][2] = R[i][2] * s[2], m[i][3] = t[i];
}
static void quat_matrix(const float s[3], const float q[4], const float t[3], float m[3][4]) {
    float x = q[0], y = q[1], z = q[2], w = q[3];
    float R[3][3] = {{1 - 2 * (y * y + z * z), 2 * (x * y - z * w), 2 * (x * z + y * w)},
                     {2 * (x * y + z * w), 1 - 2 * (x * x + z * z), 2 * (y * z - x * w)},
                     {2 * (x * z - y * w), 2 * (y * z + x * w), 1 - 2 * (x * x + y * y)}};
    for (int i = 0; i < 3; i++) m[i][0] = R[i][0] * s[0], m[i][1] = R[i][1] * s[1], m[i][2] = R[i][2] * s[2], m[i][3] = t[i];
}
static void mmul(const float a[3][4], const float b[3][4], float o[3][4]) {
    for (int r = 0; r < 3; r++)
        for (int c = 0; c < 4; c++) o[r][c] = a[r][0] * b[0][c] + a[r][1] * b[1][c] + a[r][2] * b[2][c] + (c == 3 ? a[r][3] : 0);
}

// a vertex attribute as floats (GX2 attribute formats)
static int attr_read(const B *b, size_t p, uint32_t fmt, float out[4]) {
    out[0] = out[1] = out[2] = 0, out[3] = 1;
    switch (fmt) {
    case 0x811: for (int k = 0; k < 3; k++) out[k] = f32(b, p + 4 * (size_t)k); return 3;  // 32_32_32 float
    case 0x813: for (int k = 0; k < 4; k++) out[k] = f32(b, p + 4 * (size_t)k); return 4;
    case 0x80d: for (int k = 0; k < 2; k++) out[k] = f32(b, p + 4 * (size_t)k); return 2;  // 32_32 float
    case 0x806: out[0] = f32(b, p); return 1;
    case 0x808: case 0x80f: {  // 16_16 / 16_16_16_16 float
        int nc = fmt == 0x808 ? 2 : 4;
        for (int k = 0; k < nc; k++) {
            uint16_t h = u16(b, p + 2 * (size_t)k);
            int e = h >> 10 & 31, m = h & 1023;
            float v = e == 0 ? ldexpf((float)m, -24) : e == 31 ? 65504.0f : ldexpf((float)(m | 1024), e - 25);
            out[k] = h & 0x8000 ? -v : v;
        }
        return nc;
    }
    case 0x007: for (int k = 0; k < 2; k++) out[k] = u16(b, p + 2 * (size_t)k) / 65535.0f; return 2;  // 16_16 unorm
    case 0x004: for (int k = 0; k < 2; k++) out[k] = u8(b, p + (size_t)k) / 255.0f; return 2;          // 8_8 unorm
    case 0x00a: for (int k = 0; k < 4; k++) out[k] = u8(b, p + (size_t)k) / 255.0f; return 4;          // 8_8_8_8 unorm
    case 0x100: out[0] = u8(b, p); return 1;                                                           // 8 uint
    case 0x104: for (int k = 0; k < 2; k++) out[k] = u8(b, p + (size_t)k); return 2;                   // 8_8 uint
    case 0x10a: for (int k = 0; k < 4; k++) out[k] = u8(b, p + (size_t)k); return 4;                   // 8_8_8_8 uint
    case 0x20a: for (int k = 0; k < 4; k++) out[k] = (int8_t)u8(b, p + (size_t)k) / 127.0f; return 4;  // 8_8_8_8 snorm
    case 0x207: for (int k = 0; k < 2; k++) out[k] = (int16_t)u16(b, p + 2 * (size_t)k) / 32767.0f; return 2;
    default: return 0;
    }
}

static void read_shape(const B *b, size_t o, size_t *fvtx_list, int nfvtx, BShape *s) {
    str(b, rel(b, o + 4), s->name, sizeof s->name);
    s->mat = u16(b, o + 0x0E);
    s->bone = u16(b, o + 0x10);
    int vb = u16(b, o + 0x12);
    s->skin_count = u8(b, o + 0x16);
    size_t fvtx = rel(b, o + 0x20);
    if (!fvtx && vb < nfvtx) fvtx = fvtx_list[vb];
    if (fvtx && !memcmp(b->d + fvtx, "FVTX", 4)) {
        int nattr = u8(b, fvtx + 4);
        uint32_t nv = u32(b, fvtx + 8);
        size_t attrs = rel(b, fvtx + 0x10), bufs = rel(b, fvtx + 0x18);
        s->nverts = (int)nv;
        s->pos = zalloc(sizeof(float) * 3 * nv);
        s->uv = zalloc(sizeof(float) * 2 * nv);
        for (int a = 0; a < nattr; a++) {
            size_t at = attrs + (size_t)a * 0xC;
            char an[16];
            str(b, rel(b, at), an, sizeof an);
            int bi = u8(b, at + 4);
            uint32_t boff = u16(b, at + 6), fmt = u32(b, at + 8);
            size_t buf = bufs + (size_t)bi * 0x18;
            uint32_t stride = u16(b, buf + 0xC);
            size_t data = rel(b, buf + 0x14);
            if (!data) continue;
            bool p0 = !strcmp(an, "_p0"), u0 = !strcmp(an, "_u0"), i0 = !strcmp(an, "_i0");
            if (!p0 && !u0 && !i0) continue;
            if (i0) s->idx0 = zalloc(4 * nv);
            for (uint32_t v = 0; v < nv; v++) {
                float f[4];
                if (!attr_read(b, data + boff + (size_t)v * stride, fmt, f)) {
                    fprintf(stderr, "[smm1ui] bfres %s: attribute %s format %#x not read\n", s->name, an, fmt);
                    break;
                }
                if (p0) s->pos[v][0] = f[0], s->pos[v][1] = f[1], s->pos[v][2] = f[2];
                if (u0) s->uv[v][0] = f[0], s->uv[v][1] = f[1];
                if (i0) for (int k = 0; k < 4; k++) s->idx0[v][k] = (uint8_t)f[k];
            }
        }
    }
    size_t lods = rel(b, o + 0x24);
    if (lods) {  // LOD 0
        uint32_t prim = u32(b, lods), ifmt = u32(b, lods + 4), count = u32(b, lods + 8);
        size_t ib = rel(b, lods + 0x14);
        uint32_t skip = u32(b, lods + 0x18);
        size_t data = ib ? rel(b, ib + 0x14) : 0;
        if (prim == 4 && data) {
            s->nidx = (int)count;
            s->idx = zalloc(sizeof(uint32_t) * count);
            for (uint32_t i = 0; i < count; i++) s->idx[i] = (ifmt == 9 ? u32(b, data + i * 4) : u16(b, data + i * 2)) + skip;
        } else
            fprintf(stderr, "[smm1ui] bfres %s: primitive %u index format %u not read\n", s->name, prim, ifmt);
    }
}

static void read_model(const B *b, size_t o, BModel *m) {
    str(b, rel(b, o + 4), m->name, sizeof m->name);
    size_t fskl = rel(b, o + 0x0C), fvtx_arr = rel(b, o + 0x10), fshp = rel(b, o + 0x14), fmat = rel(b, o + 0x18);
    int nfvtx = u16(b, o + 0x20);
    size_t *fv = zalloc(sizeof(size_t) * (size_t)(nfvtx + 1));
    for (int i = 0; i < nfvtx; i++) fv[i] = fvtx_arr + (size_t)i * 0x20;
    if (fskl && !memcmp(b->d + fskl, "FSKL", 4)) {
        uint32_t flags = u32(b, fskl + 4);
        m->quat = ((flags >> 12) & 7) == 0;  // rotation mode (bits 12-14): 0 quaternion, 1 euler XYZ [inf]
        m->skl_flags = flags;
        int nb = u16(b, fskl + 8), nsm = u16(b, fskl + 0xA) + u16(b, fskl + 0xC);  // smooth + rigid matrices
        size_t bones = rel(b, fskl + 0x14), smooth = rel(b, fskl + 0x18);
        m->nbones = nb;
        m->bones = zalloc(sizeof(BBone) * (size_t)(nb + 1));
        for (int i = 0; i < nb; i++) {
            size_t bo = bones + (size_t)i * 0x40;
            BBone *bn = &m->bones[i];
            str(b, rel(b, bo), bn->name, sizeof bn->name);
            bn->parent = (int16_t)u16(b, bo + 6);
            for (int k = 0; k < 3; k++) bn->s[k] = f32(b, bo + 0x14 + 4 * (size_t)k), bn->t[k] = f32(b, bo + 0x30 + 4 * (size_t)k);
            for (int k = 0; k < 4; k++) bn->r[k] = f32(b, bo + 0x20 + 4 * (size_t)k);
            float local[3][4];
            if (m->quat) quat_matrix(bn->s, bn->r, bn->t, local);
            else euler_matrix(bn->s, bn->r, bn->t, local);
            if (bn->parent >= 0 && bn->parent < i) mmul(m->bones[bn->parent].world, local, bn->world);
            else memcpy(bn->world, local, sizeof local);
        }
        m->nsmooth = nsm;
        m->smooth = zalloc(sizeof(int) * (size_t)(nsm + 1));
        for (int i = 0; i < nsm; i++) m->smooth[i] = u16(b, smooth + (size_t)i * 2);
    }
    size_t data[64];
    char names[64][32];
    m->nshapes = group(b, fshp, data, names, 64);
    m->shapes = zalloc(sizeof(BShape) * (size_t)(m->nshapes + 1));
    for (int i = 0; i < m->nshapes; i++) read_shape(b, data[i], fv, nfvtx, &m->shapes[i]);
    m->nmats = group(b, fmat, data, names, 64);
    m->mats = zalloc(sizeof(BMat) * (size_t)(m->nmats + 1));
    for (int i = 0; i < m->nmats; i++) {
        size_t mo = data[i];
        BMat *mt = &m->mats[i];
        str(b, rel(b, mo + 4), mt->name, sizeof mt->name);
        int ntex = u8(b, mo + 0x10), nsamp = u8(b, mo + 0x11), nparam = u16(b, mo + 0x12);
        size_t texref = rel(b, mo + 0x28), samp = rel(b, mo + 0x2C), params = rel(b, mo + 0x34), pdata = rel(b, mo + 0x3C);
        mt->ntex = ntex > 4 ? 4 : ntex;
        for (int t = 0; t < mt->ntex; t++) str(b, rel(b, texref + (size_t)t * 8), mt->tex[t], 32);
        for (int t = 0; t < nsamp && t < 4; t++) {  // GX2Sampler word 0: clamp x bits 0-2, y bits 3-5 (0 wrap, 1 mirror)
            uint32_t w0 = u32(b, samp + (size_t)t * 0x18);
            int cx = w0 & 7, cy = w0 >> 3 & 7;
            mt->wrap_s[t] = cx == 0 ? 1 : cx == 1 ? 2 : 0;
            mt->wrap_t[t] = cy == 0 ? 1 : cy == 1 ? 2 : 0;
        }
        mt->srt[0] = 1, mt->srt[1] = 1;  // scale x, y, rotate, translate x, y of tex_srt0
        for (int k = 0; k < nparam; k++) {
            size_t pp = params + (size_t)k * 0x14;
            char pn[32];
            str(b, rel(b, pp + 0x10), pn, sizeof pn);
            if (strcmp(pn, "tex_srt0") || u8(b, pp) != 30) continue;
            size_t at = pdata + u16(b, pp + 2);
            for (int q = 0; q < 5; q++) mt->srt[q] = f32(b, at + 4 + 4 * (size_t)q);
        }
    }
    free(fv);
}

// ---------------------------------------------------------------- texture-pattern animations
static void read_curve(const B *b, size_t c, BCurve *cv) {
    uint16_t flags = u16(b, c), nk = u16(b, c + 2);
    int ctype = flags >> 4 & 7;  // 4, 5: integer curves (step / linear): the offset is an integer
    float foff = ctype >= 4 ? (float)(int32_t)u32(b, c + 0x14) : f32(b, c + 0x14);
    size_t frames = rel(b, c + 0x1C), keys = rel(b, c + 0x20);
    int ftype = flags & 3, ktype = flags >> 2 & 3;
    cv->nkeys = nk;
    cv->frame = zalloc(sizeof(float) * nk);
    cv->value = zalloc(sizeof(float) * nk);
    for (int k = 0; k < nk; k++) {
        cv->frame[k] = ftype == 0 ? f32(b, frames + 4 * (size_t)k) : ftype == 1 ? (int16_t)u16(b, frames + 2 * (size_t)k) / 32.0f
                                                                                : u8(b, frames + (size_t)k);
        cv->value[k] = ktype == 0 ? f32(b, keys + 4 * (size_t)k) : ktype == 1 ? (int16_t)u16(b, keys + 2 * (size_t)k)
                                                                               : (int8_t)u8(b, keys + (size_t)k);
        cv->value[k] += foff;
    }
}

static void read_texpat(const B *b, size_t o, BTexPat *p) {
    str(b, rel(b, o + 4), p->name, sizeof p->name);
    uint16_t flags = u16(b, o + 0x0C);
    p->loop = flags & 4;
    p->frames = (int32_t)u32(b, o + 0x10);
    int ntex = u16(b, o + 0x14), nmat = u16(b, o + 0x16);
    size_t mats = rel(b, o + 0x2C);
    // material animations (0x1C each), then the texture references (name, FTEX; 8 each)
    size_t texs = mats + (size_t)nmat * 0x1C;
    p->ntex = ntex;
    p->tex = zalloc(32 * (size_t)(ntex + 1));
    for (int i = 0; i < ntex; i++) str(b, rel(b, texs + (size_t)i * 8), p->tex[i], 32);
    p->nmat = nmat;
    p->mat = zalloc(sizeof(BMatPat) * (size_t)(nmat + 1));
    for (int i = 0; i < nmat; i++) {
        size_t mo = mats + (size_t)i * 0x1C;
        BMatPat *mp = &p->mat[i];
        int npat = u16(b, mo), ncurve = u16(b, mo + 2);
        str(b, rel(b, mo + 0xC), mp->mat, sizeof mp->mat);
        size_t pinfo = rel(b, mo + 0x10), curves = rel(b, mo + 0x14), base = rel(b, mo + 0x18);
        mp->npat = npat;
        mp->base = zalloc(sizeof(int) * (size_t)(npat + 1));
        mp->pat_curve = zalloc(sizeof(int) * (size_t)(npat + 1));
        for (int k = 0; k < npat; k++) {
            mp->base[k] = base ? u16(b, base + 2 * (size_t)k) : 0;
            mp->pat_curve[k] = (int8_t)u8(b, pinfo + (size_t)k * 8);
        }
        mp->ncurves = ncurve;
        mp->curve = zalloc(sizeof(BCurve) * (size_t)(ncurve + 1));
        for (int k = 0; k < ncurve; k++) read_curve(b, curves + (size_t)k * 0x24, &mp->curve[k]);
    }
}

const char *bfres_texpat_at(const BTexPat *p, const char *mat, float frame) {
    for (int i = 0; p && i < p->nmat; i++) {
        const BMatPat *mp = &p->mat[i];
        if (strcmp(mp->mat, mat)) continue;
        int idx = mp->npat ? mp->base[0] : 0, ci = mp->npat ? mp->pat_curve[0] : -1;
        if (ci >= 0 && ci < mp->ncurves && mp->curve[ci].nkeys > 0) {
            const BCurve *c = &mp->curve[ci];
            int k = 0;
            while (k + 1 < c->nkeys && c->frame[k + 1] <= frame) k++;
            idx = (int)c->value[k];
        }
        return idx >= 0 && idx < p->ntex ? p->tex[idx] : NULL;
    }
    return NULL;
}

// ---------------------------------------------------------------- skeletal animations
// FSKA (v3): +0x0C flags (bit 2 loop; bits 12-13 rotation mode, 1 = euler XYZ), +0x10 frame count, +0x14 bone
// animation count, +0x20 bone animations (0x18 each: flags, name, u8 begin rotate / translate, curve count, begin
// base translate, begin curve, curves, base data = scale[3] rotate[4] translate[3] as the flags say). A curve: flags
// (frame type bits 0-1, key type 2-3, curve type 4-6), key count, target offset, start / end frame, scale, offset,
// delta, frames, keys (per key 4 coefficients for cubic, 2 for linear).
static void read_skelanim(const B *b, size_t o, BSkelAnim *a) {
    str(b, rel(b, o + 4), a->name, sizeof a->name);
    uint16_t flags = u16(b, o + 0x0E);
    a->loop = flags & 4;
    a->euler = ((flags >> 12) & 3) == 1;
    a->frames = (int32_t)u32(b, o + 0x10);
    a->nb = u16(b, o + 0x14);
    size_t ba = rel(b, o + 0x20);
    a->b = zalloc(sizeof(BBoneAnim) * (size_t)(a->nb + 1));
    for (int i = 0; i < a->nb; i++) {
        size_t p = ba + (size_t)i * 0x18;
        BBoneAnim *bn = &a->b[i];
        bn->flags = u32(b, p);
        str(b, rel(b, p + 4), bn->name, sizeof bn->name);
        bn->nc = u8(b, p + 0xA);
        size_t cv = rel(b, p + 0x10), base = rel(b, p + 0x14);
        size_t q = base;
        if (bn->flags & 8) for (int k = 0; k < 3; k++, q += 4) bn->base_s[k] = f32(b, q);
        if (bn->flags & 16) for (int k = 0; k < 4; k++, q += 4) bn->base_r[k] = f32(b, q);
        if (bn->flags & 32) for (int k = 0; k < 3; k++, q += 4) bn->base_t[k] = f32(b, q);
        bn->c = zalloc(sizeof(BFCurve) * (size_t)(bn->nc + 1));
        for (int c = 0; c < bn->nc; c++) {
            size_t cp = cv + (size_t)c * 0x24;
            BFCurve *cu = &bn->c[c];
            uint16_t cf = u16(b, cp);
            int ftype = cf & 3, ktype = cf >> 2 & 3;
            cu->type = cf >> 4 & 7;
            cu->nk = u16(b, cp + 2);
            cu->target = (int)u32(b, cp + 4);
            cu->scale = f32(b, cp + 0x10), cu->offset = f32(b, cp + 0x14);
            if (ktype != 0) cu->scale = cu->scale == 0 ? 1 : cu->scale;
            size_t fr = rel(b, cp + 0x1C), ky = rel(b, cp + 0x20);
            int nco = cu->type == 1 ? 2 : 4;
            cu->frame = zalloc(sizeof(float) * (size_t)(cu->nk + 1));
            cu->coef = zalloc(sizeof(float) * 4 * (size_t)(cu->nk + 1));
            for (int k = 0; k < cu->nk; k++) {
                cu->frame[k] = ftype == 0 ? f32(b, fr + 4 * (size_t)k) : ftype == 1 ? (int16_t)u16(b, fr + 2 * (size_t)k) / 32.0f
                                                                                  : u8(b, fr + (size_t)k);
                for (int e = 0; e < nco; e++) {
                    size_t at = (size_t)(k * nco + e);
                    cu->coef[k][e] = ktype == 0 ? f32(b, ky + 4 * at) : ktype == 1 ? (int16_t)u16(b, ky + 2 * at) : (int8_t)u8(b, ky + at);
                }
            }
        }
    }
}

// ---------------------------------------------------------------- file
Bfres *bfres_load(const uint8_t *d, size_t n) {
    if (n < 0x80 || memcmp(d, "FRES", 4)) return NULL;
    B b = {d, n};
    Bfres *r = zalloc(sizeof *r);
    size_t data[256];
    char names[256][32];
    int nm = group(&b, rel(&b, 0x20), data, names, 256);
    r->models = zalloc(sizeof(BModel) * (size_t)(nm + 1));
    for (int i = 0; i < nm; i++) read_model(&b, data[i], &r->models[r->nmodels++]);
    int nt = group(&b, rel(&b, 0x24), data, names, 256);
    r->tex_name = zalloc(32 * (size_t)(nt + 1));
    r->tex = zalloc(sizeof(int) * (size_t)(nt + 1));
    for (int i = 0; i < nt; i++) {
        snprintf(r->tex_name[i], 32, "%s", names[i]);
        // (normal maps: the scene is drawn unlit)
        r->tex[i] = strstr(names[i], "_Nrm") ? -1 : ftex(&b, data[i], r->tex_name[i]);
    }
    r->ntex = nt;
    int np = group(&b, rel(&b, 0x20 + 6 * 4), data, names, 256);
    r->pats = zalloc(sizeof(BTexPat) * (size_t)(np + 1));
    for (int i = 0; i < np; i++) read_texpat(&b, data[i], &r->pats[r->npat++]);
    int ns = group(&b, rel(&b, 0x20 + 2 * 4), data, names, 256);
    r->skels = zalloc(sizeof(BSkelAnim) * (size_t)(ns + 1));
    for (int i = 0; i < ns; i++) read_skelanim(&b, data[i], &r->skels[r->nskel++]);
    return r;
}
const BSkelAnim *bfres_skelanim(const Bfres *b, const char *name) {
    for (int i = 0; b && i < b->nskel; i++)
        if (!strcmp(b->skels[i].name, name)) return &b->skels[i];
    return NULL;
}
int bfres_skin_bone(const BModel *m, int index) {
    return index >= 0 && index < m->nsmooth ? m->smooth[index] : 0;
}
static float curve_eval(const BFCurve *c, float f) {
    if (!c->nk) return 0;
    int k = 0;
    while (k + 1 < c->nk && c->frame[k + 1] <= f) k++;
    float span = k + 1 < c->nk ? c->frame[k + 1] - c->frame[k] : 1, t = span > 0 ? (f - c->frame[k]) / span : 0;
    if (t < 0) t = 0;
    if (t > 1) t = 1;
    const float *q = c->coef[k];
    float v = c->type == 1 ? q[0] + q[1] * t : q[0] + q[1] * t + q[2] * t * t + q[3] * t * t * t;
    return v * c->scale + c->offset;
}
void bfres_pose(const BModel *m, const BSkelAnim *a, float frame, float (*world)[3][4]) {
    for (int i = 0; i < m->nbones; i++) {
        const BBone *bn = &m->bones[i];
        float s[3], r[4], t[3];
        memcpy(s, bn->s, sizeof s), memcpy(r, bn->r, sizeof r), memcpy(t, bn->t, sizeof t);
        bool euler = !m->quat;
        const BBoneAnim *ba = NULL;
        for (int k = 0; a && k < a->nb; k++)
            if (!strcmp(a->b[k].name, bn->name)) { ba = &a->b[k]; break; }
        if (ba) {
            if (ba->flags & 8) memcpy(s, ba->base_s, sizeof s);
            if (ba->flags & 16) memcpy(r, ba->base_r, sizeof r), euler = a->euler;
            if (ba->flags & 32) memcpy(t, ba->base_t, sizeof t);
            for (int c = 0; c < ba->nc; c++) {
                float v = curve_eval(&ba->c[c], frame);
                int tg = ba->c[c].target;
                if (tg >= 0x4 && tg <= 0xC) s[(tg - 4) / 4] = v;
                else if (tg >= 0x10 && tg <= 0x18) t[(tg - 0x10) / 4] = v;
                else if (tg >= 0x20 && tg <= 0x2C) r[(tg - 0x20) / 4] = v, euler = a->euler;
            }
        }
        float local[3][4];
        if (euler) euler_matrix(s, r, t, local);
        else quat_matrix(s, r, t, local);
        if (bn->parent >= 0 && bn->parent < i) mmul(world[bn->parent], local, world[i]);
        else memcpy(world[i], local, sizeof local);
    }
}
int bfres_texture(const Bfres *b, const char *name) {
    for (int i = 0; b && i < b->ntex; i++)
        if (!strcmp(b->tex_name[i], name)) return b->tex[i];
    return -1;
}
const BModel *bfres_model(const Bfres *b, const char *name) {
    for (int i = 0; b && i < b->nmodels; i++)
        if (!strcmp(b->models[i].name, name)) return &b->models[i];
    return NULL;
}
const BTexPat *bfres_texpat(const Bfres *b, const char *name) {
    for (int i = 0; b && i < b->npat; i++)
        if (!strcmp(b->pats[i].name, name)) return &b->pats[i];
    return NULL;
}

void bfres_dump(const Bfres *b) {
    for (int i = 0; i < b->ntex; i++) {
        const LTex *t = lyt_tex(b->tex[i]);
        fprintf(stderr, "tex %s %dx%d\n", b->tex_name[i], t ? t->w : 0, t ? t->h : 0);
    }
    for (int i = 0; i < b->nmodels; i++) {
        const BModel *m = &b->models[i];
        fprintf(stderr, "model %s: %d bones (%s, flags %#x), %d shapes, %d materials\n", m->name, m->nbones, m->quat ? "quat" : "euler", m->skl_flags, m->nshapes, m->nmats);
        for (int k = 0; k < m->nbones; k++) {
            const BBone *bn = &m->bones[k];
            fprintf(stderr, "  bone %s parent %d s %.2f %.2f %.2f r %.3f %.3f %.3f %.3f t %.2f %.2f %.2f\n", bn->name, bn->parent,
                    bn->s[0], bn->s[1], bn->s[2], bn->r[0], bn->r[1], bn->r[2], bn->r[3], bn->t[0], bn->t[1], bn->t[2]);
        }
        for (int k = 0; k < m->nshapes; k++) {
            const BShape *s = &m->shapes[k];
            fprintf(stderr, "  shape %s mat %d bone %d skin %d verts %d idx %d\n", s->name, s->mat, s->bone, s->skin_count, s->nverts, s->nidx);
            for (int v = 0; v < s->nverts && v < 8; v++)
                fprintf(stderr, "    v%d p %.2f %.2f %.2f uv %.3f %.3f%s\n", v, s->pos[v][0], s->pos[v][1], s->pos[v][2], s->uv[v][0],
                        s->uv[v][1], s->idx0 ? "" : "");
            for (int v = 0; v < s->nidx && v < 12; v++) fprintf(stderr, "%s%u", v ? " " : "    i ", s->idx[v]);
            if (s->nidx) fputc('\n', stderr);
        }
        for (int k = 0; k < m->nmats; k++)
            fprintf(stderr, "  mat %s tex %s %s wrap %d %d\n", m->mats[k].name, m->mats[k].ntex > 0 ? m->mats[k].tex[0] : "-",
                    m->mats[k].ntex > 1 ? m->mats[k].tex[1] : "", m->mats[k].wrap_s[0], m->mats[k].wrap_t[0]);
    }
    for (int i = 0; i < b->npat; i++) {
        const BTexPat *p = &b->pats[i];
        fprintf(stderr, "texpat %s frames %d loop %d tex", p->name, p->frames, p->loop);
        for (int k = 0; k < p->ntex; k++) fprintf(stderr, " %s", p->tex[k]);
        fputc('\n', stderr);
        for (int m = 0; m < p->nmat; m++) {
            fprintf(stderr, "  mat %s npat %d base %d curves %d:", p->mat[m].mat, p->mat[m].npat, p->mat[m].npat ? p->mat[m].base[0] : -1, p->mat[m].ncurves);
            for (int c = 0; c < p->mat[m].ncurves; c++)
                for (int k = 0; k < p->mat[m].curve[c].nkeys && k < 12; k++)
                    fprintf(stderr, " %.1f:%.0f", p->mat[m].curve[c].frame[k], p->mat[m].curve[c].value[k]);
            fputc('\n', stderr);
        }
    }
}
