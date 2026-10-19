// Course World against our server (games/smm2/mods/mod.h): the course lists, course details, makers, likes,
// comments, uploads and leaderboards that the game would ask Nintendo's servers for. Each request function
// ("wrapper", this = the network-session object) is answered from core/crates/server's /api routes, writing what
// the game's own converters would write from a NEX answer. Names of the requests (DataStore SMM2 methods) come
// from the client vtable slot each wrapper calls (v3.0.3).
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "mod.h"
#include "../../../runtime/net/json.h"

void mod_fill_course(uint64_t e, Json *m, uint32_t data_id, uint32_t difficulty);  // endless.c
bool mod_maker_id(uint64_t pid, char out[16]);
const char *mod_own_maker_id(void);

#define NO_STATUS 0x60
#define NO_COURSES 0x30780       // course list ring: entries (0x548 bytes), +8 capacity, +0xc head, +0x10 count
#define COURSE_ENTRY 0x548
#define NO_PAGE 0x737a8          // u32: the list page (the request asks for 100 from page * 100)
#define NO_SEARCH_TYPE 0x746e8   // u32: which search
#define NO_SEARCH_DIFF 0x746ec   // u32: preferred difficulty (Popular)
#define NO_SEARCH_PID 0x746f0    // u64: the maker (posted by / liked by / played by ...)
#define NO_ADV_DIFF 0x746fc      // u32: detailed search: difficulty, u8 style +0x74704, u8 theme +0x74705, sort +0x7470c
#define NO_ADV_STYLE 0x74704
#define NO_ADV_THEME 0x74705
#define NO_ADV_SORT 0x7470c

static void finish(Cpu *c, uint64_t no, bool ok, uint32_t status) {
    s32(no + NO_STATUS, ok ? 0 : status);
    c->x[0] = ok ? wrapper_ok() : wrapper_fail();
}

static const char *const STYLES[5] = {"M1", "M3", "MW", "WU", "3W"};

// The difficulty band of a course from the server's difficulty value (0-100, its fail rate model): Easy, Normal,
// Expert, Super Expert as the game shows them.
static uint32_t band(Json *m) {
    uint32_t v = (uint32_t)json_num(json_get(m, "difficulty"), 50);
    return v < 35 ? 0 : v < 75 ? 1 : v < 96 ? 2 : 3;
}

// The search types of the course-list wrapper (main+0x14d8550, NetObj +0x746e8) and the DataStore method each
// asks for: 0 PickUp (84), 1 PointRanking (71, "Popular", by difficulty), 2 Latest (73, "New"), 3 FolloweePostedBy
// (82), 4 Advanced (72, detailed search), 5 TermsRanking (83), 6 PostedBy (74), 7 PositiveRatedBy (75), 8 BestTime
// (81), 9 FirstClear (80), 10 PlayedBy (76), 11-13 Battle (77, 78), 14 EndlessMode (79, endless.c).
static const char *const SEARCH_NAMES[14] = {"pick up", "popular", "new", "followed makers", "detailed search",
                                             "terms ranking", "posted by", "liked by", "world records",
                                             "first clears", "played by", "versus", "versus", "versus"};

