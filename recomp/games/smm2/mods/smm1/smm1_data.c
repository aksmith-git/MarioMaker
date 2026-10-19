// The SMM1 files the host-drawn 100 Mario screens use, from the player's own Super Mario Maker (Wii U) dump.
//
// Finding the dump: RECOMP_SMM1_DUMP (one or more folders, ':'-separated; ';' on Windows), else smm1/ in the data
// folder, else ~/smm-work/extracted (a developer checkout). Each is searched (3 levels) for decrypted titles - a
// meta/meta.xml whose title id is Super Mario Maker's (US 1018DC00, EU 1018DD00, JP 1014B700) next to a content/
// folder. The disc's base game (v16) and the update (v208, which has Super Expert) are overlaid, newer over older.
//
// At the first launch that needs them, the files are generated into <data folder>/mods-generated/smm1-ui-v4/:
//   layout/<Name>.arc   every layout archive of Pack/{Layout,Nwk,NormalPlay,Lcl}.pack and the loose Layout/*.szs
//                       (Yaz0 removed: SARC of blyt/ anim/ timg/)
//   font/<Name>.bffnt   the bitmap fonts of Font/BmpFont_<region>.szs
//   msg/<Layout>.msbt   the English layout texts (Message/LayoutMsg_<US|EU>en.szs), common/<Group>/<Name>.msbt
//   model/<Name>.bfres  the ending scene's models from Pack/StaticSkin.pack: <Style>_Object_peach (Peach / Toad and
//                       the ground), <Style>_Player_{Mario,SpMario}Mdl (Mario), Yaz0 and SARC removed
//   sound/<Name>.bfstm  the screens' music (Sound/Resource/Stream), sound/<Name>.bars their sound effects
// Nothing of it is in the repository; the folder is the player's own, like the rest of the data folder.
#include "smm1_data.h"

#include <dirent.h>
#include <errno.h>
#include <pthread.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>

#include "lyt.h"

#define CACHE "mods-generated/smm1-ui-v4"
#define LOGP "[smm1ui] "

#ifdef _WIN32
#include <direct.h>
#define MKDIR(p) _mkdir(p)
#define PATHSEP ';'
#else
#define MKDIR(p) mkdir(p, 0755)
#define PATHSEP ':'
#endif

static void mkdirs(const char *path) {
    char b[1024];
    snprintf(b, sizeof b, "%s", path);
    for (char *p = b + 1; *p; p++)
        if (*p == '/') { *p = 0; MKDIR(b); *p = '/'; }
    MKDIR(b);
}

uint8_t *smm1_read_file(const char *path, size_t *n) {
    FILE *f = fopen(path, "rb");
    if (!f) return NULL;
    fseek(f, 0, SEEK_END);
    long len = ftell(f);
    fseek(f, 0, SEEK_SET);
    uint8_t *b = len >= 0 ? malloc((size_t)len + 1) : NULL;
    if (b && fread(b, 1, (size_t)len, f) != (size_t)len) { free(b); b = NULL; }
    fclose(f);
    if (b) *n = (size_t)len;
    return b;
}
static bool write_file(const char *path, const uint8_t *d, size_t n) {
    char tmp[1100];
    snprintf(tmp, sizeof tmp, "%s.tmp", path);
    FILE *f = fopen(tmp, "wb");
    if (!f) return false;
    bool ok = fwrite(d, 1, n, f) == n;
    ok = fclose(f) == 0 && ok;
    if (ok) {
        remove(path);
        ok = rename(tmp, path) == 0;
    }
    return ok;
}

// ---------------------------------------------------------------- the dump
typedef struct {
    char content[1024];
    int version;
} Title;
static Title g_titles[8];
static int g_ntitles;

