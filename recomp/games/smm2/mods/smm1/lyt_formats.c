// SMM1 UI file formats (lyt.h): Yaz0, SARC, GX2 surface untiling, BFLIM, BFFNT v3, MSBT, BFLYT, BFLAN, and the
// layout loader. Ported from SMCraft's mmk-formats (our own Rust: archive/, gx2.rs, bflim.rs, bffnt.rs, msbt.rs,
// lyt/), which was written from public descriptions of these formats.
#include "lyt.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "bcsoft.h"  // runtime/nvn: BC1-5 block decoding (bcdec)

// ================================================================ bounds-checked reader
typedef struct {
    const uint8_t *d;
    size_t n;
    bool be;
} R;
static uint8_t r8(const R *r, size_t o) { return o < r->n ? r->d[o] : 0; }
static uint16_t r16(const R *r, size_t o) {
    if (o + 2 > r->n) return 0;
    const uint8_t *p = r->d + o;
    return r->be ? (uint16_t)(p[0] << 8 | p[1]) : (uint16_t)(p[1] << 8 | p[0]);
}
static uint32_t r32(const R *r, size_t o) {
    if (o + 4 > r->n) return 0;
    const uint8_t *p = r->d + o;
    return r->be ? (uint32_t)p[0] << 24 | (uint32_t)p[1] << 16 | (uint32_t)p[2] << 8 | p[3]
                 : (uint32_t)p[3] << 24 | (uint32_t)p[2] << 16 | (uint32_t)p[1] << 8 | p[0];
}
static float rf(const R *r, size_t o) {
    uint32_t u = r32(r, o);
    float f;
    memcpy(&f, &u, 4);
    return f;
}
static void rrgba(const R *r, size_t o, uint8_t out[4]) {
    for (int i = 0; i < 4; i++) out[i] = r8(r, o + i);
}
static void rstr(const R *r, size_t o, size_t max, char *out, size_t cap) {  // NUL-terminated or fixed field
    size_t k = 0;
    while (k + 1 < cap && k < max && o + k < r->n && r->d[o + k]) {
        out[k] = (char)r->d[o + k];
        k++;
    }
    out[k] = 0;
}
static void *zalloc(size_t n) { return calloc(1, n ? n : 1); }

// ================================================================ Yaz0, SARC
uint8_t *lyt_yaz0(const uint8_t *d, size_t n, size_t *out_n) {
    if (n < 16 || memcmp(d, "Yaz0", 4)) return NULL;
    size_t size = (size_t)d[4] << 24 | (size_t)d[5] << 16 | (size_t)d[6] << 8 | d[7], o = 0, i = 16;
    uint8_t *out = malloc(size ? size : 1);
    while (o < size) {
        if (i >= n) goto bad;
        uint8_t code = d[i++];
        for (int b = 7; b >= 0 && o < size; b--) {
            if (code >> b & 1) {
                if (i >= n) goto bad;
                out[o++] = d[i++];
                continue;
            }
            if (i + 1 >= n) goto bad;
            size_t b1 = d[i], b2 = d[i + 1];
            i += 2;
            size_t dist = ((b1 & 15) << 8 | b2) + 1, len = b1 >> 4;
            if (len == 0) {
                if (i >= n) goto bad;
                len = (size_t)d[i++] + 0x12;
            } else
                len += 2;
            if (dist > o) goto bad;
            for (size_t k = 0; k < len && o < size; k++, o++) out[o] = out[o - dist];
        }
    }
    *out_n = size;
    return out;
bad:
    free(out);
    return NULL;
}

bool lyt_sarc_open(LSarc *s, const uint8_t *d, size_t n) {
    memset(s, 0, sizeof *s);
    if (n < 0x20 || memcmp(d, "SARC", 4)) return false;
    R r = {d, n, d[6] == 0xFE && d[7] == 0xFF};
    size_t hs = r16(&r, 4), doff = r32(&r, 0xC), sfat = hs;
    if (memcmp(d + sfat, "SFAT", 4)) return false;
    size_t count = r16(&r, sfat + 6), nodes = sfat + r16(&r, sfat + 4), sfnt = nodes + count * 16;
    if (sfnt + 8 > n || memcmp(d + sfnt, "SFNT", 4)) return false;
    size_t names = sfnt + r16(&r, sfnt + 4);
    s->data = d, s->n = n, s->count = (int)count;
    s->e = zalloc(count * sizeof *s->e);
    for (size_t i = 0; i < count; i++) {
        size_t nd = nodes + i * 16;
        uint32_t attr = r32(&r, nd + 4);
        s->e[i].start = (uint32_t)(doff + r32(&r, nd + 8));
        s->e[i].end = (uint32_t)(doff + r32(&r, nd + 12));
        if (s->e[i].end > n || s->e[i].start > s->e[i].end) s->e[i].start = s->e[i].end = 0;
        if (attr & 0x01000000)
            rstr(&r, names + (attr & 0xFFFFFF) * 4, 127, s->e[i].name, sizeof s->e[i].name);
        else
            snprintf(s->e[i].name, sizeof s->e[i].name, "%08x", r32(&r, nd));
    }
    return true;
}
const uint8_t *lyt_sarc_get(const LSarc *s, const char *name, size_t *len) {
    for (int i = 0; i < s->count; i++)
        if (!strcmp(s->e[i].name, name)) {
            *len = s->e[i].end - s->e[i].start;
            return s->data + s->e[i].start;
        }
    return NULL;
}
void lyt_sarc_close(LSarc *s) {
    free(s->e);
    memset(s, 0, sizeof *s);
}

// ================================================================ textures
static LTex *g_tex;
static int g_ntex, g_captex;
int lyt_tex_add(int w, int h, uint8_t *px) {
    if (g_ntex == g_captex) {
        g_captex = g_captex ? g_captex * 2 : 256;
        g_tex = realloc(g_tex, (size_t)g_captex * sizeof *g_tex);
    }
    g_tex[g_ntex] = (LTex){w, h, px};
    return g_ntex++;
}
const LTex *lyt_tex(int id) { return id >= 0 && id < g_ntex ? &g_tex[id] : NULL; }

