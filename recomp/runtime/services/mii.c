// mii:e / mii:u (the console's Mii database): an empty database. Calls are logged (the commands follow Ryujinx's
// IDatabaseService, MIT) so a game that needs Miis shows which ones.
#include <stdlib.h>
#include <string.h>

#include "../hle.h"
#include "../ipc.h"

static void database(Service *s, IpcReq *req, IpcResp *resp) {
    (void)s;
    switch (req->cmd) {
    case 0: case 1: case 20: OUT(uint8_t, 0) = 0; resp_size(resp, 1); break;  // IsUpdated, IsFullDatabase, IsBroken..
    case 2: OUT(uint32_t, 0) = 0; resp_size(resp, 4); break;                   // GetCount(flag)
    case 3: case 4: case 8: case 9: OUT(int32_t, 0) = 0; resp_size(resp, 4); break;  // Get, Get1, Get2, Get3: none
    case 22: break;                                                            // SetInterfaceVersion
    default: LOG("[mii] IDatabaseService cmd %u (unhandled, ok)\n", req->cmd); resp_size(resp, 0x10);
    }
}

static void static_service(Service *s, IpcReq *req, IpcResp *resp) {
    (void)s;
    switch (req->cmd) {
    case 0: ipc_return_object(resp, service_new("mii:IDatabaseService", database, NULL)); break;  // GetDatabaseService
    default: LOG("[mii] IStaticService cmd %u (ok)\n", req->cmd);
    }
}

SERVICE("mii:e", mii_e) { static_service(s, req, resp); }
SERVICE("mii:u", mii_u) { static_service(s, req, resp); }