static bool is_smm1(const char *meta) {
    size_t n;
    char *x = (char *)smm1_read_file(meta, &n);
    if (!x) return false;
    x[n] = 0;
    bool ok = false;
    char *t = strstr(x, "<title_id");
    if (t && (t = strchr(t, '>'))) {
        char id[17] = {0};
        memcpy(id, t + 1, 16);
        for (char *p = id; *p; p++) *p = (char)(*p >= 'a' && *p <= 'f' ? *p - 32 : *p);
        ok = !strcmp(id + 8, "1018DC00") || !strcmp(id + 8, "1018DD00") || !strcmp(id + 8, "1014B700");
    }
    free(x);
    return ok;
}
static int title_version(const char *meta) {
    size_t n;
    char *x = (char *)smm1_read_file(meta, &n);
    if (!x) return 0;
    x[n] = 0;
    int v = 0;
    char *t = strstr(x, "<title_version");
    if (t && (t = strchr(t, '>'))) v = atoi(t + 1);
    free(x);
    return v;
}
static void scan(const char *dir, int depth) {
    char meta[1100], content[1100];
    snprintf(meta, sizeof meta, "%s/meta/meta.xml", dir);
    snprintf(content, sizeof content, "%s/content", dir);
    struct stat st;
    if (stat(content, &st) == 0 && S_ISDIR(st.st_mode) && is_smm1(meta) && g_ntitles < 8) {
        for (int i = 0; i < g_ntitles; i++)
            if (!strcmp(g_titles[i].content, content)) return;
        snprintf(g_titles[g_ntitles].content, sizeof g_titles[0].content, "%s", content);
        g_titles[g_ntitles++].version = title_version(meta);
        return;
    }
    if (depth <= 0) return;
    DIR *d = opendir(dir);
    if (!d) return;
    struct dirent *e;
    while ((e = readdir(d))) {
        if (e->d_name[0] == '.') continue;
        char sub[1100];
        snprintf(sub, sizeof sub, "%s/%s", dir, e->d_name);
        if (stat(sub, &st) == 0 && S_ISDIR(st.st_mode)) scan(sub, depth - 1);
    }
    closedir(d);
}
static int by_version(const void *a, const void *b) { return ((const Title *)a)->version - ((const Title *)b)->version; }

static void find_dump(void) {
    g_ntitles = 0;
    const char *env = getenv("RECOMP_SMM1_DUMP");
    if (env && *env) {
        char buf[4096];
        snprintf(buf, sizeof buf, "%s", env);
        for (char *s = buf, *e; s && *s; s = e ? e + 1 : NULL) {
            e = strchr(s, PATHSEP);
            if (e) *e = 0;
            scan(s, 3);
        }
    }
    if (!g_ntitles) scan("smm1", 3);
    if (!g_ntitles && getenv("HOME")) {
        char p[1024];
        snprintf(p, sizeof p, "%s/smm-work/extracted", getenv("HOME"));
        scan(p, 3);
    }
    qsort(g_titles, (size_t)g_ntitles, sizeof g_titles[0], by_version);  // oldest first: newer overwrite
    for (int i = 0; i < g_ntitles; i++) fprintf(stderr, LOGP "SMM1 dump: v%d at %s\n", g_titles[i].version, g_titles[i].content);
}

// a file of the dump (rel to content/), newest title first
static uint8_t *dump_file(const char *rel, size_t *n) {
    for (int i = g_ntitles - 1; i >= 0; i--) {
        char p[1200];
        snprintf(p, sizeof p, "%s/%s", g_titles[i].content, rel);
        uint8_t *b = smm1_read_file(p, n);
        if (b) return b;
    }
    return NULL;
}

