# SMM2 Recomp

A native port of Super Mario Maker 2 **v3.0.3** built by **static recompilation** of the player's own
dump: the game's AArch64 code is translated to C ahead of time and compiled for the host, its
system calls run on a native runtime, and its NVN graphics run on Metal (macOS, iOS) or Vulkan
(Windows, Linux). SMM1's 100 Mario Challenge and Mystery Mushroom costumes come as mods.

This folder is separate from SMCraft (the clean-room engine in `../core` and `../app`), which keeps
working on its own. The approach and the projects it builds on are in `../DECISIONS.md` ("Native
SMM2 port by static recompilation"); status, with evidence per platform, is in `../PROGRESS.md`
("Recomp").

**Nothing from the game is in this repository.** You supply your own dump; the recompiled code,
translated shaders and extracted data are generated on your machine into `private/`, `generated/`
and `build/`, which git ignores and the repo's proprietary-content check blocks.

Build instructions follow once the macOS bring-up lands.
