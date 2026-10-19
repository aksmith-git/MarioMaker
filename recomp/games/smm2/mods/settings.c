// Mod settings (games/smm2/mods/mod.h) and start-up: environment variables win over smm2mods.ini (key=value lines
// in the data folder, next to larecomp.ini). Nothing is on unless asked for.
//
//   RECOMP_MOD_100MARIO=1        hundred=1          100 Mario Challenge on SMM2's Endless Challenge
//   RECOMP_MOD_SERVER=<url>      server=<url>       our course server, plain http (default http://127.0.0.1:8080)
//   RECOMP_MOD_TOKEN=<token>     token=<token>      the account's token (`mmk register NAME --server URL`)
//   RECOMP_MOD_RULESET=<name>    ruleset=<name>     hundred (SMM1's 100 lives, 8/16/16/6 clears; default) or smm2
//   RECOMP_MOD_TIMEOUT_MS=<n>    timeout_ms=<n>     per request (default 8000)
//   RECOMP_MOD_LOG=1             log=1              log requests and overrides
//   RECOMP_MOD_COURSES=<src>     courses=<src>      server (the server above) or tgrcode (a server built into the
//                                                   game, fed from TGRCode's archive of SMM2 courses; iOS default)
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <pthread.h>
#include <sys/stat.h>
#if defined(__APPLE__)
#include <TargetConditionals.h>
#endif

#include "mod.h"
#include "../../../runtime/net/online.h"

ModSettings g_mod;

static void set(const char *key, const char *val) {
    if (!strcmp(key, "hundred")) g_mod.hundred = atoi(val) != 0;
    else if (!strcmp(key, "courses")) snprintf(g_mod.courses, sizeof g_mod.courses, "%s", val);
    else if (!strcmp(key, "server")) snprintf(g_mod.server, sizeof g_mod.server, "%s", val);
    else if (!strcmp(key, "token")) snprintf(g_mod.token, sizeof g_mod.token, "%s", val);
    else if (!strcmp(key, "ruleset")) snprintf(g_mod.ruleset, sizeof g_mod.ruleset, "%s", val);
    else if (!strcmp(key, "timeout_ms")) g_mod.timeout_ms = atoi(val);
    else if (!strcmp(key, "log")) g_mod.log = atoi(val) != 0;
}

void mod_settings_load(void) {
    static int done;
    if (done) return;
    done = 1;
    snprintf(g_mod.server, sizeof g_mod.server, "http://127.0.0.1:8080");
    snprintf(g_mod.ruleset, sizeof g_mod.ruleset, "hundred");
    snprintf(g_mod.courses, sizeof g_mod.courses, "server");
#if defined(__APPLE__) && TARGET_OS_IOS
    g_mod.hundred = true;  // (iOS: online with courses from TGRCode unless the ini says otherwise)
    snprintf(g_mod.courses, sizeof g_mod.courses, "tgrcode");
#endif
    g_mod.timeout_ms = 8000;
    FILE *f = fopen(getenv("RECOMP_MOD_INI") ? getenv("RECOMP_MOD_INI") : "smm2mods.ini", "r");
    if (f) {
        char line[512];
        while (fgets(line, sizeof line, f)) {
            char *eq = strchr(line, '='), *e;
            if (line[0] == '#' || !eq) continue;
            *eq = 0;
            for (e = eq + 1 + strlen(eq + 1); e > eq + 1 && (e[-1] == '\n' || e[-1] == '\r' || e[-1] == ' '); ) *--e = 0;
            set(line, eq + 1);
        }
        fclose(f);
    }
    static const struct { const char *env, *key; } envs[] = {
        {"RECOMP_MOD_100MARIO", "hundred"}, {"RECOMP_MOD_SERVER", "server"}, {"RECOMP_MOD_TOKEN", "token"},
        {"RECOMP_MOD_RULESET", "ruleset"},  {"RECOMP_MOD_TIMEOUT_MS", "timeout_ms"}, {"RECOMP_MOD_LOG", "log"},
        {"RECOMP_MOD_COURSES", "courses"},
    };
    for (size_t i = 0; i < sizeof envs / sizeof *envs; i++)
        if (getenv(envs[i].env)) set(envs[i].key, getenv(envs[i].env));
    size_t n = strlen(g_mod.server);
    while (n && g_mod.server[n - 1] == '/') g_mod.server[--n] = 0;
    if (strcmp(g_mod.ruleset, "smm2") && strcmp(g_mod.ruleset, "hundred")) snprintf(g_mod.ruleset, sizeof g_mod.ruleset, "hundred");
    if (g_mod.timeout_ms <= 0) g_mod.timeout_ms = 8000;
    if (strcmp(g_mod.courses, "tgrcode")) snprintf(g_mod.courses, sizeof g_mod.courses, "server");
    if (g_mod.hundred && !strcmp(g_mod.courses, "tgrcode"))
        LOG("[mod] 100 Mario Challenge on: courses from TGRCode (built-in server), ruleset %s\n", g_mod.ruleset);
    else if (g_mod.hundred)
        LOG("[mod] 100 Mario Challenge on: server %s, ruleset %s, %s\n", g_mod.server, g_mod.ruleset,
            g_mod.token[0] ? "signed in" : "NO TOKEN (set RECOMP_MOD_TOKEN)");
}

