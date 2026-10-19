// SMM1's sound for the 100 Mario screens (smm1_audio.h).
//
// BFSTM (Cafe stream, big-endian): header with section references (0x4000 INFO, 0x4001 SEEK, 0x4002 DATA); INFO
// holds the stream info (codec, loop, channels, rate, loop start, sample count, block count / size / samples, the last
// block's sizes, a reference to the sample data in DATA) and a channel table whose entries point to each channel's
// DSP-ADPCM coefficients. Samples are stored block by block, each block one piece per channel.
// BFWAV (Cafe wave): INFO (encoding, loop, rate, loop start / end, channel table: sample reference into DATA and the
// ADPCM coefficients per channel). Both written from public descriptions of the NW4F formats; the DSP-ADPCM decoder
// follows SMCraft's mmk-formats audio/dsp.rs.
#include <stddef.h>
#include <pthread.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "smm1_audio.h"
#include "smm1_data.h"
#include "lyt.h"

static uint32_t be32(const uint8_t *d, size_t n, size_t o) {
    return o + 4 <= n ? (uint32_t)d[o] << 24 | (uint32_t)d[o + 1] << 16 | (uint32_t)d[o + 2] << 8 | d[o + 3] : 0;
}
static uint16_t be16(const uint8_t *d, size_t n, size_t o) { return o + 2 <= n ? (uint16_t)(d[o] << 8 | d[o + 1]) : 0; }

typedef struct {
    int16_t coefs[16], h1, h2;
} Dsp;
static int16_t clamp16(int v) { return v < -32768 ? -32768 : v > 32767 ? 32767 : (int16_t)v; }
// decode `samples` DSP-ADPCM samples from d (frames of 8 bytes: header, 14 nibbles) into out with the given stride
static void dsp_decode(const uint8_t *d, size_t n, int samples, Dsp *st, int16_t *out, int stride) {
    for (int i = 0; i < samples; i++) {
        size_t f = (size_t)(i / 14) * 8;
        if (f >= n) break;
        int k = i % 14;
        uint8_t hdr = d[f];
        if (f + 1 + (size_t)k / 2 >= n) break;
        uint8_t byte = d[f + 1 + k / 2];
        int nib = k % 2 == 0 ? byte >> 4 : byte & 15;
        if (nib >= 8) nib -= 16;
        int pair = hdr >> 4 & 7, scale = hdr & 15;
        int p = ((nib << scale) << 11) + st->coefs[pair * 2] * st->h1 + st->coefs[pair * 2 + 1] * st->h2;
        int16_t s = clamp16((p + 1024) >> 11);
        st->h2 = st->h1, st->h1 = s;
        out[(size_t)i * (size_t)stride] = s;
    }
}

SPcm *smm1_bfstm_decode(const uint8_t *d, size_t n) {
    if (n < 0x40 || memcmp(d, "FSTM", 4)) return NULL;
    int nsec = be16(d, n, 0x10);
    size_t info = 0, data = 0;
    for (int i = 0; i < nsec; i++) {
        uint16_t id = be16(d, n, 0x14 + (size_t)i * 12);
        size_t off = be32(d, n, 0x14 + (size_t)i * 12 + 4);
        if (id == 0x4000) info = off;
        if (id == 0x4002) data = off;
    }
    if (!info || !data) return NULL;
    size_t ib = info + 8, s = ib + be32(d, n, ib + 4), ct = ib + be32(d, n, ib + 0x14);
    int codec = d[s], loop = d[s + 1], ch = d[s + 2];
    SPcm *p = calloc(1, sizeof *p);
    p->rate = (int)be32(d, n, s + 4), p->loop = loop, p->loop_start = (int)be32(d, n, s + 8);
    p->frames = (int)be32(d, n, s + 0xC), p->channels = ch;
    uint32_t blocks = be32(d, n, s + 0x10), bsize = be32(d, n, s + 0x14), bsamples = be32(d, n, s + 0x18);
    uint32_t lsamples = be32(d, n, s + 0x20), lpadded = be32(d, n, s + 0x24);
    size_t sample0 = data + 8 + be32(d, n, s + 0x34);
    if (ch < 1 || ch > 16 || (codec != 1 && codec != 2) || !p->frames) { free(p); return NULL; }
    Dsp st[16];  // (multi-track streams: the mixer plays the first track, channels 0-1)
    memset(st, 0, sizeof st);
    if (codec == 2)
        for (int c = 0; c < ch; c++) {
            size_t ci = ct + be32(d, n, ct + 4 + (size_t)c * 8 + 4);
            size_t ai = ci + be32(d, n, ci + 4);
            for (int k = 0; k < 16; k++) st[c].coefs[k] = (int16_t)be16(d, n, ai + (size_t)k * 2);
        }
    p->pcm = calloc((size_t)p->frames * (size_t)ch, sizeof(int16_t));
    size_t at = sample0;
    for (uint32_t b = 0; b < blocks; b++) {
        bool last = b + 1 == blocks;
        size_t sz = last ? lpadded : bsize;
        int ns = (int)(last ? lsamples : bsamples);
        int16_t *o = p->pcm + (size_t)b * bsamples * (size_t)ch;
        if ((size_t)b * bsamples + (size_t)ns > (size_t)p->frames) ns = p->frames - (int)(b * bsamples);
        for (int c = 0; c < ch; c++, at += sz) {
            if (at >= n) break;
            size_t avail = n - at < sz ? n - at : sz;
            if (codec == 2) dsp_decode(d + at, avail, ns, &st[c], o + c, ch);
            else
                for (int i = 0; i < ns && (size_t)i * 2 + 1 < avail; i++) o[(size_t)i * (size_t)ch + (size_t)c] = (int16_t)be16(d, n, at + (size_t)i * 2);
        }
    }
    return p;
}

