// "Pretend online" for Course World (games/smm2/mods/mod.h): the game's network session logs in without a network,
// and Course World's login sequence gets neutral answers, so its menus open; the Endless Challenge requests then
// go to our server (endless.c). Every other Course World request runs the game's own code, which finds no NEX
// connection and answers "not connected" (status 1), and the game shows its usual message for that screen.
//
// The session (Nintendo's "enl" layer over NEX) is a state machine: cConnectNetwork -> cLogInReady (starts the NEX
// login) -> cLogIn, which is also the logged-in resting state (it waits there for the next command in +0xe4). With
// the mod, cLogInReady goes straight to cLogIn and cLogIn handles the commands without a NEX connection.
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "mod.h"

#define STATE_CHANGE MAIN_ADDR(0x8b9320)  // (state machine, new state)

// enl session states (v3.0.3 exec functions)
#define ENL_LOGIN_READY MAIN_ADDR(0x60fe20)
#define ENL_LOGIN MAIN_ADDR(0x6101d0)
enum { ENL_ST_LOGIN = 4, ENL_ST_LOGOUT_ONLY = 0x1b, ENL_ST_LOGOUT = 0x1c };

static void change_state(Cpu *c, uint64_t sm, uint32_t st) {
    c->x[0] = sm;
    c->x[1] = st;
    guest_call(c, STATE_CHANGE);
}

static void enl_login_ready(Cpu *c) {
    if (!mod_hundred()) { guest_call_orig(c, ENL_LOGIN_READY); return; }
    uint64_t self = c->x[0];
    MLOG("enl session %llx: logged in (no NEX connection)\n", (unsigned long long)self);
    if (g_mod.log) {
        uint64_t vt = g64(self);
        for (int i = 0x20; i <= 0xb0; i += 8) MLOG("  enl vtable +%#x = %llx\n", i, (unsigned long long)g64(vt + i));
    }
    s8(self + 0x19, 1);  // "logged in" (the session's own query reads this byte; NEX's login callback sets it)
    s8(self + 0x1a, 0);  // no error
    change_state(c, self + 0x30, ENL_ST_LOGIN);
}

// cLogIn, the logged-in state: +0xe4 is the command the game posted (1 log in: done; 0 log out; 0xf: log out and
// stop). Matchmaking commands (3..7, online multiplayer) are not available: they are dropped.
static void enl_login(Cpu *c) {
    if (!mod_hundred()) { guest_call_orig(c, ENL_LOGIN); return; }
    uint64_t self = c->x[0];
    int32_t cmd = (int32_t)g32(self + 0xe4);
    s8(self + 0x19, 1);
    s8(self + 0x1a, 0);
    switch (cmd) {
    case 0: change_state(c, self + 0x30, ENL_ST_LOGOUT_ONLY); break;
    case 0xf: change_state(c, self + 0x30, ENL_ST_LOGOUT); break;
    case -1: break;
    default:
        MLOG("enl command %d done\n", cmd);
        s32(self + 0xe4, 0xffffffffu);
    }
}

// ---- Course World's login sequence: each state runs one request ("wrapper", this = the network-session object)
// on the network worker and goes on when the result byte it leaves at +0x10 says success. The answers here are
// neutral: no application-config overrides, the player's own user record, an unrestricted name, no Mii costumes,
// no notifications, no event course.

static void answered(Cpu *c, const char *what) {
    MLOG("%s: answered locally\n", what);
    s32(c->x[0] + 0x60, 0);
    c->x[0] = wrapper_ok();
}