// ================================================================ GX2 (R7xx AddrLib) untiling, thin modes, mip 0
// Hardware constants (Wii U "Latte"): 2 pipes, 4 banks, 256-byte pipe interleave.
enum { PIPES = 2, BANKS = 4, PIPE_INTERLEAVE = 256, GROUP_BITS = 8, PIPE_BITS = 1, BANK_BITS = 2 };
static uint32_t gx2_thickness(uint32_t m) { return (m == 3 || m == 7 || m == 11 || m == 13 || m == 15) ? 4 : 1; }
static bool gx2_bank_swapped(uint32_t m) { return m >= 8 && m <= 11 ? true : (m == 14 || m == 15); }
static uint32_t gx2_rotation(uint32_t m) { return m >= 4 && m <= 11 ? PIPES * ((BANKS >> 1) - 1) : m >= 12 ? 1 : 0; }
static uint32_t gx2_aspect(uint32_t m) { return (m == 5 || m == 9) ? 2 : (m == 6 || m == 10) ? 4 : 1; }
static uint32_t slice_swizzle(uint32_t mode, uint32_t swizzle, uint32_t slice) {
    if (mode < 4 || gx2_thickness(mode) != 1) return swizzle;
    uint32_t base = ((swizzle >> 8) & 1) + PIPES * ((swizzle >> 9) & 3);
    return ((base + slice * gx2_rotation(mode)) % (PIPES * BANKS)) << 8;
}
static uint32_t bank_swapped_width(uint32_t mode, uint32_t bpp, uint32_t pitch) {
    if (!gx2_bank_swapped(mode)) return 0;
    uint32_t micro = 64 * gx2_thickness(mode) * bpp / 8, factor = gx2_aspect(mode);
    uint32_t swap_tiles = (256 >> 1) / bpp, swap_width = swap_tiles * 8 * BANKS;
    uint32_t mb = micro < 2048 ? micro : 2048;
    uint32_t height_bytes = (mb / gx2_thickness(mode)) * 8 * PIPES * factor / 8;
    uint32_t swap_max = PIPES * BANKS * 2048 / (height_bytes ? height_bytes : 1);
    uint32_t swap_min = PIPE_INTERLEAVE * 8 * BANKS / bpp;
    if (swap_max < swap_min) swap_max = swap_min;
    uint32_t w = swap_width < swap_min ? swap_min : swap_width > swap_max ? swap_max : swap_width;
    while (w >= 2 * pitch) w >>= 1;
    return w;
}
static void surface_pitch(uint32_t width, uint32_t height, uint32_t bpp, uint32_t mode, uint32_t *pitch, uint32_t *ph) {
    uint32_t pa, ha;
    if (mode == 0) pa = 1, ha = 1;
    else if (mode == 1) pa = 2048 / bpp > 64 ? 2048 / bpp : 64, ha = 1;
    else if (mode == 2 || mode == 3) {
        uint32_t t = gx2_thickness(mode);
        pa = PIPE_INTERLEAVE / bpp / t > 8 ? PIPE_INTERLEAVE / bpp / t : 8;
        ha = 8;
    } else {
        uint32_t a = gx2_aspect(mode), t = gx2_thickness(mode), mw = 8 * BANKS / a, mh = 8 * PIPES * a;
        uint32_t k = mw * (PIPE_INTERLEAVE / bpp / (8 * t));
        pa = mw > k ? mw : k;
        if (gx2_bank_swapped(mode)) {
            uint32_t s = bank_swapped_width(mode, bpp, width);
            if (s > pa) pa = s;
        }
        ha = mh;
    }
    if (!width) width = 1;
    if (!height) height = 1;
    *pitch = (width + pa - 1) / pa * pa;
    *ph = (height + ha - 1) / ha * ha;
}
static uint32_t micro_index(uint32_t x, uint32_t y, uint32_t bpp) {
    uint32_t x0 = x & 1, x1 = x >> 1 & 1, x2 = x >> 2 & 1, y0 = y & 1, y1 = y >> 1 & 1, y2 = y >> 2 & 1, b[6];
    switch (bpp) {
    case 8: b[0] = x0, b[1] = x1, b[2] = x2, b[3] = y1, b[4] = y0, b[5] = y2; break;
    case 16: b[0] = x0, b[1] = x1, b[2] = x2, b[3] = y0, b[4] = y1, b[5] = y2; break;
    case 64: b[0] = x0, b[1] = y0, b[2] = x1, b[3] = x2, b[4] = y1, b[5] = y2; break;
    case 128: b[0] = y0, b[1] = x0, b[2] = x1, b[3] = x2, b[4] = y1, b[5] = y2; break;
    default: b[0] = x0, b[1] = x1, b[2] = y0, b[3] = x2, b[4] = y1, b[5] = y2; break;
    }
    uint32_t v = 0;
    for (int i = 0; i < 6; i++) v |= b[i] << i;
    return v;
}
static uint64_t elem_addr(uint32_t x, uint32_t y, uint32_t bpp, uint32_t pitch, uint32_t mode, uint32_t swizzle) {
    if (mode <= 1) return ((uint64_t)y * pitch + x) * bpp / 8;
    if (mode == 2 || mode == 3) {
        uint64_t micro = 64 * bpp / 8, tile = (x >> 3) + (uint64_t)(y >> 3) * (pitch >> 3);
        return tile * micro + (uint64_t)micro_index(x, y, bpp) * bpp / 8;
    }
    uint64_t elem = (uint64_t)micro_index(x, y, bpp) * bpp / 8;
    uint32_t pipe = ((y >> 3) ^ (x >> 3)) & 1;
    uint32_t bank = (((y / (16 * PIPES)) ^ (x >> 3)) & 1) | ((((y / (8 * PIPES)) ^ (x >> 4)) & 1) << 1);
    uint32_t bp = (pipe + PIPES * bank) ^ (((swizzle >> 8) & 1) + PIPES * ((swizzle >> 9) & 3));
    bp %= PIPES * BANKS;
    pipe = bp % PIPES, bank = bp / PIPES;
    uint32_t a = gx2_aspect(mode), mp = 8 * BANKS / a, mh = 8 * PIPES * a;
    uint64_t per_row = pitch / mp, mbytes = ((uint64_t)gx2_thickness(mode) * bpp * mh * mp + 7) / 8;
    uint64_t mx = x / mp, my = y / mh, moff = (mx + per_row * my) * mbytes;
    if (gx2_bank_swapped(mode)) {
        static const uint32_t order[4] = {0, 1, 3, 2};
        uint64_t w = bank_swapped_width(mode, bpp, pitch);
        if (!w) w = 1;
        bank ^= order[(mp * mx / w) & (BANKS - 1)];
    }
    uint64_t gmask = (1ull << GROUP_BITS) - 1, total = elem + (moff >> (BANK_BITS + PIPE_BITS));
    return ((uint64_t)bank << (PIPE_BITS + GROUP_BITS)) | ((uint64_t)pipe << GROUP_BITS) | (total & gmask) |
           ((total & ~gmask) << (BANK_BITS + PIPE_BITS));
}
// tiled -> linear rows of w x h elements; NULL if the data is short
static uint8_t *gx2_untile(const uint8_t *d, size_t n, uint32_t w, uint32_t h, uint32_t bpp, uint32_t mode, uint32_t swz) {
    if (gx2_thickness(mode) != 1 || mode > 15) return NULL;
    uint32_t pitch, ph, eb = bpp / 8;
    surface_pitch(w, h, bpp, mode, &pitch, &ph);
    uint8_t *out = zalloc((size_t)w * h * eb);
    for (uint32_t y = 0; y < h; y++)
        for (uint32_t x = 0; x < w; x++) {
            uint64_t s = elem_addr(x, y, bpp, pitch, mode, swz);
            if (s + eb > n) { free(out); return NULL; }
            memcpy(out + ((size_t)y * w + x) * eb, d + s, eb);
        }
    return out;
}

