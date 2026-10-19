// Virtual LAN over our course server's relay (runtime/net/vlan.h).
//
// Relay messages (core/crates/netplay/src/relay.rs): "MMKJ" slot u8, room length u8, room (join / keepalive; the
// relay answers "MMKW" with the members); "MMKD" + payload, forwarded to the room's other members. Our payload:
// "VL1" version, src ip u32 BE, src port u16 BE, dst ip u32 BE, dst port u16 BE, then the datagram. Each receiver keeps
// what is addressed to it (its address, the subnet broadcast or 255.255.255.255).
#include "vlan.h"

#include <pthread.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "../hle.h"

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
#else
#include <netdb.h>
#include <netinet/in.h>
#include <sys/socket.h>
#include <sys/time.h>
#include <unistd.h>
typedef int sock_t;
#define SOCK_BAD (-1)
#define sock_close close
static void net_init(void) {}
#endif

// a game may supply the relay and room instead of the environment (runtime/game_hooks.c, weak: none)
bool game_lan_config(char *relay, size_t relay_len, char *room, size_t room_len, int *slot);

static pthread_once_t once = PTHREAD_ONCE_INIT;
static bool on;
static sock_t sock = SOCK_BAD;
static struct sockaddr_storage relay_addr;
static socklen_t relay_len;
static char room[17];
static int slot;
static void (*deliver_fn)(uint32_t, uint16_t, uint16_t, const uint8_t *, size_t);

static size_t join_msg(uint8_t *out) {
    size_t n = strlen(room);
    memcpy(out, "MMKJ", 4);
    out[4] = (uint8_t)slot, out[5] = (uint8_t)n;
    memcpy(out + 6, room, n);
    return 6 + n;
}

static uint32_t rd32(const uint8_t *p) { return (uint32_t)p[0] << 24 | (uint32_t)p[1] << 16 | (uint32_t)p[2] << 8 | p[3]; }
static void wr32(uint8_t *p, uint32_t v) { p[0] = (uint8_t)(v >> 24), p[1] = (uint8_t)(v >> 16), p[2] = (uint8_t)(v >> 8), p[3] = (uint8_t)v; }

static void *rx_thread(void *arg) {
    (void)arg;
    uint8_t buf[2048], j[32];
    size_t jn = join_msg(j);
    int quiet = 0, welcomed = 0;
    for (;;) {
        int n = (int)recv(sock, (char *)buf, sizeof buf, 0);
        if (n <= 0) {  // (timeout: 1 s) keepalive
            sendto(sock, (const char *)j, (int)jn, 0, (struct sockaddr *)&relay_addr, relay_len);
            if (++quiet % 30 == 0 && !welcomed) LOG("[vlan] no answer from the relay yet\n");
            continue;
        }
        if (n >= 5 && !memcmp(buf, "MMKW", 4)) {
            if (!welcomed || buf[4] != welcomed) LOG("[vlan] room \"%s\": %u member(s)\n", room, buf[4]);
            welcomed = buf[4];
            continue;
        }
        if (n < 4 + 15 || memcmp(buf, "MMKD", 4) || memcmp(buf + 4, "VL1", 3)) continue;
        const uint8_t *h = buf + 7;
        uint32_t sip = rd32(h), dip = rd32(h + 6);
        uint16_t sport = (uint16_t)(h[4] << 8 | h[5]), dport = (uint16_t)(h[10] << 8 | h[11]);
        uint32_t me = vlan_ip();
        if (dip != me && dip != 0xFFFFFFFFu && dip != (VLAN_NET | ~VLAN_MASK)) continue;
        if (deliver_fn) deliver_fn(sip, sport, dport, h + 12, (size_t)n - 4 - 15);
    }
    return NULL;
}

