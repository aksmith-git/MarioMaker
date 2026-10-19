// SMM1's 100 Mario Challenge ending (ending.h), as Block.rpx v208 plays it [obs: research/specs/smm1-ending.md]:
//
// 1. NetEndingDemoScene, the rescue (rescue.c): fades in from black (~25 frames [inf]), Mario walks to Peach (Toad on
//    Easy) in the style of the course just cleared; once he has stopped the balloon Play_EndingText_00 shows one line
//    (In 24, Wait looping, Out 300 frames later): T_ThankYou_<row><variant>, row by difficulty (Easy 1, Normal 2,
//    Expert 3, Super Expert 3), variant by the costume Mario wore at the goal (0 none). Then the scene fades to black.
// 2. NetEndingScene: on black, after its fade-in, the staff credits play once at speed 1 - Nwk_StaffCredit_00 (16
//    rows, InOut 1420 frames, "END") for Normal and Expert, _01 (8 rows, 1020, "TO BE CONTINUED...") for Easy, _03
//    (6 rows, 1020, "THE END") for Super Expert - listing each cleared course (name, maker) under "Course Design".
//    180 frames after the credits the Mystery Mushroom costume window opens (the costumes mod: hundred_costume_reward).
// Neither scene reads the controller: nothing skips [obs].
#include "ending.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "rescue.h"
#include "smm1_audio.h"

__attribute__((weak)) void hundred_costume_reward(int difficulty) { (void)difficulty; }
__attribute__((weak)) int hundred_ending_costume(void) { return -1; }  // costume id at the goal (-1 none)

#define FADE 25  // the default fader: a 100-frame animation at speed 4 [inf]

struct Ending {
    int d, tick, rescue_len, credits_start, credits_len, end, text_in, text_out, style;
    bool done;
    Rescue *rescue;
    Lay *text, *credits;
};

static int costume_variant(int id) {  // [obs 0x024e8c84]
    if (id < 0 || id == 0) return 0;  // none, Mario
    if (id == 1) return 1;            // Peach
    if (id == 6) return 2;            // Toad
    if (id == 3) return 3;            // Bowser
    return 4;
}

Ending *ending_new(int d, int lives, int n, const struct hundred_credit *c) {
    (void)lives;
    return ending_new_style(d, n, c, 0, 0);
}