// ---- courses=tgrcode: the course server built into the app (core/crates/embed, runtime/net/embed_server.h; it
// returns false when the build doesn't link it), on a loopback port with a local account, fed from TGRCode's course
// archive (it finds the course key table in the loaded game itself). The same server the runtime starts at boot
// on iOS (embed_server_boot: one server per process, a second start returns the first's address). If it can't
// start, the mod falls back to server= / token=.
#include "../../../runtime/net/embed_server.h"

static void start_builtin_server(void) {
    char url[256], token[160];
    const char *name = getenv("RECOMP_USER_NAME") ? getenv("RECOMP_USER_NAME") : "Player";
    if (embed_server_start("server", "tgrcode", name, url, sizeof url, token, sizeof token)) {
        snprintf(g_mod.server, sizeof g_mod.server, "%s", url);
        snprintf(g_mod.token, sizeof g_mod.token, "%s", token);
        LOG("[mod] built-in course server on %s (courses from TGRCode)\n", url);
    } else {
        LOG("[mod] the built-in course server didn't start; using server=%s\n", g_mod.server);
    }
}

void mod_server_ready(void) {
    static pthread_once_t once = PTHREAD_ONCE_INIT;
    mod_settings_load();
    if (!strcmp(g_mod.courses, "tgrcode")) pthread_once(&once, start_builtin_server);
}

bool mod_hundred(void) { mod_settings_load(); return g_mod.hundred; }
// Both challenges in one build: with 100 Mario offered (ruleset=hundred) Course World's Endless Challenge button opens
// SMM1's 100 Mario Challenge; B on its difficulty select leads to SMM2's own Endless Challenge (its menu is underneath)
// until the player leaves that menu. SMM1's rules apply only in the 100 Mario part.
static volatile bool smm2_session;
bool mod_hundred_offered(void) { return mod_hundred() && !strcmp(g_mod.ruleset, "hundred"); }
bool mod_hundred_rules(void) { return mod_hundred_offered() && !smm2_session; }
bool mod_smm2_session(void) { return smm2_session; }
void mod_set_smm2_session(bool on) {
    if (on == smm2_session) return;
    smm2_session = on;
    extern void endless_set_start_lives(uint8_t lives);
    endless_set_start_lives(on || !mod_hundred_offered() ? 5 : 100);
    LOG("[mod] %s\n", on ? "SMM2's own Endless Challenge (SMM2's rules)" : "back to the 100 Mario Challenge");
}