static void *keepalive_thread(void *arg) {
    (void)arg;
    uint8_t j[32];
    size_t jn = join_msg(j);
    for (;;) {
        sendto(sock, (const char *)j, (int)jn, 0, (struct sockaddr *)&relay_addr, relay_len);
#ifdef _WIN32
        Sleep(2000);
#else
        sleep(2);
#endif
    }
    return NULL;
}

static void start(void) {
    char relay[256] = "", r[64] = "";
    int s = 0;
    const char *er = getenv("LARECOMP_LAN_RELAY"), *eroom = getenv("LARECOMP_LAN_ROOM"), *es = getenv("LARECOMP_LAN_SLOT");
    if (er && eroom) {
        snprintf(relay, sizeof relay, "%s", er), snprintf(r, sizeof r, "%s", eroom);
        s = es ? atoi(es) : 0;
    } else if (!game_lan_config(relay, sizeof relay, r, sizeof r, &s)) {
        return;
    }
    if (s < 0 || s > 3 || !r[0] || strlen(r) > 16) { LOG("[vlan] bad room / slot\n"); return; }
    char host[200], *colon = strrchr(relay, ':');
    if (!colon) { LOG("[vlan] relay %s: host:port wanted\n", relay); return; }
    snprintf(host, sizeof host, "%.*s", (int)(colon - relay), relay);
    net_init();
    struct addrinfo hints, *ai = NULL;
    memset(&hints, 0, sizeof hints);
    hints.ai_family = AF_INET, hints.ai_socktype = SOCK_DGRAM;
    if (getaddrinfo(host, colon + 1, &hints, &ai) || !ai) { LOG("[vlan] relay %s: not found\n", relay); return; }
    memcpy(&relay_addr, ai->ai_addr, ai->ai_addrlen);
    relay_len = (socklen_t)ai->ai_addrlen;
    freeaddrinfo(ai);
    sock = socket(AF_INET, SOCK_DGRAM, 0);
    if (sock == SOCK_BAD) return;
#ifdef _WIN32
    DWORD tmo = 1000;
    setsockopt(sock, SOL_SOCKET, SO_RCVTIMEO, (const char *)&tmo, sizeof tmo);
#else
    struct timeval tv = {1, 0};
    setsockopt(sock, SOL_SOCKET, SO_RCVTIMEO, &tv, sizeof tv);
#endif
    snprintf(room, sizeof room, "%s", r);
    slot = s, on = true;
    pthread_t th;
    pthread_create(&th, NULL, rx_thread, NULL);
    pthread_detach(th);
    pthread_create(&th, NULL, keepalive_thread, NULL);
    pthread_detach(th);
    LOG("[vlan] virtual LAN: room \"%s\" on relay %s, this player 10.13.37.%d\n", room, relay, slot + 1);
}

bool vlan_active(void) {
    pthread_once(&once, start);
    return on;
}

uint32_t vlan_ip(void) { return vlan_active() ? VLAN_NET | (uint32_t)(slot + 1) : 0; }

void vlan_set_receiver(void (*deliver)(uint32_t, uint16_t, uint16_t, const uint8_t *, size_t)) { deliver_fn = deliver; }

int vlan_send(uint16_t src_port, uint32_t dst_ip, uint16_t dst_port, const void *data, size_t len) {
    if (!vlan_active() || len > VLAN_MAX_PAYLOAD) return -1;
    uint8_t buf[4 + 15 + VLAN_MAX_PAYLOAD];
    memcpy(buf, "MMKDVL1", 7);
    uint8_t *h = buf + 7;
    wr32(h, vlan_ip()), h[4] = (uint8_t)(src_port >> 8), h[5] = (uint8_t)src_port;
    wr32(h + 6, dst_ip), h[10] = (uint8_t)(dst_port >> 8), h[11] = (uint8_t)dst_port;
    memcpy(h + 12, data, len);
    int n = (int)sendto(sock, (const char *)buf, (int)(4 + 15 + len), 0, (struct sockaddr *)&relay_addr, relay_len);
    return n < 0 ? -1 : (int)len;
}
