// mii:e / mii:u (the console's Mii database): an empty database; BuildDefault / BuildRandom give the default Mii.
// Calls are logged (the commands follow Ryujinx's
// IDatabaseService, MIT) so a game that needs Miis shows which ones.
#include <stdlib.h>
#include <string.h>

#include "../hle.h"
#include "../ipc.h"

// nn::mii::CharInfo (0x58 bytes, layout as in Ryujinx's CharInfo) of the console's first default Mii (Ryujinx's
// DefaultMii table, entry 2, colours through its Ver3 tables): a valid Mii for games that need one (BuildDefault,
// BuildRandom; mods that give the player a profile). The nickname is UTF-8, up to 10 characters.
void mii_default_charinfo(uint8_t out[0x58], const char *nickname) {
    static const uint8_t face[0x58 - 0x26] = {
        0,  0,  0,  64, 64, 0,  // font region, favourite colour, gender, height, build, type
        0,                      // region move
        0,  0,  0,  0,          // faceline type, colour, wrinkle, make
        33, 1,  0,              // hair type, colour, flip
        2,  8,  4,  3,  4, 2, 12,   // eye type, colour, scale, aspect, rotate, x, y
        6,  1,  4,  3,  6, 2, 10,   // eyebrow type, colour, scale, aspect, rotate, x, y
        1,  4,  9,              // nose type, scale, y
        23, 19, 4,  3,  13,     // mouth type, colour, scale, aspect, y
        8,  0,  0,  4,  10,     // beard colour, beard type, mustache type, scale, y
        0,  8,  4,  10,         // glass type, colour, scale, y
        0,  4,  2,  20,         // mole type, scale, x, y
        0,                      // reserved
    };
    memset(out, 0, 0x58);
    static const uint8_t create_id[16] = {0x52, 0x65, 0x63, 0x6f, 0x6d, 0x70, 0x4d, 0x69,
                                          0x80, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07};
    memcpy(out, create_id, 16);  // (byte 8: the "valid" bits 10xxxxxx)
    const unsigned char *p = (const unsigned char *)(nickname && *nickname ? nickname : "Player");
    for (int n = 0; *p && n < 10; n++) {  // UTF-8 -> UTF-16 (BMP)
        uint32_t cp = *p++;
        if (cp >= 0xC0) {
            int extra = cp >= 0xF0 ? 3 : cp >= 0xE0 ? 2 : 1;
            cp &= 0x3F >> extra;
            while (extra-- && (*p & 0xC0) == 0x80) cp = cp << 6 | (*p++ & 0x3F);
            if (cp > 0xFFFF) cp = '?';
        }
        out[16 + 2 * n] = (uint8_t)cp, out[17 + 2 * n] = (uint8_t)(cp >> 8);
    }
    memcpy(out + 0x26, face, sizeof face);
}

static void database(Service *s, IpcReq *req, IpcResp *resp) {
    (void)s;
    switch (req->cmd) {
    case 6: case 7:  // BuildRandom, BuildDefault -> CharInfo: the default Mii
        mii_default_charinfo(resp->data, getenv("RECOMP_USER_NAME"));
        resp_size(resp, 0x58);
        break;
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