void world_search(Cpu *c, uint64_t no, uint32_t type) {
    uint32_t page = g32(no + NO_PAGE), want_band = 4, want_style = 5;
    char path[256], id[16] = "";
    const char *sort = "popular";
    bool by_maker = false, none = false;
    switch (type) {
    case 0: case 5: break;                                      // pick up, terms ranking: popular
    case 1: want_band = g32(no + NO_SEARCH_DIFF); break;        // popular, by difficulty
    case 2: sort = "new"; break;                                // new
    case 4:                                                     // detailed search
        want_band = g32(no + NO_ADV_DIFF);
        want_style = g8(no + NO_ADV_STYLE);
        if (g32(no + NO_ADV_SORT) == 1) sort = "new";
        break;
    case 6: by_maker = true; break;                             // a maker's courses
    case 8: case 9: by_maker = true; break;                     // records / first clears by a maker: filtered below
    default: none = true; break;                                // followed / liked / played by, versus: none kept
    }
    Json *j = NULL;
    if (by_maker) {
        if (mod_maker_id(g64(no + NO_SEARCH_PID), id)) {
            snprintf(path, sizeof path, "/api/makers/%s", id);
            j = srv_call("GET", path, NULL, NULL);
        }
    } else if (!none) {
        snprintf(path, sizeof path, "/api/courses?sort=%s&offset=%u&limit=100%s%s", sort, page * 100,
                 want_style < 5 ? "&style=" : "", want_style < 5 ? STYLES[want_style] : "");
        j = srv_call("GET", path, NULL, NULL);
    }
    Json *list = json_get(j, "courses");
    netobj_lock(c);
    uint64_t ring = no + NO_COURSES, entries = g64(ring);
    uint32_t cap = g32(ring + 8), count = 0;
    s64(ring + 0xc, 0);
    for (size_t i = 0; i < json_len(list) && count < cap && entries; i++) {
        Json *m = json_at(list, i);
        uint32_t data_id;
        if (!code_to_data_id(json_str(json_get(m, "code"), ""), &data_id)) continue;
        if (want_band < 4 && band(m) != want_band) continue;
        if (type == 8 && strcmp(json_str(json_get(m, "record_holder"), ""), id)) continue;
        if (type == 9 && strcmp(json_str(json_get(m, "first_clear"), ""), id)) continue;
        mod_fill_course(entries + (uint64_t)COURSE_ENTRY * count++, m, data_id, band(m));
    }
    s32(ring + 0x10, count);
    netobj_unlock(c);
    LOG("[mod] course list (%s%s%s, page %u): %u courses\n", type < 14 ? SEARCH_NAMES[type] : "?", id[0] ? " " : "",
        id, page, count);
    bool ok = none || by_maker || j;
    json_free(j);
    finish(c, no, ok, 2);
}

// ---- helpers

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

static void get_utf16(uint64_t buf, char *out, size_t cap) {  // (ASCII is all a code or an id needs)
    size_t n = 0;
    for (uint32_t i = 0; buf && n + 1 < cap; i++) {
        uint16_t ch = *(volatile uint16_t *)(uintptr_t)(buf + 2 * i);
        if (!ch) break;
        out[n++] = ch < 0x80 ? (char)ch : '?';
    }
    out[n] = 0;
}

static void course_code_of(uint64_t data_id, char out[12]) { data_id_to_code((uint32_t)data_id, out); }

static bool post_ok(const char *path, const char *body) {
    int st = 0;
    Json *j = srv_call("POST", path, body, &st);
    json_free(j);
    return j != NULL;
}

// ---- users (makers): the user record (0x1f0 bytes) as the game's converter (main+0x14d0a60) fills it from a NEX
// UserInfo: maker code +0x08, name +0x30 (both UTF-16, the code without dashes), player id +0x58, Mii +0x9c, play
// stats +0xfc.. (plays, clears, attempts, deaths), Endless best clears +0x114 + 4 * difficulty, maker stats +0x124
// (likes received) / +0x128 (maker points), versus / co-op +0x130.., "valid" +0x1ec.
void user_entry_reset(uint64_t e);                                     // online.c
void mii_default_charinfo(uint8_t out[0x58], const char *nickname);  // runtime/services/mii.c
uint64_t mod_maker_pid(const char *maker_id, const char *name);
uint64_t mod_player_pid(void);
#define USER_RING 0x68
#define USER_ENTRY 0x1f0

// The maker's record from GET /api/makers/<id> (name, courses -> likes and maker points); NULL: unknown maker.
static Json *maker_profile(const char *maker_id) {
    if (!maker_id || !*maker_id) return NULL;
    char path[64];
    snprintf(path, sizeof path, "/api/makers/%s", maker_id);
    return srv_call("GET", path, NULL, NULL);
}

