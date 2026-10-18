// The 100 Mario mod's RomFS overlay, made on the player's machine from their own dump at the first launch with the mod
// on (nothing of it is in the repository): <data folder>/mods-generated/100mario-v2/romfs/, served over the game's
// files (runtime/services/fs.c, game_romfs_overlay()).
//
// - Layout/Nwk_WorldMap_00: the lives number on the Endless map (T_RestMarioNum_00) is made for two digits; with 100
//   lives it runs into the clears counter next to it. The text box is scaled to 0.68 of its width, so three digits
//   take the room of two (the font size alone is not enough: the game sets it again). (The HUD's and the course card's boxes already fit three.)
//
// - Message/LayoutMsg_*, CommonMsg_* (English): "Endless Challenge" reads "100 Mario Challenge" (below).
//
// The layout edit is in place (same sizes), so that archive needs no repacking: decompress (Yaz0), patch the text box
// section found by its pane name, compress again (Yaz0 with literal blocks only: valid, a little larger).
#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>

#include "mod.h"

#define OVERLAY "mods-generated/100mario-v2"

static uint8_t *read_file(const char *path, size_t *len) {
    FILE *f = fopen(path, "rb");
    if (!f) return NULL;
    fseek(f, 0, SEEK_END);
    long n = ftell(f);
    fseek(f, 0, SEEK_SET);
    uint8_t *b = n > 0 ? malloc((size_t)n) : NULL;
    if (b && fread(b, 1, (size_t)n, f) != (size_t)n) { free(b); b = NULL; }
    fclose(f);
    *len = b ? (size_t)n : 0;
    return b;
}

static uint32_t be32(const uint8_t *p) { return (uint32_t)p[0] << 24 | p[1] << 16 | p[2] << 8 | p[3]; }

static uint8_t *yaz0_decode(const uint8_t *d, size_t n, size_t *out_len) {
    if (n < 16 || memcmp(d, "Yaz0", 4)) return NULL;
    size_t size = be32(d + 4), o = 0, i = 16;
    uint8_t *out = malloc(size ? size : 1);
    while (o < size && i < n) {
        uint8_t code = d[i++];
        for (int b = 0; b < 8 && o < size && i < n; b++) {
            if (code & (0x80 >> b)) { out[o++] = d[i++]; continue; }
            if (i + 1 >= n) break;
            size_t dist = (size_t)((d[i] & 0xF) << 8 | d[i + 1]) + 1, len = d[i] >> 4;
            i += 2;
            if (len == 0) { if (i >= n) break; len = (size_t)d[i++] + 0x12; } else len += 2;
            if (dist > o) { free(out); return NULL; }
            for (size_t k = 0; k < len && o < size; k++, o++) out[o] = out[o - dist];
        }
    }
    if (o != size) { free(out); return NULL; }
    *out_len = size;
    return out;
}

static uint8_t *yaz0_encode_literal(const uint8_t *d, size_t n, size_t *out_len) {
    size_t cap = 16 + n + n / 8 + 16, o = 16;
    uint8_t *out = calloc(1, cap);
    memcpy(out, "Yaz0", 4);
    out[4] = (uint8_t)(n >> 24), out[5] = (uint8_t)(n >> 16), out[6] = (uint8_t)(n >> 8), out[7] = (uint8_t)n;
    for (size_t i = 0; i < n; i += 8) {
        out[o++] = 0xFF;  // eight literal bytes
        for (size_t k = i; k < i + 8 && k < n; k++) out[o++] = d[k];
    }
    *out_len = o;
    return out;
}

// The text box ("txt1" section) of the pane `name`: its section offset, or 0.
static size_t find_textbox(const uint8_t *d, size_t n, const char *name) {
    size_t nl = strlen(name);
    for (size_t k = 0x0C; k + nl < n; k++)
        if (!memcmp(d + k, name, nl) && d[k + nl] == 0 && !memcmp(d + k - 0x0C, "txt1", 4)) return k - 0x0C;
    return 0;
}

static bool make_world_map(const char *romfs, const char *out_path) {
    char src[1200];
    snprintf(src, sizeof src, "%s/Layout/Nwk_WorldMap_00.Nin_NX_NVN.szs", romfs);
    size_t n = 0, raw_n = 0, enc_n = 0;
    uint8_t *file = read_file(src, &n), *raw = file ? yaz0_decode(file, n, &raw_n) : NULL;
    free(file);
    if (!raw) { LOG("[mod] overlay: cannot read %s\n", src); return false; }
    size_t t = find_textbox(raw, raw_n, "T_RestMarioNum_00");
    if (!t || t + 0x80 > raw_n) { LOG("[mod] overlay: the map's lives text box is not where expected\n"); free(raw); return false; }
    float sx;
    memcpy(&sx, raw + t + 0x44, 4);  // the pane's scale x (little-endian float: the host is little-endian too)
    if (!(sx > 0.1f && sx < 10)) { LOG("[mod] overlay: unexpected pane scale\n"); free(raw); return false; }
    sx *= 0.68f;
    memcpy(raw + t + 0x44, &sx, 4);
    uint8_t *enc = yaz0_encode_literal(raw, raw_n, &enc_n);
    free(raw);
    FILE *f = fopen(out_path, "wb");
    bool ok = f && fwrite(enc, 1, enc_n, f) == enc_n;
    if (f) fclose(f);
    free(enc);
    return ok;
}

