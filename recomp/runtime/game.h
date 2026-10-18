// Which game this runtime is built for. The shared runtime holds no game; games/<game>/game.mk passes these as -D
// flags (the defaults below only keep a bare compile working).
#pragma once

#ifndef GAME_ID
#define GAME_ID "game"  // short name: file names (the game code library, the settings file, the data folder)
#endif
#ifndef GAME_TITLE
#define GAME_TITLE "Recomp"  // shown to the player: window title, message boxes
#endif
#ifndef GAME_TITLE_ID
#define GAME_TITLE_ID 0x0100000000000000ull  // the game's program (title) id
#endif
#ifndef GAME_BUNDLE_ID
#define GAME_BUNDLE_ID "org.smcraft.recomp." GAME_ID  // macOS data folder (~/Library/Application Support/<this>)
#endif

#ifndef GAME_DISPLAY_VERSION
#define GAME_DISPLAY_VERSION "1.0.0"  // the version the game shows (control data; am GetDisplayVersion)
#endif

#define GAME_EXE GAME_ID "recomp"       // the executable's name (logs and crash reports)
#define GAME_SETTINGS GAME_ID ".ini"    // settings file in the data root