static void fill_user(uint64_t e, uint64_t pid, const char *maker_id, const char *name_hint, Json *prof) {
    const char *name = json_str(json_get(json_get(prof, "maker"), "name"), name_hint && *name_hint ? name_hint : "Maker");
    uint32_t likes = 0, points = 0;
    Json *courses = json_get(prof, "courses");
    for (size_t i = 0; i < json_len(courses); i++) {
        int32_t l = (int32_t)json_num(json_get(json_at(courses, i), "likes"), 0);
        int32_t b = (int32_t)json_num(json_get(json_at(courses, i), "boos"), 0);
        likes += (uint32_t)l;
        points += l > b ? (uint32_t)(l - b) : 0;
    }
    char code[12] = "";
    if (maker_id) snprintf(code, sizeof code, "%.3s%.3s%.3s", maker_id, maker_id + 4, maker_id + 8);
    user_entry_reset(e);
    put_utf16(g64(e + 0x08), g32(e + 0x10), code);  // (the converter: NEX maker code -> +0x08, name -> +0x30)
    put_utf16(g64(e + 0x30), g32(e + 0x38), name);
    s64(e + 0x58, pid);
    mii_default_charinfo((uint8_t *)(uintptr_t)(e + 0x9c), name);
    s32(e + 0x124, likes);
    s32(e + 0x128, points);
    s8(e + 0x1ec, 1);
}

// Users by player id into the user ring: a list of ids (ring at +0x72b10: entries, +8 capacity, +0xc head, +0x10
// count; GetUsers main+0x14d02d0 / +0x14d7780) or the one id at +0x73a30 (the maker page, main+0x14d2f00).
static void users_answer(Cpu *c, uint64_t no, const uint64_t *pids, uint32_t n, const char *what) {
    char ids[32][16], names[32][48];
    uint32_t k = 0;
    for (uint32_t i = 0; i < n && k < 32; i++) {
        if (!pids[i]) continue;
        mod_maker_id(pids[i], ids[k]);
        extern void mod_maker_name(uint64_t pid, char *out, size_t cap);
        mod_maker_name(pids[i], names[k], sizeof names[k]);
        k++;
    }
    Json *prof[32] = {0};
    for (uint32_t i = 0; i < k; i++) prof[i] = maker_profile(ids[i]);  // (outside the lock: HTTP)
    netobj_lock(c);
    uint64_t out = no + USER_RING;
    uint32_t cap = g32(out + 8), m = 0;
    s64(out + 0xc, 0);
    for (uint32_t i = 0, j = 0; i < n && m < cap && j < k; i++) {
        if (!pids[i]) continue;
        fill_user(g64(out) + (uint64_t)USER_ENTRY * m++, pids[i], ids[j], names[j], prof[j]);
        j++;
    }
    s32(out + 0x10, m);
    netobj_unlock(c);
    for (uint32_t i = 0; i < k; i++) json_free(prof[i]);
    MLOG("%s: %u users\n", what, m);
    finish(c, no, true, 0);
}

static uint32_t pid_list(uint64_t no, uint64_t pids[32]) {
    uint64_t req = no + 0x72b10;
    uint32_t rcap = g32(req + 8), rhead = g32(req + 0xc), rcount = g32(req + 0x10), n = 0;
    for (uint32_t i = 0; i < rcount && rcap && n < 32; i++) pids[n++] = g64(g64(req) + 8ull * ((rhead + i) % rcap));
    return n;
}

static void get_users_list(Cpu *c) {  // main+0x14d02d0 and +0x14d7780 (GetUsers for listed players)
    if (!mod_hundred()) { guest_call_orig(c, c->pc); return; }
    uint64_t no = c->x[0], pids[32];
    netobj_lock(c);
    uint32_t n = pid_list(no, pids);
    netobj_unlock(c);
    users_answer(c, no, pids, n, "GetUsers");
}

