// A small blocking HTTP/1.1 client for mods that talk to a self-hosted server (plain http:// on a LAN or localhost; no
// TLS). Portable: BSD sockets on macOS / Linux / iOS, Winsock on Windows. Each request opens its own connection
// ("Connection: close"), so it is safe from any thread; calls block for at most the timeout given.
#pragma once
#include <stddef.h>

typedef struct {
    int status;   // HTTP status (200, 404, ...); 0 when no answer came (connect / send / read failed or timed out)
    char *body;   // the response body, NUL-terminated (malloc'd; NULL when empty or on failure)
    size_t len;   // body length in bytes (without the terminator)
    char error[96];  // why status is 0
} HttpResponse;

// method "GET" / "POST" / ...; url "http://host[:port]/path?query"; headers: extra header lines, each ending in
// "\r\n" (or NULL); body / body_len: the request body (NULL / 0 for none); timeout_ms bounds connect, each send and
// each read. Returns the status (0 on failure). Free the response with http_free.
int http_request(const char *method, const char *url, const char *headers, const void *body, size_t body_len,
                 int timeout_ms, HttpResponse *out);
void http_free(HttpResponse *r);
