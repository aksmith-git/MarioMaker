// SMM1's 100 Mario Challenge ending (ending.h).
//
// Sequence [inf, pending research/specs/smm1-ending.md]: the Thank-you balloon Play_EndingText_00 (In, Wait, Out),
// then the staff credits Nwk_StaffCredit_0x (InOut, 1420 frames) listing the run's courses under "Course Design",
// ending on T_END_00. A skips ahead.
#include "ending.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

__attribute__((weak)) void hundred_costume_reward(int difficulty) { (void)difficulty; }

struct Ending {
    int d, tick, credits_start;
    bool done;
    Lay *text, *credits;
    WorldMap *map;  // (stand-in background until the rescue scene is in)
};

#define TEXT_HOLD 240

Ending *ending_new(int d, int lives, int n, const struct hundred_credit *c) {
    Ending *e = calloc(1, sizeof *e);
    e->d = d < 0 ? 0 : d > 3 ? 3 : d;
    e->text = lay_open("Play_EndingText_00");
    e->map = wm_new(e->d);
    if (e->map) {
        lay_hold_end(e->map->l, "MapIn");
        lay_hold_end(e->map->l, "AnimB");
        wm_set_cleared(e->map, e->map->count);
        wm_set_counters(e->map, lives, e->map->count);
        wm_place_mario(e->map, e->map->count - 1);
    }
    int out_at = 20;
    if (e->text) {
        static const char *label[4] = {"T_ThankYou_10", "T_ThankYou_20", "T_ThankYou_30", "T_ThankYou_30"};
        lay_text_from(e->text, "T_ThankYou_00", label[e->d]);
        lay_play(e->text, "In", 20);
        int wait_at = 20 + (int)lay_length(e->text, "In");
        lay_play_loop(e->text, "Wait", wait_at);
        out_at = wait_at + TEXT_HOLD;
        lay_play(e->text, "Out", out_at);
        out_at += lay_ticks(e->text, "Out", 1);
    }
    e->credits_start = out_at + 20;
    // 01: 8 entries + "TO BE CONTINUED" (Easy), 00: 16 entries (Normal, Expert), 03: 6 entries + "THE END" [inf]
    static const char *which[4] = {"Nwk_StaffCredit_01", "Nwk_StaffCredit_00", "Nwk_StaffCredit_00", "Nwk_StaffCredit_03"};
    e->credits = lay_open(which[e->d]);
    if (e->credits) {
        lay_play(e->credits, "InOut", e->credits_start);
        for (int i = 0; i < 16; i++) {
            char part[64], p[96];
            snprintf(part, sizeof part, "L_MiiInfo_%02d", i);
            if (lyt_pane_index(e->credits->res, part) < 0) continue;
            if (i < n) {
                snprintf(p, sizeof p, "%s/T_CourseName_00", part), lay_text(e->credits, p, c[i].name);
                snprintf(p, sizeof p, "%s/T_MiiName_00", part), lay_text(e->credits, p, c[i].maker);
                snprintf(p, sizeof p, "%s/L_NationalFlag_00", part), lay_show(e->credits, p, false);
                snprintf(p, sizeof p, "%s/P_Frame_00", part), lay_show(e->credits, p, false);
            } else
                lay_show(e->credits, part, false);
        }
    }
    return e;
}
void ending_free(Ending *e) {
    if (!e) return;
    lay_free(e->text), lay_free(e->credits), wm_free(e->map);
    free(e);
}
int ending_step(Ending *e, const UiIn *in) {
    e->tick++;
    if (e->done) return EV_NONE;
    bool a = in->pressed & (BT_A | BT_B);
    if (e->tick < e->credits_start) {
        if (a && e->tick > 30) {
            e->credits_start = e->tick;
            if (e->credits) lay_play(e->credits, "InOut", e->tick);
        }
        return EV_NONE;
    }
    int len = e->credits ? lay_ticks(e->credits, "InOut", 1) : 1;
    if (e->tick >= e->credits_start + len || (a && e->tick > e->credits_start + 30)) {
        e->done = true;
        hundred_costume_reward(e->d);
        return EV_DONE;
    }
    return EV_NONE;
}
void ending_draw(Ending *e, LQuads *out) {
    if (e->tick >= e->credits_start && e->credits) {
        lay_draw(e->credits, e->tick, out);
        return;
    }
    if (e->map) lay_draw(e->map->l, e->tick, out);
    if (e->text) lay_draw(e->text, e->tick, out);
}
void ending_background(Ending *e, uint8_t rgba[4]) {
    (void)e;
    rgba[0] = rgba[1] = rgba[2] = 0, rgba[3] = 255;
}
