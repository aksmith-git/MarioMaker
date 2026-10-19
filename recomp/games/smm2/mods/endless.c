// The Endless Challenge's network requests answered by our server (games/smm2/mods/mod.h).
//
// Each request function ("wrapper", this = the network-session object NetObj) reads its parameters from NetObj,
// asks the server, writes the answer where the game's own code would (the run records, the history rings, the
// course list), sets the status word +0x60 (0 ok, 1 not connected, 2 error) and returns the game's success or failure
// byte. They run on the game's network worker thread, which waits for them, so the HTTP calls block there. Every
// write is made under the lock the originals take (NetObj + 0x20).
//
// Server routes (core/crates/server): GET /api/endless, POST /api/endless/<d>/start | result | suspend | finish,
// GET /api/endless/<d>/next?count=10 | history, GET /api/rankings/<board>, GET /api/me, GET /api/courses/<code>/data.
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "mod.h"
#include "../../../runtime/net/json.h"
#include "hundred.h"
#include <unistd.h>

void bridge_event(const char *what, uint32_t d);  // bridge.c
bool endless_run_complete(uint32_t d);
int style_index(const char *s);
bool bridge_take_start_over(void);  // bridge.c: the life being posted is a Start Over from the pause
void endless_prefetch(int d);

static const char *const DIFF[4] = {"easy", "normal", "expert", "super_expert"};

// NetObj fields (v3.0.3)
#define NO_STATUS 0x60
#define NO_RECORDS_COUNT 0xe9970   // u32: run records (one per difficulty, 0x38 bytes) ...
#define NO_RECORDS 0xe9978         // ... and their array
#define NO_HISTORY 0xe9980         // per difficulty (stride 0x498): entries, +8 capacity, +0xc head, +0x10 count
#define HISTORY_STRIDE 0x498
#define NO_DIFF 0xeabe0            // u32 difficulty of the request
#define NO_CLEAR_ID 0xeabe8        // u64 course data id (clear report)
#define NO_PENDING 0xeabf4         // u8 pending 1-Ups (clear report)
#define NO_COINS 0xeabf5           // u8 coins (clear report)
#define NO_SCORE 0xeabf8           // u32 score with the time bonus (clear report)
#define NO_RANK 0xeac00            // u32 global rank (answer)
#define NO_COURSE_ID 0xeac08       // u64 course data id (start / skip)
#define NO_STYLE 0xeac10           // u8 style, u8 theme
#define NO_SEARCH_TYPE 0x746e8     // u32 search type (0xE: Endless)
#define NO_SEARCH_DIFF 0x746ec     // u32 difficulty of the search
#define NO_COURSES 0x30780         // course list: entries (0x548 bytes each), +8 capacity, +0xc head, +0x10 count
#define COURSE_ENTRY 0x548
#define NO_DL_BUFFER 0x8e5e8       // ptr: where a downloaded course file goes (0x5c000 bytes)
#define NO_DL_ID 0x8e5f0           // u64: data id of the course to download

// ---- the signed-in account and its runs

static pthread_mutex_t state_lock = PTHREAD_MUTEX_INITIALIZER;
static struct {
    uint32_t coins, score;   // what the server holds for the run (the clear report sends deltas)
    uint64_t started;        // data id of the course the run is on (StartCourse seen, no result yet)
    bool complete;           // the server said the run reached its goal (100 Mario)
    bool active;             // the server's run: can go on / lives / clears / goal (0: endless)
    uint32_t lives, clears, goal;
    uint64_t current;        // the course being (or about to be) played
    int ncredits;            // the run's cleared courses (for the ending's credits)
    int form;                // Mario at the last goal: 0 small, 1 big (the ending's Mario)
    struct hundred_credit credits[32];
} runs[4];

static volatile int last_search_d = -1;  // the difficulty of the last Endless course list

// Names and makers of the courses served (data id -> credit), for the ending's credits.
static struct { uint64_t id; struct hundred_credit c; int style; } course_names[64];
static int ncourse_names;

static void note_course(uint64_t id, const char *name, const char *maker, int style) {
    pthread_mutex_lock(&state_lock);
    int i = 0;
    while (i < ncourse_names && course_names[i].id != id) i++;
    if (i == ncourse_names) i = ncourse_names < 64 ? ncourse_names++ : (int)(id % 64);
    course_names[i].id = id;
    course_names[i].style = style;
    snprintf(course_names[i].c.name, sizeof course_names[i].c.name, "%s", name);
    snprintf(course_names[i].c.maker, sizeof course_names[i].c.maker, "%s", maker);
    pthread_mutex_unlock(&state_lock);
}

static void add_credit(uint32_t d, uint64_t id) {  // (state_lock held)
    if (runs[d].ncredits >= 32) return;
    struct hundred_credit *c = &runs[d].credits[runs[d].ncredits++];
    snprintf(c->name, sizeof c->name, "?");
    c->maker[0] = 0;
    for (int i = 0; i < ncourse_names; i++)
        if (course_names[i].id == id) *c = course_names[i].c;
}

static char own_maker_id[16];
static uint64_t pid_of_maker_id(const char *maker_id);

uint64_t mod_player_pid(void) {
    // The signed-in account's player id: the same hash of its maker ID as every maker's (Course World compares
    // course owners with it: own courses). Asked from the server once.
    static volatile int known;
    if (!known) {
        Json *me = srv_call("GET", "/api/me", NULL, NULL);
        const char *id = json_str(json_get(me, "maker_id"), NULL);
        if (id) snprintf(own_maker_id, sizeof own_maker_id, "%s", id), known = 1;
        json_free(me);
        if (!known) {  // (no server answer: a stable id from the token, for this session)
            uint64_t h = 1469598103934665603ull;
            for (const char *p = g_mod.token; *p; p++) h = (h ^ (uint8_t)*p) * 1099511628211ull;
            return 0x4000000000000000ull | (h & 0x0000ffffffffffffull) | 1;
        }
    }
    return pid_of_maker_id(own_maker_id);
}

// Makers of the courses served (fake player ids -> names), for the user records the game asks for next.
// A maker's player id is a hash of their maker ID on our server (the player's own too, once /api/me answered), so the
// game's "is this my course / me" comparisons work; the table maps ids back to maker IDs and names.
#define NMAKERS 512
static struct { uint64_t pid; char name[48]; char maker_id[16]; } makers[NMAKERS];
static int nmakers;

