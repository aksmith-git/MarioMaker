// bsd:u / bsd:s (the system's BSD sockets) for a game's local multiplayer: UDP sockets on the virtual LAN
// (runtime/net/vlan.h) and nothing else. Without a virtual LAN every socket call fails with ENETDOWN, as on a console
// without a network; there is no way from here to any real host (never Nintendo's servers): datagrams only go to the
// relay room. TCP sockets are refused.
//
// Commands (IClient): 0 RegisterClient, 1 StartMonitoring, 2 Socket, 3 SocketExempt, 5 Select, 6 Poll, 7 Sysctl,
// 8 Recv, 9 RecvFrom, 10 Send, 11 SendTo, 13 Bind, 14 Connect, 15 GetPeerName, 16 GetSockName, 17 GetSockOpt,
// 19 Ioctl, 20 Fcntl, 21 SetSockOpt, 22 Shutdown, 24 Write, 25 Read, 26 Close. Results: {i32 ret, u32 errno} (+ a
// length), errno numbers as FreeBSD's. Addresses: struct sockaddr_in as BSD lays it out {u8 len, u8 family 2, u16 port
// BE, u32 addr BE, 8 zero}.
#include <pthread.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

#include "../hle.h"
#include "../ipc.h"
#include "../net/vlan.h"

enum { E_OK = 0, E_BADF = 9, E_AGAIN = 35, E_NOTSOCK = 38, E_PROTONOSUPPORT = 43, E_AFNOSUPPORT = 47,
       E_NETDOWN = 50, E_INVAL = 22, E_MSGSIZE = 40, E_NOTCONN = 57, E_DESTADDRREQ = 39 };

#define MAX_SOCKS 64
#define QUEUE 128

typedef struct { uint32_t ip; uint16_t port; uint16_t len; uint8_t data[VLAN_MAX_PAYLOAD]; } Dgram;
typedef struct {
    bool used, nonblock, connected;
    uint16_t port;            // bound port (host order), 0 none
    uint32_t peer_ip;         // connect(): default destination
    uint16_t peer_port;
    Dgram *q;                 // ring of received datagrams
    int head, count;
} Sock;

static Sock socks[MAX_SOCKS];
static pthread_mutex_t lock = PTHREAD_MUTEX_INITIALIZER;
static pthread_cond_t arrived = PTHREAD_COND_INITIALIZER;
static uint16_t next_port = 49152;

static void deliver(uint32_t sip, uint16_t sport, uint16_t dport, const uint8_t *data, size_t len) {
    pthread_mutex_lock(&lock);
    for (int i = 0; i < MAX_SOCKS; i++) {
        Sock *s = &socks[i];
        if (!s->used || s->port != dport || s->count == QUEUE) continue;
        Dgram *d = &s->q[(s->head + s->count++) % QUEUE];
        d->ip = sip, d->port = sport, d->len = (uint16_t)len;
        memcpy(d->data, data, len);
    }
    pthread_cond_broadcast(&arrived);
    pthread_mutex_unlock(&lock);
}

static void put_addr(IpcBuf *b, uint32_t ip, uint16_t port, uint32_t *alen) {
    uint8_t a[16] = {16, 2, (uint8_t)(port >> 8), (uint8_t)port, (uint8_t)(ip >> 24), (uint8_t)(ip >> 16),
                     (uint8_t)(ip >> 8), (uint8_t)ip};
    if (b && b->size) memcpy((void *)(uintptr_t)b->addr, a, b->size < 16 ? b->size : 16);
    if (alen) *alen = 16;
}

static bool get_addr(IpcBuf *b, uint32_t *ip, uint16_t *port) {
    if (!b || b->size < 8) return false;
    const uint8_t *a = (const uint8_t *)(uintptr_t)b->addr;
    *port = (uint16_t)(a[2] << 8 | a[3]);
    *ip = (uint32_t)a[4] << 24 | (uint32_t)a[5] << 16 | (uint32_t)a[6] << 8 | a[7];
    return true;
}

static Sock *sock_of(int fd) { return fd >= 0 && fd < MAX_SOCKS && socks[fd].used ? &socks[fd] : NULL; }

static void result(IpcResp *resp, int32_t ret, uint32_t err) {
    OUT(int32_t, 0) = ret, OUT(uint32_t, 4) = err;
    resp_size(resp, 8);
}

static void autobind(Sock *s) {
    if (!s->port) s->port = next_port++;
}

// wait (lock held) until s has data or the deadline (ms; <0 forever) passes
static bool wait_data(Sock *s, int ms) {
    if (s->count) return true;
    if (ms == 0) return false;
    struct timespec ts;
    clock_gettime(CLOCK_REALTIME, &ts);
    long long end = (long long)ts.tv_sec * 1000 + ts.tv_nsec / 1000000 + (ms < 0 ? 3600000 : ms);
    while (!s->count) {
        struct timespec t = {(time_t)(end / 1000), (long)(end % 1000) * 1000000};
        if (pthread_cond_timedwait(&arrived, &lock, &t)) break;
    }
    return s->count > 0;
}

