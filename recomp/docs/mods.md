# SMM2 recomp mods

Mods change the recompiled game at run time. Their code is ours (`games/smm2/mods/`, `runtime/net/`); they ship no
game code or data. Anything they need from a game is made on the player's machine from the player's own dumps, into
`private/`, which git ignores.

## Online against our server, and the 100 Mario Challenge

One mod does two things:

- **Course World against our own server** (`mmk server`, core/crates/server) or a **local library** run the same way,
  never Nintendo's servers. SMM2's own **Endless Challenge** works as SMM2 made it, under the server's `smm2` rules
  (5 lives, endless).
- **SMM1's 100 Mario Challenge**, SMM1's rules: **100 lives**, lives cap at **100**, a run is won after
  **8 / 16 / 16 / 6** clears (Easy / Normal / Expert / Super Expert), **skips are free** (also during the miss
  animation), a miss throws away the coins and score of the failed attempt, an award worth several lives banks one
  extra life (at most 3 per course, paid on the clear), Expert opens after a won Normal run and Super Expert after a
  won Expert run. SMM1's screens, map and ending are drawn host-side from the player's SMM1 dump (another part of the
  mod: `games/smm2/mods/smm1/`, the SMM1 screens; needs the SMM1 dump: `RECOMP_SMM1_DUMP`); the courses play in SMM2's own course play. The run side is
  described below ("The bridge").

### Turning it on

Environment variables, or `smm2mods.ini` (`key=value` lines) in the data folder (`private/smm2/` in a source build):

| Variable | ini key | Meaning |
|---|---|---|
| `RECOMP_MOD_100MARIO=1` | `hundred=1` | the mod is on (Course World online against our server) |
| `RECOMP_MOD_SERVER=<url>` | `server=` | the course server, plain `http://` (default `http://127.0.0.1:8080`) |
| `RECOMP_MOD_TOKEN=<token>` | `token=` | your account's token on that server (see below); keep it private |
| `RECOMP_MOD_RULESET=hundred\|smm2` | `ruleset=` | `hundred`: Course World's Endless Challenge button opens SMM1's 100 Mario Challenge (default); `smm2`: it opens SMM2's own Endless Challenge |
| `RECOMP_MOD_TIMEOUT_MS=<n>` | `timeout_ms=` | per request (default 8000) |
| `RECOMP_MOD_LOG=1` | `log=1` | log every request and answer |

    RECOMP_MOD_100MARIO=1 RECOMP_MOD_SERVER=http://127.0.0.1:8640 RECOMP_MOD_TOKEN=... make -C recomp run-smm2

Then: Play -> Course World -> Endless Challenge.

### Server