static uint64_t pid_of_maker_id(const char *maker_id) {
    uint64_t h = 1469598103934665603ull;
    for (const char *p = maker_id ? maker_id : ""; *p; p++) h = (h ^ (uint8_t)*p) * 1099511628211ull;
    return 0x5000000000000000ull | (h & 0x0000ffffffffffffull) | 1;
}

static uint64_t maker_pid(const char *maker_id, const char *name) {
    uint64_t pid = pid_of_maker_id(maker_id);
    pthread_mutex_lock(&state_lock);
    int i = 0;
    while (i < nmakers && makers[i].pid != pid) i++;
    if (i == nmakers) {
        i = nmakers < NMAKERS ? nmakers++ : (int)(pid % NMAKERS);
        makers[i].pid = pid;
    }
    if (name && *name) snprintf(makers[i].name, sizeof makers[i].name, "%s", name);
    else if (!makers[i].name[0]) snprintf(makers[i].name, sizeof makers[i].name, "Maker");
    snprintf(makers[i].maker_id, sizeof makers[i].maker_id, "%s", maker_id ? maker_id : "");
    pthread_mutex_unlock(&state_lock);
    return pid;
}

static void maker_name(uint64_t pid, char *out, size_t cap) {
    pthread_mutex_lock(&state_lock);
    snprintf(out, cap, "Maker");
    for (int i = 0; i < nmakers; i++)
        if (makers[i].pid == pid) snprintf(out, cap, "%s", makers[i].name);
    pthread_mutex_unlock(&state_lock);
}

// for world.c
uint64_t mod_maker_pid(const char *maker_id, const char *name) { return maker_pid(maker_id, name); }
void mod_maker_name(uint64_t pid, char *out, size_t cap) { maker_name(pid, out, cap); }
bool mod_maker_id(uint64_t pid, char out[16]) {  // the maker ID behind a player id ("" unknown)
    pthread_mutex_lock(&state_lock);
    out[0] = 0;
    if (pid == mod_player_pid()) snprintf(out, 16, "%s", own_maker_id);
    for (int i = 0; i < nmakers && !out[0]; i++)
        if (makers[i].pid == pid) snprintf(out, 16, "%s", makers[i].maker_id);
    pthread_mutex_unlock(&state_lock);
    return out[0] != 0;
}
const char *mod_own_maker_id(void) { return own_maker_id; }

static bool hundred_rules(void) { return mod_hundred_rules(); }

// Name and best per difficulty for the player's own record (online.c GetUsers). Best = the best clears of SMM2's
// rules, or with SMM1's rules the number of completed 100 Mario runs on that difficulty (the server's hundred_<d>
// boards): the menu's "High Score" slot shows that.
// SMM1's unlocks: Expert opens after a completed Normal run, Super Expert after a completed Expert run (from the
// server's hundred_<d> boards). RECOMP_MOD_UNLOCK_ALL=1 opens everything (tests); SMM2's rules have no locks.
static volatile bool unlocked[4] = {true, true, false, false};

static bool difficulty_open(int d) {
    if (!hundred_rules() || (getenv("RECOMP_MOD_UNLOCK_ALL") && atoi(getenv("RECOMP_MOD_UNLOCK_ALL")))) return true;
    return d < 2 || unlocked[d];
}

void mod_account_info(char *name, size_t cap, uint32_t best[4]) {
    Json *me = srv_call("GET", "/api/me", NULL, NULL);
    if (me && json_str(json_get(me, "name"), NULL)) snprintf(name, cap, "%s", json_str(json_get(me, "name"), ""));
    json_free(me);
    Json *st = srv_call("GET", "/api/endless", NULL, NULL);
    for (int d = 0; d < 4; d++) best[d] = (uint32_t)json_num(json_get(json_get(st, "best"), DIFF[d]), 0);
    json_free(st);
    if (hundred_rules())
        for (int d = 0; d < 4; d++) {
            char path[96];
            snprintf(path, sizeof path, "/api/rankings/hundred_%s?limit=1", DIFF[d]);
            Json *r = srv_call("GET", path, NULL, NULL);
            best[d] = (uint32_t)json_num(json_get(json_get(r, "me"), "score"), 0);
            json_free(r);
        }
    unlocked[2] = best[1] > 0, unlocked[3] = best[2] > 0;
}

// The menu's Expert / Super Expert tabs: a locked one doesn't open (the selection stays where it is).
static void tab_handler(Cpu *c, int d, uint64_t addr) {
    if (!mod_hundred() || difficulty_open(d)) { guest_call_orig(c, addr); return; }
    static const char *const names[4] = {"Easy", "Normal", "Expert", "Super Expert"};
    LOG("[mod] %s is locked: complete a %s run first\n", names[d], names[d - 1]);
}
static void tab_expert(Cpu *c) { tab_handler(c, 2, MAIN_ADDR(0x1a223e0)); }
static void tab_super_expert(Cpu *c) { tab_handler(c, 3, MAIN_ADDR(0x1a224a0)); }

// ---- helpers

static uint32_t req_diff(uint64_t no, uint64_t field) {
    uint32_t d = g32(no + field);
    return d < 4 ? d : 0;
}

static uint64_t record(uint64_t no, uint32_t d) {
    uint64_t base = g64(no + NO_RECORDS);
    return d < g32(no + NO_RECORDS_COUNT) ? base + 0x38ull * d : base;
}

static void finish(Cpu *c, uint64_t no, bool ok, uint32_t status) {
    s32(no + NO_STATUS, ok ? 0 : status);
    c->x[0] = ok ? wrapper_ok() : wrapper_fail();
}

static void code_of(uint64_t data_id, char out[12]) { data_id_to_code((uint32_t)data_id, out); }

// The server's answer {lives, clears} into the run record of difficulty d (what the game's answers set).
static void set_play(Cpu *c, uint64_t no, uint32_t d, uint32_t lives, uint32_t clears) {
    pthread_mutex_lock(&state_lock);
    runs[d].lives = lives, runs[d].clears = clears, runs[d].active = lives > 0;
    pthread_mutex_unlock(&state_lock);
    netobj_lock(c);
    uint64_t r = record(no, d);
    s8(r + 0x28, 1);
    s32(r + 0x2c, clears);
    s8(r + 0x30, (uint8_t)(lives > 255 ? 255 : lives));
    netobj_unlock(c);
}