static void get_user_page(Cpu *c) {  // main+0x14d2f00 (a maker's page: GetUsers + their Super World: none)
    if (!mod_hundred()) { guest_call_orig(c, c->pc); return; }
    uint64_t no = c->x[0], pid = g64(no + 0x73a30);
    users_answer(c, no, &pid, 1, "maker page");
}

static void get_user_counts(Cpu *c) {  // main+0x14d74a0 (two counters of one user, map +0x190 keys 0 / 1): 0
    if (!mod_hundred()) { guest_call_orig(c, c->pc); return; }
    uint64_t no = c->x[0];
    netobj_lock(c);
    s64(no + 0x73a3c, 0);
    netobj_unlock(c);
    finish(c, no, true, 0);
}

// ---- leaderboards (SearchUsers*, main+0x14d1820): kind +0x73a14: 0 maker points (50), 1 Endless (51, difficulty
// +0x73a18, when +0x73a1c is 3..6) or the clear ranking (57, +0x73a20), 2 versus (52), 4 terms (58), 5 official
// (147), 6 friends (167), 7..9 players of a course (+0x73a28). Our server: /api/rankings/<kind>.
static void search_users(Cpu *c) {
    if (!mod_hundred()) { guest_call_orig(c, c->pc); return; }
    uint64_t no = c->x[0];
    static const char *const DIFF[4] = {"easy", "normal", "expert", "super_expert"};
    uint32_t kind = g32(no + 0x73a14), page = g32(no + 0x737a8);
    char board[48] = "";
    int endless_d = -1;
    if (kind == 0 || kind == 4) snprintf(board, sizeof board, "maker_points");  // (4: the period's ranking)
    else if (kind == 1 && g32(no + 0x73a1c) - 3 < 4) {
        endless_d = (int)(g32(no + 0x73a18) & 3);
        snprintf(board, sizeof board, "endless_%s", DIFF[endless_d]);
    } else if (kind == 1) snprintf(board, sizeof board, "clears");
    Json *j = NULL;
    if (*board) {
        char path[96];
        snprintf(path, sizeof path, "/api/rankings/%s?limit=%u", board, (page + 1) * 100);
        j = srv_call("GET", path, NULL, NULL);
    }
    Json *rows = json_get(j, "rankings");
    size_t n = json_len(rows), first = (size_t)page * 100;
    uint32_t cnt = 0;
    uint64_t pids[100];
    char ids[100][16];
    uint32_t scores[100];
    for (size_t i = first; i < n && cnt < 100; i++) {
        Json *r = json_at(rows, i);
        snprintf(ids[cnt], 16, "%s", json_str(json_get(r, "maker_id"), ""));
        pids[cnt] = mod_maker_pid(ids[cnt], json_str(json_get(r, "name"), ""));
        scores[cnt++] = (uint32_t)json_num(json_get(r, "score"), 0);
    }
    netobj_lock(c);
    uint64_t out = no + USER_RING;
    uint32_t cap = g32(out + 8), m = 0;
    s64(out + 0xc, 0);
    for (uint32_t i = 0; i < cnt && m < cap; i++) {
        uint64_t e = g64(out) + (uint64_t)USER_ENTRY * m++;
        fill_user(e, pids[i], ids[i], json_str(json_get(json_at(rows, first + i), "name"), ""), NULL);
        if (kind == 0 || kind == 4) s32(e + 0x128, scores[i]), s32(e + 0x110, scores[i]);  // maker points [? +0x110: the period's]
        else if (endless_d >= 0) s32(e + 0x114 + 4 * endless_d, scores[i]);  // Endless best
        else if (kind == 1) s32(e + 0x100, scores[i]);              // courses cleared (play stats key 1)
    }
    s32(out + 0x10, m);
    netobj_unlock(c);
    LOG("[mod] leaderboard %s (kind %u, page %u): %u makers\n", *board ? board : "-", kind, page, m);
    json_free(j);
    finish(c, no, true, 0);
}

