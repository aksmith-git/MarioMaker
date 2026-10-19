// Blocking HTTP/1.1 client (runtime/net/http.h): one connection per request, timeouts on connect / send / receive,
// Content-Length, chunked or read-to-close bodies.
#include "http.h"

#include <errno.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#ifdef _WIN32
#include <winsock2.h>
#include <ws2tcpip.h>
typedef SOCKET sock_t;
#define SOCK_BAD INVALID_SOCKET
#define sock_close closesocket
static void net_init(void) {
    static int done;
    if (!done) { WSADATA w; WSAStartup(MAKEWORD(2, 2), &w); done = 1; }
}
static int sock_err_inprogress(void) { return WSAGetLastError() == WSAEWOULDBLOCK; }
static void sock_nonblock(sock_t s, int on) { u_long v = (u_long)on; ioctlsocket(s, FIONBIO, &v); }
static void sock_timeouts(sock_t s, int ms) {
    DWORD t = (DWORD)ms;
    setsockopt(s, SOL_SOCKET, SO_RCVTIMEO, (const char *)&t, sizeof t);
    setsockopt(s, SOL_SOCKET, SO_SNDTIMEO, (const char *)&t, sizeof t);
}
#else
#include <fcntl.h>
#include <netdb.h>
#include <netinet/in.h>
#include <netinet/tcp.h>
#include <sys/select.h>
#include <sys/socket.h>
#include <sys/time.h>
#include <unistd.h>
typedef int sock_t;
#define SOCK_BAD (-1)
#define sock_close close
static void net_init(void) {}
static int sock_err_inprogress(void) { return errno == EINPROGRESS; }
static void sock_nonblock(sock_t s, int on) {
    int f = fcntl(s, F_GETFL, 0);
    fcntl(s, F_SETFL, on ? f | O_NONBLOCK : f & ~O_NONBLOCK);
}
static void sock_timeouts(sock_t s, int ms) {
    struct timeval tv = {ms / 1000, (ms % 1000) * 1000};
    setsockopt(s, SOL_SOCKET, SO_RCVTIMEO, &tv, sizeof tv);
    setsockopt(s, SOL_SOCKET, SO_SNDTIMEO, &tv, sizeof tv);
#ifdef SO_NOSIGPIPE
    int one = 1;
    setsockopt(s, SOL_SOCKET, SO_NOSIGPIPE, &one, sizeof one);
#endif
}
#endif

#ifndef MSG_NOSIGNAL
#define MSG_NOSIGNAL 0
#endif

static int fail(HttpResponse *out, const char *fmt, ...) {
    va_list ap;
    va_start(ap, fmt);
    vsnprintf(out->error, sizeof out->error, fmt, ap);
    va_end(ap);
    out->status = 0;
    return 0;
}

// "http://host[:port]/path" -> host, port, path
static int parse_url(const char *url, char *host, size_t hcap, char *port, size_t pcap, const char **path) {
    if (strncmp(url, "http://", 7)) return 0;
    const char *h = url + 7, *end = h + strcspn(h, ":/?");
    if (end == h || (size_t)(end - h) >= hcap) return 0;
    memcpy(host, h, (size_t)(end - h));
    host[end - h] = 0;
    snprintf(port, pcap, "80");
    if (*end == ':') {
        const char *p = end + 1, *pe = p + strspn(p, "0123456789");
        if (pe == p || (size_t)(pe - p) >= pcap) return 0;
        memcpy(port, p, (size_t)(pe - p));
        port[pe - p] = 0;
        end = pe;
    }
    *path = *end ? end : "/";
    return 1;
}

static sock_t connect_to(const char *host, const char *port, int timeout_ms, HttpResponse *out) {
    struct addrinfo hints, *res = NULL;
    memset(&hints, 0, sizeof hints);
    hints.ai_family = AF_UNSPEC;
    hints.ai_socktype = SOCK_STREAM;
    if (getaddrinfo(host, port, &hints, &res) || !res) { fail(out, "cannot resolve %s", host); return SOCK_BAD; }
    sock_t s = SOCK_BAD;
    for (struct addrinfo *a = res; a; a = a->ai_next) {
        s = socket(a->ai_family, a->ai_socktype, a->ai_protocol);
        if (s == SOCK_BAD) continue;
        sock_nonblock(s, 1);
        int r = connect(s, a->ai_addr, (int)a->ai_addrlen);
        if (r != 0 && sock_err_inprogress()) {
            fd_set w;
            FD_ZERO(&w);
            FD_SET(s, &w);
            struct timeval tv = {timeout_ms / 1000, (timeout_ms % 1000) * 1000};
            r = select((int)s + 1, NULL, &w, NULL, &tv) == 1 ? 0 : -1;
            if (r == 0) {
                int err = 0;
                socklen_t len = sizeof err;
                getsockopt(s, SOL_SOCKET, SO_ERROR, (char *)&err, &len);
                if (err) r = -1;
            }
        }
        if (r == 0) {
            sock_nonblock(s, 0);
            sock_timeouts(s, timeout_ms);
            int one = 1;
            setsockopt(s, IPPROTO_TCP, TCP_NODELAY, (const char *)&one, sizeof one);
            break;
        }
        sock_close(s);
        s = SOCK_BAD;
    }
    freeaddrinfo(res);
    if (s == SOCK_BAD) fail(out, "cannot connect to %s:%s", host, port);
    return s;
}

