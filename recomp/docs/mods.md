# SMM2 recomp mods

Mods change the recompiled game at run time. Their code is ours (`games/smm2/mods/`, `runtime/net/`); they ship no
game code or data. Anything they need from the game (a layout to widen, a text to relabel) is made on the player's
machine from the player's own dump, into `private/`, which git ignores.

## 100 Mario Challenge (on SMM2's Endless Challenge)

SMM1's 100 Mario Challenge played through SMM2's own Endless Challenge: its menu, world map, course-in card, HUD,
pause menu and game over stay as they are. The run starts with **100 lives**, lives cap at **100**, a run is done
after **8 / 16 / 16 / 6** clears (Easy / Normal / Expert / Super Expert), **skips are free** (also during the miss
animation), and a miss throws away the coins and score of the failed attempt. Courses come from **our own server**
(`mmk server`, core/crates/server) or a **local library** run the same way, never from Nintendo's servers.

### Turning it on

Environment variables, or `smm2mods.ini` (`key=value` lines) in the data folder (`private/smm2/` in a source build):

| Variable | ini key | Meaning |
|---|---|---|
| `RECOMP_MOD_100MARIO=1` | `hundred=1` | the mod is on |
| `RECOMP_MOD_SERVER=<url>` | `server=` | the course server, plain `http://` (default `http://127.0.0.1:8080`) |
| `RECOMP_MOD_TOKEN=<token>` | `token=` | your account's token on that server (see below); keep it private |
| `RECOMP_MOD_RULESET=hundred\|smm2` | `ruleset=` | `hundred` (SMM1's rules, default) or `smm2` (SMM2's Endless rules against our server) |
| `RECOMP_MOD_TIMEOUT_MS=<n>` | `timeout_ms=` | per request (default 8000) |
| `RECOMP_MOD_LOG=1` | `log=1` | log every request and answer |

    RECOMP_MOD_100MARIO=1 RECOMP_MOD_SERVER=http://127.0.0.1:8640 RECOMP_MOD_TOKEN=... make -C recomp run-smm2

Then: Play -> Course World -> Endless Challenge -> a difficulty -> New Game.

### Server

Any `mmk server` works (yours, a friend's, a community one):

    cd core && cargo build --release -p mmk-cli
    target/release/mmk server --dir my-courses --addr 127.0.0.1:8640
    target/release/mmk register MyName --server http://127.0.0.1:8640      # prints your maker ID and token
    target/release/mmk upload --server http://127.0.0.1:8640 courses/*.zlib # raw, zlib, or .bcd with --keys

**Local library** (no network at all): the same server on 127.0.0.1 with a folder of your own courses (your saved
courses, the decrypted course data, or exports). Courses without play statistics fall into the difficulty bands by
the server's rules (unplayed = 50 % fail rate, the band widens when one runs dry).

The server holds the run: lives, clears, the course queue, the history (the map's dots), completed runs
(`hundred_<difficulty>` leaderboards) and SMM2-rule high scores.

### How it works

| Piece | Where | What |
|---|---|---|
| HTTP client | `runtime/net/http.c` | blocking HTTP/1.1, timeouts, sockets (Winsock on Windows); one connection per request |
| JSON | `runtime/net/json.c` | a small reader for the server's answers |
| Pretend online | `runtime/net/online.h`, `services/nifm.c`, `acc.c`, `mii.c` | with a mod on: nifm reports a connection, acc a network service account (a local placeholder token), mii a default Mii; nothing talks to Nintendo |
| Login | `games/smm2/mods/online.c` | the game's network session ("enl") logs in without NEX; Course World's login sequence gets neutral answers and the player's record (name from the server, a default Mii, registered) |
| Endless | `games/smm2/mods/endless.c` | the Endless Challenge's request functions answered from the server (below) |
| Rules | `games/smm2/mods/hooks.c` + `patches.py` | host code after single instructions: lives clamps 99 -> 100, free skip, coin / score rollback on a miss |
| Settings | `games/smm2/mods/settings.c` | the variables above |

The game's Course World requests are each one function (a "wrapper") that the network worker calls through a
pointer; the mod replaces those it needs with `hle_override` and runs the game's own otherwise. Every other Course
World screen runs the game's own code, which finds no connection and shows "A communication error occurred."

| Request (v3.0.3) | Answered from |
|---|---|
| GetEndlessModeStatus | `GET /api/endless`: lives, clears, coins, score per difficulty; start lives 100 (5 under `smm2`) |
| InitEndlessMode (New Game) | `POST /api/endless/<d>/start {"ruleset"}` |
| GetEndlessModePlayInfo | `GET /api/endless/<d>/history` (the map) |
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

### Run end

When the server answers a clear with `run_complete` (the 8th / 16th / 16th / 6th clear), the game goes back to the map
as after any clear; the next course start is answered with no lives left, so the game ends the run there and returns
to the menu, where the run shows as finished (no Continue) and the completion counts on the `hundred_<d>` board.

### Layout and texts (made from your dump)

At the first launch with the mod on, `games/smm2/mods/overlay.c` writes `mods-generated/100mario-v2/romfs/` in the
data folder from your own RomFS and lays it over the game's files:

- `Layout/Nwk_WorldMap_00`: the map's lives number is drawn narrower, so "100" no longer runs into the clears
  counter (the HUD and the course card already fit three digits);
- `Message/LayoutMsg_*`, `CommonMsg_*` (English): "Endless Challenge" reads "100 Mario Challenge", and the menu's
  description line is ours.

Delete the folder to have it made again.

### Unattended runs

`RECOMP_MOD_AUTOPLAY=1` (or `=jump`) is a small autopilot for tests: it presses A on the map, holds right + run in
a course (`jump`: and jumps every 1.5 s) and skips a course after `RECOMP_MOD_AUTOPLAY_SKIP` seconds (default 20). It
needs `LARECOMP_INPUT` set (the test-input hook), which also scripts the way to the Endless menu.
`RECOMP_MOD_TRACE=1` logs Course World's states and every request the mod doesn't answer.

### Limits

- Only the Endless Challenge is online. Course lists, uploads, Network Play, leaderboards and Super Worlds answer
  "not connected".
- The Maker avatar is the console's default Mii with your account's name (creating one needs the Mii editor applet,
  which the runtime doesn't have).
- No SMM2 Mii-costume rewards (the server sends no notifications).
