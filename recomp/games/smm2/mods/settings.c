// Mod settings (games/smm2/mods/mod.h) and start-up: environment variables win over smm2mods.ini (key=value lines
// in the data folder, next to larecomp.ini). Nothing is on unless asked for.
//
//   RECOMP_MOD_100MARIO=1        hundred=1          100 Mario Challenge on SMM2's Endless Challenge
//   RECOMP_MOD_SERVER=<url>      server=<url>       our course server, plain http (default http://127.0.0.1:8080)
//   RECOMP_MOD_TOKEN=<token>     token=<token>      the account's token (`mmk register NAME --server URL`)
//   RECOMP_MOD_RULESET=<name>    ruleset=<name>     hundred (SMM1's 100 lives, 8/16/16/6 clears; default) or smm2
//   RECOMP_MOD_TIMEOUT_MS=<n>    timeout_ms=<n>     per request (default 8000)
//   RECOMP_MOD_LOG=1             log=1              log requests and overrides
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "mod.h"
#include "../../../runtime/net/online.h"

ModSettings g_mod;

static void set(const char *key, const char *val) {
    if (!strcmp(key, "hundred")) g_mod.hundred = atoi(val) != 0;
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
    };
    for (size_t i = 0; i < sizeof envs / sizeof *envs; i++)
        if (getenv(envs[i].env)) set(envs[i].key, getenv(envs[i].env));
    size_t n = strlen(g_mod.server);
    while (n && g_mod.server[n - 1] == '/') g_mod.server[--n] = 0;
    if (strcmp(g_mod.ruleset, "smm2") && strcmp(g_mod.ruleset, "hundred")) snprintf(g_mod.ruleset, sizeof g_mod.ruleset, "hundred");
    if (g_mod.timeout_ms <= 0) g_mod.timeout_ms = 8000;
    if (g_mod.hundred)
        LOG("[mod] 100 Mario Challenge on: server %s, ruleset %s, %s\n", g_mod.server, g_mod.ruleset,
            g_mod.token[0] ? "signed in" : "NO TOKEN (set RECOMP_MOD_TOKEN)");
}

bool mod_hundred(void) { mod_settings_load(); return g_mod.hundred; }
bool mod_hundred_rules(void) { return mod_hundred() && !strcmp(g_mod.ruleset, "hundred"); }

// The system services report a connection while a mod answers the online calls (runtime/net/online.h).
bool net_pretend_online(void) { return mod_hundred() || (getenv("RECOMP_PRETEND_ONLINE") && atoi(getenv("RECOMP_PRETEND_ONLINE"))); }

// Overrides are registered before the game's first instruction runs (dispatch reads them on every first call). Each
// one checks the settings when it is called (they are read then, from the data folder) and runs the game's own
// function when its mod is off.
__attribute__((constructor)) static void mods_start(void) {
    online_register();
    endless_register();
}