SPcm *smm1_bfwav_decode(const uint8_t *d, size_t n) {
    if (n < 0x40 || memcmp(d, "FWAV", 4)) return NULL;
    int nsec = be16(d, n, 0x10);
    size_t info = 0, data = 0;
    for (int i = 0; i < nsec; i++) {
        uint16_t id = be16(d, n, 0x14 + (size_t)i * 12);
        size_t off = be32(d, n, 0x14 + (size_t)i * 12 + 4);
        if (id == 0x7000) info = off;
        if (id == 0x7001) data = off;
    }
    if (!info || !data) return NULL;
    int enc = d[info + 8];
    SPcm *p = calloc(1, sizeof *p);
    p->loop = d[info + 9], p->rate = (int)be32(d, n, info + 0xC), p->loop_start = (int)be32(d, n, info + 0x10);
    p->frames = (int)be32(d, n, info + 0x14);
    size_t tab = info + 0x1C;
    int ch = (int)be32(d, n, tab);
    if (ch < 1 || ch > 8 || (enc != 1 && enc != 2) || !p->frames) { free(p); return NULL; }
    p->channels = ch;
    p->pcm = calloc((size_t)p->frames * (size_t)ch, sizeof(int16_t));
    for (int c = 0; c < ch; c++) {
        size_t ci = tab + be32(d, n, tab + 4 + (size_t)c * 8 + 4);
        size_t samples = data + 8 + be32(d, n, ci + 4);
        if (samples >= n) continue;
        if (enc == 2) {
            size_t ai = ci + be32(d, n, ci + 8 + 4);
            Dsp st;
            memset(&st, 0, sizeof st);
            for (int k = 0; k < 16; k++) st.coefs[k] = (int16_t)be16(d, n, ai + (size_t)k * 2);
            dsp_decode(d + samples, n - samples, p->frames, &st, p->pcm + c, ch);
        } else
            for (int i = 0; i < p->frames; i++) p->pcm[(size_t)i * (size_t)ch + (size_t)c] = (int16_t)be16(d, n, samples + (size_t)i * 2);
    }
    return p;
}

// ---------------------------------------------------------------- the mixer
typedef struct {
    SPcm *p;
    double pos;
    float gain, fade;  // fade: gain change per output sample (< 0 fading out)
    bool loop, on;
    int ch0;  // the first channel played (a multi-track stream's track: channels 2t, 2t + 1)
} Voice;
static pthread_mutex_t g_mx = PTHREAD_MUTEX_INITIALIZER;
static Voice g_bgm, g_se[8];
static char g_bgm_name[64];
static float g_game_gain = 1, g_game_target = 1;

static struct { char name[64]; SPcm *p; } g_pcm[48];
static int g_npcm;
static SPcm *get_stream(const char *name) {
    for (int i = 0; i < g_npcm; i++)
        if (!strcmp(g_pcm[i].name, name)) return g_pcm[i].p;
    size_t n;
    uint8_t *d = smm1_sound_file(name, &n);
    SPcm *p = d ? smm1_bfstm_decode(d, n) : NULL;
    free(d);
    if (!p) fprintf(stderr, "[smm1ui] sound %s: missing\n", name);
    if (g_npcm < 48) snprintf(g_pcm[g_npcm].name, sizeof g_pcm[0].name, "%s", name), g_pcm[g_npcm++].p = p;
    return p;
}