#define LOCAL_WRAPPER(name, off, body)                                                           \
    static void name(Cpu *c) {                                                                   \
        if (!mod_hundred()) { guest_call_orig(c, MAIN_ADDR(off)); return; }                      \
        uint64_t no = c->x[0];                                                                   \
        (void)no;                                                                                \
        netobj_lock(c);                                                                          \
        body;                                                                                    \
        netobj_unlock(c);                                                                        \
        answered(c, #name);                                                                      \
    }

// The server's application config: 38 u32 values at +0x737b0 (copied to the UI cache at +0x28). Values 0-3 switch the
// Endless Challenge on per difficulty (> 0; with 0 the game says "This service is not active right now"); the rest
// stay 0 (features the mod doesn't serve).
LOCAL_WRAPPER(get_application_config, 0x14ce2f0, { for (int d = 0; d < 4; d++) s32(no + 0x737b0 + 4 * d, 1); })
LOCAL_WRAPPER(get_user_name_ng_type, 0x14f2cc0, s32(no + 0x141608, 0))  // the name is fine
LOCAL_WRAPPER(sync_user_profile, 0x14cef40, {})
LOCAL_WRAPPER(get_mii_clothes, 0x14d8360, s32(no + 0x73a58, 0))      // no costume rewards (ring count)
LOCAL_WRAPPER(login_check, 0x14d7d20, {})
LOCAL_WRAPPER(update_last_login, 0x14d7e00, {})
LOCAL_WRAPPER(post_activity_quest, 0x14f0b30, {})
LOCAL_WRAPPER(get_new_notification, 0x14edee0, { s8(no + 0xeac12, 0); s8(no + 0xeac14, 0); })
LOCAL_WRAPPER(get_event_course_status, 0x14f3ab0, {
    s64(no + 0x1a0810, 0); s8(no + 0x1a0818, 0); s64(no + 0x1a0820, 0); s8(no + 0x1a0828, 0);
})

// The player's own user record (GetUsers for the signed-in player): one entry of 0x1f0 bytes in the ring at +0x68
// (entries, +0x70 capacity, +0x74 head, +0x78 count). Name and best Endless clears come from our server. The entry
// is reset the way the game's own converter resets one (strings empty, statistics 0, an invalid Mii, which the game
// replaces with a default Mii), then filled.
#define USER_RING 0x68
#define USER_ENTRY 0x1f0

static void put_utf16(uint64_t buf, uint32_t cap, const char *s) {
    if (!buf || !cap) return;
    uint32_t n = 0;
    for (const unsigned char *p = (const unsigned char *)s; *p && n + 1 < cap; ) {
        uint32_t cp = *p++;
        if (cp >= 0xC0) {  // UTF-8 -> code point (BMP only; anything else becomes '?')
            int extra = cp >= 0xF0 ? 3 : cp >= 0xE0 ? 2 : 1;
            cp &= 0x3F >> extra;
            while (extra-- && (*p & 0xC0) == 0x80) cp = cp << 6 | (*p++ & 0x3F);
            if (cp > 0xFFFF) cp = '?';
        }
        s16(buf + 2 * n++, (uint16_t)cp);
    }
    s16(buf + 2 * n, 0);
}

void user_entry_reset(uint64_t e) {
    s16(g64(e + 0x08), 0);  // name (UTF-16)
    s16(g64(e + 0x30), 0);  // second UTF-16 string
    s64(e + 0x58, 0);       // player id
    s8(g64(e + 0x68), 0);   // UTF-8 string (country)
    s16(e + 0x80, 0xff);
    for (uint64_t o = 0x88; o < 0x168; o += 8) s64(e + o, 0);
    s8(e + 0x168, 0);
    static const uint8_t flags[29] = {1, 0, 0, 1, 0, 0, 1, 0, 0, 1, 0, 0, 1, 0, 0, 1, 0, 0, 1, 0, 0, 1, 0, 0, 1, 0, 0, 1, 0};
    for (int i = 0; i < 29; i++) s8(e + 0x169 + i, flags[i]);
    for (uint64_t o = 0x188; o < 0x1a8; o += 8) s64(e + o, 0);
    s8(g64(e + 0x1b0), 0);
    s32(e + 0x1e8, 0);
    s8(e + 0x1ec, 0);
}

uint64_t mod_player_pid(void);  // endless.c: the fake player id the overrides use for the signed-in account

static void get_my_user(Cpu *c) {
    if (!mod_hundred()) { guest_call_orig(c, MAIN_ADDR(0x14d5cd0)); return; }
    uint64_t no = c->x[0];
    char name[64];
    snprintf(name, sizeof name, "%s", getenv("RECOMP_USER_NAME") ? getenv("RECOMP_USER_NAME") : "Player");
    uint32_t best[4] = {0};
    extern void mod_account_info(char *name, size_t cap, uint32_t best[4]);
    mod_account_info(name, sizeof name, best);
    netobj_lock(c);
    s64(no + USER_RING + 0xc, 0);  // head 0, count 0
    if (g32(no + USER_RING + 8) > 0) {
        uint64_t e = g64(no + USER_RING);
        user_entry_reset(e);
        uint64_t pid = mod_player_pid();  // (asks /api/me once: the maker ID)
        extern const char *mod_own_maker_id(void);
        const char *mid = mod_own_maker_id();
        char code[12] = "";
        if (strlen(mid) == 11) snprintf(code, sizeof code, "%.3s%.3s%.3s", mid, mid + 4, mid + 8);
        put_utf16(g64(e + 0x08), g32(e + 0x10), code);  // maker code (the converter: NEX code -> +0x08)
        put_utf16(g64(e + 0x30), g32(e + 0x38), name);  // name
        s64(e + 0x58, pid);
        // The Maker avatar: a valid Mii (the default one, named after the account). Without one Course World asks
        // for a new avatar, which needs the console's Mii editor.
        extern void mii_default_charinfo(uint8_t out[0x58], const char *nickname);
        mii_default_charinfo((uint8_t *)(uintptr_t)(e + 0x9c), name);
        for (int d = 0; d < 4; d++) s32(e + 0x114 + 4 * d, best[d]);
        s8(e + 0x1ec, 1);  // a registered Course World user (Course World's top screen checks it, with the id)
        s32(no + USER_RING + 0x10, 1);
    }
    netobj_unlock(c);
    // The Maker avatar's name in the player's settings (an empty one sends Course World to "Create Maker Avatar",
    // which needs the console's Mii editor): the account's name when there is none yet.
    uint64_t settings = g64(MAIN_ADDR(0x2a57d60));
    if (settings && g64(settings + 0x2b0) && *(volatile uint16_t *)(uintptr_t)g64(settings + 0x2b0) == 0) {
        put_utf16(g64(settings + 0x2b0), 11, name);
        LOG("[mod] Maker avatar name set to \"%s\" (default Mii)\n", name);
    }
    MLOG("GetUsers (own): %s, best %u/%u/%u/%u\n", name, best[0], best[1], best[2], best[3]);
    s32(no + 0x60, 0);
    c->x[0] = wrapper_ok();
}

// RECOMP_MOD_TRACE=1: log each state entered by the machines above (their enter functions, reached through pointers)
static const struct { uint64_t off; const char *name; } traced[] = {
    {0x1532060, "CW login cIdle"}, {0x1532080, "CW login cWaitStartApplet"},
    {0x15320b0, "CW login cCheckParentalControl"}, {0x1532140, "CW login cLogin"},
    {0x1532260, "CW login cGetApplicationSetting"}, {0x15323d0, "CW login cGetMyUserInfo"},
    {0x15325e0, "CW login cGetUserNameNgType"}, {0x1532750, "CW login cSyncUserProfile"},
    {0x15328e0, "CW login cToSaveUserSetting"}, {0x1532910, "CW login cSaveUserSetting"},
    {0x1533010, "CW login cGetMiiClothes"}, {0x1533270, "CW login cLoginCheck"},
    {0x15333c0, "CW login cUpdateLastLoginInfo"}, {0x15334f0, "CW login cPostActivityQuest"},
    {0x1533620, "CW login cGetNewNotification"}, {0x15337e0, "CW login cGetEventCourseStatus"},
    {0x14940d0, "NetAppTask cIdle"}, {0x14941e0, "NetAppTask cReady"}, {0x1494250, "NetAppTask cLogIn"},
    {0x14942f0, "NetAppTask cEnlTask"}, {0x14945d0, "NetAppTask cError"}, {0x1494640, "NetAppTask cGetFriendList"},
};

static void trace_state(Cpu *c) {
    uint64_t pc = c->pc;
    for (size_t i = 0; i < sizeof traced / sizeof *traced; i++)
        if (MAIN_ADDR(traced[i].off) == pc) { LOG("[mod] state: %s\n", traced[i].name); break; }
    guest_call_orig(c, pc);
}

// Every Course World request function of v3.0.3 (the callers of the session's "connected" check). With
// RECOMP_MOD_TRACE=1 the ones the mod doesn't answer are logged with their result (status word +0x60).
static const uint32_t all_wrappers[] = {
    0x14ce500, 0x14cef40, 0x14d02d0, 0x14d1820, 0x14d2f00, 0x14d5cd0, 0x14d74a0, 0x14d7780, 0x14d7b00, 0x14d7c30,
    0x14d7d20, 0x14d7e00, 0x14d7f50, 0x14d8360, 0x14d8550, 0x14dbae0, 0x14dbeb0, 0x14dc0d0, 0x14de010, 0x14de3f0,
    0x14e0c50, 0x14e0d40, 0x14e0e70, 0x14e2530, 0x14e2cf0, 0x14e30e0, 0x14e3f90, 0x14e5110, 0x14e5440, 0x14e55a0,
    0x14e58b0, 0x14e5c10, 0x14e5f20, 0x14e6060, 0x14e6b90, 0x14e6da0, 0x14e7240, 0x14e73d0, 0x14e7560, 0x14e7730,
    0x14e7af0, 0x14e8950, 0x14e8c60, 0x14e8d60, 0x14e8ec0, 0x14e96b0, 0x14e9c10, 0x14eaa20, 0x14eadb0, 0x14eaf30,
    0x14ebc60, 0x14ec0d0, 0x14ec2d0, 0x14ece20, 0x14ecf30, 0x14ed120, 0x14ed360, 0x14ed4a0, 0x14ed5e0, 0x14ed750,
    0x14eddc0, 0x14edee0, 0x14ee050, 0x14ef610, 0x14ef720, 0x14f0090, 0x14f0470, 0x14f0b30, 0x14f1210, 0x14f1330,
    0x14f15c0, 0x14f17a0, 0x14f1920, 0x14f1cf0, 0x14f22e0, 0x14f2910, 0x14f2cc0, 0x14f2de0, 0x14f3770, 0x14f3980,
    0x14f3ab0, 0x14f3cb0, 0x14f3da0, 0x14f4260, 0x14f4710, 0x14f4da0, 0x14f5940, 0x14f6100, 0x14f6600, 0x14f79a0,
    0x14f7c30, 0x14f7d10, 0x14f8260, 0x14f84d0, 0x14f8850,
};

static void trace_wrapper(Cpu *c) {
    uint64_t pc = c->pc, self = c->x[0];
    guest_call_orig(c, pc);
    LOG("[mod] wrapper main+%#llx (not answered by the mod): status %u\n", (unsigned long long)(pc - MAIN_ADDR(0)),
        g32(self + 0x60));
}

void online_register(void) {
    hle_override(ENL_LOGIN_READY, enl_login_ready, "mod: enl cLogInReady");
    hle_override(ENL_LOGIN, enl_login, "mod: enl cLogIn");
    hle_override(MAIN_ADDR(0x14ce2f0), get_application_config, "mod: GetApplicationConfig");
    hle_override(MAIN_ADDR(0x14d5cd0), get_my_user, "mod: GetUsers (own)");
    hle_override(MAIN_ADDR(0x14f2cc0), get_user_name_ng_type, "mod: GetUserNameNgType");
    hle_override(MAIN_ADDR(0x14cef40), sync_user_profile, "mod: SyncUserProfile");
    hle_override(MAIN_ADDR(0x14d8360), get_mii_clothes, "mod: GetMiiClothes");
    hle_override(MAIN_ADDR(0x14d7d20), login_check, "mod: LoginCheck");
    hle_override(MAIN_ADDR(0x14d7e00), update_last_login, "mod: UpdateLastLoginInfo");
    hle_override(MAIN_ADDR(0x14f0b30), post_activity_quest, "mod: PostActivityQuest");
    hle_override(MAIN_ADDR(0x14edee0), get_new_notification, "mod: GetNewNotification");
    hle_override(MAIN_ADDR(0x14f3ab0), get_event_course_status, "mod: GetEventCourseStatus");
    if (getenv("RECOMP_MOD_TRACE")) {
        for (size_t i = 0; i < sizeof traced / sizeof *traced; i++)
            hle_override(MAIN_ADDR(traced[i].off), trace_state, traced[i].name);
        extern void endless_register(void);
        endless_register();  // (the answered ones first: the first override of an address wins)
        for (size_t i = 0; i < sizeof all_wrappers / sizeof *all_wrappers; i++)
            hle_override(MAIN_ADDR(all_wrappers[i]), trace_wrapper, "mod: trace wrapper");
    }
}