// ---------------------------------------------------------------- generating the cache
static int g_written;
static void put(const char *rel, const uint8_t *d, size_t n) {
    char p[1200];
    snprintf(p, sizeof p, CACHE "/%s", rel);
    char dir[1200];
    snprintf(dir, sizeof dir, "%s", p);
    char *slash = strrchr(dir, '/');
    if (slash) *slash = 0, mkdirs(dir);
    if (write_file(p, d, n)) g_written++;
}
static const char *base_name(const char *path) {
    const char *s = strrchr(path, '/');
    return s ? s + 1 : path;
}
static void put_layout(const char *szs_name, const uint8_t *szs, size_t n) {
    char name[160];
    snprintf(name, sizeof name, "%s", base_name(szs_name));
    char *dot = strstr(name, ".szs");
    if (!dot) return;
    *dot = 0;
    size_t an = 0;
    uint8_t *outer = lyt_yaz0(szs, n, &an);  // Yaz0 -> SARC holding <Name>.arc
    if (!outer) return;
    LSarc s;
    if (lyt_sarc_open(&s, outer, an)) {
        for (int i = 0; i < s.count; i++)
            if (strstr(s.e[i].name, ".arc")) {
                char rel[256];
                snprintf(rel, sizeof rel, "layout/%s.arc", name);
                put(rel, s.data + s.e[i].start, s.e[i].end - s.e[i].start);
                break;
            }
        lyt_sarc_close(&s);
    }
    free(outer);
}
static void put_pack(const uint8_t *d, size_t n) {
    LSarc s;
    if (!lyt_sarc_open(&s, d, n)) return;
    for (int i = 0; i < s.count; i++)
        if (!strncmp(s.e[i].name, "Layout/", 7) && strstr(s.e[i].name, ".szs"))
            put_layout(s.e[i].name, s.data + s.e[i].start, s.e[i].end - s.e[i].start);
    lyt_sarc_close(&s);
}
static void put_archive(const char *rel_in_dump, const char *prefix, bool flatten) {
    size_t n;
    uint8_t *z = dump_file(rel_in_dump, &n);
    if (!z) return;
    size_t an;
    uint8_t *d = lyt_yaz0(z, n, &an);
    if (!d) d = z, an = n, z = NULL;
    LSarc s;
    if (lyt_sarc_open(&s, d, an)) {
        for (int i = 0; i < s.count; i++) {
            char rel[300];
            snprintf(rel, sizeof rel, "%s/%s", prefix, flatten ? base_name(s.e[i].name) : s.e[i].name);
            put(rel, s.data + s.e[i].start, s.e[i].end - s.e[i].start);
        }
        lyt_sarc_close(&s);
    }
    free(d);
    free(z);
}
static const char *first_existing(const char *const *rels) {
    for (int i = 0; rels[i]; i++) {
        size_t n;
        uint8_t *b = dump_file(rels[i], &n);
        if (b) { free(b); return rels[i]; }
    }
    return NULL;
}