static uint32_t lives_now(uint64_t no, uint32_t d) { return g8(record(no, d) + 0x30); }
static uint32_t clears_now(uint64_t no, uint32_t d) { return g32(record(no, d) + 0x2c); }

#define WRAPPER(name, off)                                                                       \
    static void name##_impl(Cpu *c, uint64_t no);                                                \
    static void name(Cpu *c) {                                                                   \
        if (!mod_hundred()) { guest_call_orig(c, MAIN_ADDR(off)); return; }                      \
        name##_impl(c, c->x[0]);                                                                 \
    }                                                                                            \
    static void name##_impl(Cpu *c, uint64_t no)

// ---- GetEndlessModeStatus: the four run records

WRAPPER(get_status, 0x14ec2d0) {
    Json *st = srv_call("GET", "/api/endless", NULL, NULL);
    if (!st) { finish(c, no, false, 2); return; }
    uint8_t start_lives = hundred_rules() ? 100 : 5;
    netobj_lock(c);
    for (uint32_t d = 0; d < 4; d++) {
        uint64_t r = record(no, d);
        Json *run = json_get(json_get(st, "runs"), DIFF[d]);
        bool active = json_bool(json_get(run, "active"), false);
        bool complete = json_bool(json_get(run, "complete"), false);
        uint32_t lives = (uint32_t)json_num(json_get(run, "lives"), 0);
        uint32_t clears = (uint32_t)json_num(json_get(run, "clears"), 0);
        uint32_t coins = (uint32_t)json_num(json_get(run, "coins"), 0);
        uint32_t score = (uint32_t)json_num(json_get(run, "score"), 0);
        // Present for every difficulty: New Game reads the start lives from here (the game has no default). The
        // run state +4 is 1 for a run that can go on: the menu offers Continue only for state 1 or 2 with lives
        // (+0x30) and a history [obs 0x7101a26850]; 0 resets its map to an empty run.
        bool cont = active && lives > 0 && !complete;
        uint8_t lv = (uint8_t)(lives > 255 ? 255 : lives);
        s8(r + 0x00, 1);
        s32(r + 0x04, cont ? 1 : 0);
        s8(r + 0x08, (uint8_t)(coins % 100));
        s32(r + 0x0c, score);
        s8(r + 0x10, cont ? lv : 0);
        s8(r + 0x11, start_lives);
        s64(r + 0x18, 0);
        s64(r + 0x20, 0);
        // The play part: a run that can go on (Continue) has lives; a finished or completed one shows none.
        s8(r + 0x28, cont ? 1 : 0);
        s32(r + 0x2c, cont ? clears : 0);
        s8(r + 0x30, cont ? lv : 0);
        pthread_mutex_lock(&state_lock);
        runs[d].coins = coins, runs[d].score = score, runs[d].complete = complete;
        runs[d].active = cont, runs[d].lives = lives, runs[d].clears = clears;
        runs[d].goal = (uint32_t)json_num(json_get(run, "goal"), 0);
        pthread_mutex_unlock(&state_lock);
        MLOG("status %s: %s lives %u clears %u%s\n", DIFF[d], active ? "run" : "no run", lives, clears,
             complete ? " (complete)" : "");
    }
    netobj_unlock(c);
    json_free(st);
    finish(c, no, true, 0);
}

// ---- the next course, known before the game asks for it (the flow's start card shows it, as SMM1's does): the
// server's queue only moves on a result, so asking for its head now gives what the game's own search gets later.

// The run's next courses: the server may first gather them (TGRCode upstream: about a minute for a difficulty's first
// batch), so the request may take long; meanwhile hundred_run_status().loading says so (the flow shows it).
static volatile int loading;
static Json *next_courses(const char *path) {
    __sync_fetch_and_add(&loading, 1);
    Json *j = srv_call_slow("GET", path, NULL, NULL, 180000);
    __sync_fetch_and_sub(&loading, 1);
    return j;
}

static void *prefetch_thread(void *arg) {
    int d = (int)(intptr_t)arg;
    char path[96];
    snprintf(path, sizeof path, "/api/endless/%s/next?count=1", DIFF[d]);
    Json *j = next_courses(path);
    Json *m = json_at(json_get(j, "courses"), 0);
    uint32_t id;
    if (m && code_to_data_id(json_str(json_get(m, "code"), ""), &id)) {
        const char *maker = json_str(json_get(m, "maker"), "");
        note_course(id, json_str(json_get(m, "name"), ""), maker, style_index(json_str(json_get(m, "style"), "")));
        pthread_mutex_lock(&state_lock);
        if (!runs[d].started) runs[d].current = id;
        pthread_mutex_unlock(&state_lock);
        MLOG("next course (%s): %s %s\n", DIFF[d], json_str(json_get(m, "code"), ""), json_str(json_get(m, "name"), ""));
    }
    json_free(j);
    return NULL;
}

void endless_prefetch(int d) {
    if (d < 0 || d > 3) return;
    pthread_t th;
    pthread_create(&th, NULL, prefetch_thread, (void *)(intptr_t)d);
    pthread_detach(th);
}

// A run the flow starts (hundred_run_start): a new one has the start lives at once (the status the start card reads
// before the game's New Game request is answered); a continued one has the server's.
void endless_prepare_run(int d, bool resume) {
    if (d < 0 || d > 3) return;
    static const uint32_t totals[4] = {8, 16, 16, 6};
    pthread_mutex_lock(&state_lock);
    if (!resume) {
        memset(&runs[d], 0, sizeof runs[d]);
        runs[d].active = true, runs[d].lives = hundred_rules() ? 100 : 5, runs[d].goal = totals[d];
    }
    runs[d].current = 0;
    pthread_mutex_unlock(&state_lock);
    if (resume) endless_prefetch(d);
}

// ---- InitEndlessMode: New Game

