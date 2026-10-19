// SMM1's 100 Mario Challenge ending (ending.c): the rescue scene, the Thank-you text and the staff credits listing the
// run's courses, as Block.rpx v208 plays them (research/specs/smm1-ending.md).
#pragma once
#include "screens.h"

typedef struct Ending Ending;
Ending *ending_new(int difficulty, int lives, int n, const struct hundred_credit *credits);
// style: the cleared course's game style (0 SMB1 .. 3 NSMBU, 4 SM3DW shows NSMBU's), form: Mario's power-up (0 small, 1 big)
Ending *ending_new_style(int difficulty, int n, const struct hundred_credit *credits, int style, int form);
void ending_free(Ending *e);
int ending_step(Ending *e, const UiIn *in);  // EV_DONE when over
int ending_length(const Ending *e);          // ticks until EV_DONE
void ending_draw(Ending *e, LQuads *out);
// what the ending wants behind its layouts this frame: a background colour (RGBA) the frame is cleared to
void ending_background(Ending *e, uint8_t rgba[4]);
// Hook for the Mystery Mushroom reward that follows SMM1's ending (the costumes mod, later): called once when the
// credits are over, before the flow closes. Weak: does nothing.
void hundred_costume_reward(int difficulty);
