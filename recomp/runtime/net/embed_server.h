// The course server inside the app: core/crates/embed (Rust, a static library), linked when the build has it
// (`make EMBED_SERVER=1`, or automatically when its library is built: `make embed-server`). A device then needs no
// server of its own; with the upstream "tgrcode" the Endless Challenge and 100 Mario courses come from TGRCode's API
// (tgrcode.com/mm2, a third-party service reading Nintendo's Course World). Without the library both functions do
// nothing and return false: the mods use their server= setting.
#pragma once
#include <stdbool.h>
#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

// Start it (once per process; later calls return the first result): its folder data_dir, upstream "tgrcode" or NULL,
// a local account named name. Fills url_out ("http://127.0.0.1:<port>") and token_out. Call after the game is
// loaded: the course key table is found in the game's own executable with the first download (never stored).
bool embed_server_start(const char *data_dir, const char *upstream, const char *name, char *url_out, size_t url_len,
                        char *token_out, size_t token_len);

// At boot (runtime/main.c: the game loaded, the mods not set up yet). RECOMP_EMBED_SERVER=tgrcode | local | off
// (default: tgrcode on iOS / iPadOS when the library is linked, off elsewhere). When it is on and no server is
// chosen (RECOMP_MOD_SERVER, or server= in smm2mods.ini), the server starts in <data>/server and the mods are pointed
// at it through their own settings (RECOMP_MOD_SERVER, RECOMP_MOD_TOKEN; RECOMP_MOD_100MARIO=1 unless set).
void embed_server_boot(void);

#ifdef __cplusplus
}
#endif