WRAPPER(init_run, 0x14ece20) {
    uint32_t d = req_diff(no, NO_DIFF);
    char path[64], body[64];
    snprintf(path, sizeof path, "/api/endless/%s/start", DIFF[d]);
    snprintf(body, sizeof body, "{\"ruleset\":\"%s\"}", hundred_rules() ? "hundred" : "smm2");
    int st;
    Json *j = srv_call("POST", path, body, &st);
    bool ok = j != NULL;
    if (ok) {
        pthread_mutex_lock(&state_lock);
        memset(&runs[d], 0, sizeof runs[d]);
        Json *r = json_get(j, "run");
        runs[d].active = true, runs[d].lives = (uint32_t)json_num(json_get(r, "lives"), 0);
        runs[d].goal = (uint32_t)json_num(json_get(r, "goal"), 0);
        pthread_mutex_unlock(&state_lock);
        bridge_event("new", d);
        LOG("[mod] New Game: %s, %s rules, %u lives\n", DIFF[d], hundred_rules() ? "hundred" : "smm2",
            (uint32_t)json_num(json_get(json_get(j, "run"), "lives"), 0));
        endless_prefetch(d);
    }
    json_free(j);
    finish(c, no, ok, 2);
}

// ---- GetEndlessModePlayInfo: the history rings (the map's dots)

WRAPPER(get_play_info, 0x14ed750) {
    Json *h[4] = {0};
    for (int d = 0; d < 4; d++) {
        char path[64];
        snprintf(path, sizeof path, "/api/endless/%s/history", DIFF[d]);
        int st;
        h[d] = srv_call("GET", path, NULL, &st);  // (404: no run on that difficulty)
    }
    netobj_lock(c);
    for (int d = 0; d < 4; d++) {
        uint64_t ring = no + NO_HISTORY + (uint64_t)HISTORY_STRIDE * d;
        uint64_t entries = g64(ring);
        uint32_t cap = g32(ring + 8);
        s64(ring + 0xc, 0);  // head 0, count 0
        Json *list = json_get(h[d], "history");
        size_t n = json_len(list), first = n > cap ? n - cap : 0;
        uint32_t count = 0;
        for (size_t i = first; i < n && entries; i++) {
            Json *e = json_at(list, i);
            uint32_t id;
            if (!code_to_data_id(json_str(json_get(e, "code"), ""), &id)) continue;
            uint64_t p = entries + 0x18ull * count++;
            s64(p, id);
            s32(p + 8, strcmp(json_str(json_get(e, "result"), ""), "clear") ? 1 : 0);  // 0 clear, 1 skip [inf]
            s32(p + 0xc, (uint32_t)json_num(json_get(e, "seq"), 0));
            extern int style_index(const char *);
            s8(p + 0x10, (uint8_t)style_index(json_str(json_get(e, "style"), "")));
            s8(p + 0x11, (uint8_t)json_num(json_get(e, "theme"), 0));
        }
        s32(ring + 0x10, count);
    }
    netobj_unlock(c);
    for (int d = 0; d < 4; d++) json_free(h[d]);
    finish(c, no, true, 0);
}

// ---- GetEndlessModeRank: the player's place on the difficulty's board

WRAPPER(get_rank, 0x14eddc0) {
    uint32_t d = req_diff(no, NO_DIFF);
    char path[96];
    snprintf(path, sizeof path, "/api/rankings/%s_%s?limit=1", hundred_rules() ? "hundred" : "endless", DIFF[d]);
    Json *j = srv_call("GET", path, NULL, NULL);
    uint32_t rank = (uint32_t)json_num(json_get(json_get(j, "me"), "rank"), 0);
    json_free(j);
    netobj_lock(c);
    s32(no + NO_RANK, rank ? rank : 0xffffffffu);
    netobj_unlock(c);
    finish(c, no, true, 0);
}

// ---- the course list (SearchCoursesEndlessMode) and the course download (GetObject)

int style_index(const char *s) {
    static const char *const styles[5] = {"M1", "M3", "MW", "WU", "3W"};
    for (int i = 0; i < 5; i++)
        if (!strcmp(s, styles[i])) return i;
    return 0;
}

static void put_utf16(uint64_t buf, uint32_t cap, const char *s) {
    if (!buf || !cap) return;
    uint32_t n = 0;
    for (const unsigned char *p = (const unsigned char *)s; *p && n + 1 < cap;) {
        uint32_t cp = *p++;
        if (cp >= 0xC0) {
            int extra = cp >= 0xF0 ? 3 : cp >= 0xE0 ? 2 : 1;
            cp &= 0x3F >> extra;
            while (extra-- && (*p & 0xC0) == 0x80) cp = cp << 6 | (*p++ & 0x3F);
            if (cp > 0xFFFF) cp = '?';
        }
        s16(buf + 2 * n++, (uint16_t)cp);
    }
    s16(buf + 2 * n, 0);
}

