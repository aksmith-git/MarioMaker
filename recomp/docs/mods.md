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
| `RECOMP_MOD_RULESET=hundred\|smm2` | `ruleset=` | `hundred` (default): 100 Mario is offered: Course World's Endless Challenge button opens SMM1's 100 Mario Challenge, and B on its difficulty select leads on to SMM2's own Endless Challenge (both in one build; SMM2's rules there until the player leaves that menu); `smm2`: the button opens SMM2's own Endless Challenge only |
| `RECOMP_MOD_COURSES=server\|tgrcode` | `courses=` | `server` (default): the server above; `tgrcode` (iOS default, with the mod on): a course server built into the app on a loopback port, with a local account, fed from TGRCode's archive of SMM2 courses (needs the course key table: the mod writes it once from your game into `mods-generated/course_table.bin`, private); falls back to `server=` when the build has no built-in server. Also in the Mods tab ("Courses: TGRCode"), from the next start |
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
| GetUsers (own) | `GET /api/me` (name, maker ID), best per difficulty (completed 100 Mario runs under `hundred`) |
| GetCourses (by id) | `GET /api/courses/<code>` |
| course list info, an id check | answered empty / passes |
| ReportFromBugDetection (the game found a downloaded course invalid: "The course data is corrupt.") | in a run: `result: broken` (the course leaves the queue); the bridge answers the window, the flow hears `HUNDRED_SKIP`, the next course comes |
| GetApplicationConfig | Endless switched on for the four difficulties |

Data ids and course codes map 1:1 (`games/smm2/mods/course.c`, the same arithmetic as `mmk_formats::course_id`).
A maker's player id is a hash of their maker ID on our server (the player's own too), so the game's "my course"
and "me" checks work.

### Course World against our server (`games/smm2/mods/world.c`)

The rest of Course World goes to our server too. Each request's name is the DataStore (SMM2) method its function
calls (read from the client vtable slot it uses):