// ---- English texts (Message/LayoutMsg_USen, _EUen: a SARC of MSBT files): "Endless Challenge" -> "100 Mario
// Challenge", and the menu's one-line description in the sense of SMM1's (our own wording). The archive is rebuilt
// (the strings change length): each MSBT's TXT2 section is rewritten, then the SARC with the same file order.

typedef struct { uint8_t *p; size_t n, cap; } Buf;
static void put(Buf *b, const void *d, size_t n) {
    if (b->n + n > b->cap) {
        b->cap = (b->n + n) * 2 + 4096;
        b->p = realloc(b->p, b->cap);
    }
    if (d) memcpy(b->p + b->n, d, n);
    else memset(b->p + b->n, 0, n);
    b->n += n;
}
static void pad(Buf *b, size_t align, uint8_t v) {
    while (b->n % align) put(b, &v, 1);
}
static uint32_t le32(const uint8_t *p) { return (uint32_t)p[0] | p[1] << 8 | p[2] << 16 | (uint32_t)p[3] << 24; }
static uint16_t le16(const uint8_t *p) { return (uint16_t)(p[0] | p[1] << 8); }
static void w32(uint8_t *p, uint32_t v) { p[0] = (uint8_t)v, p[1] = (uint8_t)(v >> 8), p[2] = (uint8_t)(v >> 16), p[3] = (uint8_t)(v >> 24); }

static const struct { const char *from, *to; } relabels[] = {
    {"Endless Challenge", "100 Mario Challenge"},
    {"Clear as many courses as possible\nbefore you run out of lives!", "Clear the run's courses\nbefore all 100 Marios are gone!"},
    {"Clear as many courses as possible before you run out of lives!", "Clear the run's courses before all 100 Marios are gone!"},
};

// UTF-16LE string at s (n code units, no terminator) with the replacements applied, into out; true if changed
static bool relabel(const uint16_t *s, size_t n, Buf *out) {
    bool changed = false;
    for (size_t i = 0; i < n;) {
        if (s[i] == 0x0E && i + 4 <= n) {  // a control tag: tag, group, type, parameter bytes, parameters
            size_t len = 4 + (s[i + 3] + 1) / 2;
            if (i + len > n) len = n - i;
            put(out, s + i, len * 2);
            i += len;
            continue;
        }
        bool hit = false;
        for (size_t r = 0; r < sizeof relabels / sizeof *relabels && !hit; r++) {
            size_t fl = strlen(relabels[r].from);
            if (i + fl > n) continue;
            size_t k = 0;
            while (k < fl && s[i + k] == (uint8_t)relabels[r].from[k]) k++;
            if (k < fl) continue;
            for (const char *t = relabels[r].to; *t; t++) { uint16_t ch = (uint8_t)*t; put(out, &ch, 2); }
            i += fl;
            hit = changed = true;
        }
        if (!hit) put(out, s + i++, 2);
    }
    return changed;
}

// One MSBT with its TXT2 strings relabelled (NULL if nothing changed or not a little-endian UTF-16 MSBT)
static uint8_t *msbt_relabel(const uint8_t *m, size_t n, size_t *out_n) {
    if (n < 0x20 || memcmp(m, "MsgStdBn", 8) || le16(m + 8) != 0xFEFF || m[0xC] != 1) return NULL;
    uint16_t nsec = le16(m + 0xE);
    Buf out = {0};
    put(&out, m, 0x20);
    bool changed = false;
    size_t o = 0x20;
    for (int s = 0; s < nsec && o + 0x10 <= n; s++) {
        uint32_t size = le32(m + o + 4);
        if (o + 0x10 + size > n) { free(out.p); return NULL; }
        const uint8_t *data = m + o + 0x10;
        if (!memcmp(m + o, "TXT2", 4) && size >= 4) {
            uint32_t count = le32(data);
            Buf txt = {0};
            put(&txt, NULL, 4 + 4 * (size_t)count);
            w32(txt.p, count);
            for (uint32_t i = 0; i < count; i++) {
                uint32_t a = le32(data + 4 + 4 * i), b = i + 1 < count ? le32(data + 8 + 4 * i) : size;
                if (a > size || b > size || b < a) { free(txt.p); free(out.p); return NULL; }
                w32(txt.p + 4 + 4 * i, (uint32_t)txt.n);
                size_t units = (b - a) / 2;
                uint16_t *str = malloc(units * 2 + 2);
                memcpy(str, data + a, units * 2);
                changed |= relabel(str, units, &txt);
                free(str);
            }
            uint8_t hdr[0x10] = {'T', 'X', 'T', '2'};
            w32(hdr + 4, (uint32_t)txt.n);
            put(&out, hdr, 0x10);
            put(&out, txt.p, txt.n);
            free(txt.p);
        } else {
            put(&out, m + o, 0x10 + size);
        }
        pad(&out, 0x10, 0xAB);
        o += 0x10 + size;
        o = (o + 0xF) & ~(size_t)0xF;
    }
    if (!changed) { free(out.p); return NULL; }
    w32(out.p + 0x12, (uint32_t)out.n);
    *out_n = out.n;
    return out.p;
}