// ---- a course or a maker by ID (GetUserOrCourse 131, main+0x14dc0d0): the code at +0x74728 (UTF-16); a course
// goes to the course list ring, a maker to the user ring.
static void get_user_or_course(Cpu *c) {
    if (!mod_hundred()) { guest_call_orig(c, c->pc); return; }
    uint64_t no = c->x[0];
    char raw[32], code[16];
    get_utf16(g64(no + 0x74728), raw, sizeof raw);
    size_t k = 0;
    for (const char *p = raw; *p && k < 9; p++)
        if (*p != '-' && *p != ' ') code[k++] = (char)(*p >= 'a' && *p <= 'z' ? *p - 32 : *p);
    code[k] = 0;
    char dashed[16];
    snprintf(dashed, sizeof dashed, "%.3s-%.3s-%.3s", code, code + 3, code + 6);
    char path[64];
    snprintf(path, sizeof path, "/api/courses/%s", dashed);
    Json *course = srv_call("GET", path, NULL, NULL);
    Json *maker = course ? NULL : maker_profile(dashed);
    netobj_lock(c);
    s64(no + NO_COURSES + 0xc, 0);
    s64(no + USER_RING + 0xc, 0);
    uint32_t id;
    if (course && code_to_data_id(dashed, &id) && g32(no + NO_COURSES + 8)) {
        mod_fill_course(g64(no + NO_COURSES), course, id, band(course));
        s32(no + NO_COURSES + 0x10, 1);
    } else if (maker && g32(no + USER_RING + 8)) {
        fill_user(g64(no + USER_RING), mod_maker_pid(dashed, json_str(json_get(json_get(maker, "maker"), "name"), "")),
                  dashed, "", maker);
        s32(no + USER_RING + 0x10, 1);
    }
    netobj_unlock(c);
    LOG("[mod] ID search %s: %s\n", dashed, course ? "course" : maker ? "maker" : "nothing");
    json_free(course);
    json_free(maker);
    finish(c, no, true, 0);
}

// ---- likes and boos, comments allowed

// CanPostRatingAndComment (61, main+0x14ebc60): data id +0xe8578; two answers, rating +0xe8580 {u64 id, bool can,
// u32, two counts, valid +0xe8592} and comment +0xe8530 {.., valid +0xe8542}. Both allowed.
static void can_rate(Cpu *c) {
    if (!mod_hundred()) { guest_call_orig(c, c->pc); return; }
    uint64_t no = c->x[0], id = g64(no + 0xe8578);
    const char *cv = getenv("RECOMP_MOD_CANRATE");  // (test: the flag's value)
    uint8_t flag = cv ? (uint8_t)atoi(cv) : 1;
    netobj_lock(c);
    for (int i = 0; i < 2; i++) {
        uint64_t r = no + (i ? 0xe8530 : 0xe8580);
        s64(r, id), s8(r + 8, flag), s32(r + 0xc, 0), s16(r + 0x10, 0), s8(r + 0x12, 1);
    }
    netobj_unlock(c);
    MLOG("rating / comments allowed for %llu\n", (unsigned long long)id);
    finish(c, no, true, 0);
}

// PostRatingInfo (104, main+0x14e6b90): data id +0xe8548, u8 +0xe8550 (the rating [inf]: 1 like, 2 boo), u8
// +0xe8554. -> POST /api/courses/<code>/like | boo.
static void post_rating(Cpu *c) {
    if (!mod_hundred()) { guest_call_orig(c, c->pc); return; }
    uint64_t no = c->x[0], id = g64(no + 0xe8548);
    uint32_t a = g32(no + 0xe8550), b = g32(no + 0xe8554);
    char code[12], path[64];
    course_code_of(id, code);
    bool ok = true;
    if (a == 1 || a == 2) {
        snprintf(path, sizeof path, "/api/courses/%s/%s", code, a == 1 ? "like" : "boo");
        ok = post_ok(path, "{}");
    }
    LOG("[mod] rating %s: %u / %u%s\n", code, a, b, a == 1 ? " (like)" : a == 2 ? " (boo)" : "");
    finish(c, no, ok, 2);
}

