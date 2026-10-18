// nifm (network interface manager): always offline. Behaviour follows Ryujinx's (MIT) with internet access off,
// the reference the recomp is compared against: requests end in the Error state, connection queries fail with
// NoInternetConnection.
#include <stdlib.h>
#include <string.h>

#include "../hle.h"
#include "../ipc.h"

#define NIFM_NO_INTERNET MAKE_RESULT(110, 300)
#define NIFM_UNKNOWN180 MAKE_RESULT(110, 180)

enum { REQUEST_ERROR = 1, REQUEST_ON_HOLD = 2, REQUEST_AVAILABLE = 3 };

typedef struct { uint32_t ev0, ev1; } NifmRequest;

static void request(Service *s, IpcReq *req, IpcResp *resp) {
    NifmRequest *r = s->state;
    switch (req->cmd) {
    case 0: OUT(uint32_t, 0) = REQUEST_ERROR; resp_size(resp, 4); break;  // GetRequestState
    case 1: break;                                                         // GetResult
    case 2: {                                                              // GetSystemEventReadableHandles
        if (!r->ev0) {
            uint32_t w;
            kernel_create_event_pair(&w, &r->ev0);
            kernel_create_event_pair(&w, &r->ev1);
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