// A SARC (little-endian) with its MSBT members relabelled; NULL if nothing changed
static uint8_t *sarc_relabel(const uint8_t *s, size_t n, size_t *out_n) {
    if (n < 0x20 || memcmp(s, "SARC", 4) || le16(s + 6) != 0xFEFF) return NULL;
    uint16_t hl = le16(s + 4);
    uint32_t data_off = le32(s + 0xC);
    const uint8_t *sfat = s + hl;
    if (memcmp(sfat, "SFAT", 4)) return NULL;
    uint16_t count = le16(sfat + 6);
    const uint8_t *nodes = sfat + 0xC;
    const uint8_t *sfnt = nodes + 0x10 * count;
    if ((size_t)(sfnt - s) + 8 > n || memcmp(sfnt, "SFNT", 4) || data_off > n) return NULL;
    uint32_t align = 0x2000;  // the largest power of two all members start on
    for (int i = 0; i < count; i++)
        while (align > 4 && le32(nodes + 0x10 * i + 8) % align) align /= 2;
    Buf out = {0};
    put(&out, s, data_off);  // header, SFAT, SFNT (offsets fixed below)
    bool changed = false;
    for (int i = 0; i < count; i++) {
        uint32_t a = le32(nodes + 0x10 * i + 8), b = le32(nodes + 0x10 * i + 12);
        if (data_off + b > n || b < a) { free(out.p); return NULL; }
        size_t mn = 0;
        uint8_t *m = msbt_relabel(s + data_off + a, b - a, &mn);
        pad(&out, align, 0);
        uint32_t start = (uint32_t)(out.n - data_off);
        if (m) { put(&out, m, mn); free(m); changed = true; }
        else put(&out, s + data_off + a, b - a);
        w32(out.p + hl + 0xC + 0x10 * i + 8, start);
        w32(out.p + hl + 0xC + 0x10 * i + 12, (uint32_t)(out.n - data_off));
    }
    if (!changed) { free(out.p); return NULL; }
    w32(out.p + 8, (uint32_t)out.n);
    *out_n = out.n;
    return out.p;
}

static bool make_texts(const char *romfs, const char *dir, const char *name) {
    char src[1200], dst[1400];
    snprintf(src, sizeof src, "%s/Message/%s", romfs, name);
    snprintf(dst, sizeof dst, "%s/Message/%s", dir, name);
    size_t n = 0, raw_n = 0, out_n = 0, enc_n = 0;
    uint8_t *file = read_file(src, &n), *raw = file ? yaz0_decode(file, n, &raw_n) : NULL;
    free(file);
    uint8_t *out = raw ? sarc_relabel(raw, raw_n, &out_n) : NULL;
    free(raw);
    if (!out) { LOG("[mod] overlay: %s: nothing relabelled\n", name); return false; }
    uint8_t *enc = yaz0_encode_literal(out, out_n, &enc_n);
    free(out);
    FILE *f = fopen(dst, "wb");
    bool ok = f && fwrite(enc, 1, enc_n, f) == enc_n;
    if (f) fclose(f);
    free(enc);
    return ok;
}

static void mkdirs(const char *path) {
    char p[1200];
    snprintf(p, sizeof p, "%s", path);
    for (char *s = p + 1; *s; s++)
        if (*s == '/') { *s = 0; mkdir(p, 0755); *s = '/'; }
    mkdir(p, 0755);
}

// runtime/services/fs.c: a folder of files laid over the game's RomFS (NULL: none)
const char *game_romfs_overlay(void) {
    static char dir[1200];
    static int state;  // 0 not yet, 1 ready, -1 none
    if (state) return state > 0 ? dir : NULL;
    state = -1;
    if (!mod_hundred() || !g_romfs_dir) return NULL;
    snprintf(dir, sizeof dir, OVERLAY "/romfs");
    char layout[1300], target[1400];
    snprintf(layout, sizeof layout, "%s/Layout", dir);
    snprintf(target, sizeof target, "%s/Nwk_WorldMap_00.Nin_NX_NVN.szs", layout);
    struct stat st;
    if (stat(target, &st)) {
        mkdirs(layout);
        if (!make_world_map(g_romfs_dir, target)) return NULL;
        LOG("[mod] overlay made from your dump: %s\n", target);
        char msg[1300];
        snprintf(msg, sizeof msg, "%s/Message", dir);
        mkdirs(msg);
        static const char *const texts[] = {"LayoutMsg_USen.szs", "LayoutMsg_EUen.szs", "CommonMsg_USen.szs",
                                            "CommonMsg_EUen.szs"};
        for (size_t i = 0; i < sizeof texts / sizeof *texts; i++)
            if (make_texts(g_romfs_dir, dir, texts[i])) LOG("[mod] overlay: %s relabelled\n", texts[i]);
    }
    state = 1;
    return dir;
}