// PostPlayResult (96, main+0x14e30e0): data id +0x8e618, cleared +0x8e620, time ms +0x8e624, u8 +0x8e628, u64 +0x8e630,
// death position +0x8e638 / +0x8e63c, ... (plus a recording uploaded first [inf]). Our server counts a play, and a
// clear with its time. Endless plays are counted by the Endless reports.
bool endless_has_course(uint64_t id);  // endless.c
static void post_play_result(Cpu *c) {
    if (!mod_hundred()) { guest_call_orig(c, c->pc); return; }
    uint64_t no = c->x[0], id = g64(no + 0x8e618);
    // [obs: a clear in 12.851 s posted 1 / 12851 / 1]: +0x8e620 cleared, +0x8e624 the time in ms
    uint32_t cleared = g32(no + 0x8e620), time_ms = g32(no + 0x8e624), v2 = g8(no + 0x8e628);
    char code[12], path[64], body[64];
    course_code_of(id, code);
    LOG("[mod] play result %s: %s, %u ms (%u)\n", code, cleared == 1 ? "clear" : "no clear", time_ms, v2);
    if (!endless_has_course(id)) {
        snprintf(path, sizeof path, "/api/courses/%s/play", code);
        post_ok(path, "{}");
        if (cleared == 1) {
            snprintf(path, sizeof path, "/api/courses/%s/clear", code);
            snprintf(body, sizeof body, "{\"time_ms\":%u}", time_ms);
            post_ok(path, body);
        }
    }
    finish(c, no, true, 0);
}

// ---- comments (text)

static size_t utf16_to_utf8(uint64_t buf, uint32_t max, char *out, size_t cap) {
    size_t n = 0;
    for (uint32_t i = 0; buf && i < max; i++) {
        uint32_t ch = *(volatile uint16_t *)(uintptr_t)(buf + 2 * i);
        if (!ch) break;
        if (ch < 0x80 && n + 1 < cap) out[n++] = (char)ch;
        else if (ch < 0x800 && n + 2 < cap) out[n++] = (char)(0xC0 | ch >> 6), out[n++] = (char)(0x80 | (ch & 0x3F));
        else if (n + 3 < cap)
            out[n++] = (char)(0xE0 | ch >> 12), out[n++] = (char)(0x80 | (ch >> 6 & 0x3F)), out[n++] = (char)(0x80 | (ch & 0x3F));
    }
    out[n] = 0;
    return n;
}

static void json_escape(const char *s, char *out, size_t cap) {
    size_t n = 0;
    for (; *s && n + 7 < cap; s++) {
        unsigned char ch = (unsigned char)*s;
        if (ch == '"' || ch == '\\') out[n++] = '\\', out[n++] = (char)ch;
        else if (ch < 0x20) n += (size_t)snprintf(out + n, cap - n, "\\u%04x", ch);
        else out[n++] = (char)ch;
    }
    out[n] = 0;
}