// One course-list entry (0x548 bytes), as the game's converter fills it from a NEX CourseInfo. The string objects
// inside the entry already own their buffers: only their contents are written.
static void fill_course(uint64_t e, Json *m, uint32_t data_id, uint32_t difficulty) {
    char code[12], bare[10];
    data_id_to_code(data_id, code);
    snprintf(bare, sizeof bare, "%.3s%.3s%.3s", code, code + 4, code + 8);  // (the game adds the dashes itself)
    s64(e + 0x000, data_id);
    put_utf16(g64(e + 0x010), g32(e + 0x018), bare);
    const char *maker = json_str(json_get(m, "maker"), "");
    note_course(data_id, json_str(json_get(m, "name"), ""), maker, style_index(json_str(json_get(m, "style"), "")));
    s64(e + 0x030, maker_pid(json_str(json_get(m, "maker_id"), ""), maker));
    put_utf16(g64(e + 0x040), g32(e + 0x048), json_str(json_get(m, "name"), ""));
    put_utf16(g64(e + 0x098), g32(e + 0x0a0), json_str(json_get(m, "description"), ""));
    s8(e + 0x170, (uint8_t)style_index(json_str(json_get(m, "style"), "")));
    s8(e + 0x171, (uint8_t)json_num(json_get(m, "theme"), 0));
    s64(e + 0x178, (uint64_t)json_num(json_get(m, "uploaded"), 0));
    s32(e + 0x180, difficulty);
    s32(e + 0x184, 0), s32(e + 0x188, 0), s32(e + 0x18c, 0);
    s8(e + 0x190, 0), s8(e + 0x191, 0);
    s32(e + 0x194, 0), s32(e + 0x198, 0), s32(e + 0x19c, 0), s32(e + 0x1a0, 0);  // no clear condition
    s32(e + 0x1a4, 0);
    uint32_t plays = (uint32_t)json_num(json_get(m, "plays"), 0), clears = (uint32_t)json_num(json_get(m, "clears"), 0),
             attempts = (uint32_t)json_num(json_get(m, "attempts"), 0);
    // play statistics (keys 0-4: plays, attempts, ?, clears, versus plays), two unknown counters, the ratings
    // (key 0: likes) [obs converter 0x71014daf60: +0x1a8.. play stats, +0x1bc/+0x1c0 map 0x40, +0x1c4.. ratings]
    uint32_t stats[10] = {plays, attempts, 0, clears, 0, 0, 0, (uint32_t)json_num(json_get(m, "likes"), 0)};
    for (int i = 0; i < 10; i++) s32(e + 0x1a8 + 4 * i, stats[i]);
    // time stats: first clear, world record holder (player ids), world record and the maker's time (ms)
    const char *fc = json_str(json_get(m, "first_clear"), ""), *wr = json_str(json_get(m, "record_holder"), "");
    s64(e + 0x1d0, *fc ? maker_pid(fc, NULL) : 0), s64(e + 0x1d8, *wr ? maker_pid(wr, NULL) : 0);
    s32(e + 0x1e0, (uint32_t)json_num(json_get(m, "record_ms"), 0)), s32(e + 0x1e4, 0);
    s32(e + 0x1e8, (uint32_t)json_num(json_get(m, "comments"), 0));
    // thumbnails / record holders: none (empty addresses: the game downloads nothing for them)
    s64(e + 0x1f0, data_id);
    if (g64(e + 0x200)) s8(g64(e + 0x200), 0);
    s8(e + 0x310, 0), s32(e + 0x314, 0);
    if (g64(e + 0x320)) s8(g64(e + 0x320), 0);
    s64(e + 0x398, data_id);
    if (g64(e + 0x3a8)) s8(g64(e + 0x3a8), 0);
    s8(e + 0x4b8, 0), s32(e + 0x4bc, 0);
    if (g64(e + 0x4c8)) s8(g64(e + 0x4c8), 0);
    s8(e + 0x540, 1);
}

void mod_fill_course(uint64_t e, Json *m, uint32_t data_id, uint32_t difficulty) {
    fill_course(e, m, data_id, difficulty);
}

WRAPPER(search_courses, 0x14d8550) {
    if (g32(no + NO_SEARCH_TYPE) != 0xE) {  // Course World's lists (world.c)
        extern void world_search(Cpu * c, uint64_t no, uint32_t type);
        world_search(c, no, g32(no + NO_SEARCH_TYPE));
        return;
    }
    uint32_t d = req_diff(no, NO_SEARCH_DIFF);
    char path[96];
    snprintf(path, sizeof path, "/api/endless/%s/next?count=10", DIFF[d]);
    Json *j = next_courses(path);
    if (!j) { finish(c, no, false, 2); return; }
    Json *list = json_get(j, "courses");
    netobj_lock(c);
    uint64_t ring = no + NO_COURSES, entries = g64(ring);
    uint32_t cap = g32(ring + 8), count = 0;
    s64(ring + 0xc, 0);
    for (size_t i = 0; i < json_len(list) && count < cap && entries; i++) {
        Json *m = json_at(list, i);
        uint32_t id;
        if (!code_to_data_id(json_str(json_get(m, "code"), ""), &id)) continue;
        fill_course(entries + (uint64_t)COURSE_ENTRY * count++, m, id, d);
        MLOG("  course %u: %s %s\n", count, json_str(json_get(m, "code"), ""), json_str(json_get(m, "name"), ""));
    }
    s32(ring + 0x10, count);
    netobj_unlock(c);
    last_search_d = (int)d;
    bridge_event("list", d);
    LOG("[mod] course list (%s): %u courses%s\n", DIFF[d], count, json_bool(json_get(j, "widened"), false) ? " (band widened)" : "");
    json_free(j);
    finish(c, no, count > 0, 2);
}

// Course details the game asks for around a course start (names [?]; outputs as the originals write them): two
// player records about the course (+0xe8530 and +0xe8580, each with a "valid" byte; [inf] world record / first
// clear), a list for a course (ring at +0xe85a0, 0x18-byte entries), and a check on an id (+0x141480 -> u32 result
// at +0x141604, 0 = fine). Answered empty: no records, an empty list, the check passes. Not connected, the game
// shows a communication error at times (it doesn't need them to play).
WRAPPER(course_list_info, 0x14ec0d0) {
    netobj_lock(c);
    s64(no + 0xe85ac, 0);  // head 0, count 0
    netobj_unlock(c);
    finish(c, no, true, 0);
}

WRAPPER(id_check, 0x14f17a0) {
    netobj_lock(c);
    s32(no + 0x141604, 0);
    netobj_unlock(c);
    finish(c, no, true, 0);
}

// The course's comments (SearchCommentsInOrder, read while a course plays): none. Ring of 0x390-byte comments at
// +0x8f410; +0xe8528 / +0xe852c / +0xe852d: the request it answered and "answered".
WRAPPER(course_comments, 0x14e8ec0) {
    netobj_lock(c);
    s64(no + 0x8f41c, 0);  // head 0, count 0
    s32(no + 0xe8528, g32(no + 0x737a8));
    s8(no + 0xe852c, g8(no + 0x737ac));
    s8(no + 0xe852d, 1);
    netobj_unlock(c);
    finish(c, no, true, 0);
}

// (CanPostRatingAndComment, PostRatingInfo, PostPlayResult and the GetUsers of listed players: world.c)