void smm1_bgm(const char *name, bool loop) {
    SPcm *p = name ? get_stream(name) : NULL;
    pthread_mutex_lock(&g_mx);
    g_bgm = (Voice){p, 0, 1, 0, loop && p && p->loop, p != NULL, 0};
    if (p && loop && !p->loop) g_bgm.loop = true, p->loop_start = 0;
    snprintf(g_bgm_name, sizeof g_bgm_name, "%s", name ? name : "");
    pthread_mutex_unlock(&g_mx);
}
void smm1_bgm_track(int track) {
    pthread_mutex_lock(&g_mx);
    if (g_bgm.p) {
        int c = 2 * track;
        g_bgm.ch0 = c + 1 < g_bgm.p->channels ? c : 0;
    }
    pthread_mutex_unlock(&g_mx);
}
double smm1_bgm_pos(void) {
    pthread_mutex_lock(&g_mx);
    double p = g_bgm.on && g_bgm.p ? g_bgm.pos : -1;
    pthread_mutex_unlock(&g_mx);
    return p;
}
const char *smm1_bgm_name(void) { return g_bgm.on ? g_bgm_name : ""; }
void smm1_bgm_fade(int frames) {
    pthread_mutex_lock(&g_mx);
    if (g_bgm.on) g_bgm.fade = -1.0f / (float)(frames > 0 ? frames * 800 : 1);  // (48000 / 60 samples a frame)
    pthread_mutex_unlock(&g_mx);
}
void smm1_game_sound(bool on) { g_game_target = on ? 1 : 0; }

// the sound effects: the screens' sound banks (Pack/Sound.pack -> Sound/Resource/<bank>.bars: a SARC of
// <name>.bfwav + <name>.bameta), loaded on first use (smm1_se_bank)
static struct { char name[48]; SPcm *p; } g_sfx[512];
static int g_nsfx;
static char g_banks[32][64];
static int g_nbanks;
void smm1_se_bank(const char *bank) {
    for (int i = 0; i < g_nbanks; i++)
        if (!strcmp(g_banks[i], bank)) return;
    if (g_nbanks < 32) snprintf(g_banks[g_nbanks++], 64, "%s", bank);
    size_t n;
    uint8_t *d = smm1_sound_file(bank, &n);
    LSarc s;
    if (!d || !lyt_sarc_open(&s, d, n)) { free(d); return; }
    for (int i = 0; i < s.count && g_nsfx < 512; i++) {
        const char *nm = s.e[i].name, *dot = strstr(nm, ".bfwav");
        if (!dot) continue;
        SPcm *p = smm1_bfwav_decode(s.data + s.e[i].start, s.e[i].end - s.e[i].start);
        if (!p) continue;
        snprintf(g_sfx[g_nsfx].name, sizeof g_sfx[0].name, "%.*s", (int)(dot - nm), nm);
        g_sfx[g_nsfx++].p = p;
    }
    lyt_sarc_close(&s);
    free(d);
}
void smm1_se(const char *name) {
    for (int i = 0; i < g_nsfx; i++)
        if (!strcmp(g_sfx[i].name, name)) {
            pthread_mutex_lock(&g_mx);
            for (int k = 0; k < 8; k++)
                if (!g_se[k].on) { g_se[k] = (Voice){g_sfx[i].p, 0, 1, 0, false, true, 0}; break; }
            pthread_mutex_unlock(&g_mx);
            return;
        }
}


static void mix_voice(Voice *v, float *out, int n, int rate) {
    if (!v->on || !v->p) return;
    const SPcm *p = v->p;
    double step = (double)p->rate / rate;
    for (int i = 0; i < n; i++) {
        int k = (int)v->pos;
        if (k >= p->frames - 1) {
            if (v->loop && p->loop_start < p->frames - 1) v->pos = p->loop_start + (v->pos - k), k = (int)v->pos;
            else { v->on = false; return; }
        }
        float f = (float)(v->pos - k), g = v->gain * (1.0f / 32768);
        const int16_t *a = p->pcm + (size_t)k * (size_t)p->channels + v->ch0, *b = a + p->channels;
        float l = (a[0] + (b[0] - a[0]) * f) * g;
        float r = p->channels > 1 + v->ch0 ? (a[1] + (b[1] - a[1]) * f) * g : l;
        out[2 * i] += l, out[2 * i + 1] += r;
        v->pos += step;
        if (v->fade < 0 && (v->gain += v->fade) <= 0) { v->on = false; return; }
    }
}

// runtime/services/audio.c: the game's mixed output (interleaved stereo float, rate Hz), just before it is played
void game_audio_mix(float *stereo, int n, int rate) {
    pthread_mutex_lock(&g_mx);
    float step = 1.0f / (rate / 4.0f);  // the game's sound fades in / out over a quarter of a second
    if (g_game_gain != 1 || g_game_target != 1)
        for (int i = 0; i < n; i++) {
            if (g_game_gain < g_game_target) g_game_gain = g_game_gain + step > g_game_target ? g_game_target : g_game_gain + step;
            else if (g_game_gain > g_game_target) g_game_gain = g_game_gain - step < g_game_target ? g_game_target : g_game_gain - step;
            stereo[2 * i] *= g_game_gain, stereo[2 * i + 1] *= g_game_gain;
        }
    mix_voice(&g_bgm, stereo, n, rate);
    for (int k = 0; k < 8; k++) mix_voice(&g_se[k], stereo, n, rate);
    pthread_mutex_unlock(&g_mx);
}