// ================================================================ BFLIM / surfaces -> RGBA8
// Cafe format codes: 0 L8, 1 A8, 2 LA4, 3 LA8, 5 RGB565, 7 RGB5A1, 8 RGBA4, 9 RGBA8, 12 BC1, 13 BC2, 14 BC3,
// 15 BC4 (luminance), 16 BC4 (alpha), 17 BC5 (luminance + alpha), 20 RGBA8 sRGB, 21/22/23 BC1/2/3 sRGB, 25 RGB565.
// sRGB formats are kept as their stored (encoded) values: the screens are composed in gamma space [inf].
static int elem_bits(int f) {
    switch (f) {
    case 0: case 1: case 2: return 8;
    case 3: case 5: case 7: case 8: case 25: return 16;
    case 9: case 20: return 32;
    case 12: case 15: case 16: case 21: return 64;
    case 13: case 14: case 17: case 22: case 23: return 128;
    default: return 0;
    }
}
static uint8_t *surface_rgba_x(const uint8_t *d, size_t n, uint32_t w, uint32_t h, int fmt, uint32_t mode, uint32_t swz, bool raw) {
    int bits = elem_bits(fmt);
    if (!bits) return NULL;
    bool block = bits >= 64 && fmt != 9 && fmt != 20;
    uint32_t ew = block ? (w + 3) / 4 : w, eh = block ? (h + 3) / 4 : h;
    uint8_t *lin = gx2_untile(d, n, ew, eh, (uint32_t)bits, mode, swz);
    if (!lin) return NULL;
    uint8_t *px = zalloc((size_t)w * h * 4);
    size_t np = (size_t)w * h;
    if (block) {
        int kind = (fmt == 12 || fmt == 21) ? BCSOFT_BC1 : (fmt == 13 || fmt == 22) ? BCSOFT_BC2
                 : (fmt == 14 || fmt == 23) ? BCSOFT_BC3 : (fmt == 15 || fmt == 16) ? BCSOFT_BC4 : BCSOFT_BC5;
        bcsoft_decode(kind, lin, w, h, 1, px);
        for (size_t i = 0; !raw && i < np; i++) {
            uint8_t *p = px + i * 4, r = p[0], g = p[1];
            if (fmt == 15) p[0] = p[1] = p[2] = r, p[3] = 255;
            else if (fmt == 16) p[0] = p[1] = p[2] = 255, p[3] = r;
            else if (fmt == 17) p[0] = p[1] = p[2] = r, p[3] = g;
        }
    } else {
        for (size_t i = 0; i < np; i++) {
            uint8_t *p = px + i * 4;
            const uint8_t *s = lin + i * (bits / 8);
            uint16_t v = (uint16_t)(s[0] | (bits >= 16 ? s[1] << 8 : 0));
#define X5(v) (uint8_t)(((v) << 3) | ((v) >> 2))
#define X6(v) (uint8_t)(((v) << 2) | ((v) >> 4))
            switch (fmt) {
            case 0: p[0] = p[1] = p[2] = s[0], p[3] = 255; break;
            case 1: p[0] = p[1] = p[2] = 255, p[3] = s[0]; break;
            case 2: p[0] = p[1] = p[2] = (uint8_t)((s[0] >> 4) * 17), p[3] = (uint8_t)((s[0] & 15) * 17); break;
            case 3: p[0] = p[1] = p[2] = s[0], p[3] = s[1]; break;
            case 5: case 25: p[0] = X5(v & 31), p[1] = X6((v >> 5) & 63), p[2] = X5(v >> 11), p[3] = 255; break;
            case 7: p[0] = X5(v & 31), p[1] = X5((v >> 5) & 31), p[2] = X5((v >> 10) & 31), p[3] = v >> 15 ? 255 : 0; break;
            case 8: p[0] = (uint8_t)((v & 15) * 17), p[1] = (uint8_t)((v >> 4 & 15) * 17), p[2] = (uint8_t)((v >> 8 & 15) * 17),
                    p[3] = (uint8_t)((v >> 12) * 17); break;
            default: memcpy(p, s, 4); break;
            }
#undef X5
#undef X6
        }
    }
    free(lin);
    return px;
}

static uint8_t *surface_rgba(const uint8_t *d, size_t n, uint32_t w, uint32_t h, int fmt, uint32_t mode, uint32_t swz) {
    return surface_rgba_x(d, n, w, h, fmt, mode, swz, false);
}
uint8_t *lyt_surface_raw(const uint8_t *d, size_t n, uint32_t w, uint32_t h, int fmt, uint32_t mode, uint32_t swz) {
    return surface_rgba_x(d, n, w, h, fmt, mode, swz, true);
}

uint8_t *lyt_flim_decode(const uint8_t *d, size_t n, int *w, int *h) {
    if (n < 0x28) return NULL;
    const uint8_t *f = d + n - 0x28;
    if (memcmp(f, "FLIM", 4) || memcmp(f + 0x14, "imag", 4)) return NULL;
    R r = {f, 0x28, f[4] == 0xFE && f[5] == 0xFF};
    uint32_t tile = r8(&r, 0x23), size = r32(&r, 0x24);
    if (size > n - 0x28) return NULL;
    *w = r16(&r, 0x1C), *h = r16(&r, 0x1E);
    return surface_rgba(d, size, (uint32_t)*w, (uint32_t)*h, r8(&r, 0x22), tile & 0x1F, (tile >> 5) << 8);
}

// ================================================================ BFFNT (Wii U v3)
bool lyt_font_has(const LFont *f, uint32_t c) { return c < 0x10000 && f->cmap[c] != 0xFFFF; }