// Course records by data id (asked for after a clear, for the map): the ids are in a linked list at +0x74748 (count
// +0x74760); the answer goes to the course list ring at +0x30780, as for a search. From GET /api/courses/<code>.
WRAPPER(get_courses, 0x14dbae0) {
    uint64_t ids[16];
    uint32_t n = 0;
    netobj_lock(c);
    uint64_t sentinel = no + 0x74748, p = g64(no + 0x74758);
    if (p != no + 0x74750)
        for (int guard = 0; guard < 64 && n < 16; guard++) {
            p = g64(p + 8);
            if (!p || p - 8 == sentinel) break;
            ids[n++] = g64(p - 8);
        }
    netobj_unlock(c);
    Json *metas[16] = {0};
    for (uint32_t i = 0; i < n; i++) {
        char code[12], path[96];
        code_of(ids[i], code);
        snprintf(path, sizeof path, "/api/courses/%s", code);
        metas[i] = srv_call("GET", path, NULL, NULL);
    }
    netobj_lock(c);
    uint64_t ring = no + NO_COURSES, entries = g64(ring);
    uint32_t cap = g32(ring + 8), count = 0;
    s64(ring + 0xc, 0);
    for (uint32_t i = 0; i < n && count < cap && entries; i++) {
        if (!metas[i]) continue;
        uint32_t diff = (uint32_t)json_num(json_get(metas[i], "difficulty"), 50);
        fill_course(entries + (uint64_t)COURSE_ENTRY * count++, metas[i], (uint32_t)ids[i],
                    diff < 35 ? 0 : diff < 75 ? 1 : diff < 96 ? 2 : 3);
    }
    s32(ring + 0x10, count);
    netobj_unlock(c);
    for (uint32_t i = 0; i < n; i++) json_free(metas[i]);
    MLOG("GetCourses: %u of %u\n", count, n);
    finish(c, no, count > 0 || n == 0, 2);
}

// ReportFromBugDetection (142): data id +0x141480, a kind +0x141518, a text +0x141528. The game posts it when a
// downloaded course fails its own validity check (main+0xfcab10 in the download job; then "The course data is
// corrupt."). In an Endless run the course is reported to the server as broken (it leaves the run's queue) and the
// bridge answers the window and goes on.
WRAPPER(course_post, 0x14f2910) {
    uint64_t id = g64(no + 0x141480);
    LOG("[mod] course %llu reported broken by the game (kind %u)\n", (unsigned long long)id, g8(no + 0x141518));
    int d = -1;
    pthread_mutex_lock(&state_lock);
    for (int i = 0; i < 4 && d < 0; i++)  // (the run whose next course this is: the head of its server queue)
        if (runs[i].current == id || runs[i].started == id) d = i;
    pthread_mutex_unlock(&state_lock);
    if (d < 0 && last_search_d >= 0 && runs[last_search_d].active) d = last_search_d;
    if (d >= 0) {
        char code[12], path[64], body[96];
        code_of(id, code);
        snprintf(path, sizeof path, "/api/endless/%s/result", DIFF[d]);
        snprintf(body, sizeof body, "{\"code\":\"%s\",\"result\":\"broken\"}", code);
        Json *j = srv_call("POST", path, body, NULL);
        json_free(j);
        bridge_event("broken", (uint32_t)d);
    }
    finish(c, no, true, 0);
}

// The course file the game expects: a 0x10-byte header {u32 1, u16 0x10, u16 0, u32 CRC32 of the course, "SCDL"},
// the course (0x5bfc0 bytes) encrypted with the game's own course encryption (main+0x8b3070, AES-128-CBC with a key
// from the game's key table, IV and CMAC in the 0x30-byte block that follows). Nothing of the key leaves the game.
#define COURSE_SIZE 0x5bfc0
#define FILE_SIZE 0x5c000
#define ENCRYPT_FN MAIN_ADDR(0x8b3070)
#define KEY_TABLE MAIN_ADDR(0x23d68c0)

static uint32_t crc32(const uint8_t *p, size_t n) {
    static uint32_t t[256];
    if (!t[1])
        for (uint32_t i = 0; i < 256; i++) {
            uint32_t c = i;
            for (int k = 0; k < 8; k++) c = c & 1 ? 0xEDB88320u ^ (c >> 1) : c >> 1;
            t[i] = c;
        }
    uint32_t c = 0xFFFFFFFFu;
    while (n--) c = t[(c ^ *p++) & 0xFF] ^ (c >> 8);
    return c ^ 0xFFFFFFFFu;
}

WRAPPER(get_course_file, 0x14de010) {
    uint64_t id = g64(no + NO_DL_ID), dest = g64(no + NO_DL_BUFFER);
    char code[12], path[96];
    code_of(id, code);
    snprintf(path, sizeof path, "/api/courses/%s/data", code);
    size_t len = 0;
    uint8_t *data = srv_get_bytes(path, &len);
    if (!data || len != COURSE_SIZE || !dest) {
        LOG("[mod] course %s: %s\n", code, data ? "not a course (size)" : "download failed");
        free(data);
        finish(c, no, false, data ? 2 : 7);  // 7: retryable (the game retries once)
        return;
    }
    static uint8_t *plain;  // guest-visible buffers for the game's encryption (one download at a time: the worker)
    static uint64_t keyobj;
    if (!plain) {
        plain = hle_alloc(COURSE_SIZE);
        keyobj = (uint64_t)(uintptr_t)hle_alloc(16);
    }
    memcpy(plain, data, COURSE_SIZE);
    free(data);
    s64(keyobj, KEY_TABLE);
    uint8_t *out = (uint8_t *)(uintptr_t)dest;
    uint32_t crc = crc32(plain, COURSE_SIZE);
    uint64_t sx[8];
    for (int i = 0; i < 6; i++) sx[i] = c->x[i];
    c->x[0] = keyobj;                     // key table holder
    c->x[1] = dest + 0x10 + COURSE_SIZE;  // crypto block out (IV, seed, CMAC)
    c->x[2] = COURSE_SIZE;
    c->x[3] = dest + 0x10;                // encrypted course out
    c->x[4] = (uint64_t)(uintptr_t)plain;
    guest_call(c, ENCRYPT_FN);
    for (int i = 0; i < 6; i++) c->x[i] = sx[i];
    s32(dest + 0, 1);
    s16(dest + 4, 0x10);
    s16(dest + 6, 0);
    s32(dest + 8, crc);
    memcpy(out + 0xc, "SCDL", 4);
    MLOG("course %s downloaded (%u bytes), CRC %08x\n", code, FILE_SIZE, crc);
    finish(c, no, true, 0);
}

// ---- StartEndlessModeCourse: posted when a new course starts, and again during a miss demo (the life it costs).
// The first post for a course is the start; another post for the same course before its result is a miss.