static bool generate(void) {
    find_dump();
    if (!g_ntitles) {
        fprintf(stderr, LOGP "no SMM1 dump found (set RECOMP_SMM1_DUMP to the folder of your extracted Super Mario Maker "
                        "base game and update): the SMM1 screens are off\n");
        return false;
    }
    g_written = 0;
    // layouts: loose ones (the disc), then the packs, oldest title first so the update's win
    for (int t = 0; t < g_ntitles; t++) {
        char dir[1200];
        snprintf(dir, sizeof dir, "%s/Layout", g_titles[t].content);
        DIR *dd = opendir(dir);
        struct dirent *e;
        while (dd && (e = readdir(dd))) {
            if (!strstr(e->d_name, ".szs")) continue;
            char p[1400];
            snprintf(p, sizeof p, "%s/%s", dir, e->d_name);
            size_t n;
            uint8_t *b = smm1_read_file(p, &n);
            if (b) put_layout(e->d_name, b, n), free(b);
        }
        if (dd) closedir(dd);
        static const char *packs[] = {"Layout", "Nwk", "NormalPlay", "Lcl"};
        for (int k = 0; k < 4; k++) {
            char p[1300];
            snprintf(p, sizeof p, "%s/Pack/%s.pack", g_titles[t].content, packs[k]);
            size_t n;
            uint8_t *b = smm1_read_file(p, &n);
            if (b) put_pack(b, n), free(b);
        }
    }
    // the ending scene's models (Pack/StaticSkin.pack -> Model/<Name>.szs -> Output.bfres)
    {
        size_t n;
        uint8_t *pack = dump_file("Pack/StaticSkin.pack", &n);
        LSarc ps;
        if (pack && lyt_sarc_open(&ps, pack, n)) {
            static const char *styles[4] = {"M1", "M3", "MW", "WU"}, *kinds[3] = {"Object_peach", "Player_MarioMdl", "Player_SpMarioMdl"};
            for (int st = 0; st < 4; st++)
                for (int k = 0; k < 3; k++) {
                    char rel[128], out[128];
                    snprintf(rel, sizeof rel, "Model/%s_%s.szs", styles[st], kinds[k]);
                    size_t zn, an;
                    const uint8_t *z = lyt_sarc_get(&ps, rel, &zn);
                    uint8_t *arc = z ? lyt_yaz0(z, zn, &an) : NULL;
                    LSarc inner;
                    if (arc && lyt_sarc_open(&inner, arc, an)) {
                        size_t bn;
                        const uint8_t *bf = lyt_sarc_get(&inner, "Output.bfres", &bn);
                        snprintf(out, sizeof out, "model/%s_%s.bfres", styles[st], kinds[k]);
                        if (bf) put(out, bf, bn);
                        lyt_sarc_close(&inner);
                    }
                    free(arc);
                }
            lyt_sarc_close(&ps);
        }
        free(pack);
    }
    // the screens' music and sound effects
    {
        static const char *streams[] = {"BGM_100MarioMap", "BGM_100MarioMapExpert", "BGM_100MarioMapSuperExpert", "BGM_100mario_staffroll_easy",
                                        "BGM_100mario_staffroll_normal", "BGM_NetworkMainScene", "BGM_CourseListScene",
                                        "M1_BGM_Ending", "M3_BGM_EndingDemo1", "M3_BGM_EndingDemo2", "MW_BGM_Ending2",
                                        "WU_STRM_BGM_LAST_BOSS2_HARP", "WU_STRM_BGM_LAST_BOSS_HARP_Dummy", "M1_GameOver",
                                        NULL};
        for (int i = 0; streams[i]; i++) {
            char rel[160], out[160];
            snprintf(rel, sizeof rel, "Sound/Resource/Stream/%s.bfstm", streams[i]);
            snprintf(out, sizeof out, "sound/%s.bfstm", streams[i]);
            size_t n;
            uint8_t *b = dump_file(rel, &n);
            if (b) put(out, b, n), free(b);
        }
        size_t n;
        uint8_t *pack = dump_file("Pack/Sound.pack", &n);
        LSarc ps;
        if (pack && lyt_sarc_open(&ps, pack, n)) {
            for (int i = 0; i < ps.count; i++) {
                const char *nm = ps.e[i].name;
                if (strncmp(nm, "Sound/Resource/", 15) || !strstr(nm, ".bars")) continue;
                const char *base = nm + 15;  // the screens' and scenes' archives only (not the course objects')
                if (strncmp(base, "UI_Nwk", 6) && strncmp(base, "UI_Play", 7) && strncmp(base, "UI_Cmn", 6) && !strstr(base, "Scene"))
                    continue;
                char out[200];
                snprintf(out, sizeof out, "sound/%s", nm + 15);
                put(out, ps.data + ps.e[i].start, ps.e[i].end - ps.e[i].start);
            }
            lyt_sarc_close(&ps);
        }
        free(pack);
    }
    static const char *fonts[] = {"Font/BmpFont_US.szs", "Font/BmpFont_EU.szs", "Font/BmpFont_JP.szs", NULL};
    static const char *lmsg[] = {"Message/LayoutMsg_USen.szs", "Message/LayoutMsg_EUen.szs", NULL};
    static const char *cmsg[] = {"Message/CommonMsg_USen.szs", "Message/CommonMsg_EUen.szs", NULL};
    const char *f = first_existing(fonts), *l = first_existing(lmsg), *c = first_existing(cmsg);
    if (f) put_archive(f, "font", true);
    if (l) put_archive(l, "msg", true);
    if (c) put_archive(c, "common", false);
    fprintf(stderr, LOGP "generated %d files into " CACHE " from the SMM1 dump\n", g_written);
    if (!f || !l) return false;
    char ver[64];
    int n = snprintf(ver, sizeof ver, "smm1-ui-v2 titles %d newest v%d\n", g_ntitles, g_titles[g_ntitles - 1].version);
    put("done", (const uint8_t *)ver, (size_t)n);
    return true;
}

