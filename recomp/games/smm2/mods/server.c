// Calls to our course server (core/crates/server) with the account's token, plus the shared game-state helpers.
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "mod.h"
#include "../../../runtime/net/http.h"
#include "../../../runtime/net/json.h"

static __thread int slow_ms;  // (srv_call_slow: this thread's request may take this long)

static int request_typed(const char *method, const char *path, const char *body, size_t blen, const char *type,
                         HttpResponse *r) {
    char url[512], hdr[256];
    mod_server_ready();  // (courses=tgrcode: the built-in server starts before the first request)
    snprintf(url, sizeof url, "%s%s", g_mod.server, path);
    hdr[0] = 0;
    if (g_mod.token[0]) snprintf(hdr, sizeof hdr, "Authorization: Bearer %s\r\n", g_mod.token);
    if (body) {
        size_t n = strlen(hdr);
        snprintf(hdr + n, sizeof hdr - n, "Content-Type: %s\r\n", type);
    }
    int st = http_request(method, url, hdr, body, blen, slow_ms ? slow_ms : g_mod.timeout_ms, r);
    bool text = body && !strcmp(type, "application/json");
    if (!st) LOG("[mod] %s %s: %s\n", method, path, r->error);
    else MLOG("%s %s -> %d (%zu bytes)%s%.300s\n", method, path, st, r->len, text ? " body " : "", text ? body : "");
    return st;
}

static int request(const char *method, const char *path, const char *body, size_t blen, HttpResponse *r) {
    return request_typed(method, path, body, blen, "application/json", r);
}

Json *srv_post_bytes(const char *path, const void *data, size_t len, int *status) {
    HttpResponse r;
    int st = request_typed("POST", path, data, len, "application/octet-stream", &r);
    if (status) *status = st;
    Json *j = st >= 200 && st < 300 ? json_parse(r.body, r.len) : NULL;
    if (st && !j) LOG("[mod] POST %s: HTTP %d %.200s\n", path, st, r.body ? r.body : "");
    http_free(&r);
    return j;
}

Json *srv_call(const char *method, const char *path, const char *body, int *status) {
    HttpResponse r;
    int st = request(method, path, body, body ? strlen(body) : 0, &r);
    if (status) *status = st;
    Json *j = NULL;
    if (st >= 200 && st < 300) {
        j = json_parse(r.body, r.len);
        if (!j) LOG("[mod] %s %s: answer is not JSON\n", method, path);
    } else if (st) {
        LOG("[mod] %s %s: HTTP %d %.200s\n", method, path, st, r.body ? r.body : "");
    }
    if (j && g_mod.log) MLOG("  answer %.600s\n", r.body ? r.body : "");
    http_free(&r);
    return j;
}

// A request the server may take long over (TGRCode: the first courses of a difficulty, about a minute).
Json *srv_call_slow(const char *method, const char *path, const char *body, int *status, int timeout_ms) {
    slow_ms = timeout_ms;
    Json *j = srv_call(method, path, body, status);
    slow_ms = 0;
    return j;
}

void *srv_get_bytes(const char *path, size_t *len) {
    HttpResponse r;
    int st = request("GET", path, NULL, 0, &r);
    if (st != 200) {
        if (st) LOG("[mod] GET %s: HTTP %d\n", path, st);
        http_free(&r);
        return NULL;
    }
    *len = r.len;
    return r.body;  // (the caller frees it)
}

// ---- game state

#define NETOBJ_HOLDER MAIN_ADDR(0x2a37380) // + 0x3e0: the network-session object
#define WRAPPER_TRUE MAIN_ADDR(0x2a17ad8) // the bytes the request functions return (success / failure)
#define WRAPPER_FALSE MAIN_ADDR(0x2a17ad9)
#define LOCK_FN MAIN_ADDR(0x292410)    // the lock / unlock the wrappers take on NetObj + 0x20
#define UNLOCK_FN MAIN_ADDR(0x292430)

uint64_t netobj(void) { return g64(g64(NETOBJ_HOLDER) + 0x3e0); }
uint8_t wrapper_ok(void) { return g8(WRAPPER_TRUE); }
uint8_t wrapper_fail(void) { return g8(WRAPPER_FALSE); }

void guest_call_orig(Cpu *c, uint64_t addr) {
    GuestFn f = dispatch_lookup(addr);
    if (!f) FATAL("[mod] no recompiled function at %llx\n", (unsigned long long)addr);
    uint64_t lr = c->x[30];
    f(c);
    c->x[30] = lr;
}

void netobj_lock(Cpu *c) {
    uint64_t save0 = c->x[0];
    c->x[0] = netobj() + 0x20;
    guest_call(c, LOCK_FN);
    c->x[0] = save0;
}

void netobj_unlock(Cpu *c) {
    uint64_t save0 = c->x[0];
    c->x[0] = netobj() + 0x20;
    guest_call(c, UNLOCK_FN);
    c->x[0] = save0;
}