Ending *ending_new_style(int d, int n, const struct hundred_credit *c, int style, int form) {
    Ending *e = calloc(1, sizeof *e);
    e->d = d < 0 ? 0 : d > 3 ? 3 : d;
    e->style = style == 4 ? 3 : style < 0 || style > 3 ? 0 : style;  // (SM3DW has no SMM1 scene: NSMBU's, the 3-D one [inf])
    e->rescue = rescue_new(e->style, e->d != 0, form);
    e->rescue_len = rescue_length(e->rescue);
    e->text = lay_open("Play_EndingText_00");
    if (e->text) {
        char label[32];
        snprintf(label, sizeof label, "T_ThankYou_%d%d", e->d == 0 ? 1 : e->d == 1 ? 2 : 3, costume_variant(hundred_ending_costume()));
        lay_text_from(e->text, "T_ThankYou_00", label);
    }
    e->text_in = rescue_text_in(e->rescue), e->text_out = rescue_text_out(e->rescue);
    static const char *which[4] = {"Nwk_StaffCredit_01", "Nwk_StaffCredit_00", "Nwk_StaffCredit_00", "Nwk_StaffCredit_03"};
    e->credits = lay_open(which[e->d]);
    e->credits_start = e->rescue_len + FADE + FADE;
    if (e->credits) {
        lay_hold(e->credits, "InOut", 0);
        e->credits_len = lay_ticks(e->credits, "InOut", 1);
        for (int i = 0; i < 16; i++) {
            char part[32], p[96];
            snprintf(part, sizeof part, "L_MiiInfo_%02d", i);
            if (lyt_pane_index(e->credits->res, part) < 0) continue;
            if (i >= n) { lay_show(e->credits, part, false); continue; }
            snprintf(p, sizeof p, "%s/T_CourseName_00", part), lay_text(e->credits, p, c[i].name);
            snprintf(p, sizeof p, "%s/T_MiiName_00", part), lay_text(e->credits, p, c[i].maker);
            // no SMM1 country code for our makers: as SMM1 does for codes >= 195, no flag frame and no flag
            snprintf(p, sizeof p, "%s/P_Frame_00", part), lay_show(e->credits, p, false);
            snprintf(p, sizeof p, "%s/L_NationalFlag_00", part), lay_show(e->credits, p, false);
        }
    }
    e->end = e->credits_start + e->credits_len + 180;
    return e;
}
void ending_free(Ending *e) {
    if (!e) return;
    rescue_free(e->rescue);
    lay_free(e->text), lay_free(e->credits);
    free(e);
}
int ending_length(const Ending *e) { return e ? e->end : 0; }
int ending_step(Ending *e, const UiIn *in) {
    (void)in;  // nothing skips [obs]
    e->tick++;
    if (e->done) return EV_NONE;
    if (e->text) {  // the balloon: In when Mario has stopped, then Wait (looping) until Out [obs 0x024e8c84]
        if (e->tick == e->text_in) lay_play(e->text, "In", e->tick);
        if (e->tick == e->text_in + (int)lay_length(e->text, "In")) lay_stop(e->text, "In"), lay_play_loop(e->text, "Wait", e->tick);
        if (e->tick == e->text_out) lay_stop(e->text, "Wait"), lay_play(e->text, "Out", e->tick);
    }
    if (e->tick == 1) {  // EndingPeachBGM / EndingKinopioBGM by style [obs data: SLink2DB]
        static const char *peach[4] = {"M1_BGM_Ending", "M3_BGM_EndingDemo2", "MW_BGM_Ending2", "WU_STRM_BGM_LAST_BOSS2_HARP"};
        static const char *toad[4] = {"M1_BGM_Ending", "M3_BGM_EndingDemo1", NULL, "WU_STRM_BGM_LAST_BOSS_HARP_Dummy"};
        const char *s = e->d ? peach[e->style] : toad[e->style];  // (SMW's Toad tune is a sound-bank wave: not yet)
        smm1_bgm(s, false);
    }
    if (e->tick == e->rescue_len) smm1_bgm_fade(FADE);
    if (e->tick == e->rescue_len + FADE)  // the staff roll: Easy and Super Expert "easy", Normal and Expert "normal"
        smm1_bgm(e->d == 0 || e->d == 3 ? "BGM_100mario_staffroll_easy" : "BGM_100mario_staffroll_normal", false);
    if (e->tick == e->credits_start && e->credits) lay_play(e->credits, "InOut", e->tick);
    if (e->tick >= e->end) {
        e->done = true;
        hundred_costume_reward(e->d);
        return EV_DONE;
    }
    return EV_NONE;
}

static void fade_quad(LQuads *out, float a) {  // black over everything, alpha a
    if (a <= 0) return;
    LQuad q;
    memset(&q, 0, sizeof q);
    float c[4][2] = {{-640, 360}, {640, 360}, {-640, -360}, {640, -360}};
    memcpy(q.pos, c, sizeof c);
    for (int k = 0; k < 4; k++) q.col[k][0] = q.col[k][1] = q.col[k][2] = 255, q.col[k][3] = (uint8_t)(a > 1 ? 255 : a * 255);
    memset(q.black, 0, 4);
    q.white[3] = 255;
    q.tex0 = q.tex1 = -1;
    q.flags = LQ_BLEND_NORMAL | LQ_COMB_NONE << LQ_COMB_C | LQ_COMB_NONE << LQ_COMB_A;
    if (out->n == out->cap) out->cap = out->cap ? out->cap * 2 : 256, out->q = realloc(out->q, sizeof(LQuad) * (size_t)out->cap);
    out->q[out->n++] = q;
}

static uint8_t g_bg[4] = {0, 0, 0, 255};
void ending_draw(Ending *e, LQuads *out) {
    int t = e->tick;
    if (t < e->rescue_len + FADE) {
        rescue_draw(e->rescue, t, out, g_bg);
        if (e->text && t >= e->text_in && t < e->text_out + lay_ticks(e->text, "Out", 1)) lay_draw(e->text, t, out);
        if (t < FADE) fade_quad(out, 1 - (float)t / FADE);
        if (t >= e->rescue_len) fade_quad(out, (float)(t - e->rescue_len) / FADE);
        return;
    }
    g_bg[0] = g_bg[1] = g_bg[2] = 0, g_bg[3] = 255;
    if (e->credits) lay_draw(e->credits, t, out);
    int cf = t - (e->rescue_len + FADE);
    if (cf < FADE) fade_quad(out, 1 - (float)cf / FADE);
}
void ending_background(Ending *e, uint8_t rgba[4]) {
    (void)e;
    memcpy(rgba, g_bg, 4);
}