// ---------------------------------------------------------------- the loader the layout player uses
static uint8_t *ld_arc(const char *name, size_t *n) {
    char p[300];
    snprintf(p, sizeof p, CACHE "/layout/%s.arc", name);
    return smm1_read_file(p, n);
}
static uint8_t *ld_msbt(const char *layout, size_t *n) {
    char p[300];
    snprintf(p, sizeof p, CACHE "/msg/%s.msbt", layout);
    return smm1_read_file(p, n);
}
static struct { char name[32]; LFont *f; } g_fonts[16];
static int g_nfonts;
static LFont *load_font(const char *name) {
    for (int i = 0; i < g_nfonts; i++)
        if (!strcmp(g_fonts[i].name, name)) return g_fonts[i].f;
    char p[300];
    snprintf(p, sizeof p, CACHE "/font/%s.bffnt", name);
    size_t n;
    uint8_t *b = smm1_read_file(p, &n);
    LFont *f = b ? lyt_font_parse(name, b, n) : NULL;
    free(b);
    if (g_nfonts < 16) {
        snprintf(g_fonts[g_nfonts].name, sizeof g_fonts[0].name, "%s", name);
        g_fonts[g_nfonts++].f = f;
    }
    return f;
}
static LFont *ld_font(const char *name) {
    // the Wii U system fonts the layouts name (CafeStd_*) are not part of the game: MessageFont stands in
    LFont *f = load_font(name);
    if (!f) f = load_font("MessageFont");
    if (f && strcmp(f->name, "SpecialFont") && !f->fallback) f->fallback = load_font("SpecialFont");
    return f;
}

static pthread_mutex_t g_lock = PTHREAD_MUTEX_INITIALIZER;
static int g_state;  // 0 not tried, 1 ready, -1 unavailable

bool smm1_data_open(void) {
    pthread_mutex_lock(&g_lock);
    if (g_state == 0) {
        size_t n;
        uint8_t *done = smm1_read_file(CACHE "/done", &n);
        bool ok = done != NULL;
        free(done);
        if (!ok) ok = generate();
        if (ok) {
            LLoader l = {ld_arc, ld_font, ld_msbt};
            lyt_set_loader(&l);
        }
        g_state = ok ? 1 : -1;
    }
    bool ok = g_state == 1;
    pthread_mutex_unlock(&g_lock);
    return ok;
}

uint8_t *smm1_model(const char *name, size_t *n) {
    char p[300];
    snprintf(p, sizeof p, CACHE "/model/%s.bfres", name);
    return smm1_read_file(p, n);
}

uint8_t *smm1_sound_file(const char *name, size_t *n) {
    char p[300];
    snprintf(p, sizeof p, CACHE "/sound/%s.bfstm", name);
    uint8_t *b = smm1_read_file(p, n);
    if (!b) {
        snprintf(p, sizeof p, CACHE "/sound/%s.bars", name);
        b = smm1_read_file(p, n);
    }
    return b;
}

uint8_t *smm1_common_msbt(const char *group_and_name, size_t *n) {
    char p[300];
    snprintf(p, sizeof p, CACHE "/common/%s.msbt", group_and_name);
    return smm1_read_file(p, n);
}