int lyt_font_sheet(LFont *f, int i) {
    if (i < 0 || i >= f->nsheets) return -1;
    if (f->sheet_tex[i] != -2) return f->sheet_tex[i];
    f->sheet_tex[i] = -1;
    if (f->sheet_fmt < 0 || f->data_off + (size_t)(i + 1) * f->sheet_size > f->raw_n) return -1;
    // the sheets are the slices of one 2D array surface (each its own bank rotation), stored bottom row first
    uint8_t *px = surface_rgba(f->raw + f->data_off + (size_t)i * f->sheet_size, f->sheet_size, f->sheet_w, f->sheet_h,
                               f->sheet_fmt, f->sheet_mode, slice_swizzle(f->sheet_mode, 0, (uint32_t)i));
    if (!px) return -1;
    size_t row = (size_t)f->sheet_w * 4;
    uint8_t *tmp = malloc(row);
    for (int y = 0; y < f->sheet_h / 2; y++) {
        memcpy(tmp, px + y * row, row);
        memcpy(px + y * row, px + (size_t)(f->sheet_h - 1 - y) * row, row);
        memcpy(px + (size_t)(f->sheet_h - 1 - y) * row, tmp, row);
    }
    free(tmp);
    return f->sheet_tex[i] = lyt_tex_add(f->sheet_w, f->sheet_h, px);
}

LFont *lyt_font_parse(const char *name, const uint8_t *d, size_t n) {
    if (n < 0x14 || memcmp(d, "FFNT", 4)) return NULL;
    R r = {d, n, d[4] == 0xFE && d[5] == 0xFF};
    size_t fi = r16(&r, 6);
    if (fi + 4 > n || memcmp(d + fi, "FINF", 4)) return NULL;
    LFont *f = zalloc(sizeof *f);
    snprintf(f->name, sizeof f->name, "%s", name);
    size_t b = fi + 8;
    f->height = r8(&r, b + 1), f->width = r8(&r, b + 2), f->ascent = r8(&r, b + 3);
    f->line_feed = r16(&r, b + 4), f->alter = r16(&r, b + 6);
    f->defw = (LCharW){(int8_t)r8(&r, b + 8), r8(&r, b + 9), r8(&r, b + 10)};
    size_t t = r32(&r, b + 12), cwdh = r32(&r, b + 16), cmap = r32(&r, b + 20);
    f->cell_w = r8(&r, t), f->cell_h = r8(&r, t + 1);
    int sheets = r8(&r, t + 2);
    size_t sheet_size = r32(&r, t + 4);
    f->baseline = r16(&r, t + 8);
    uint16_t sheet_format = r16(&r, t + 10);
    f->cells_row = r16(&r, t + 12), f->cells_col = r16(&r, t + 14);
    f->sheet_w = r16(&r, t + 16), f->sheet_h = r16(&r, t + 18);
    size_t data = r32(&r, t + 20);
    // sheet formats: 0 RGBA8, 3 RGB565, 5 LA8, 6 LA4, 8 A8, 9 BC1, 10 BC2, 11 BC3, 12 BC4 (alpha), 13 BC5; bit 15 linear
    static const int fmt_of[14] = {9, -1, -1, 5, -1, 3, 2, -1, 1, 12, 13, 14, 16, 17};
    int code = sheet_format & 0x3FFF, fmt = code < 14 ? fmt_of[code] : -1;
    uint32_t mode = sheet_format & 0x8000 ? 0 : 4;
    f->sheet_tex = zalloc(sizeof(int) * (size_t)(sheets ? sheets : 1));
    for (int i = 0; i < sheets; i++) f->sheet_tex[i] = -2;  // (not decoded yet)
    f->nsheets = sheets;
    f->raw = malloc(n);
    memcpy(f->raw, d, n);
    f->raw_n = n, f->sheet_size = sheet_size, f->data_off = data, f->sheet_fmt = fmt, f->sheet_mode = mode;
    // widths: chained CWDH blocks
    int maxg = 0;
    for (size_t c = cwdh, g = 0; c && g < 4096; g++) {
        int last = r16(&r, c + 2);
        if (last + 1 > maxg) maxg = last + 1;
        c = r32(&r, c + 4);
    }
    f->nglyphs = maxg;
    f->widths = zalloc(sizeof(LCharW) * (size_t)(maxg ? maxg : 1));
    for (int i = 0; i < maxg; i++) f->widths[i] = f->defw;
    for (size_t c = cwdh, g = 0; c && g < 4096; g++) {
        int first = r16(&r, c), last = r16(&r, c + 2);
        for (int k = 0; first + k <= last; k++) {
            size_t e = c + 8 + (size_t)k * 3;
            f->widths[first + k] = (LCharW){(int8_t)r8(&r, e), r8(&r, e + 1), r8(&r, e + 2)};
        }
        c = r32(&r, c + 4);
    }
    f->cmap = malloc(0x10000 * sizeof(uint16_t));
    memset(f->cmap, 0xFF, 0x10000 * sizeof(uint16_t));
    for (size_t c = cmap, g = 0; c && g < 4096; g++) {
        uint32_t first = r16(&r, c), last = r16(&r, c + 2), method = r16(&r, c + 4);
        size_t next = r32(&r, c + 8), dd = c + 12;
        if (last >= first) {
            if (method == 0) {
                uint16_t base = r16(&r, dd);
                for (uint32_t k = first; k <= last; k++) f->cmap[k] = (uint16_t)(base + (k - first));
            } else if (method == 1) {
                for (uint32_t k = first; k <= last; k++) {
                    uint16_t idx = r16(&r, dd + (k - first) * 2);
                    if (idx != 0xFFFF) f->cmap[k] = idx;
                }
            } else {
                int cnt = r16(&r, dd);
                for (int k = 0; k < cnt; k++) {
                    uint16_t code16 = r16(&r, dd + 2 + (size_t)k * 4), idx = r16(&r, dd + 4 + (size_t)k * 4);
                    if (idx != 0xFFFF) f->cmap[code16] = idx;
                }
            }
        }
        c = next;
    }
    return f;
}

