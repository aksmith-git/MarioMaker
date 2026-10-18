// acc:u0 (one local user) and the preselected-user launch parameter.
#include <stdlib.h>
#include <string.h>

#include "../hle.h"
#include "../ipc.h"
#include "../net/online.h"

const uint8_t g_user_id[16] = {0x4c, 0x41, 0x52, 0x65, 0x63, 0x6f, 0x6d, 0x70, 0x01, 0, 0, 0, 0, 0, 0, 0x01};

static void write_uid_list(IpcReq *req) {
    IpcBuf *c = out_buf(req, 0);
    if (!c) return;
    memset((void *)(uintptr_t)c->addr, 0, c->size);
    if (c->size >= 16) memcpy((void *)(uintptr_t)c->addr, g_user_id, 16);
}

static void profile(Service *s, IpcReq *req, IpcResp *resp) {
    (void)s;
    switch (req->cmd) {
    case 0: case 1: {  // Get / GetBase -> ProfileBase {uid, u64 timestamp, nickname[0x21]}
        memset(resp->data, 0, 0x38);
        memcpy(resp->data, g_user_id, 16);
        *(uint64_t *)(resp->data + 16) = 1600000000;
        strcpy((char *)resp->data + 24, getenv("RECOMP_USER_NAME") ? getenv("RECOMP_USER_NAME") : "Player");
        resp_size(resp, 0x38);
        if (req->cmd == 0 && out_buf(req, 0)) memset((void *)(uintptr_t)out_buf(req, 0)->addr, 0, out_buf(req, 0)->size);
        break;
    }
    case 10: OUT(uint32_t, 0) = 0; resp_size(resp, 4); break;  // GetImageSize
    case 11: OUT(uint32_t, 0) = 0; resp_size(resp, 4); break;  // LoadImage
    default: LOG("[acc] IProfile cmd %u (ok)\n", req->cmd);
    }
}

// Pretend online (runtime/net/online.h): the user has a network service account whose ID token is ready. The token
// is a local placeholder (no Nintendo service is asked; mods answer the game's online calls themselves).
static void async_context(Service *s, IpcReq *req, IpcResp *resp) {
    (void)s;
    switch (req->cmd) {
    case 0: {  // GetSystemEvent: already signalled (done)
        uint32_t w, r;
        KEvent *e = kernel_create_event_pair(&w, &r);
        e->signaled = true;
        resp->copy_handles[resp->ncopy++] = r;
        break;
    }
    case 2: OUT(uint8_t, 0) = 1; resp_size(resp, 1); break;  // HasDone
    default: break;                                          // Cancel, GetResult: success
    }
}

static bool online_manager(IpcReq *req, IpcResp *resp) {
    static const char token[] = "recomp-local-placeholder-token";
    switch (req->cmd) {
    case 2: case 141:  // EnsureIdTokenCacheAsync, EnsureNetworkServiceAccountIdTokenCacheAsync...
        ipc_return_object(resp, service_new("IAsyncContext", async_context, NULL));
        return true;
    case 3: case 142: {  // LoadIdTokenCache -> buffer, u32 size
        IpcBuf *b = out_buf(req, 0);
        uint32_t n = sizeof token - 1;
        if (b && b->size) {
            if (n > b->size) n = (uint32_t)b->size;
            memset((void *)(uintptr_t)b->addr, 0, b->size);
            memcpy((void *)(uintptr_t)b->addr, token, n);
        }
        OUT(uint32_t, 0) = n;
        resp_size(resp, 4);
        return true;
    }
    case 130:  // GetNintendoAccountUserResourceCacheForApplication: u64 id, buffers left empty
        for (int i = 0; i < 2; i++) {
            IpcBuf *b = out_buf(req, i);
            if (b && b->size) memset((void *)(uintptr_t)b->addr, 0, b->size);
        }
        OUT(uint64_t, 0) = 0x1234567890abcdefull;
        resp_size(resp, 8);
        return true;
    case 150:  // CreateAuthorizationRequest
        ipc_return_object(resp, service_new("IAsyncContext", async_context, NULL));
        return true;
    default: return false;
    }
}

static void manager(Service *s, IpcReq *req, IpcResp *resp) {
    (void)s;
    if (net_pretend_online() && online_manager(req, resp)) return;
    switch (req->cmd) {
    case 0: break;  // CheckAvailability
    case 1: OUT(uint64_t, 0) = 0x1234567890abcdefull; resp_size(resp, 8); break;  // GetAccountId
    default: LOG("[acc] IManagerForApplication cmd %u (ok)\n", req->cmd); resp_size(resp, 0x10);
    }
}

SERVICE("acc:u0", acc_u0) {
    (void)s;
    switch (req->cmd) {
    case 0: OUT(int32_t, 0) = 1; resp_size(resp, 4); break;  // GetUserCount
    case 1: OUT(uint8_t, 0) = !memcmp(req->data, g_user_id, 16); resp_size(resp, 1); break;
    case 2: case 3: case 141: write_uid_list(req); break;  // ListAllUsers / ListOpenUsers / ListQualifiedUsers
    case 4: case 51: memcpy(resp->data, g_user_id, 16); resp_size(resp, 16); break;  // GetLastOpenedUser
    case 5: ipc_return_object(resp, service_new("IProfile", profile, NULL)); break;
    case 50: OUT(uint8_t, 0) = 0; resp_size(resp, 1); break;
    case 100: case 140: break;  // InitializeApplicationInfo
    case 101: ipc_return_object(resp, service_new("IManagerForApplication", manager, NULL)); break;
    case 150: OUT(uint8_t, 0) = 0; resp_size(resp, 1); break;  // IsUserAccountSwitchLocked
    default: LOG("[acc] acc:u0 cmd %u (ok)\n", req->cmd); resp_size(resp, 0x10);
    }
}