Any `mmk server` works (yours, a friend's, a community one):

    cd core && cargo build --release -p mmk-cli
    target/release/mmk server --dir my-courses --addr 127.0.0.1:8640
    target/release/mmk register MyName --server http://127.0.0.1:8640      # prints your maker ID and token
    target/release/mmk upload --server http://127.0.0.1:8640 courses/*.zlib # raw, zlib, or .bcd with --keys

**Local library** (no network at all): the same server on 127.0.0.1 with a folder of your own courses (your saved
courses, the decrypted course data, or exports). Courses without play statistics fall into the difficulty bands by
the server's rules (unplayed = 50 % fail rate, the band widens when one runs dry).

The server holds the run: lives, clears, the course queue, the history, completed runs (`hundred_<difficulty>`
leaderboards) and SMM2-rule high scores.

### How it works

| Piece | Where | What |
|---|---|---|
| HTTP client | `runtime/net/http.c` | blocking HTTP/1.1, timeouts, sockets (Winsock on Windows); one connection per request |
| JSON | `runtime/net/json.c` | a small reader for the server's answers |
| Pretend online | `runtime/net/online.h`, `services/nifm.c`, `acc.c`, `mii.c` | with the mod on: nifm reports a connection, acc a network service account (a local placeholder token), mii a default Mii; nothing talks to Nintendo |
| Login | `games/smm2/mods/online.c` | the game's network session ("enl") logs in without NEX; Course World's login sequence gets neutral answers and the player's record (name from the server, a default Mii, registered) |
| Endless | `games/smm2/mods/endless.c` | the Endless Challenge's request functions answered from the server (below) |
| Bridge | `games/smm2/mods/bridge.c`, `hundred.h` | 100 Mario: SMM1's screens on top, SMM2's play underneath |
| Rules | `games/smm2/mods/hooks.c` + `patches.py` | host code after single instructions (22 points): lives clamps 99 -> 100, free skip, coin / score rollback on a miss, one banked life per award; only in a 100 Mario run |
| Settings | `games/smm2/mods/settings.c` | the variables above |

The game's Course World requests are each one function (a "wrapper") that the network worker calls through a
pointer; the mod replaces those it needs with `hle_override` and runs the game's own otherwise. Every other Course
World screen runs the game's own code, which finds no connection and shows "A communication error occurred."

| Request (v3.0.3) | Answered from |
|---|---|
| GetEndlessModeStatus | `GET /api/endless`: lives, clears, coins, score per difficulty; start lives 100 (5 under `smm2`) |
| InitEndlessMode (New Game) | `POST /api/endless/<d>/start {"ruleset"}` |
| GetEndlessModePlayInfo | `GET /api/endless/<d>/history` |
| GetEndlessModeRank | `GET /api/rankings/hundred_<d>` (or `endless_<d>`), the `me` row |
| SearchCoursesEndlessMode | `GET /api/endless/<d>/next?count=10` |
| GetObject (course file) | `GET /api/courses/<code>/data`, given the game's header and its own course encryption, so its CRC and validity checks run unchanged |
| StartEndlessModeCourse | first post for a course: the run as it is; a second post for the same course (sent during a miss): `result: death` |
| DominateEndlessModeCourse (clear) | `POST /api/endless/<d>/result {"result":"clear", pending, coins, score}` |
| PassEndlessModeCourse (skip) | `result: skip` |
| Suspend / FinishEndlessMode | `POST /api/endless/<d>/suspend`, `/finish` |
| GetUsers (own) | `GET /api/me` (name), best per difficulty (completed 100 Mario runs under `hundred`) |
| GetUsers (players) | the makers of the listed courses (names from the course list, the default Mii) |
| GetCourses (by id) | `GET /api/courses/<code>` |
| comments, play report / result, two course records, a course post, an id check | answered empty or accepted (nothing the run needs) |
| GetApplicationConfig | Endless switched on for the four difficulties |

Data ids and course codes map 1:1 (`games/smm2/mods/course.c`, the same arithmetic as `mmk_formats::course_id`).

### The bridge (100 Mario)

`hundred.h` is the interface between the run side (here) and the flow (SMM1's screens). With `ruleset=hundred`,
when the game enters its Endless menu the run side calls `hundred_flow_open()`; the flow then shows SMM1's screens
and calls `hundred_run_start()`, `hundred_play_next()`, `hundred_play_skip()`, `hundred_run_finish()` and reads
`hundred_run_status()`; the run side reports `hundred_flow_course_started()` and `hundred_course_result()`.

How a server course starts in SMM2's play without SMM2's Endless screens: they stay underneath, hidden by the flow,
and the run side drives them. SMM2's Endless machinery is what makes a server course play in SMM2's own play (the
run manager, the course list and download jobs, the checks, the course-in card, the HUD with the lives and three
banked 1-Ups, the skip, the retry after a miss, the goal), and its requests already go to our server. The run side
catches the Endless menu's page in its Disp state, picks the difficulty with the game's own tab handler (on the game
thread), and presses the menu's own buttons (New Game, the map's Play, skip, back) through the test-input hook while
the player's input goes to the flow. The course's outcome comes back through the game's own requests (course start,
miss, clear, skip). After a skip SMM2's Endless starts the next course at once; after a miss it retries by itself.

The win: the Nth clear's answer has `run_complete`; the flow plays SMM1's ending (`hundred_ending_show`) and calls
`hundred_run_finish()`, which finishes the run on the server (the completion is on its `hundred_<d>` board already).

The flow is `games/smm2/mods/smm1/` (strong `hundred_flow_*`, `hundred_course_result`, the ending); it keeps weak
stand-ins of the run side's functions, which `bridge.c` / `endless.c` replace. `hundred_run_status()` returns a
snapshot under the run's lock (it is called on the render thread); `style` is the current course's game style.

`RECOMP_MOD_FLOW_TEST=<difficulty>` plays a whole run unattended in place of the flow (the flow's screens are left
out): the run starts on entering the Endless menu, Play follows every clear, the win plays the ending and finishes
the run. `RECOMP_MOD_AUTOPLAY=1` (or `=jump`) plays the courses (right + run [+ jump], skip after
`RECOMP_MOD_AUTOPLAY_SKIP` seconds, default 20). Both need `LARECOMP_INPUT` set (the test-input hook), which also
scripts the way to the Endless menu. `RECOMP_MOD_UNLOCK_ALL=1` opens every difficulty; `RECOMP_MOD_TRACE=1` logs
Course World's states and every request the mod doesn't answer.

Not driven yet: Continue (a saved run is started as New Game) and SMM2's pause during a run course (to become
`hundred_flow_pause()`: the play scene's pause state is a virtual of the scene object, `cPausePlay` at index 3 of the
machine registered at main+0x1bc19c0, so the interception needs the scene's vtable caught at run time).

### Limits

- Only the Endless Challenge is online. Course lists, uploads, Network Play, leaderboards and Super Worlds answer
  "not connected".
- The Maker avatar is the console's default Mii with your account's name (creating one needs the Mii editor applet,
  which the runtime doesn't have).
- No SMM2 Mii-costume rewards (the server sends no notifications).
