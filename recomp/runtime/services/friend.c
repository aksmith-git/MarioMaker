// friend:* (friends): offline and empty, following Ryujinx's (MIT) friends service: no friends, no blocked users, no
// notifications; everything else succeeds with zeroed outputs.
#include <stdlib.h>
#include <string.h>

#include "../hle.h"
#include "../ipc.h"

#define FRIEND_RESULT(d) MAKE_RESULT(121, d)

static uint32_t new_event(void) {
    uint32_t w, r;
    kernel_create_event_pair(&w, &r);
    return r;
}

static void friend_service(Service *s, IpcReq *req, IpcResp *resp) {
    (void)s;
    switch (req->cmd) {
    case 0: resp->copy_handles[resp->ncopy++] = new_event(); break;  // GetCompletionEvent
    case 10120: case 10420: OUT(uint8_t, 0) = 1; resp_size(resp, 1); break;  // Check{Friend,BlockedUser}ListAvailability
    default:
        if (ENV("LARECOMP_LOG_FRIEND")) LOG("[friend] IFriendService cmd %u (ok)\n", req->cmd);
        memset(resp->data, 0, 0x20);
        resp_size(resp, 0x10);  // (counts 0)
    }
}

static void notification_service(Service *s, IpcReq *req, IpcResp *resp) {
    (void)s;
    switch (req->cmd) {
    case 0: resp->copy_handles[resp->ncopy++] = new_event(); break;  // GetEvent
    case 1: break;                                                    // Clear
    case 2: resp->result = FRIEND_RESULT(15); break;                  // Pop: NotificationQueueEmpty
    default: LOG("[friend] INotificationService cmd %u (ok)\n", req->cmd);
    }
}

static void creator(Service *s, IpcReq *req, IpcResp *resp) {
    (void)s;
    switch (req->cmd) {
    case 0: ipc_return_object(resp, service_new("friend:IFriendService", friend_service, NULL)); break;
    case 1: ipc_return_object(resp, service_new("friend:INotificationService", notification_service, NULL)); break;
    case 2: ipc_return_object(resp, service_new("friend:IDaemonSuspendSessionService", notification_service, NULL)); break;
    default: LOG("[friend] IServiceCreator cmd %u (ok)\n", req->cmd);
    }
}

SERVICE("friend:u", friend_u) { creator(s, req, resp); }
SERVICE("friend:v", friend_v) { creator(s, req, resp); }
SERVICE("friend:m", friend_m) { creator(s, req, resp); }
SERVICE("friend:s", friend_s) { creator(s, req, resp); }
SERVICE("friend:a", friend_a) { creator(s, req, resp); }