WRAPPER(start_course, 0x14ecf30) {
    uint32_t d = req_diff(no, NO_DIFF);
    uint64_t id = g64(no + NO_COURSE_ID);
    pthread_mutex_lock(&state_lock);
    bool miss = runs[d].started == id && id;
    runs[d].started = id;
    pthread_mutex_unlock(&state_lock);
    char code[12];
    code_of(id, code);
    if (!miss) {  // a new course: the server's lives and clears as they are
        Json *st = srv_call("GET", "/api/endless", NULL, NULL);
        Json *run = json_get(json_get(st, "runs"), DIFF[d]);
        bool ok = run != NULL;
        if (ok) set_play(c, no, d, (uint32_t)json_num(json_get(run, "lives"), 0), (uint32_t)json_num(json_get(run, "clears"), 0));
        pthread_mutex_lock(&state_lock);
        runs[d].current = id;
        pthread_mutex_unlock(&state_lock);
        bridge_event("start", d);
        LOG("[mod] course start %s (%s): lives %u\n", code, DIFF[d], lives_now(no, d));
        json_free(st);
        finish(c, no, ok, 2);
        return;
    }
    char path[64], body[160];
    snprintf(path, sizeof path, "/api/endless/%s/result", DIFF[d]);
    snprintf(body, sizeof body, "{\"code\":\"%s\",\"result\":\"%s\"}", code,
             bridge_take_start_over() ? "start_over" : "death");
    Json *j = srv_call("POST", path, body, NULL);
    if (j) {
        set_play(c, no, d, (uint32_t)json_num(json_get(j, "lives"), 0), (uint32_t)json_num(json_get(j, "clears"), 0));
        bridge_event("miss", d);
        LOG("[mod] miss on %s: lives %u%s\n", code, lives_now(no, d), json_bool(json_get(j, "game_over"), false) ? " (game over)" : "");
    }
    json_free(j);
    finish(c, no, j != NULL, 2);
}

// Mario's form at the goal (player 0, read when the clear is posted): 0 small, 1 big, -1 unknown. Player actors at
// *(main+0x2c19290) + 0x28, form at player +0x4a8, the form -> size class table at main+0x23d5e0c (1 = small size).
static int player_form(void) {
    uint64_t holder = g64(MAIN_ADDR(0x2c19290));
    uint64_t pl = holder ? g64(holder + 0x28) : 0;
    if (!pl) return -1;
    uint32_t form = g32(pl + 0x4a8);
    if (form >= 16) return -1;
    return g32(MAIN_ADDR(0x23d5e0c) + 4ull * form) == 1 ? 0 : 1;
}

// ---- DominateEndlessModeCourse: a clear (pending 1-Ups, coins, score with the time bonus)

WRAPPER(clear_course, 0x14ed120) {
    uint32_t d = req_diff(no, NO_DIFF);
    uint64_t id = g64(no + NO_CLEAR_ID);
    uint32_t pending = g8(no + NO_PENDING), coins = g8(no + NO_COINS), score = g32(no + NO_SCORE);
    int form = player_form();
    pthread_mutex_lock(&state_lock);
    uint32_t dcoins = (coins + 100 - runs[d].coins % 100) % 100;  // (the game sends coins below 100)
    uint32_t dscore = score >= runs[d].score ? score - runs[d].score : 0;
    pthread_mutex_unlock(&state_lock);
    char code[12], path[64], body[256];
    code_of(id, code);
    snprintf(path, sizeof path, "/api/endless/%s/result", DIFF[d]);
    snprintf(body, sizeof body,
             "{\"code\":\"%s\",\"result\":\"clear\",\"pending\":%u,\"coins\":%u,\"score\":%u,\"frames_left\":0}", code,
             pending, dcoins, dscore);
    Json *j = srv_call("POST", path, body, NULL);
    if (j) {
        uint32_t lives = (uint32_t)json_num(json_get(j, "lives"), 0), clears = (uint32_t)json_num(json_get(j, "clears"), 0);
        bool complete = json_bool(json_get(j, "run_complete"), false);
        set_play(c, no, d, lives, clears);
        pthread_mutex_lock(&state_lock);
        runs[d].coins += dcoins, runs[d].score += dscore, runs[d].started = 0;
        if (form >= 0) runs[d].form = form;
        add_credit(d, id);
        if (complete) runs[d].complete = true;
        pthread_mutex_unlock(&state_lock);
        bridge_event(complete ? "clear_last" : "clear", d);
        if (!complete) endless_prefetch((int)d);
        LOG("[mod] clear %s (%s): lives %u, clears %u, pending %u, Mario %s%s\n", code, DIFF[d], lives, clears,
            pending, form == 0 ? "small" : form == 1 ? "big" : "?",
            complete ? " -- 100 Mario Challenge complete!" : "");
    }
    json_free(j);
    finish(c, no, j != NULL, 2);
}

// ---- PassEndlessModeCourse: a skip (free under SMM1's rules; the server knows the ruleset)

WRAPPER(skip_course, 0x14ed5e0) {
    uint32_t d = req_diff(no, NO_DIFF);
    uint64_t id = g64(no + NO_COURSE_ID);
    char code[12], path[64], body[160];
    code_of(id, code);
    snprintf(path, sizeof path, "/api/endless/%s/result", DIFF[d]);
    snprintf(body, sizeof body, "{\"code\":\"%s\",\"result\":\"skip\",\"during_miss\":false}", code);
    Json *j = srv_call("POST", path, body, NULL);
    if (j) {
        pthread_mutex_lock(&state_lock);
        runs[d].started = 0;
        pthread_mutex_unlock(&state_lock);
        bridge_event("skip", d);
        endless_prefetch((int)d);
        LOG("[mod] skip %s (%s): lives %u\n", code, DIFF[d], (uint32_t)json_num(json_get(j, "lives"), 0));
    }
    json_free(j);
    finish(c, no, j != NULL, 2);
}

// ---- SuspendEndlessMode (quit, run kept) and FinishEndlessMode (game over)