// ================================================================ MSBT
bool lyt_msbt_parse(LMsbt *out, const uint8_t *d, size_t n) {
    memset(out, 0, sizeof *out);
    if (n < 0x20 || memcmp(d, "MsgStdBn", 8)) return false;
    R r = {d, n, d[8] == 0xFE && d[9] == 0xFF};
    int enc = r8(&r, 0x0C), sections = r16(&r, 0x0E);
    if (enc != 1) return false;  // SMM1's English tables are UTF-16
    struct { char label[64]; uint32_t index; } *labels = NULL;
    int nl = 0, capl = 0;
    struct { uint32_t *t; int n; } *texts = NULL;
    int nt = 0;
    size_t o = 0x20;
    for (int s = 0; s < sections && o + 16 <= n; s++) {
        size_t size = r32(&r, o + 4), dd = o + 0x10;
        if (dd + size > n) break;
        if (!memcmp(d + o, "LBL1", 4)) {
            uint32_t buckets = r32(&r, dd);
            for (uint32_t b = 0; b < buckets; b++) {
                uint32_t cnt = r32(&r, dd + 4 + b * 8);
                size_t p = dd + r32(&r, dd + 8 + b * 8);
                for (uint32_t k = 0; k < cnt && p < n; k++) {
                    int len = r8(&r, p);
                    if (nl == capl) capl = capl ? capl * 2 : 64, labels = realloc(labels, (size_t)capl * sizeof *labels);
                    rstr(&r, p + 1, (size_t)len, labels[nl].label, sizeof labels[nl].label);
                    labels[nl].index = r32(&r, p + 1 + len);
                    nl++;
                    p += 1 + (size_t)len + 4;
                }
            }
        } else if (!memcmp(d + o, "TXT2", 4)) {
            uint32_t cnt = r32(&r, dd);
            texts = zalloc(sizeof *texts * (cnt ? cnt : 1));
            nt = (int)cnt;
            for (uint32_t i = 0; i < cnt; i++) {
                size_t start = dd + r32(&r, dd + 4 + i * 4), end = i + 1 < cnt ? dd + r32(&r, dd + 8 + i * 4) : dd + size;
                uint32_t *t = zalloc(sizeof(uint32_t) * ((end > start ? end - start : 0) / 2 + 1));
                int k = 0;
                for (size_t p = start; p + 2 <= end;) {
                    uint32_t u = r16(&r, p);
                    p += 2;
                    if (u == 0) break;
                    if (u == 0x0E) {  // tag: group, type, param size, params
                        uint16_t group = r16(&r, p), psz = r16(&r, p + 4);
                        p += 6 + psz;
                        if (group == 2 || group == 4 || group == 250) t[k++] = LYT_ARG;
                        continue;
                    }
                    if (u == 0x0F) { p += 4; continue; }  // tag end
                    if (u >= 0xD800 && u < 0xDC00 && p + 2 <= end) {
                        uint32_t lo = r16(&r, p);
                        if (lo >= 0xDC00 && lo < 0xE000) { p += 2; u = 0x10000 + ((u - 0xD800) << 10) + (lo - 0xDC00); }
                    }
                    t[k++] = u;
                }
                texts[i].t = t, texts[i].n = k;
            }
        }
        o = (dd + size + 15) / 16 * 16;
    }
    out->m = zalloc(sizeof(LMsg) * (size_t)(nl ? nl : 1));
    for (int i = 0; i < nl; i++) {
        LMsg *m = &out->m[out->count];
        snprintf(m->label, sizeof m->label, "%s", labels[i].label);
        if ((int)labels[i].index < nt) {
            m->n = texts[labels[i].index].n;
            m->text = malloc(sizeof(uint32_t) * (size_t)(m->n + 1));
            memcpy(m->text, texts[labels[i].index].t, sizeof(uint32_t) * (size_t)m->n);
        } else
            m->text = zalloc(4);
        out->count++;
    }
    for (int i = 0; i < nt; i++) free(texts[i].t);
    free(texts);
    free(labels);
    return true;
}
const LMsg *lyt_msbt_get(const LMsbt *t, const char *label) {
    for (int i = 0; t && i < t->count; i++)
        if (!strcmp(t->m[i].label, label)) return &t->m[i];
    return NULL;
}

// ================================================================ BFLYT
// texture-coordinate generator entries are 8 bytes in Wii U v5+ files, 4 in older ones: pick the size that makes
// the material end where the offset table says (else the 8-byte reading)
static size_t parse_material_with(const R *r, size_t start, size_t gen, LMat *m);
static void parse_material(const R *r, size_t start, size_t end, LMat *m) {
    if (parse_material_with(r, start, 8, m) == end) return;
    if (parse_material_with(r, start, 4, m) == end) return;
    parse_material_with(r, start, 8, m);
}
static size_t parse_material_with(const R *r, size_t start, size_t gen, LMat *m) {
    {
        memset(m, 0, sizeof *m);
        rstr(r, start, 0x1C, m->name, sizeof m->name);
        rrgba(r, start + 0x1C, m->black);
        rrgba(r, start + 0x20, m->white);
        uint32_t fl = r32(r, start + 0x24);
        int ntex = fl & 3, nmtx = fl >> 2 & 3, ngen = fl >> 4 & 3, ntev = fl >> 6 & 7, nproj = fl >> 15 & 3;
        bool ac = fl >> 9 & 1, bl = fl >> 10 & 1, bll = fl >> 12 & 1, ind = fl >> 14 & 1, sh = fl >> 17 & 1;
        m->texture_only = fl >> 11 & 1;
        size_t o = start + 0x28;
        for (int i = 0; i < ntex; i++, o += 4) {
            uint8_t a = r8(r, o + 2), b = r8(r, o + 3);
            m->tex[i] = (LTexMap){r16(r, o), a & 3, b & 3, a >> 2 & 3, b >> 2 & 3};
        }
        for (int i = 0; i < nmtx; i++, o += 20)
            m->srt[i] = (LTexSrt){{rf(r, o), rf(r, o + 4)}, rf(r, o + 8), {rf(r, o + 12), rf(r, o + 16)}};
        for (int i = 0; i < ngen; i++, o += gen) m->gen_src[i] = r8(r, o + 1);
        for (int i = 0; i < ntev; i++, o += 4) m->tev_c[i] = r8(r, o), m->tev_a[i] = r8(r, o + 1);
        if (ac) o += 8;
        if (bl) {
            m->has_blend = true;
            m->blend_op = r8(r, o), m->blend_src = r8(r, o + 1), m->blend_dst = r8(r, o + 2);
            o += 4;
        }
        if (bll) o += 4;
        if (ind) o += 12;
        for (int i = 0; i < nproj; i++, o += 20)
            m->proj[i] = (LProj){{rf(r, o), rf(r, o + 4)}, {rf(r, o + 8), rf(r, o + 12)}, r8(r, o + 16)};
        if (sh) o += 8;
        m->ntex = ntex, m->nsrt = nmtx, m->ngen = ngen, m->ntev = ntev, m->nproj = nproj;
        return o;
    }
}

static void parse_picture(const R *r, size_t o, LPic *p) {
    for (int i = 0; i < 4; i++) rrgba(r, o + (size_t)i * 4, p->vc[i]);
    p->mat = r16(r, o + 16);
    int nuv = r8(r, o + 18);
    if (nuv > 3) nuv = 3;
    p->nuv = nuv;
    for (int s = 0; s < nuv; s++)
        for (int k = 0; k < 4; k++) {
            p->uv[s][k][0] = rf(r, o + 20 + (size_t)s * 32 + (size_t)k * 8);
            p->uv[s][k][1] = rf(r, o + 24 + (size_t)s * 32 + (size_t)k * 8);
        }
}

