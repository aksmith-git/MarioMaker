// The 100 Mario Challenge as in SMM1: the bridge between the host-drawn SMM1 screens ("the flow",
// games/smm2/mods/smm1/flow.c) and the 100 Mario mod (games/smm2/mods/*.c: our server, the run's rules, SMM2's
// course play).
//
// The flow shows SMM1's screens (difficulty select, world map, start card, pause, clear, game over, ending) over or
// instead of the game's frame and sends the player's choices; the mod owns the run (lives, the course list, saving)
// and plays each course in SMM2's own play scene. Each side defines its functions strongly and has weak stubs of
// the other's, so either builds alone. Everything is called on the game's threads except where noted; the flow
// queues what it is told and acts on it at its next tick (60 Hz, the presented frames).
#pragma once
#include <stdbool.h>

struct hundred_credit {
    char name[66];   // course name, UTF-8
    char maker[22];  // maker name, UTF-8
};

struct hundred_status {
    int difficulty;  // 0..3 Easy, Normal, Expert, Super Expert; -1 no run
    int lives, cleared, total;  // total: 8, 16, 16, 6 (SMM1's run lengths)
    bool unlocked[4], can_continue[4];
    int continue_lives[4], continue_cleared[4];
    struct hundred_credit current;  // the course about to be / being played
    char current_code[12];          // its course code ("" none)
    int style;                      // the current (or last) course's game style: 0 SMB1, 1 SMB3, 2 SMW, 3 NSMBU, 4 SM3DW
    int ncredits;                   // the courses cleared so far in this run, in order
    struct hundred_credit credits[16];
    int form;   // Mario's power-up at the last goal: 0 small, 1 big (the ending's Mario)
    bool loading;  // the server is still gathering this difficulty's courses (TGRCode: up to a minute): show it
};

enum {
    HUNDRED_CLEAR,        // reached the goal
    HUNDRED_MISS,         // lost a life (the mod has taken it off; status.lives says what is left)
    HUNDRED_SKIP,         // the skip finished (SMM1: free, the next course)
    HUNDRED_GIVEUP_SAVE,  // Save & Quit done
    HUNDRED_ERROR,        // the course could not be fetched or loaded
};

// ---- the flow's (mods/smm1/flow.c), called by the mod
void hundred_flow_open(void);            // the player chose 100 Mario (Course World's Endless Challenge, ruleset hundred)
bool hundred_flow_active(void);          // an SMM1 full screen is up: the game's frame is hidden, input goes to the flow
bool hundred_flow_overlay(void);         // an SMM1 overlay is up over the course (pause, skip, clear, card): input to it
void hundred_flow_pause(void);           // the player paused during a run (the mod keeps SMM2's own pause from opening)
void hundred_flow_course_started(void);  // the course is on screen and playing (the start card may go)
void hundred_course_result(int outcome);

// ---- the mod's, called by the flow
bool hundred_run_status(struct hundred_status *out);        // any time (unlocks / continue data with no run)
bool hundred_run_start(int difficulty, bool resume_saved);  // false: not enough courses or a server error
bool hundred_play_next(void);  // starts fetching the next course and loading it into SMM2's play (asynchronous: an
                               // error comes back as hundred_course_result(HUNDRED_ERROR))
void hundred_play_resume(void);    // close the pause: back to the course
void hundred_play_retry(void);     // the same course again (after a miss with lives left, or Start Over)
void hundred_play_skip(void);      // SMM1's skip (held button): end this course, the next one comes
void hundred_run_save_quit(void);  // Save & Quit: keep the continue data, leave the course
void hundred_run_finish(void);     // the run is over (won, or game over): clear its continue data
void hundred_flow_exit(void);      // B on the difficulty select: back to SMM2's Course World

// ---- the earlier ending-only interface (kept for the mod's first calls): the ending alone, with these credits
void hundred_ending_show(int difficulty, int n, const struct hundred_credit *credits);
bool hundred_ending_done(void);