// SearchComments / InOrder (94 / 95, main+0x14e8ec0): data id +0x8e680; answer: the ring of comment records (0x390
// bytes) at +0x8f410, then +0xe8528 the page, +0xe852c, +0xe852d answered. A record as the game's converter
// (main+0x14b7720) fills it from a NEX CommentInfo [inf from the copies]: id (UTF-8) +0x08, two bytes +0x58 / +0x5c,
// the commenter's player id +0x60, position x / y +0x68 / +0x6c, +0x70.., posted time +0x78, the text (UTF-16)
// +0x88, +0x130 / +0x134 / +0x138 (a picture: none), the commenter's name (UTF-16) +0x2c8, Mii +0x2f0.
static void search_comments(Cpu *c) {
    if (!mod_hundred()) { guest_call_orig(c, c->pc); return; }
    uint64_t no = c->x[0], id = g64(no + 0x8e680);
    char code[12], path[64];
    course_code_of(id, code);
    snprintf(path, sizeof path, "/api/courses/%s/comments", code);
    Json *j = srv_call("GET", path, NULL, NULL);
    Json *list = json_get(j, "comments");
    uint64_t pids[64];
    size_t n = json_len(list) < 64 ? json_len(list) : 64;
    for (size_t i = 0; i < n; i++) {
        Json *m = json_at(list, i);
        pids[i] = mod_maker_pid(json_str(json_get(m, "maker_id"), ""), json_str(json_get(m, "name"), ""));
    }
    netobj_lock(c);
    uint64_t ring = no + 0x8f410, entries = g64(ring);
    uint32_t cap = g32(ring + 8), cnt = 0;
    s64(ring + 0xc, 0);
    for (size_t i = 0; i < n && cnt < cap && entries; i++) {
        Json *m = json_at(list, i);
        uint64_t e = entries + 0x390ull * cnt++;
        char cid[24];
        snprintf(cid, sizeof cid, "%zu", i + 1);
        if (g64(e + 0x08) && g32(e + 0x10) > strlen(cid)) memcpy((void *)(uintptr_t)g64(e + 0x08), cid, strlen(cid) + 1);
        s32(e + 0x58, 0), s32(e + 0x5c, 0);
        s64(e + 0x60, pids[i]);
        s32(e + 0x68, 0), s32(e + 0x6c, 0);
        s8(e + 0x70, 0), s8(e + 0x71, 0), s8(e + 0x72, 0);
        s64(e + 0x78, (uint64_t)json_num(json_get(m, "posted"), 0));
        put_utf16(g64(e + 0x88), g32(e + 0x90), json_str(json_get(m, "text"), ""));
        s16(e + 0x130, 0), s32(e + 0x134, 0), s8(e + 0x138, 0);
        put_utf16(g64(e + 0x2c8), g32(e + 0x2d0), json_str(json_get(m, "name"), ""));
        mii_default_charinfo((uint8_t *)(uintptr_t)(e + 0x2f0), json_str(json_get(m, "name"), ""));
    }
    s32(ring + 0x10, cnt);
    s32(no + 0xe8528, g32(no + 0x737a8));
    s8(no + 0xe852c, g8(no + 0x737ac));
    s8(no + 0xe852d, 1);
    netobj_unlock(c);
    LOG("[mod] comments on %s: %u\n", code, cnt);
    json_free(j);
    finish(c, no, true, 0);
}

// PostCommentText (91, main+0x14e96b0): data id +0x8e688, u8 +0x8e690, position +0x8e698 / +0x8e69c, +0x8e6a0,
// +0x8e6a2, the text (UTF-16, up to 75 characters) at *(+0x8e6b0); answer: the comment's id (UTF-8) at *(+0x8e768).
static void post_comment(Cpu *c) {
    if (!mod_hundred()) { guest_call_orig(c, c->pc); return; }
    uint64_t no = c->x[0], id = g64(no + 0x8e688);
    char code[12], path[64], text[256], esc[600], body[700];
    course_code_of(id, code);
    utf16_to_utf8(g64(no + 0x8e6b0), 75, text, sizeof text);
    json_escape(text, esc, sizeof esc);
    snprintf(path, sizeof path, "/api/courses/%s/comments", code);
    snprintf(body, sizeof body, "{\"text\":\"%s\"}", esc);
    Json *j = srv_call("POST", path, body, NULL);
    netobj_lock(c);
    uint64_t out = g64(no + 0x8e768);
    if (out && g32(no + 0x8e770) > 2) memcpy((void *)(uintptr_t)out, "1", 2);
    netobj_unlock(c);
    LOG("[mod] comment on %s: \"%s\" %s\n", code, text, j ? "posted" : "failed");
    finish(c, no, j != NULL, 2);
    json_free(j);
}

// ---- uploads from the Course Maker