static uint32_t *utf16_at(const R *r, size_t o, size_t max_units, int *n) {
    uint32_t *t = zalloc(sizeof(uint32_t) * (max_units + 1));
    int k = 0;
    for (size_t i = 0; i < max_units; i++) {
        uint32_t u = r16(r, o + i * 2);
        if (!u || o + i * 2 + 2 > r->n) break;
        t[k++] = u;
    }
    *n = k;
    return t;
}

static void parse_pane(const R *r, size_t o, size_t size, LPane *p);
static void parse_pane(const R *r, size_t o, size_t size, LPane *p) {
    memset(p, 0, sizeof *p);
    char m[5] = {0};
    memcpy(m, r->d + o, 4);
    size_t b = o + 8;
    p->flags = r8(r, b), p->origin = r8(r, b + 1), p->alpha = r8(r, b + 2), p->mag = r8(r, b + 3);
    rstr(r, b + 4, 24, p->name, sizeof p->name);
    for (int i = 0; i < 3; i++) p->t[i] = rf(r, b + 0x24 + (size_t)i * 4), p->r[i] = rf(r, b + 0x30 + (size_t)i * 4);
    p->s[0] = rf(r, b + 0x3C), p->s[1] = rf(r, b + 0x40), p->size[0] = rf(r, b + 0x44), p->size[1] = rf(r, b + 0x48);
    p->parent = -1;
    size_t x = o + 0x54;
    if (!strcmp(m, "pic1")) {
        p->kind = LP_PIC;
        parse_picture(r, x, &p->pic);
    } else if (!strcmp(m, "txt1")) {
        p->kind = LP_TXT;
        LTxt *t = &p->txt;
        size_t text_off = r32(r, x + 0x10), str_bytes = r16(r, x + 2), id_off = r32(r, x + 0x2C);
        t->mat = r16(r, x + 4), t->font = r16(r, x + 6);
        t->pos = r8(r, x + 8), t->lalign = r8(r, x + 9), t->flags = r8(r, x + 10);
        t->italic = rf(r, x + 0x0C);
        rrgba(r, x + 0x14, t->top);
        rrgba(r, x + 0x18, t->bot);
        t->fsize[0] = rf(r, x + 0x1C), t->fsize[1] = rf(r, x + 0x20);
        t->cspace = rf(r, x + 0x24), t->lspace = rf(r, x + 0x28);
        if (id_off && id_off < size) rstr(r, o + id_off, 63, t->text_id, sizeof t->text_id);
        if (text_off && text_off < size) t->text = utf16_at(r, o + text_off, str_bytes / 2, &t->ntext);
        else t->text = zalloc(4);
        t->sh_off[0] = rf(r, x + 0x30), t->sh_off[1] = rf(r, x + 0x34);
        t->sh_scale[0] = rf(r, x + 0x38), t->sh_scale[1] = rf(r, x + 0x3C);
        rrgba(r, x + 0x40, t->sh_top);
        rrgba(r, x + 0x44, t->sh_bot);
    } else if (!strcmp(m, "wnd1")) {
        p->kind = LP_WND;
        LWnd *w = &p->wnd;
        for (int i = 0; i < 4; i++) w->infl[i] = (int16_t)r16(r, x + (size_t)i * 2), w->fsz[i] = r16(r, x + 8 + (size_t)i * 2);
        int nf = r8(r, x + 0x10);
        w->flags = r8(r, x + 0x11);
        parse_picture(r, o + r32(r, x + 0x14), &w->content);
        size_t table = o + r32(r, x + 0x18);
        if (nf > 8) nf = 8;
        w->nframes = nf;
        for (int i = 0; i < nf; i++) {
            size_t f = o + r32(r, table + (size_t)i * 4);
            w->fmat[i] = r16(r, f), w->fflip[i] = r8(r, f + 2);
        }
    } else if (!strcmp(m, "prt1")) {
        p->kind = LP_PRT;
        LPrt *pt = &p->prt;
        uint32_t count = r32(r, x);
        pt->magnify[0] = rf(r, x + 4), pt->magnify[1] = rf(r, x + 8);
        size_t props = x + 12;
        pt->props = zalloc(sizeof(LProp) * (count ? count : 1));
        pt->nprops = (int)count;
        for (uint32_t i = 0; i < count; i++) {
            size_t q = props + (size_t)i * 40;
            LProp *pr = &pt->props[i];
            rstr(r, q, 24, pr->name, sizeof pr->name);
            pr->usage = r8(r, q + 24), pr->basic_usage = r8(r, q + 25), pr->mat_usage = r8(r, q + 26);
            size_t po = r32(r, q + 28);
            if (po && po + 8 <= size) {
                size_t at = o + po;
                if (!memcmp(r->d + at, "pan1", 4) || !memcmp(r->d + at, "pic1", 4) || !memcmp(r->d + at, "txt1", 4) ||
                    !memcmp(r->d + at, "wnd1", 4) || !memcmp(r->d + at, "bnd1", 4)) {
                    pr->pane = zalloc(sizeof(LPane));
                    parse_pane(r, at, r32(r, at + 4), pr->pane);
                }
            }
            size_t bo = r32(r, q + 36);
            if (bo && bo + 52 <= size) {
                size_t bb = o + bo;
                pr->has_basic = true;
                for (int k = 0; k < 3; k++) pr->t[k] = rf(r, bb + 8 + (size_t)k * 4), pr->r[k] = rf(r, bb + 20 + (size_t)k * 4);
                pr->s[0] = rf(r, bb + 32), pr->s[1] = rf(r, bb + 36), pr->size[0] = rf(r, bb + 40), pr->size[1] = rf(r, bb + 44);
                pr->alpha = r8(r, bb + 48);
            }
        }
        rstr(r, props + (size_t)count * 40, 63, pt->layout, sizeof pt->layout);
    } else if (!strcmp(m, "bnd1"))
        p->kind = LP_BND;
    else
        p->kind = LP_NULL;
}

static bool is_pane(const uint8_t *m) {
    return !memcmp(m, "pan1", 4) || !memcmp(m, "pic1", 4) || !memcmp(m, "txt1", 4) || !memcmp(m, "wnd1", 4) ||
           !memcmp(m, "bnd1", 4) || !memcmp(m, "prt1", 4);
}

typedef struct {
    char (*tex)[64];
    int ntex;
    char (*font)[64];
    int nfont;
} Names;

static void name_table(const R *r, size_t body, char (**out)[64], int *n) {
    int count = r16(r, body);
    *out = zalloc(64 * (size_t)(count ? count : 1));
    for (int i = 0; i < count; i++) rstr(r, body + 4 + r32(r, body + 4 + (size_t)i * 4), 63, (*out)[i], 64);
    *n = count;
}

