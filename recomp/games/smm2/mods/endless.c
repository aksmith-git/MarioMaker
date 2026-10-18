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

void autoplay_event(const char *what);  // autoplay.c
bool endless_run_complete(uint32_t d);

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
} runs[4];

uint64_t mod_player_pid(void) {
    // A stable fake player id for the signed-in account (Course World compares makers with it: own courses).
    uint64_t h = 1469598103934665603ull;
    for (const char *p = g_mod.token; *p; p++) h = (h ^ (uint8_t)*p) * 1099511628211ull;
    return 0x4000000000000000ull | (h & 0x0000ffffffffffffull) | 1;
}

// Makers of the courses served (fake player ids -> names), for the user records the game asks for next.
static struct { uint64_t pid; char name[48]; } makers[64];
static int nmakers;

static uint64_t maker_pid(const char *maker_id, const char *name) {
    uint64_t h = 1469598103934665603ull;
    for (const char *p = maker_id ? maker_id : ""; *p; p++) h = (h ^ (uint8_t)*p) * 1099511628211ull;
    uint64_t pid = 0x5000000000000000ull | (h & 0x0000ffffffffffffull) | 1;
    pthread_mutex_lock(&state_lock);
    int i = 0;
    while (i < nmakers && makers[i].pid != pid) i++;
    if (i == nmakers) {
        i = nmakers < 64 ? nmakers++ : (int)(pid % 64);
        makers[i].pid = pid;
    }
    snprintf(makers[i].name, sizeof makers[i].name, "%s", name && *name ? name : "Maker");
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

static bool hundred_rules(void) { return mod_hundred_rules(); }

// Name and best per difficulty for the player's own record (online.c GetUsers). Best = the best clears of SMM2's
// rules, or with SMM1's rules the number of completed 100 Mario runs on that difficulty (the server's hundred_<d>
// boards): the menu's "High Score" slot shows that.
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
}

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
        // Present for every difficulty: New Game reads the start lives from here (the game has no default).
        s8(r + 0x00, 1);
        s32(r + 0x04, 0);
        s8(r + 0x08, (uint8_t)(coins % 100));
        s32(r + 0x0c, score);
        s8(r + 0x10, 0);
        s8(r + 0x11, start_lives);
        s64(r + 0x18, 0);
        s64(r + 0x20, 0);
        // The play part: a run that can go on (Continue) has lives; a finished or completed one shows none.
        bool cont = active && lives > 0 && !complete;
        s8(r + 0x28, cont ? 1 : 0);
        s32(r + 0x2c, cont ? clears : 0);
        s8(r + 0x30, cont ? (uint8_t)(lives > 255 ? 255 : lives) : 0);
        pthread_mutex_lock(&state_lock);
        runs[d].coins = coins, runs[d].score = score, runs[d].complete = complete;
        pthread_mutex_unlock(&state_lock);
        MLOG("status %s: %s lives %u clears %u%s\n", DIFF[d], active ? "run" : "no run", lives, clears,
             complete ? " (complete)" : "");
    }
    netobj_unlock(c);
    json_free(st);
    finish(c, no, true, 0);
}

// ---- InitEndlessMode: New Game