static int recv_one(Sock *s, IpcBuf *data, IpcBuf *addr, uint32_t *alen, uint32_t *err) {
    if (!wait_data(s, s->nonblock ? 0 : -1)) { *err = E_AGAIN; return -1; }
    Dgram *d = &s->q[s->head];
    size_t n = data && data->size < d->len ? data->size : d->len;
    if (data && n) memcpy((void *)(uintptr_t)data->addr, d->data, n);
    put_addr(addr, d->ip, d->port, alen);
    s->head = (s->head + 1) % QUEUE, s->count--;
    return (int)n;
}

static void client(Service *svc, IpcReq *req, IpcResp *resp) {
    (void)svc;
    bool lan = vlan_active();
    static bool hooked;
    if (lan && !hooked) hooked = true, vlan_set_receiver(deliver);
    int32_t fd = req->data_size >= 4 ? IN(int32_t, 0) : -1;
    switch (req->cmd) {
    case 0:  // RegisterClient -> u64 0
        OUT(uint64_t, 0) = 0;
        resp_size(resp, 8);
        return;
    case 1: return;  // StartMonitoring
    case 2: case 3: {  // Socket(domain, type, protocol)
        int32_t domain = IN(int32_t, 0), type = IN(int32_t, 4) & 0xffff;
        if (!lan) { result(resp, -1, E_NETDOWN); return; }
        if (domain != 2) { result(resp, -1, E_AFNOSUPPORT); return; }
        if (type != 2) { LOG("[bsd] socket type %d refused (UDP only)\n", type); result(resp, -1, E_PROTONOSUPPORT); return; }
        pthread_mutex_lock(&lock);
        int i = 0;
        while (i < MAX_SOCKS && socks[i].used) i++;
        if (i < MAX_SOCKS) {
            memset(&socks[i], 0, sizeof socks[i]);
            socks[i].used = true;
            socks[i].q = calloc(QUEUE, sizeof(Dgram));
        }
        pthread_mutex_unlock(&lock);
        result(resp, i < MAX_SOCKS ? i : -1, i < MAX_SOCKS ? 0 : 24);
        return;
    }
    case 7: result(resp, 0, 0); return;  // Sysctl
    case 13: case 14: {  // Bind / Connect(fd) + address
        pthread_mutex_lock(&lock);
        Sock *s = sock_of(fd);
        uint32_t ip;
        uint16_t port;
        if (!s) { pthread_mutex_unlock(&lock); result(resp, -1, E_BADF); return; }
        if (!get_addr(in_buf(req, 0), &ip, &port)) { pthread_mutex_unlock(&lock); result(resp, -1, E_INVAL); return; }
        if (req->cmd == 13) s->port = port ? port : next_port++;
        else s->peer_ip = ip, s->peer_port = port, s->connected = true, autobind(s);
        pthread_mutex_unlock(&lock);
        LOG("[bsd] %s %d: %u.%u.%u.%u:%u\n", req->cmd == 13 ? "bind" : "connect", fd, ip >> 24, ip >> 16 & 255,
            ip >> 8 & 255, ip & 255, port);
        result(resp, 0, 0);
        return;
    }
    case 15: case 16: {  // GetPeerName / GetSockName(fd) -> address
        pthread_mutex_lock(&lock);
        Sock *s = sock_of(fd);
        uint32_t alen = 0;
        if (s) {
            if (req->cmd == 16) put_addr(out_buf(req, 0), vlan_ip(), s->port, &alen);
            else if (s->connected) put_addr(out_buf(req, 0), s->peer_ip, s->peer_port, &alen);
        }
        pthread_mutex_unlock(&lock);
        result(resp, s && (req->cmd == 16 || s->connected) ? 0 : -1, !s ? E_BADF : req->cmd == 15 && !s->connected ? E_NOTCONN : 0);
        OUT(uint32_t, 8) = alen;
        resp_size(resp, 12);
        return;
    }
    case 8: case 9: case 25: {  // Recv / RecvFrom / Read(fd[, flags]) -> data [, address]
        uint32_t flags = req->cmd == 25 ? 0 : IN(uint32_t, 4), err = 0, alen = 0;
        pthread_mutex_lock(&lock);
        Sock *s = sock_of(fd);
        int n = -1;
        if (!s) err = E_BADF;
        else {
            bool nb = s->nonblock;
            if (flags & 0x80) s->nonblock = true;  // MSG_DONTWAIT
            n = recv_one(s, out_buf(req, 0), req->cmd == 9 ? out_buf(req, 1) : NULL, &alen, &err);
            s->nonblock = nb;
        }
        pthread_mutex_unlock(&lock);
        result(resp, n, n < 0 ? err : 0);
        OUT(uint32_t, 8) = alen;
        resp_size(resp, 12);
        return;
    }
    case 10: case 11: case 24: {  // Send / SendTo / Write(fd[, flags]) + data [+ address]
        pthread_mutex_lock(&lock);
        Sock *s = sock_of(fd);
        uint32_t ip = 0;
        uint16_t port = 0, sport = 0;
        bool have = false;
        if (s) {
            autobind(s);
            sport = s->port;
            if (req->cmd == 11) have = get_addr(in_buf(req, 1), &ip, &port);
            else if (s->connected) ip = s->peer_ip, port = s->peer_port, have = true;
        }
        pthread_mutex_unlock(&lock);
        IpcBuf *d = in_buf(req, 0);
        if (!s) { result(resp, -1, E_BADF); return; }
        if (!have) { result(resp, -1, E_DESTADDRREQ); return; }
        if (d && d->size > VLAN_MAX_PAYLOAD) { result(resp, -1, E_MSGSIZE); return; }
        int n = vlan_send(sport, ip, port, d ? (const void *)(uintptr_t)d->addr : "", d ? d->size : 0);
        result(resp, n, n < 0 ? E_NETDOWN : 0);
        return;
    }
    case 6: {  // Poll(nfds, timeout ms) + pollfd array in -> out
        uint32_t nfds = IN(uint32_t, 0);
        int32_t timeout = IN(int32_t, 4);
        IpcBuf *in = in_buf(req, 0), *out = out_buf(req, 0);
        if (!in || in->size < nfds * 8u) { result(resp, -1, E_INVAL); return; }
        uint8_t *fds = malloc(nfds * 8u + 8);
        memcpy(fds, (void *)(uintptr_t)in->addr, nfds * 8u);
        struct timespec t0;
        clock_gettime(CLOCK_MONOTONIC, &t0);
        int ready = 0;
        pthread_mutex_lock(&lock);
        for (;;) {
            ready = 0;
            for (uint32_t i = 0; i < nfds; i++) {
                int32_t f = *(int32_t *)(fds + 8 * i);
                int16_t ev = *(int16_t *)(fds + 8 * i + 4), rev = 0;
                Sock *s = sock_of(f);
                if (!s) rev = 0x20;  // POLLNVAL
                else {
                    if ((ev & (1 | 0x40)) && s->count) rev |= ev & (1 | 0x40);  // POLLIN / POLLRDNORM
                    if (ev & (4 | 0x100)) rev |= ev & (4 | 0x100);           // POLLOUT / POLLWRNORM: always
                }
                *(int16_t *)(fds + 8 * i + 6) = rev;
                if (rev) ready++;
            }
            if (ready || timeout == 0) break;
            struct timespec now;
            clock_gettime(CLOCK_MONOTONIC, &now);
            long el = (now.tv_sec - t0.tv_sec) * 1000 + (now.tv_nsec - t0.tv_nsec) / 1000000;
            if (timeout > 0 && el >= timeout) break;
            struct timespec rt;
            clock_gettime(CLOCK_REALTIME, &rt);
            long wait = timeout < 0 ? 100 : timeout - el < 100 ? timeout - el : 100;
            long long end = (long long)rt.tv_sec * 1000 + rt.tv_nsec / 1000000 + wait;
            struct timespec t = {(time_t)(end / 1000), (long)(end % 1000) * 1000000};
            pthread_cond_timedwait(&arrived, &lock, &t);
        }
        pthread_mutex_unlock(&lock);
        if (out && out->size >= nfds * 8u) memcpy((void *)(uintptr_t)out->addr, fds, nfds * 8u);
        free(fds);
        result(resp, ready, 0);
        return;
    }
    case 5: {  // Select(nfds, timeval, null-timeout) + read / write / except sets in -> out
        int32_t nfds = IN(int32_t, 0);
        uint64_t sec = IN(uint64_t, 8), usec = IN(uint64_t, 16);
        bool forever = req->data_size >= 25 && IN(uint8_t, 24);
        long ms = forever ? -1 : (long)(sec * 1000 + usec / 1000);
        IpcBuf *rin = in_buf(req, 0), *win = in_buf(req, 1), *rout = out_buf(req, 0), *wout = out_buf(req, 1),
               *eout = out_buf(req, 2);
        uint8_t rset[128] = {0}, wset[128] = {0}, ro[128], wo[128];
        if (rin && rin->size) memcpy(rset, (void *)(uintptr_t)rin->addr, rin->size < 128 ? rin->size : 128);
        if (win && win->size) memcpy(wset, (void *)(uintptr_t)win->addr, win->size < 128 ? win->size : 128);
        struct timespec t0;
        clock_gettime(CLOCK_MONOTONIC, &t0);
        int ready = 0;
        pthread_mutex_lock(&lock);
        for (;;) {
            memset(ro, 0, sizeof ro), memset(wo, 0, sizeof wo);
            ready = 0;
            for (int f = 0; f < nfds && f < MAX_SOCKS; f++) {
                Sock *s = sock_of(f);
                if (!s) continue;
                if ((rset[f >> 3] >> (f & 7) & 1) && s->count) ro[f >> 3] |= (uint8_t)(1 << (f & 7)), ready++;
                if (wset[f >> 3] >> (f & 7) & 1) wo[f >> 3] |= (uint8_t)(1 << (f & 7)), ready++;
            }
            if (ready || ms == 0) break;
            struct timespec now;
            clock_gettime(CLOCK_MONOTONIC, &now);
            long el = (now.tv_sec - t0.tv_sec) * 1000 + (now.tv_nsec - t0.tv_nsec) / 1000000;
            if (ms > 0 && el >= ms) break;
            struct timespec rt;
            clock_gettime(CLOCK_REALTIME, &rt);
            long wait = ms < 0 ? 100 : ms - el < 100 ? ms - el : 100;
            long long end = (long long)rt.tv_sec * 1000 + rt.tv_nsec / 1000000 + wait;
            struct timespec t = {(time_t)(end / 1000), (long)(end % 1000) * 1000000};
            pthread_cond_timedwait(&arrived, &lock, &t);
        }
        pthread_mutex_unlock(&lock);
        if (rout && rout->size) memcpy((void *)(uintptr_t)rout->addr, ro, rout->size < 128 ? rout->size : 128);
        if (wout && wout->size) memcpy((void *)(uintptr_t)wout->addr, wo, wout->size < 128 ? wout->size : 128);
        if (eout && eout->size) memset((void *)(uintptr_t)eout->addr, 0, eout->size);
        result(resp, ready, 0);
        return;
    }
    case 17: {  // GetSockOpt -> 0
        IpcBuf *b = out_buf(req, 0);
        if (b && b->size) memset((void *)(uintptr_t)b->addr, 0, b->size);
        result(resp, sock_of(fd) ? 0 : -1, sock_of(fd) ? 0 : E_BADF);
        OUT(uint32_t, 8) = b ? (uint32_t)b->size : 0;
        resp_size(resp, 12);
        return;
    }
    case 21: case 22: result(resp, sock_of(fd) ? 0 : -1, sock_of(fd) ? 0 : E_BADF); return;  // SetSockOpt, Shutdown
    case 19: {  // Ioctl(fd, request, count) + buffers: FIONBIO, FIONREAD
        uint32_t rq = IN(uint32_t, 4);
        pthread_mutex_lock(&lock);
        Sock *s = sock_of(fd);
        IpcBuf *b = in_buf(req, 0), *o = out_buf(req, 0);
        if (s && rq == 0x8004667e && b && b->size >= 4) s->nonblock = *(uint32_t *)(uintptr_t)b->addr != 0;
        if (s && rq == 0x4004667f && o && o->size >= 4) *(uint32_t *)(uintptr_t)o->addr = s->count ? s->q[s->head].len : 0;
        pthread_mutex_unlock(&lock);
        result(resp, s ? 0 : -1, s ? 0 : E_BADF);
        return;
    }
    case 20: {  // Fcntl(fd, cmd, flags): F_GETFL 3, F_SETFL 4 (O_NONBLOCK 4)
        int32_t cmd = IN(int32_t, 4), fl = IN(int32_t, 8);
        pthread_mutex_lock(&lock);
        Sock *s = sock_of(fd);
        int32_t ret = -1;
        if (s && cmd == 3) ret = s->nonblock ? 4 : 0;
        else if (s && cmd == 4) s->nonblock = (fl & 4) != 0, ret = 0;
        else if (s) ret = 0;
        pthread_mutex_unlock(&lock);
        result(resp, ret, s ? 0 : E_BADF);
        return;
    }
    case 26: {  // Close
        pthread_mutex_lock(&lock);
        Sock *s = sock_of(fd);
        if (s) free(s->q), memset(s, 0, sizeof *s);
        pthread_mutex_unlock(&lock);
        result(resp, s ? 0 : -1, s ? 0 : E_BADF);
        return;
    }
    default:
        LOG("[bsd] cmd %u (fd %d): not supported\n", req->cmd, fd);
        result(resp, -1, E_NETDOWN);
    }
}

SERVICE("bsd:u", bsd_u) { client(s, req, resp); }
SERVICE("bsd:s", bsd_s) { client(s, req, resp); }