static bool parse_layout(const uint8_t *d, size_t n, LRes *res, Names *names) {
    if (n < 0x14 || memcmp(d, "FLYT", 4)) return false;
    R r = {d, n, d[4] == 0xFE && d[5] == 0xFF};
    size_t o = r16(&r, 6);
    int sections = r16(&r, 0x10), cap = 64, stack[64], sp = 0, last = -1;
    res->pane = zalloc(sizeof(LPane) * (size_t)cap);
    stack[0] = -1;
    for (int s = 0; s < sections && o + 8 <= n; s++) {
        const uint8_t *m = d + o;
        size_t size = r32(&r, o + 4), body = o + 8;
        if (size < 8 || o + size > n) break;
        if (!memcmp(m, "lyt1", 4)) {
            res->w = rf(&r, body + 4), res->h = rf(&r, body + 8);
        } else if (!memcmp(m, "txl1", 4))
            name_table(&r, body, &names->tex, &names->ntex);
        else if (!memcmp(m, "fnl1", 4))
            name_table(&r, body, &names->font, &names->nfont);
        else if (!memcmp(m, "mat1", 4)) {
            int count = r16(&r, body);
            res->mat = zalloc(sizeof(LMat) * (size_t)(count ? count : 1));
            res->nmat = count;
            for (int i = 0; i < count; i++) {
                size_t st = o + r32(&r, body + 4 + (size_t)i * 4);
                size_t en = i + 1 < count ? o + r32(&r, body + 8 + (size_t)i * 4) : o + size;
                parse_material(&r, st, en, &res->mat[i]);
            }
        } else if (is_pane(m)) {
            if (res->npane == cap) cap *= 2, res->pane = realloc(res->pane, sizeof(LPane) * (size_t)cap);
            parse_pane(&r, o, size, &res->pane[res->npane]);
            res->pane[res->npane].parent = stack[sp];
            last = res->npane++;
        } else if (!memcmp(m, "pas1", 4)) {
            if (sp < 63) stack[++sp] = last;
        } else if (!memcmp(m, "pae1", 4)) {
            if (sp > 0) last = stack[sp--];
        }
        o += size;
    }
    return res->npane > 0;
}

// ================================================================ BFLAN
float lyt_eval(const LAEntry *e, float f) {
    if (!e->nk) return 0;
    const LKey *k = e->k;
    if (e->nk == 1 || f <= k[0].f) return k[0].v;
    if (f >= k[e->nk - 1].f) return k[e->nk - 1].v;
    int i = 0;  // last key with frame <= f (ties: the later key wins)
    while (i + 1 < e->nk && k[i + 1].f <= f) i++;
    const LKey *a = &k[i], *b = &k[i + 1];
    if (e->curve != 2) return a->v;
    float span = b->f - a->f;
    if (span <= 0) return b->v;
    float t = (f - a->f) / span, t2 = t * t, t3 = t2 * t;
    return (2 * t3 - 3 * t2 + 1) * a->v + (t3 - 2 * t2 + t) * span * a->s + (-2 * t3 + 3 * t2) * b->v +
           (t3 - t2) * span * b->s;
}

static bool parse_anim(const uint8_t *d, size_t n, LAnim *a, char (**texnames)[64]) {
    memset(a, 0, sizeof *a);
    *texnames = NULL;
    if (n < 0x14 || memcmp(d, "FLAN", 4)) return false;
    R r = {d, n, d[4] == 0xFE && d[5] == 0xFF};
    size_t o = r16(&r, 6);
    int sections = r16(&r, 0x10);
    for (int s = 0; s < sections && o + 8 <= n; s++) {
        size_t size = r32(&r, o + 4), b = o + 8;
        if (size < 8) break;
        if (!memcmp(d + o, "pat1", 4)) {
            rstr(&r, o + r32(&r, b + 4), 63, a->name, sizeof a->name);
        } else if (!memcmp(d + o, "pai1", 4)) {
            a->frames = r16(&r, b);
            a->loop = r8(&r, b + 2) != 0;
            int ntex = r16(&r, b + 4), nc = r16(&r, b + 6);
            size_t table = o + r32(&r, b + 8), tt = b + 12;
            a->ntex = ntex;
            *texnames = zalloc(64 * (size_t)(ntex ? ntex : 1));
            for (int i = 0; i < ntex; i++) rstr(&r, tt + r32(&r, tt + (size_t)i * 4), 63, (*texnames)[i], 64);
            a->c = zalloc(sizeof(LACont) * (size_t)(nc ? nc : 1));
            a->nc = nc;
            for (int i = 0; i < nc; i++) {
                size_t c = o + r32(&r, table + (size_t)i * 4);
                LACont *ct = &a->c[i];
                rstr(&r, c, 28, ct->name, sizeof ct->name);
                ct->nt = r8(&r, c + 28), ct->kind = r8(&r, c + 29), ct->target = -1;
                ct->t = zalloc(sizeof(LATag) * (size_t)(ct->nt ? ct->nt : 1));
                for (int t = 0; t < ct->nt; t++) {
                    size_t to = c + r32(&r, c + 32 + (size_t)t * 4);
                    LATag *tg = &ct->t[t];
                    memcpy(tg->magic, d + (to + 4 <= n ? to : 0), 4);
                    tg->ne = r8(&r, to + 4);
                    tg->e = zalloc(sizeof(LAEntry) * (size_t)(tg->ne ? tg->ne : 1));
                    for (int e = 0; e < tg->ne; e++) {
                        size_t eo = to + r32(&r, to + 8 + (size_t)e * 4);
                        LAEntry *en = &tg->e[e];
                        en->index = r8(&r, eo), en->target = r8(&r, eo + 1);
                        uint8_t cv = r8(&r, eo + 2);
                        en->curve = cv > 2 ? 2 : cv;
                        en->nk = r16(&r, eo + 4);
                        size_t ko = eo + r32(&r, eo + 8);
                        en->k = zalloc(sizeof(LKey) * (size_t)(en->nk ? en->nk : 1));
                        for (int k = 0; k < en->nk; k++) {
                            if (en->curve == 2) {
                                size_t p = ko + (size_t)k * 12;
                                en->k[k] = (LKey){rf(&r, p), rf(&r, p + 4), rf(&r, p + 8)};
                            } else {
                                size_t p = ko + (size_t)k * 8;
                                en->k[k] = (LKey){rf(&r, p), (float)r16(&r, p + 4), 0};
                            }
                        }
                    }
                }
            }
        }
        o += size;
    }
    return true;
}

// ================================================================ loading a layout with its parts
static LLoader g_loader;
void lyt_set_loader(const LLoader *l) { g_loader = *l; }

static struct { char name[64]; LRes *res; bool loading; } *g_lyts;
static int g_nlyts;