| Request (method id) | Function (main+) | Answered from |
|---|---|---|
| course lists: PickUp (84, "Hot"), PointRanking (71, "Popular", by difficulty), Latest (73, "New"), Advanced (72, detailed search: difficulty, style, sort), PostedBy (74, a maker's courses), BestTime / FirstClear (81 / 80) | 0x14d8550 (search type +0x746e8) | `GET /api/courses?sort=popular\|new&style=&offset=&limit=100`, filtered into the game's difficulty bands; a maker's courses from `GET /api/makers/<id>`; followed / liked / played-by lists and versus: empty |
| GetUserOrCourse (131): the ID search | 0x14dc0d0 | `GET /api/courses/<code>`, else `GET /api/makers/<code>` |
| GetUsers (48): the makers of listed courses, a maker's page | 0x14d02d0, 0x14d7780, 0x14d2f00 | `GET /api/makers/<id>`: name, maker code, likes received and maker points from their courses; the default Mii |
| SearchUsers* (50, 51, 57, 58): leaderboards | 0x14d1820 (kind +0x73a14) | `GET /api/rankings/maker_points` (makers, also the period's board), `endless_<d>` (Endless), `clears` |
| CanPostRatingAndComment (61) | 0x14ebc60 | allowed |
| PostRatingInfo (104): like / boo | 0x14e6b90 | `POST /api/courses/<code>/like` or `/boo` |
| PostPlayResult (96) | 0x14e30e0 | `POST /api/courses/<code>/play`, and `/clear {time_ms}` on a clear (not for Endless courses: those are counted by the Endless reports) |
| SearchComments (94 / 95), PostCommentText (91) | 0x14e8ec0, 0x14e96b0 | `GET` / `POST /api/courses/<code>/comments` (text) |
| CanPostCourse (60), the course upload (66 + upload + 67), DeleteObject | 0x14e0d40, 0x14de3f0, 0x14e0c50 | allowed; the game's own course (header "SCDL", not yet encrypted) to `POST /api/courses` as the signed-in maker, its code back as the upload's data id; `DELETE /api/courses/<code>` |

Course World courses download and play exactly like the Endless ones (the same GetObject). Not served: thumbnails
(the game shows its placeholder), Super Worlds, Ninji speedruns, notifications, follows, Network Play.

### The bridge (100 Mario)

`hundred.h` is the interface between the run side (here) and the flow (SMM1's screens). With `ruleset=hundred`,
when the game enters its Endless menu the run side calls `hundred_flow_open()`; the flow then shows SMM1's screens
and calls `hundred_run_start()`, `hundred_play_next()`, `hundred_play_skip()`, `hundred_run_finish()` and reads
`hundred_run_status()`; the run side reports `hundred_flow_course_started()` and `hundred_course_result()`.

How a server course starts in SMM2's play without SMM2's Endless screens: they stay underneath, hidden by the flow,
and the run side drives them. SMM2's Endless machinery is what makes a server course play in SMM2's own play (the
run manager, the course list and download jobs, the checks, the course-in card, the HUD with the lives and three
banked 1-Ups, the skip, the retry after a miss, the goal), and its requests already go to our server. The run side
picks the difficulty with L / R until the menu's difficulty (main+0x2c3f618) is the one wanted, then Down to the
buttons (after L / R the focus is on the tab bar), and presses the menu's own buttons (New Game or Continue, the
confirm window's Yes, the map's Play, skip, back) through the test-input hook while the player's input goes to the
flow. The course's outcome comes back through the game's own requests (course start,
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

**Both challenges.** With 100 Mario offered, B on SMM1's difficulty select (`hundred_flow_exit()`) leaves the player
on SMM2's own Endless menu, which was underneath: runs there use SMM2's rules (5 lives, the start lives are switched
in the game's run records) until the menu closes towards Course World (its Disappear state, main+0x1a24d30); the
next visit opens 100 Mario again. **After a win** the run side presses Quit on SMM2's last result screen and answers
the suspend locally, so SMM2 is back on its Endless menu for the next run; `hundred_run_start()` waits until it is.
**Loading:** a course list may take long (TGRCode upstream: about a minute for a difficulty's first batch); the
request waits up to 3 minutes and `hundred_run_status().loading` is set meanwhile.

**Pause.** During a run course SMM2's pause state (`cPausePlay`, state 3 of the Endless play scene's machine; its exec
is the scene's virtual at vtable+0x390, main+0x1bba050) is held without opening its menu: the play stays frozen and
the flow gets `hundred_flow_pause()`. `hundred_play_resume()` leaves it as the menu's close does (state 1),
`hundred_play_retry()` as Start Over (state 0x13, a life, posted as `start_over`), `hundred_run_save_quit()` as the
confirm window's Yes (state 0x12, the suspend). **Continue:** the run record's state byte (+4 = 1) makes the menu
offer Continue (and show the run on its map); `hundred_run_start(d, true)` presses it. **Course on screen:**
`hundred_flow_course_started()` comes on the first frame of the play scene's play state (state 1, vtable+0x350,
main+0x1bb9440) after a course start or a retry. **Next course:** the head of the server's queue is read after New
Game, each clear / skip and on Continue, so `hundred_run_status().current` names the next course before the game
asks. `form` is player 0's form at the goal (+0x4a8, small / big from the size class table main+0x23d5e0c).

Flow test extras: `RECOMP_MOD_FLOW_TEST_CONTINUE=1` (Continue instead of New Game), `RECOMP_MOD_FLOW_TEST_PAUSE=<letters>`
(pause 8 s into a course and answer r resume, o Start Over, q Save & Quit), `RECOMP_MOD_SHOTS=<name>` (window shots
at the run's moments into `build/<name>_<n>_<what>.png`).

### Test recipe (end to end, unattended)

The whole chain (difficulty select -> map -> card -> course -> clear -> ... -> ending) against a local server, the
window hidden. Paths are the ones on the dev machine; any folder works.

**1. The server.** The test server is `/Volumes/SSD/smm-work/recomp-mods/server-test` on port **8650**, 12 short
test courses (flat / items / power-up courses in each style; running right and jumping clears most), one account "Runner" whose token is in
`/Volumes/SSD/smm-work/recomp-mods/account-test.txt` (line `token <hex>`; keep it out of logs and commits). The
corpus server is `server-data` on port 8640 (550 real courses, account in `account.txt`). Start one (if
`curl http://127.0.0.1:8650/api/health` doesn't answer):

    cd /Volumes/SSD/smm-work/recomp-mods
    ./cargo-target/release/mmk server --dir server-test --addr 127.0.0.1:8650 \
        --writes-per-minute 100000 --accounts-per-hour 1000 > server-test.log 2>&1 &

A new one, seeded (any raw / zlib SMM2 course files; the 30 short ones in `testcourses/` were made for this):

    cd core && cargo build --release -p mmk-cli          # (target/release/mmk)
    mmk server --dir my-test --addr 127.0.0.1:8651 --writes-per-minute 100000 --accounts-per-hour 1000 &
    mmk register Runner --server http://127.0.0.1:8651 > my-account.txt   # maker ID + token
    mmk upload --server http://127.0.0.1:8651 --maker testlib /Volumes/SSD/smm-work/recomp-mods/testcourses/*.course

A run needs at least `total` courses in the difficulty's band (or the band widens): with only short test courses,
every difficulty draws from all of them.

**2. The game.** Build (`cd recomp && make GAME=smm2 -j10 all`), then from the data folder (`recomp/private/smm2`):

    cd recomp/private/smm2
    export LARECOMP_HIDDEN=1 SMM2_SEED=4 RECOMP_LOG_TIME=1
    export RECOMP_MOD_100MARIO=1 RECOMP_MOD_RULESET=hundred RECOMP_MOD_LOG=1 RECOMP_MOD_UNLOCK_ALL=1
    export RECOMP_MOD_SERVER=http://127.0.0.1:8650
    export RECOMP_MOD_TOKEN="$(sed -n 's/^token //p' /Volumes/SSD/smm-work/recomp-mods/account-test.txt)"
    export RECOMP_MOD_AUTOPLAY=jump RECOMP_MOD_AUTOPLAY_SKIP=25
    export LARECOMP_INPUT="2500:L:20,2500:R:20,2800:RIGHT:8,2900:A:8,3300:DOWN:8,3400:A:8,4600:DOWN:8,4700:RIGHT:8,4800:A:8"
    ../../build/smm2/smm2recomp extracted > run.log 2>&1

- `LARECOMP_INPUT` (`frame:BUTTON:frames`, presented frames from boot) goes from the title to the main menu, to
  Course World, then to Endless Challenge (frame 4800: DOWN, RIGHT, A).
  It is timed for `SMM2_SEED=4` and the save in `/Volumes/SSD/smm-work/recomp-mods/private/smm2/save` (Course
  World seen before, no first-visit intro): copy that `save/` over yours if the script lands elsewhere.
  `LARECOMP_HOST_INPUT=<frame>:shot:<name>,...` saves window shots into `private/smm2/build/`.
- Entering the Endless menu under `ruleset=hundred` calls `hundred_flow_open()`: the flow takes over from there
  (it calls `hundred_run_start()`, `hundred_play_next()` ...). `RECOMP_MOD_FLOW_TEST=<0..3>` instead plays the run
  without the flow (the stand-in in `bridge.c`).
- `RECOMP_MOD_AUTOPLAY=jump` plays each course (right + run + jump) and skips it with the test input after
  `RECOMP_MOD_AUTOPLAY_SKIP` seconds; the result screen's Next is pressed by the run side after a clear.
  `RECOMP_MOD_UNLOCK_ALL=1` opens Expert and Super Expert without winning the runs before.
- A Super Expert run (6 clears) takes about 6-12 minutes of game time; Easy (8) about 10. Expected log lines
  (`grep '\[mod\]' run.log`): `New Game: super_expert, hundred rules, 100 lives`, `course start <code>`,
  `clear <code> ... clears n`, `... -- 100 Mario Challenge complete!`, `bridge: run finished`.
- After the run: `curl -s http://127.0.0.1:8650/api/rankings/hundred_super_expert` lists the completion.

### Limits

- Not online yet: Network Play (plan: `DECISIONS.md` "Online multiplayer"), Super Worlds, Ninji speedruns,
  thumbnails, follows, notifications; they answer "not connected" or empty.
- The Maker avatar is the console's default Mii with your account's name (creating one needs the Mii editor applet,
  which the runtime doesn't have).
- No SMM2 Mii-costume rewards (the server sends no notifications).