static void post_simple(Cpu *c, uint64_t no, const char *what) {
    uint32_t d = req_diff(no, NO_DIFF);
    char path[64];
    snprintf(path, sizeof path, "/api/endless/%s/%s", DIFF[d], what);
    Json *j = srv_call("POST", path, "{}", NULL);
    if (j) {
        pthread_mutex_lock(&state_lock);
        runs[d].started = 0;
        pthread_mutex_unlock(&state_lock);
        LOG("[mod] %s (%s)\n", what, DIFF[d]);
    }
    json_free(j);
    finish(c, no, j != NULL, 2);
}

WRAPPER(suspend_run, 0x14ed360) {
    if (endless_run_complete(req_diff(no, NO_DIFF))) {  // (a won run left by its result screen's Quit: nothing to keep)
        LOG("[mod] suspend of a won run (%s): answered locally\n", DIFF[req_diff(no, NO_DIFF)]);
        finish(c, no, true, 0);
        bridge_event("suspend", req_diff(no, NO_DIFF));
        return;
    }
    post_simple(c, no, "suspend");
    bridge_event("suspend", req_diff(no, NO_DIFF));
}
WRAPPER(finish_run, 0x14ed4a0) { post_simple(c, no, "finish"); }

// The run as the flow sees it (hundred.h hundred_run_status): from the last server answers, no network here.
void endless_fill_status(struct hundred_status *out, int active_d) {
    memset(out, 0, sizeof *out);
    pthread_mutex_lock(&state_lock);
    out->difficulty = active_d;
    out->loading = loading > 0;
    for (int d = 0; d < 4; d++) {
        out->unlocked[d] = difficulty_open(d);
        out->can_continue[d] = runs[d].active && runs[d].lives > 0 && !runs[d].complete;
        out->continue_lives[d] = (int)runs[d].lives;
        out->continue_cleared[d] = (int)runs[d].clears;
    }
    if (active_d >= 0 && active_d < 4) {
        static const int totals[4] = {8, 16, 16, 6};
        out->lives = (int)runs[active_d].lives;
        out->cleared = (int)runs[active_d].clears;
        out->total = !hundred_rules() ? 0 : runs[active_d].goal ? (int)runs[active_d].goal : totals[active_d];
        out->ncredits = runs[active_d].ncredits < 16 ? runs[active_d].ncredits : 16;
        out->form = runs[active_d].form;
        memcpy(out->credits, runs[active_d].credits, sizeof out->credits);
        uint64_t id = runs[active_d].current;
        if (id) {
            data_id_to_code((uint32_t)id, out->current_code);
            for (int i = 0; i < ncourse_names; i++)
                if (course_names[i].id == id) out->current = course_names[i].c, out->style = course_names[i].style;
        }
    }
    pthread_mutex_unlock(&state_lock);
}

// The run is over (hundred_run_finish): forget its client side state.
void endless_forget_run(int d) {
    if (d < 0 || d > 3) return;
    pthread_mutex_lock(&state_lock);
    memset(&runs[d], 0, sizeof runs[d]);
    pthread_mutex_unlock(&state_lock);
}

// The start lives New Game reads (run record +0x11) in the network object and the UI cache (Cache +0x15e598, the
// records' copy [obs 0x7101520fd0]): 5 for SMM2's own Endless, 100 for 100 Mario, when the player switches.
#define UI_CACHE MAIN_ADDR(0x2c115e0)
void endless_set_start_lives(uint8_t lives) {
    uint64_t no = netobj(), cache = g64(UI_CACHE);
    for (uint32_t d = 0; d < 4; d++) {
        if (no && g64(no + NO_RECORDS) && d < g32(no + NO_RECORDS_COUNT)) s8(record(no, d) + 0x11, lives);
        if (cache && g64(cache + 0x15e598) && d < g32(cache + 0x15e590)) s8(g64(cache + 0x15e598) + 0x38ull * d + 0x11, lives);
    }
}

bool endless_has_course(uint64_t id) {  // world.c: is this course the one an Endless run is on
    pthread_mutex_lock(&state_lock);
    bool v = false;
    for (int d = 0; d < 4; d++) v |= id && (runs[d].current == id || runs[d].started == id);
    pthread_mutex_unlock(&state_lock);
    return v;
}

bool endless_run_complete(uint32_t d) {
    pthread_mutex_lock(&state_lock);
    bool v = d < 4 && runs[d].complete;
    pthread_mutex_unlock(&state_lock);
    return v;
}

void endless_register(void) {
    hle_override(MAIN_ADDR(0x14ec2d0), get_status, "mod: GetEndlessModeStatus");
    hle_override(MAIN_ADDR(0x1a223e0), tab_expert, "mod: Endless tab Expert");
    hle_override(MAIN_ADDR(0x1a224a0), tab_super_expert, "mod: Endless tab Super Expert");
    hle_override(MAIN_ADDR(0x14ece20), init_run, "mod: InitEndlessMode");
    hle_override(MAIN_ADDR(0x14ed750), get_play_info, "mod: GetEndlessModePlayInfo");
    hle_override(MAIN_ADDR(0x14eddc0), get_rank, "mod: GetEndlessModeRank");
    hle_override(MAIN_ADDR(0x14d8550), search_courses, "mod: SearchCourses (Endless)");
    hle_override(MAIN_ADDR(0x14de010), get_course_file, "mod: GetObject (course file)");
    hle_override(MAIN_ADDR(0x14ec0d0), course_list_info, "mod: course list info");
    hle_override(MAIN_ADDR(0x14f17a0), id_check, "mod: id check");
    hle_override(MAIN_ADDR(0x14e8ec0), course_comments, "mod: course comments");
    hle_override(MAIN_ADDR(0x14dbae0), get_courses, "mod: GetCourses (by id)");
    hle_override(MAIN_ADDR(0x14f2910), course_post, "mod: course post");
    hle_override(MAIN_ADDR(0x14ecf30), start_course, "mod: StartEndlessModeCourse");
    hle_override(MAIN_ADDR(0x14ed120), clear_course, "mod: DominateEndlessModeCourse");
    hle_override(MAIN_ADDR(0x14ed5e0), skip_course, "mod: PassEndlessModeCourse");
    hle_override(MAIN_ADDR(0x14ed360), suspend_run, "mod: SuspendEndlessMode");
    hle_override(MAIN_ADDR(0x14ed4a0), finish_run, "mod: FinishEndlessMode");
}

void endless_note_course(uint64_t data_id, const char *code) { (void)data_id, (void)code; }