int lyt_pane_index(const LRes *r, const char *name) {
    for (int i = 0; i < r->npane; i++)
        if (!strcmp(r->pane[i].name, name)) return i;
    return -1;
}
int lyt_anim_index(const LRes *r, const char *name) {
    for (int i = 0; i < r->nanim; i++)
        if (!strcmp(r->anim[i].name, name)) return i;
    return -1;
}
static void stem(const char *s, char *out, size_t cap) {
    snprintf(out, cap, "%s", s);
    char *dot = strstr(out, ".bflim");
    if (dot) *dot = 0;
}
static int timg_lookup(const LRes *r, const char *name) {
    char s[64];
    stem(name, s, sizeof s);
    for (int i = 0; i < r->ntimg; i++)
        if (!strcmp(r->timg_name[i], s)) return r->timg[i];
    return -1;
}

LRes *lyt_load(const char *name) {
    for (int i = 0; i < g_nlyts; i++)
        if (!strcmp(g_lyts[i].name, name)) return g_lyts[i].loading ? NULL : g_lyts[i].res;
    g_lyts = realloc(g_lyts, sizeof *g_lyts * (size_t)(g_nlyts + 1));
    int slot = g_nlyts++;
    snprintf(g_lyts[slot].name, sizeof g_lyts[slot].name, "%s", name);
    g_lyts[slot].res = NULL;
    g_lyts[slot].loading = true;
    size_t an = 0;
    uint8_t *arc = g_loader.layout_arc ? g_loader.layout_arc(name, &an) : NULL;
    LSarc s;
    LRes *res = NULL;
    if (!arc || !lyt_sarc_open(&s, arc, an)) {
        fprintf(stderr, "[smm1ui] layout %s: not found\n", name);
        free(arc);
        g_lyts[slot].loading = false;
        return NULL;
    }
    res = zalloc(sizeof *res);
    snprintf(res->name, sizeof res->name, "%s", name);
    Names names = {0};
    char want[160];
    snprintf(want, sizeof want, "blyt/%s.bflyt", name);
    size_t ln = 0;
    const uint8_t *ly = lyt_sarc_get(&s, want, &ln);
    for (int i = 0; !ly && i < s.count; i++)  // (any layout file of the archive)
        if (strstr(s.e[i].name, ".bflyt")) ly = s.data + s.e[i].start, ln = s.e[i].end - s.e[i].start;
    if (!ly || !parse_layout(ly, ln, res, &names)) {
        fprintf(stderr, "[smm1ui] layout %s: bad BFLYT\n", name);
        lyt_sarc_close(&s);
        free(arc);
        g_lyts[slot].loading = false;
        return NULL;
    }
    // textures of the archive
    for (int i = 0; i < s.count; i++)
        if (!strncmp(s.e[i].name, "timg/", 5) && strstr(s.e[i].name, ".bflim")) res->ntimg++;
    res->timg_name = zalloc(64 * (size_t)(res->ntimg + 1));
    res->timg = zalloc(sizeof(int) * (size_t)(res->ntimg + 1));
    int k = 0;
    for (int i = 0; i < s.count; i++) {
        if (strncmp(s.e[i].name, "timg/", 5) || !strstr(s.e[i].name, ".bflim")) continue;
        stem(s.e[i].name + 5, res->timg_name[k], 64);
        int w = 0, h = 0;
        uint8_t *px = lyt_flim_decode(s.data + s.e[i].start, s.e[i].end - s.e[i].start, &w, &h);
        res->timg[k] = px ? lyt_tex_add(w, h, px) : -1;
        if (!px) fprintf(stderr, "[smm1ui] %s: texture %s not decoded\n", name, s.e[i].name);
        k++;
    }
    res->ntex = names.ntex;
    res->tex = zalloc(sizeof(int) * (size_t)(names.ntex + 1));
    for (int i = 0; i < names.ntex; i++) res->tex[i] = timg_lookup(res, names.tex[i]);
    res->nfont = names.nfont;
    res->font = zalloc(sizeof(LFont *) * (size_t)(names.nfont + 1));
    for (int i = 0; i < names.nfont; i++) {
        char st[64];
        snprintf(st, sizeof st, "%s", names.font[i]);
        char *dot = strchr(st, '.');
        if (dot) *dot = 0;
        res->font[i] = g_loader.font ? g_loader.font(st) : NULL;
    }
    // animations: anim/<name>_<Tag>.bflan
    for (int i = 0; i < s.count; i++)
        if (!strncmp(s.e[i].name, "anim/", 5) && strstr(s.e[i].name, ".bflan")) res->nanim++;
    res->anim = zalloc(sizeof(LAnim) * (size_t)(res->nanim + 1));
    k = 0;
    for (int i = 0; i < s.count; i++) {
        if (strncmp(s.e[i].name, "anim/", 5) || !strstr(s.e[i].name, ".bflan")) continue;
        char (*tn)[64] = NULL;
        LAnim *a = &res->anim[k];
        if (!parse_anim(s.data + s.e[i].start, s.e[i].end - s.e[i].start, a, &tn)) continue;
        if (!a->name[0]) {  // (the tag name from the file name)
            const char *u = s.e[i].name + 5 + strlen(name);
            snprintf(a->name, sizeof a->name, "%s", *u == '_' ? u + 1 : u);
            char *dot = strstr(a->name, ".bflan");
            if (dot) *dot = 0;
        }
        a->tex = zalloc(sizeof(int) * (size_t)(a->ntex + 1));
        for (int t = 0; t < a->ntex; t++) a->tex[t] = timg_lookup(res, tn[t]);
        free(tn);
        for (int c = 0; c < a->nc; c++) {
            LACont *ct = &a->c[c];
            if (ct->kind == 1) {
                for (int m = 0; m < res->nmat; m++)
                    if (!strcmp(res->mat[m].name, ct->name)) { ct->target = m; break; }
            } else
                ct->target = lyt_pane_index(res, ct->name);
        }
        k++;
    }
    res->nanim = k;
    size_t mn = 0;
    uint8_t *msg = g_loader.msbt ? g_loader.msbt(name, &mn) : NULL;
    if (msg) {
        lyt_msbt_parse(&res->msg, msg, mn);
        free(msg);
    }
    lyt_sarc_close(&s);
    free(arc);
    free(names.tex);
    free(names.font);
    // parts
    res->part = zalloc(sizeof(LRes *) * (size_t)(res->npane + 1));
    for (int i = 0; i < res->npane; i++)
        if (res->pane[i].kind == LP_PRT) res->part[i] = lyt_load(res->pane[i].prt.layout);
    g_lyts[slot].res = res;
    g_lyts[slot].loading = false;
    return res;
}
