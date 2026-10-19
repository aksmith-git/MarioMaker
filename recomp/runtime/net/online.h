// "Pretend online": whether the system services report a working internet connection and a network service
// account. Off by default (the runtime is offline, as Ryujinx is without internet). A game mod that answers the
// game's online requests itself (games/smm2/mods: 100 Mario against our own server) turns it on by defining
// net_pretend_online(); RECOMP_PRETEND_ONLINE=1 turns it on for testing. Nothing here talks to Nintendo's servers:
// the services only answer locally.
#pragma once
#include <stdbool.h>

bool net_pretend_online(void);
