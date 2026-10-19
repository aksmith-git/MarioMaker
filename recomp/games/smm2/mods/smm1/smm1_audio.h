// SMM1's sound for the host-drawn 100 Mario screens (smm1_audio.c): its music streams (BFSTM) and sound effects
// (BFWAV in BARS archives) from the player's SMM1 dump, mixed into the game's audio output (runtime/services/audio.c,
// game_audio_mix). While an SMM1 screen covers the game, the game's own sound is faded out.
#pragma once
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

typedef struct {
    int rate, channels, frames;  // frames: samples per channel
    int16_t *pcm;                // interleaved
    bool loop;
    int loop_start;
} SPcm;
SPcm *smm1_bfstm_decode(const uint8_t *d, size_t n);
SPcm *smm1_bfwav_decode(const uint8_t *d, size_t n);

void smm1_bgm(const char *name, bool loop);  // a stream of Sound/Resource/Stream (cached), from its start; NULL stops
void smm1_bgm_fade(int frames);  // fade the music out over this many 60 Hz frames
void smm1_bgm_track(int track);  // a multi-track stream's track (0-based: Course World's map values 2 / 3 / 6 -> 1 / 2 / 5)
double smm1_bgm_pos(void);       // the music's position in its own samples (-1: none)
const char *smm1_bgm_name(void);
void smm1_se_bank(const char *bank);          // load a sound bank ("NetWorldMapScene", "UI_Play_CourseIn100Mario_00")
void smm1_se(const char *name);              // a sound effect of a loaded bank, by name
void smm1_game_sound(bool on);               // the game's own sound under the screens (faded)
