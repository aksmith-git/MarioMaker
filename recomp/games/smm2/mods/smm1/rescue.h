// SMM1's rescue scene, NetEndingDemoScene (rescue.c): Mario walks in from the left to Peach (or Toad on Easy)
// standing on the style's ground model, drawn from the SMM1 dump's models (research/specs/smm1-ending.md §2).
#pragma once
#include "lyt.h"

typedef struct Rescue Rescue;
// style: 0 SMB1, 1 SMB3, 2 SMW, 3 NSMBU (the cleared course's); peach: Normal and up (else Toad);
// form: Mario's power-up at the goal (0 small, 1 big)
Rescue *rescue_new(int style, bool peach, int form);
void rescue_free(Rescue *r);
int rescue_length(const Rescue *r);    // frames until the scene asks for the credits (818; 998 NSMBU with Peach)
int rescue_text_in(const Rescue *r);   // the frame the balloon text comes in (Mario has stopped)
int rescue_text_out(const Rescue *r);  // the frame its Out starts
// the scene at frame t (0 = its start) in layout units (1280 x 720), with the colour behind it
void rescue_draw(Rescue *r, int t, LQuads *out, uint8_t bg[4]);
