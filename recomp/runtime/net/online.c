// Default for net_pretend_online() (runtime/net/online.h): a game's mod code may define it instead.
#include "online.h"

#include <stdlib.h>

__attribute__((weak)) bool net_pretend_online(void) {
    static int on = -1;
    if (on < 0) on = getenv("RECOMP_PRETEND_ONLINE") && atoi(getenv("RECOMP_PRETEND_ONLINE")) != 0;
    return on;
}
