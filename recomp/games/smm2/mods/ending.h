// The 100 Mario Challenge's win: the interface between the run (endless.c) and SMM1's ending scene
// (games/smm2/mods/ending/, drawn host-side from the player's own SMM1 dump).
//
// When a run reaches its goal (Easy 8, Normal 16, Expert 16, Super Expert 6 clears), endless.c calls
// hundred_ending_show() once, then waits until hundred_ending_done() is true before the game goes back to the menu.
// Easy shows Toad ("another castle", TO BE CONTINUED); Normal, Expert and Super Expert show Peach, the rescue text and
// the credits (each cleared course's name and maker), then THE END.
#pragma once
#include <stdbool.h>

struct hundred_credit {
    char name[66];   // course name, UTF-8
    char maker[22];  // maker name, UTF-8
};

// difficulty 0 Easy .. 3 Super Expert; n credits (the run's cleared courses in order). Called on the network worker
// thread; the scene draws itself (the game keeps running underneath, waiting on this thread).
void hundred_ending_show(int difficulty, int n, const struct hundred_credit *credits);
bool hundred_ending_done(void);