// The system services report a connection while a mod answers the online calls (runtime/net/online.h).
bool net_pretend_online(void) { return mod_hundred() || (getenv("RECOMP_PRETEND_ONLINE") && atoi(getenv("RECOMP_PRETEND_ONLINE"))); }

// ---- the overlay menu's Mods tab (runtime/ui/menu.mm): one switch per SMM1 feature, kept in smm2mods.ini. Only
// "100 Mario Challenge (SMM1)" so far (on: Course World's Endless Challenge button opens SMM1's 100 Mario Challenge;
// off: SMM2's own Endless Challenge, against our server under SMM2's rules); costumes, mid-air shell jumps and
// overlapping pipes will follow.
typedef struct { const char *label, *desc; bool *value; bool def; } GameModToggle;
static bool t_hundred = true, t_tgrcode;

static void ini_set(const char *key, const char *val) {  // replace or add one key=value line
    const char *path = getenv("RECOMP_MOD_INI") ? getenv("RECOMP_MOD_INI") : "smm2mods.ini";
    char lines[64][512];
    int n = 0;
    bool done = false;
    FILE *f = fopen(path, "r");
    if (f) {
        while (n < 64 && fgets(lines[n], sizeof lines[n], f)) {
            size_t kl = strlen(key);
            if (!strncmp(lines[n], key, kl) && lines[n][kl] == '=') snprintf(lines[n], sizeof lines[n], "%s=%s\n", key, val), done = true;
            n++;
        }
        fclose(f);
    }
    if (!done && n < 64) snprintf(lines[n++], sizeof lines[0], "%s=%s\n", key, val);
    if ((f = fopen(path, "w"))) {
        for (int i = 0; i < n; i++) fputs(lines[i], f);
        fclose(f);
    }
}

int game_mod_toggles(const GameModToggle **out) {
    static const GameModToggle toggles[] = {
        {"100 Mario Challenge (SMM1)",
         "Course World's Endless Challenge opens SMM1's 100 Mario Challenge: 100 lives, 8 / 16 / 16 / 6 courses, "
         "SMM1's rules and screens; B there leads on to SMM2's own Endless Challenge. Off: the button opens SMM2's "
         "own Endless Challenge only. Both play courses from your course server.",
         &t_hundred, true},
        {"Courses: TGRCode (off: my server)",
         "On: courses come from TGRCode's archive of SMM2 courses, through a course server built into the game "
         "(no account needed). Off: the course server set in smm2mods.ini (server=, token=). Takes effect at the "
         "next start.",
         &t_tgrcode, false},
    };
    if (!mod_hundred()) { *out = NULL; return 0; }  // (the mod itself is off: nothing to switch)
    t_hundred = !strcmp(g_mod.ruleset, "hundred");
    t_tgrcode = !strcmp(g_mod.courses, "tgrcode");
    *out = toggles;
    return (int)(sizeof toggles / sizeof *toggles);
}

void game_mod_toggles_changed(void) {
    snprintf(g_mod.ruleset, sizeof g_mod.ruleset, "%s", t_hundred ? "hundred" : "smm2");
    ini_set("ruleset", g_mod.ruleset);
    LOG("[mod] 100 Mario Challenge (SMM1) %s\n", t_hundred ? "on" : "off");
    const char *courses = t_tgrcode ? "tgrcode" : "server";
    if (strcmp(courses, g_mod.courses)) {  // (the server in use stays until the next start)
        ini_set("courses", courses);
        LOG("[mod] courses from %s at the next start\n", t_tgrcode ? "TGRCode" : "your server");
    }
}

// Overrides are registered before the game's first instruction runs (dispatch reads them on every first call). Each
// one checks the settings when it is called (they are read then, from the data folder) and runs the game's own
// function when its mod is off.
__attribute__((constructor)) static void mods_start(void) {
    extern void world_register(void);
    world_register();  // (first: the first override of an address wins, before the trace wrappers)
    online_register();
    endless_register();
    extern void bridge_register(void);
    bridge_register();
}
