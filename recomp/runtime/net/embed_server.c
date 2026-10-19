// The course server inside the app (runtime/net/embed_server.h).
#include "embed_server.h"

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#if defined(__APPLE__)
#include <TargetConditionals.h>
#endif

#include "../rt.h"

#ifdef RECOMP_EMBED_SERVER
// core/crates/embed/src/lib.rs
extern uint16_t mmk_embed_start(const char *dir, const char *upstream, const uint8_t *image, size_t image_len,
                                const char *name, char *token_out, size_t token_cap);
#endif

bool embed_server_start(const char *data_dir, const char *upstream, const char *name, char *url_out, size_t url_len,
                        char *token_out, size_t token_len) {
#ifdef RECOMP_EMBED_SERVER
    // the game's executable as loaded, read-only data up to the end of its data: where the course key table is
    const Module *m = &g_modules[0];
    const uint8_t *image = NULL;
    size_t image_len = 0;
    if (g_nmodules > 0 && m->data_off + m->data_size > m->ro_off) {
        image = (const uint8_t *)(uintptr_t)(m->base + m->ro_off);
        image_len = (size_t)(m->data_off + m->data_size - m->ro_off);
    }
    uint16_t port = mmk_embed_start(data_dir, upstream, image, image_len, name, token_out, token_len);
    if (!port) return false;
    snprintf(url_out, url_len, "http://127.0.0.1:%u", port);
    return true;
#else
    (void)data_dir, (void)upstream, (void)name, (void)url_out, (void)url_len, (void)token_out, (void)token_len;
    return false;
#endif
}

// a server= line in smm2mods.ini (the mods' own settings file): the player chose a server
static bool ini_names_server(void) {
    FILE *f = fopen(getenv("RECOMP_MOD_INI") ? getenv("RECOMP_MOD_INI") : "smm2mods.ini", "r");
    if (!f) return false;
    char line[512];
    bool found = false;
    while (!found && fgets(line, sizeof line, f)) {
        const char *p = line;
        while (*p == ' ' || *p == '\t') p++;
        found = !strncmp(p, "server", 6) && strchr(p, '=');
    }
    fclose(f);
    return found;
}

void embed_server_boot(void) {
#ifdef RECOMP_EMBED_SERVER
    const char *mode = getenv("RECOMP_EMBED_SERVER");
#if defined(TARGET_OS_IPHONE) && TARGET_OS_IPHONE
    if (!mode || !*mode) mode = "tgrcode";
#endif
    if (!mode || !*mode || !strcmp(mode, "off") || !strcmp(mode, "0")) return;
    if (getenv("RECOMP_MOD_SERVER") || ini_names_server()) {
        LOG("[embed-server] not started: the mods have a server of their own\n");
        return;
    }
    const char *name = getenv("RECOMP_USER_NAME") ? getenv("RECOMP_USER_NAME") : "Player";
    char url[64], token[128];
    if (!embed_server_start("server", strcmp(mode, "local") ? "tgrcode" : NULL, name, url, sizeof url, token,
                            sizeof token)) {
        LOG("[embed-server] didn't start (details above)\n");
        return;
    }
    setenv("RECOMP_MOD_SERVER", url, 1);
    setenv("RECOMP_MOD_TOKEN", token, 1);
    setenv("RECOMP_MOD_100MARIO", "1", 0);
    LOG("[embed-server] the mods use %s (%s)\n", url, strcmp(mode, "local") ? "courses from tgrcode.com/mm2" : "local courses");
#endif
}
