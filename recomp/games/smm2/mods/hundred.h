// The 100 Mario Challenge bridge: SMM1's screens (the "flow", drawn host-side: games/smm2/mods/ending/ and the SMM1
// screens agent's code) over SMM2's own course play and our server (the "run": games/smm2/mods/bridge.c, endless.c).
//
// The player gets in through Course World's Endless Challenge button: with the mod on and ruleset=hundred the run side
// calls hundred_flow_open() when the game enters its Endless menu, and that menu stays hidden underneath (the run side
// drives it); with ruleset=smm2 the button opens SMM2's own Endless Challenge, unchanged.
//
// Each side keeps weak stand-ins for the other side's functions (bridge.c has the flow's).
#pragma once
#include <stdbool.h>

#include "ending.h"  // struct hundred_credit, hundred_ending_show / _done (thin wrappers on the flow side)

struct hundred_status {
    int difficulty;             // 0..3 Easy..Super Expert, -1 = no run
    int lives, cleared, total;  // total = 8 / 16 / 16 / 6
    bool unlocked[4], can_continue[4];
    int continue_lives[4], continue_cleared[4];
    struct hundred_credit current;  // the course about to be / being played
    char current_code[12];
    int ncredits;
    struct hundred_credit credits[16];  // cleared so far, in order
};

enum { HUNDRED_CLEAR, HUNDRED_MISS, HUNDRED_SKIP, HUNDRED_GIVEUP_SAVE, HUNDRED_ERROR };

// ---- the flow (SMM1 screens), called by the run side
void hundred_flow_open(void);
bool hundred_flow_active(void);   // full-screen SMM1 up: the game frame is hidden, input goes to the flow
bool hundred_flow_overlay(void);  // an in-course SMM1 overlay is up (pause, skip, clear, card after a miss)
void hundred_flow_pause(void);    // the run side stops SMM2's own pause and calls this
void hundred_flow_course_started(void);  // the course has begun (the flow drops its card)
void hundred_course_result(int outcome);

// ---- the run (server, SMM2's play), called by the flow; none of these block on the network or the game
bool hundred_run_status(struct hundred_status *out);        // always callable (unlocks / Continue with no run)
bool hundred_run_start(int difficulty, bool resume_saved);  // false: not signed in / server error / locked
bool hundred_play_next(void);     // async: true once started; HUNDRED_ERROR via hundred_course_result on failure
void hundred_play_resume(void);   // un-pause SMM2's play
void hundred_play_retry(void);    // after a miss with lives left: the same course again (SMM2 retries by itself)
void hundred_play_skip(void);     // SMM1's skip (free)
void hundred_run_save_quit(void); // keep the Continue data, back to the flow's map
void hundred_run_finish(void);    // the run is over (win or game over): the server finishes it
void hundred_flow_exit(void);     // B on the difficulty select: back to SMM2's Course World