static int send_all(sock_t s, const char *p, size_t n) {
    while (n) {
        long k = send(s, p, (int)(n > 1 << 20 ? 1 << 20 : n), MSG_NOSIGNAL);
        if (k <= 0) return 0;
        p += k;
        n -= (size_t)k;
    }
    return 1;
}

typedef struct { char *p; size_t n, cap; } Buf;
static int buf_add(Buf *b, const char *p, size_t n) {
    if (b->n + n + 1 > b->cap) {
        size_t cap = b->cap ? b->cap * 2 : 8192;
        while (cap < b->n + n + 1) cap *= 2;
        char *q = realloc(b->p, cap);
        if (!q) return 0;
        b->p = q, b->cap = cap;
    }
    memcpy(b->p + b->n, p, n);
    b->n += n;
    b->p[b->n] = 0;
    return 1;
}

static const char *find_header(const char *head, const char *name) {  // case-insensitive, at a line start
    size_t ln = strlen(name);
    for (const char *l = strstr(head, "\r\n"); l; l = strstr(l + 2, "\r\n")) {
        const char *h = l + 2;
        int ok = 1;
        for (size_t i = 0; i < ln && ok; i++) {
            char a = h[i], b = name[i];
            if (a >= 'A' && a <= 'Z') a = (char)(a + 32);
            if (b >= 'A' && b <= 'Z') b = (char)(b + 32);
            ok = a == b;
        }
        if (ok && h[ln] == ':') {
            h += ln + 1;
            while (*h == ' ') h++;
            return h;
        }
        if (h[0] == '\r') break;
    }
    return NULL;
}

// Decode a chunked body in place; returns the decoded length, or -1 if incomplete / malformed.
static long dechunk(char *p, size_t n) {
    size_t r = 0, w = 0;
    for (;;) {
        char *e;
        if (r >= n) return -1;
        unsigned long sz = strtoul(p + r, &e, 16);
        char *nl = strstr(p + r, "\r\n");
        if (!nl || e == p + r) return -1;
        r = (size_t)(nl - p) + 2;
        if (sz == 0) return (long)w;
        if (r + sz + 2 > n) return -1;
        memmove(p + w, p + r, sz);
        w += sz;
        r += sz + 2;
    }
}

int http_request(const char *method, const char *url, const char *headers, const void *body, size_t body_len,
                 int timeout_ms, HttpResponse *out) {
    memset(out, 0, sizeof *out);
    net_init();
    char host[256], port[8];
    const char *path;
    if (!parse_url(url, host, sizeof host, port, sizeof port, &path)) return fail(out, "bad url %s", url);
    sock_t s = connect_to(host, port, timeout_ms, out);
    if (s == SOCK_BAD) return 0;
    Buf req = {0};
    char line[1024];
    snprintf(line, sizeof line,
             "%s %s HTTP/1.1\r\nHost: %s:%s\r\nConnection: close\r\nUser-Agent: smm2recomp\r\nContent-Length: %zu\r\n",
             method, path, host, port, body_len);
    buf_add(&req, line, strlen(line));
    if (headers) buf_add(&req, headers, strlen(headers));
    buf_add(&req, "\r\n", 2);
    if (body_len) buf_add(&req, body, body_len);
    int sent = req.p && send_all(s, req.p, req.n);
    free(req.p);
    if (!sent) { sock_close(s); return fail(out, "send failed"); }

    Buf resp = {0};
    char chunk[16384];
    long k;
    size_t head_end = 0;
    long want = -1;  // total bytes once the header and the Content-Length are known
    while ((k = recv(s, chunk, sizeof chunk, 0)) > 0) {
        if (!buf_add(&resp, chunk, (size_t)k)) break;
        if (!head_end) {
            char *e = strstr(resp.p, "\r\n\r\n");
            if (e) {
                head_end = (size_t)(e - resp.p) + 4;
                const char *cl = find_header(resp.p, "Content-Length");
                if (cl) want = (long)head_end + atol(cl);
            }
        }
        if (want >= 0 && (long)resp.n >= want) break;
    }
    sock_close(s);
    if (!head_end) { free(resp.p); return fail(out, k < 0 ? "read timed out" : "no answer"); }
    int major, minor, status;
    if (sscanf(resp.p, "HTTP/%d.%d %d", &major, &minor, &status) != 3) { free(resp.p); return fail(out, "bad answer"); }
    char *b = resp.p + head_end;
    size_t blen = resp.n - head_end;
    const char *te = find_header(resp.p, "Transfer-Encoding");
    if (te && !strncmp(te, "chunked", 7)) {
        long d = dechunk(b, blen);
        if (d < 0) { free(resp.p); return fail(out, "truncated chunked body"); }
        blen = (size_t)d;
    } else if (want >= 0 && (long)resp.n < want) {
        free(resp.p);
        return fail(out, "truncated body");
    }
    if (blen) {
        out->body = malloc(blen + 1);
        if (out->body) {
            memcpy(out->body, b, blen);
            out->body[blen] = 0;
            out->len = blen;
        }
    }
    free(resp.p);
    out->status = status;
    return status;
}

void http_free(HttpResponse *r) {
    free(r->body);
    r->body = NULL;
    r->len = 0;
}