// CanPostCourse (60, main+0x14e0d40): allowed; +0x8e610 the number the player may still upload [inf].
static void can_post_course(Cpu *c) {
    if (!mod_hundred()) { guest_call_orig(c, c->pc); return; }
    uint64_t no = c->x[0];
    netobj_lock(c);
    s32(no + 0x8e610, 100);
    netobj_unlock(c);
    finish(c, no, true, 0);
}

// The course upload (main+0x14de3f0: PreparePostObjectCourse, the course and thumbnail upload, CompletePostObject):
// the game's own course file is at *(+0x8e5e0) (header 0x10 with "SCDL" and the CRC32, then the course, 0x5bfc0
// bytes, not encrypted yet; the two thumbnails follow at +0x5c000). Our server takes the course itself (POST
// /api/courses, signed in) and gives back its code; the data id goes to +0x8e5f8, which the game keeps in the saved
// course as its upload.
static void post_course(Cpu *c) {
    if (!mod_hundred()) { guest_call_orig(c, c->pc); return; }
    uint64_t no = c->x[0], src = g64(no + 0x8e5e0);
    bool ok = false;
    char code[16] = "?";
    if (src && g32(src + 0xc) == 0x4c444353 && g32(src) == 1) {  // "SCDL", version 1
        int st = 0;
        Json *j = srv_post_bytes("/api/courses", (const void *)(uintptr_t)(src + 0x10), 0x5bfc0, &st);
        uint32_t id;
        if (j && code_to_data_id(json_str(json_get(j, "code"), ""), &id)) {
            snprintf(code, sizeof code, "%s", json_str(json_get(j, "code"), ""));
            netobj_lock(c);
            s64(no + 0x8e5f8, id);
            netobj_unlock(c);
            ok = true;
        }
        json_free(j);
    }
    LOG("[mod] course upload: %s\n", ok ? code : "failed");
    finish(c, no, ok, 2);
}

// DeleteObject (DataStore, main+0x14e0c50): data id +0x8e5f0, the player's uploaded course -> DELETE
static void delete_course(Cpu *c) {
    if (!mod_hundred()) { guest_call_orig(c, c->pc); return; }
    uint64_t no = c->x[0];
    char code[12], path[64];
    course_code_of(g64(no + 0x8e5f0), code);
    snprintf(path, sizeof path, "/api/courses/%s", code);
    Json *j = srv_call("DELETE", path, NULL, NULL);
    LOG("[mod] course %s deleted: %s\n", code, j ? "yes" : "no");
    json_free(j);
    finish(c, no, true, 0);
}

void world_register(void) {
    hle_override(MAIN_ADDR(0x14e8ec0), search_comments, "mod: SearchComments");
    hle_override(MAIN_ADDR(0x14e96b0), post_comment, "mod: PostCommentText");
    hle_override(MAIN_ADDR(0x14e0d40), can_post_course, "mod: CanPostCourse");
    hle_override(MAIN_ADDR(0x14de3f0), post_course, "mod: course upload");
    hle_override(MAIN_ADDR(0x14e0c50), delete_course, "mod: DeleteObject (course)");
    hle_override(MAIN_ADDR(0x14d02d0), get_users_list, "mod: GetUsers (players)");
    hle_override(MAIN_ADDR(0x14d7780), get_users_list, "mod: GetUsers (players, 2)");
    hle_override(MAIN_ADDR(0x14d2f00), get_user_page, "mod: GetUsers (maker page)");
    hle_override(MAIN_ADDR(0x14d74a0), get_user_counts, "mod: GetUsers (counts)");
    hle_override(MAIN_ADDR(0x14d1820), search_users, "mod: SearchUsers (leaderboards)");
    hle_override(MAIN_ADDR(0x14dc0d0), get_user_or_course, "mod: GetUserOrCourse");
    hle_override(MAIN_ADDR(0x14ebc60), can_rate, "mod: CanPostRatingAndComment");
    hle_override(MAIN_ADDR(0x14e6b90), post_rating, "mod: PostRatingInfo");
    hle_override(MAIN_ADDR(0x14e30e0), post_play_result, "mod: PostPlayResult");
}
