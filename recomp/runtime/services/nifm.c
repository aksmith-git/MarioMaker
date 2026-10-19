// nifm (network interface manager): always offline. Behaviour follows Ryujinx's (MIT) with internet access off,
// the reference the recomp is compared against: requests end in the Error state, connection queries fail with
// NoInternetConnection.
#include <stdlib.h>
#include <string.h>

#include "../hle.h"
#include "../ipc.h"
#include "../net/online.h"
#include "../net/vlan.h"

#define NIFM_NO_INTERNET MAKE_RESULT(110, 300)
#define NIFM_UNKNOWN180 MAKE_RESULT(110, 180)

enum { REQUEST_ERROR = 1, REQUEST_ON_HOLD = 2, REQUEST_AVAILABLE = 3 };

typedef struct { uint32_t ev0, ev1; KEvent *e0, *e1; } NifmRequest;

// Pretend online (runtime/net/online.h): requests are accepted at once and the connection reports as up. A local
// answer only: mods route the game's own online calls to their own server.
static void online_request(NifmRequest *r, IpcReq *req, IpcResp *resp) {
    switch (req->cmd) {
    case 0: OUT(uint32_t, 0) = REQUEST_AVAILABLE; resp_size(resp, 4); break;  // GetRequestState: accepted
    case 4:                                                                   // Submit: done at once
        if (r->e0) {
            pthread_mutex_lock(&g_klock);
            kernel_signal(r->e0);
            kernel_signal(r->e1);
            pthread_mutex_unlock(&g_klock);
        }
        break;
    default: break;  // GetResult (success), Cancel, GetAppletInfo (nothing to show), SetConnectionConfirmationOption...
    }
}

static void request(Service *s, IpcReq *req, IpcResp *resp) {
    NifmRequest *r = s->state;
    if (net_pretend_online() && req->cmd != 2) { online_request(r, req, resp); return; }
    switch (req->cmd) {
    case 0: OUT(uint32_t, 0) = REQUEST_ERROR; resp_size(resp, 4); break;  // GetRequestState
    case 1: break;                                                         // GetResult
    case 2: {                                                              // GetSystemEventReadableHandles
        if (!r->ev0) {
            uint32_t w;
            r->e0 = kernel_create_event_pair(&w, &r->ev0);
            r->e1 = kernel_create_event_pair(&w, &r->ev1);
        }
        resp->copy_handles[resp->ncopy++] = r->ev0;
        resp->copy_handles[resp->ncopy++] = r->ev1;
        break;
    }
    case 3: case 4: case 11: break;  // Cancel, Submit, SetConnectionConfirmationOption
    case 21: resp->result = NIFM_UNKNOWN180; break;  // GetAppletInfo: no applet to show
    default: LOG("[nifm] IRequest cmd %u (ok)\n", req->cmd); resp_size(resp, 0x10);
    }
}

static void general(Service *s, IpcReq *req, IpcResp *resp) {
    (void)s;
    if (vlan_active()) switch (req->cmd) {  // the virtual LAN (runtime/net/vlan.h): our address on it
    case 12: {  // GetCurrentIpAddress (network order in a u32)
        uint32_t ip = vlan_ip();
        OUT(uint32_t, 0) = (ip >> 24) | (ip >> 8 & 0xff00) | (ip << 8 & 0xff0000) | (ip << 24);
        resp_size(resp, 4);
        return;
    }
    case 15: {  // GetCurrentIpConfigInfo: {u8 auto, ip, mask, gateway} then DNS {u8 auto, dns1, dns2}
        uint32_t ip = vlan_ip(), mask = VLAN_MASK, gw = VLAN_NET | 254;
        uint8_t *o = resp->data;
        memset(o, 0, 0x16);
        o[0] = 1;
        for (int i = 0; i < 4; i++) o[1 + i] = (uint8_t)(ip >> (24 - 8 * i)), o[5 + i] = (uint8_t)(mask >> (24 - 8 * i)),
                                    o[9 + i] = (uint8_t)(gw >> (24 - 8 * i));
        resp_size(resp, 0x16);
        return;
    }
    default: break;
    }
    if (net_pretend_online()) switch (req->cmd) {
    case 5: {  // GetCurrentNetworkProfile -> buffer (left empty: no real network)
        IpcBuf *b = out_buf(req, 0);
        if (b && b->size) memset((void *)(uintptr_t)b->addr, 0, b->size);
        return;
    }
    case 12: OUT(uint32_t, 0) = 0x0100007f; resp_size(resp, 4); return;  // GetCurrentIpAddress: 127.0.0.1
    case 15: memset(resp->data, 0, 0x16); resp_size(resp, 0x16); return;  // GetCurrentIpConfigInfo
    case 18:  // GetInternetConnectionStatus {type 2 = ethernet, wifi strength 3, state 4 = connected}
        OUT(uint8_t, 0) = 2, OUT(uint8_t, 1) = 3, OUT(uint8_t, 2) = 4;
        resp_size(resp, 3);
        return;
    case 21: case 22: OUT(uint8_t, 0) = 1; resp_size(resp, 1); return;  // IsAny{Internet,Foreground}RequestAccepted
    default: break;
    }
    switch (req->cmd) {
    case 1: {  // GetClientId -> buffer
        IpcBuf *b = out_buf(req, 0);
        if (b && b->size >= 4) memset((void *)(uintptr_t)b->addr, 0, 4);
        break;
    }
    case 4:  // CreateRequest(u32 version)
        ipc_return_object(resp, service_new("nifm:IRequest", request, calloc(1, sizeof(NifmRequest))));
        break;
    case 5: case 12: case 15: case 18:  // GetCurrentNetworkProfile / IpAddress / IpConfigInfo / InternetConnectionStatus
        resp->result = NIFM_NO_INTERNET;
        break;
    case 21: OUT(uint8_t, 0) = 1; resp_size(resp, 1); break;  // IsAnyInternetRequestAccepted (as Ryujinx)
    default: LOG("[nifm] IGeneralService cmd %u (ok)\n", req->cmd); resp_size(resp, 0x10);
    }
}

static void static_service(Service *s, IpcReq *req, IpcResp *resp) {
    (void)s;
    switch (req->cmd) {
    case 4: case 5: ipc_return_object(resp, service_new("nifm:IGeneralService", general, NULL)); break;
    default: LOG("[nifm] IStaticService cmd %u (ok)\n", req->cmd); resp_size(resp, 0x10);
    }
}

SERVICE("nifm:u", nifm_u) { static_service(s, req, resp); }
SERVICE("nifm:s", nifm_s) { static_service(s, req, resp); }
SERVICE("nifm:a", nifm_a) { static_service(s, req, resp); }
