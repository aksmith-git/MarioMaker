// SMM2 v3.0.3 mods: shared declarations (games/smm2/mods/*.c).
//
// The 100 Mario Challenge mod keeps SMM2's Endless Challenge (its screens, run logic, HUD) and answers the game's
// Course World network calls from our own server (core/crates/server: /api/endless, /api/courses) instead of
// Nintendo's. The calls are replaced one level above the network library: the game's own request functions
// ("wrappers", each reached through a function pointer) get host overrides (hle_override) that talk HTTP and write
// the plain records the game's callbacks read. Addresses are v3.0.3; addresses are offsets in main (MAIN_ADDR).
#pragma once
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#include "../../../runtime/hle.h"

#define MAIN_ADDR(off) (MAIN_BASE + (off))  // an offset in main -> its address (runtime/rt.h: moves on iOS devices)

// ---- settings (settings.c): environment variables, else smm2mods.ini in the data folder
typedef struct {
    bool hundred;        // RECOMP_MOD_100MARIO / hundred=1: the 100 Mario Challenge mod is on
    char server[256];    // RECOMP_MOD_SERVER / server=: our server's base URL ("http://127.0.0.1:8080")
    char token[160];     // RECOMP_MOD_TOKEN / token=: the account token (`mmk register`)
    char ruleset[16];    // RECOMP_MOD_RULESET / ruleset=: "hundred" (SMM1's rules, default) or "smm2"
    char courses[16];    // RECOMP_MOD_COURSES / courses=: "server" (server= above) or "tgrcode" (a server built into the
                         // game, fed from TGRCode's course archive; iOS default)
    int timeout_ms;      // RECOMP_MOD_TIMEOUT_MS / timeout_ms=: per HTTP request (default 8000)
    bool log;            // RECOMP_MOD_LOG / log=1: log every request and override
} ModSettings;
extern ModSettings g_mod;
void mod_settings_load(void);   // once (idempotent)
void mod_server_ready(void);    // before the first request: with courses=tgrcode, start the built-in server (once)
bool mod_hundred(void);         // the 100 Mario mod is on
bool mod_hundred_offered(void); // ... and 100 Mario is offered (ruleset "hundred": the Endless button opens it)
bool mod_hundred_rules(void);   // ... and the player is in the 100 Mario part now: SMM1's rules
bool mod_smm2_session(void);    // the player went on to SMM2's own Endless Challenge (B on the 100 Mario select)
void mod_set_smm2_session(bool on);
#define MLOG(...) do { if (g_mod.log) LOG("[mod] " __VA_ARGS__); } while (0)

// ---- server calls (server.c): JSON over HTTP; NULL on any failure (logged)
typedef struct Json Json;
Json *srv_call(const char *method, const char *path, const char *body, int *status);
Json *srv_call_slow(const char *method, const char *path, const char *body, int *status, int timeout_ms);
void *srv_get_bytes(const char *path, size_t *len);  // raw body (course data); malloc'd
Json *srv_post_bytes(const char *path, const void *data, size_t len, int *status);  // raw body up (a course)

// ---- game state the overrides share
uint64_t netobj(void);          // the network-session object (requests and answers of the wrappers)
uint8_t wrapper_ok(void);       // the byte a wrapper returns on success / failure (game globals)
uint8_t wrapper_fail(void);
void netobj_lock(Cpu *c);       // the lock the original wrappers take around their writes
void netobj_unlock(Cpu *c);
// guest helpers
static inline uint8_t g8(uint64_t a) { return *(volatile uint8_t *)(uintptr_t)a; }
static inline uint32_t g32(uint64_t a) { return *(volatile uint32_t *)(uintptr_t)a; }
static inline uint64_t g64(uint64_t a) { return *(volatile uint64_t *)(uintptr_t)a; }
static inline void s8(uint64_t a, uint8_t v) { *(volatile uint8_t *)(uintptr_t)a = v; }
static inline void s16(uint64_t a, uint16_t v) { *(volatile uint16_t *)(uintptr_t)a = v; }
static inline void s32(uint64_t a, uint32_t v) { *(volatile uint32_t *)(uintptr_t)a = v; }
static inline void s64(uint64_t a, uint64_t v) { *(volatile uint64_t *)(uintptr_t)a = v; }
// call a recompiled guest function directly (call-through for an overridden address: dispatch would come back here)
void guest_call_orig(Cpu *c, uint64_t addr);

// ---- per-mod registration (called once at start-up when the mod is on)
void online_register(void);     // online.c: pretend-online login and default "not connected" wrappers
void endless_register(void);    // endless.c: the Endless Challenge wrappers against our server
void endless_note_course(uint64_t data_id, const char *code);  // course.c -> endless.c: which code a data id is

// ---- course codes (course.c)
bool code_to_data_id(const char *code, uint32_t *id);
void data_id_to_code(uint32_t id, char out[12]);