WRAPPER(init_run, 0x14ece20) {
    uint32_t d = req_diff(no, NO_DIFF);
    char path[64], body[64];
    snprintf(path, sizeof path, "/api/endless/%s/start", DIFF[d]);
    snprintf(body, sizeof body, "{\"ruleset\":\"%s\"}", g_mod.ruleset);
    int st;
    Json *j = srv_call("POST", path, body, &st);
    bool ok = j != NULL;
    if (ok) {
        pthread_mutex_lock(&state_lock);
        memset(&runs[d], 0, sizeof runs[d]);
        pthread_mutex_unlock(&state_lock);
        autoplay_event("new");
        LOG("[mod] New Game: %s, %s rules, %u lives\n", DIFF[d], g_mod.ruleset,
            (uint32_t)json_num(json_get(json_get(j, "run"), "lives"), 0));
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
    uint32_t stats[10] = {plays, attempts, 0, clears, 0};  // play statistics (keys 0-4); ratings unknown: 0
    for (int i = 0; i < 10; i++) s32(e + 0x1a8 + 4 * i, stats[i]);
    s64(e + 0x1d0, 0), s64(e + 0x1d8, 0), s32(e + 0x1e0, 0), s32(e + 0x1e4, 0), s32(e + 0x1e8, 0);
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

WRAPPER(search_courses, 0x14d8550) {
    if (g32(no + NO_SEARCH_TYPE) != 0xE) {  // other searches (Course World's lists): the game's own code (offline)
        guest_call_orig(c, MAIN_ADDR(0x14d8550));
        return;
    }
    uint32_t d = req_diff(no, NO_SEARCH_DIFF);
    char path[96];
    snprintf(path, sizeof path, "/api/endless/%s/next?count=10", DIFF[d]);
    Json *j = srv_call("GET", path, NULL, NULL);
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
    autoplay_event("list");
    LOG("[mod] course list (%s): %u courses%s\n", DIFF[d], count, json_bool(json_get(j, "widened"), false) ? " (band widened)" : "");
    json_free(j);
    finish(c, no, count > 0, 2);
}

// GetUsers for a list of players (the makers of the courses just listed): request ring of u64 player ids at
// +0x72b10 (+8 capacity, +0xc head, +0x10 count); answer: the user ring at +0x68, one record each (name from the
// course list, the default Mii).
void user_entry_reset(uint64_t e);                                     // online.c
void mii_default_charinfo(uint8_t out[0x58], const char *nickname);  // runtime/services/mii.c

WRAPPER(get_users, 0x14d02d0) {
    netobj_lock(c);
    uint64_t req = no + 0x72b10, out = no + 0x68;
    uint32_t rcap = g32(req + 8), rhead = g32(req + 0xc), rcount = g32(req + 0x10);
    uint32_t ocap = g32(out + 8), n = 0;
    s64(out + 0xc, 0);
    for (uint32_t i = 0; i < rcount && rcap && n < ocap; i++) {
        uint64_t pid = g64(g64(req) + 8ull * ((rhead + i) % rcap));
        if (!pid) continue;
        char name[48];
        if (pid == mod_player_pid()) snprintf(name, sizeof name, "%s", getenv("RECOMP_USER_NAME") ? getenv("RECOMP_USER_NAME") : "Player");
        else maker_name(pid, name, sizeof name);
        uint64_t e = g64(out) + 0x1f0ull * n++;
        user_entry_reset(e);
        put_utf16(g64(e + 0x08), g32(e + 0x10), name);
        s64(e + 0x58, pid);
        mii_default_charinfo((uint8_t *)(uintptr_t)(e + 0x9c), name);
        s8(e + 0x1ec, 1);
    }
    s32(out + 0x10, n);
    netobj_unlock(c);
    MLOG("GetUsers: %u players\n", n);
    finish(c, no, true, 0);
}

// Course details the game asks for around a course start (names [?]; outputs as the originals write them): two
// player records about the course (+0xe8530 and +0xe8580, each with a "valid" byte; [inf] world record / first
// clear), a list for a course (ring at +0xe85a0, 0x18-byte entries), and a check on an id (+0x141480 -> u32 result
// at +0x141604, 0 = fine). Answered empty: no records, an empty list, the check passes. Not connected, the game
// shows a communication error at times (it doesn't need them to play).
WRAPPER(course_records, 0x14ebc60) {
    netobj_lock(c);
    s8(no + 0xe8542, 0);
    s8(no + 0xe8592, 0);
    netobj_unlock(c);
    finish(c, no, true, 0);
}

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

// A play report for a course (data id +0xe8548, two bytes, the "notify the maker" setting), sent after a skip that
// followed a miss: nothing to store, accepted.
WRAPPER(play_report, 0x14e6b90) {
    MLOG("play report for %llu: accepted\n", (unsigned long long)g64(no + 0xe8548));
    finish(c, no, true, 0);
}

// The play result of a course (data id +0x8e618, time +0x8e620 and more, with a recording of the play uploaded
// first [inf]): our server keeps its own statistics from the Endless reports; accepted.
WRAPPER(play_result, 0x14e30e0) {
    MLOG("play result for %llu: accepted\n", (unsigned long long)g64(no + 0x8e618));
    finish(c, no, true, 0);
}

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

// A post about a course (id +0x141480, a byte +0x141518, a text of up to 100 characters +0x141528 [inf: a comment or
// a report]): accepted, not stored.
WRAPPER(course_post, 0x14f2910) {
    MLOG("course post for %llu (kind %u): accepted\n", (unsigned long long)g64(no + 0x141480), g8(no + 0x141518));
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
    if (!miss && endless_run_complete(d)) {
        // 100 Mario: the run reached its goal. The next course doesn't start: with no lives left in the record the
        // game ends the run (course-in -> finish -> back to the menu), and the server already counted it as complete.
        set_play(c, no, d, 0, clears_now(no, d));
        LOG("[mod] 100 Mario Challenge (%s) complete: ending the run\n", DIFF[d]);
        autoplay_event("end");
        finish(c, no, true, 0);
        return;
    }
    if (!miss) {  // a new course: the server's lives and clears as they are
        Json *st = srv_call("GET", "/api/endless", NULL, NULL);
        Json *run = json_get(json_get(st, "runs"), DIFF[d]);
        bool ok = run != NULL;
        if (ok) set_play(c, no, d, (uint32_t)json_num(json_get(run, "lives"), 0), (uint32_t)json_num(json_get(run, "clears"), 0));
        autoplay_event("start");
        LOG("[mod] course start %s (%s): lives %u\n", code, DIFF[d], lives_now(no, d));
        json_free(st);
        finish(c, no, ok, 2);
        return;
    }
    char path[64], body[160];
    snprintf(path, sizeof path, "/api/endless/%s/result", DIFF[d]);
    snprintf(body, sizeof body, "{\"code\":\"%s\",\"result\":\"death\"}", code);
    Json *j = srv_call("POST", path, body, NULL);
    if (j) {
        set_play(c, no, d, (uint32_t)json_num(json_get(j, "lives"), 0), (uint32_t)json_num(json_get(j, "clears"), 0));
        autoplay_event("miss");
        LOG("[mod] miss on %s: lives %u%s\n", code, lives_now(no, d), json_bool(json_get(j, "game_over"), false) ? " (game over)" : "");
    }
    json_free(j);
    finish(c, no, j != NULL, 2);
}

// ---- DominateEndlessModeCourse: a clear (pending 1-Ups, coins, score with the time bonus)

WRAPPER(clear_course, 0x14ed120) {
    uint32_t d = req_diff(no, NO_DIFF);
    uint64_t id = g64(no + NO_CLEAR_ID);
    uint32_t pending = g8(no + NO_PENDING), coins = g8(no + NO_COINS), score = g32(no + NO_SCORE);
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
        if (complete) runs[d].complete = true;
        pthread_mutex_unlock(&state_lock);
        autoplay_event("clear");
        LOG("[mod] clear %s (%s): lives %u, clears %u%s\n", code, DIFF[d], lives, clears,
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
        autoplay_event("skip");
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

WRAPPER(suspend_run, 0x14ed360) { post_simple(c, no, "suspend"); }
WRAPPER(finish_run, 0x14ed4a0) { post_simple(c, no, "finish"); }

bool endless_run_complete(uint32_t d) {
    pthread_mutex_lock(&state_lock);
    bool v = d < 4 && runs[d].complete;
    pthread_mutex_unlock(&state_lock);
    return v;
}

void endless_register(void) {
    hle_override(MAIN_ADDR(0x14ec2d0), get_status, "mod: GetEndlessModeStatus");
    hle_override(MAIN_ADDR(0x14ece20), init_run, "mod: InitEndlessMode");
    hle_override(MAIN_ADDR(0x14ed750), get_play_info, "mod: GetEndlessModePlayInfo");
    hle_override(MAIN_ADDR(0x14eddc0), get_rank, "mod: GetEndlessModeRank");
    hle_override(MAIN_ADDR(0x14d8550), search_courses, "mod: SearchCourses (Endless)");
    hle_override(MAIN_ADDR(0x14de010), get_course_file, "mod: GetObject (course file)");
    hle_override(MAIN_ADDR(0x14d02d0), get_users, "mod: GetUsers (players)");
    hle_override(MAIN_ADDR(0x14ebc60), course_records, "mod: course records");
    hle_override(MAIN_ADDR(0x14ec0d0), course_list_info, "mod: course list info");
    hle_override(MAIN_ADDR(0x14f17a0), id_check, "mod: id check");
    hle_override(MAIN_ADDR(0x14e8ec0), course_comments, "mod: course comments");
    hle_override(MAIN_ADDR(0x14e6b90), play_report, "mod: play report");
    hle_override(MAIN_ADDR(0x14e30e0), play_result, "mod: play result");
    hle_override(MAIN_ADDR(0x14dbae0), get_courses, "mod: GetCourses (by id)");
    hle_override(MAIN_ADDR(0x14f2910), course_post, "mod: course post");
    hle_override(MAIN_ADDR(0x14ecf30), start_course, "mod: StartEndlessModeCourse");
    hle_override(MAIN_ADDR(0x14ed120), clear_course, "mod: DominateEndlessModeCourse");
    hle_override(MAIN_ADDR(0x14ed5e0), skip_course, "mod: PassEndlessModeCourse");
    hle_override(MAIN_ADDR(0x14ed360), suspend_run, "mod: SuspendEndlessMode");
    hle_override(MAIN_ADDR(0x14ed4a0), finish_run, "mod: FinishEndlessMode");
}

void endless_note_course(uint64_t data_id, const char *code) { (void)data_id, (void)code; }
