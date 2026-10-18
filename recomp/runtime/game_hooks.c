// Per-game hook points with neutral defaults.
//
// LAS Recomp's renderers, input and file code call into its Link's Awakening features (widescreen camera, mirror
// mode, randomizer ticks, pot glow, save editor...). Those features are the game's and were not forked; their call
// sites stay (so upstream changes still merge), and land here: every hook does nothing and reports "off". A game
// overrides any of them by defining the same function in games/<game>/ (these are weak symbols).
#include <stdbool.h>
#include <stdint.h>

#include "cpu.h"
#include "settings.h"

#define WEAK __attribute__((weak))

// ---- frame / level (renderers): the current level's name, for logs and per-level render-target resets
WEAK const char *game_level_name(void) { return ""; }
WEAK void romfs_file_read_hook(const char *path) { (void)path; }
// a folder of files a game's mods generate, laid over the RomFS when no RECOMP_MOD_DIR is set (runtime/services/fs.c)
WEAK const char *game_romfs_overlay(void) { return NULL; }

// ---- once per frame on the game's main thread (runtime/hle/input.c), and per presented frame (renderers)
WEAK void warp_game_tick(Cpu *c) { (void)c; }
WEAK void give_item_tick(Cpu *c, uint64_t frame) { (void)c, (void)frame; }
WEAK void chamber_game_tick(Cpu *c, uint64_t frame) { (void)c, (void)frame; }
WEAK void digs_game_tick(Cpu *c, uint64_t frame) { (void)c, (void)frame; }
WEAK void qol_tick(void) {}
WEAK void pets_game_tick(Cpu *c) { (void)c; }
WEAK void warp_frame(uint64_t frame) { (void)frame; }
WEAK void save_editor_frame(uint64_t frame) { (void)frame; }

// ---- input (runtime/hle/input.c)
WEAK bool mirror_input(void) { return false; }
WEAK uint64_t pets_menu_input(uint64_t b, int32_t st[4]) { (void)st; return b; }
WEAK void input_note_rstick(int32_t ry) { (void)ry; }
// test scripts' held stick / buttons (frame-limited; 0 = none)
WEAK int32_t g_test_lx, g_test_ly, g_stick_lx, g_stick_ly;
WEAK uint64_t g_test_stick_until, g_test_buttons, g_test_buttons_until, g_stick_freeze_until;

// ---- save data written by the game (runtime/services/fs.c)
WEAK void flags_ext_save_written(const char *path, uint64_t addr, int64_t off, int64_t size) {
    (void)path, (void)addr, (void)off, (void)size;
}

// ---- screenshots requested by game code or tests (renderers: saved after the next present)
WEAK char g_window_shot_request[256];

// ---- widescreen (renderers): the game's own 16:9 output, unchanged
WEAK bool widescreen_allowed(void) { return false; }
WEAK void widescreen_set_factor(float f) { (void)f; }
WEAK void widescreen_frame_end(uint64_t frame) { (void)frame; }
WEAK void widescreen_present_crop(uint64_t frame, double shape, float *crop, double *vx, double *vy, double *vw,
                                  double *vh) {
    (void)frame, (void)shape, (void)vx, (void)vy, (void)vw, (void)vh;
    crop[0] = 0, crop[1] = 0, crop[2] = 1, crop[3] = 1;
}
WEAK void widescreen_backdrop_seen(uint64_t hash) { (void)hash; }
WEAK void widescreen_scene_ubo(const float *u, uint64_t size) { (void)u, (void)size; }
WEAK float widescreen_effect_stretch(uint64_t fs_hash) { (void)fs_hash; return 1.0f; }

// ---- per-draw effects (renderers): none
WEAK bool mirror_draw(bool composite) { (void)composite; return false; }
WEAK bool pets_hide_item_cursor(void) { return false; }
WEAK bool pot_glow_active(void) { return false; }
WEAK bool pot_level_active(void) { return false; }
WEAK void pot_item_draw(float *m) { (void)m; }
WEAK void pot_probe(const float *m, bool tintable, uint64_t mesh, uint32_t inst) {
    (void)m, (void)tintable, (void)mesh, (void)inst;
}
WEAK bool pot_tint(const float *m, float tint[8], uint64_t mesh) { (void)m, (void)tint, (void)mesh; return false; }
WEAK bool pot_instance_gold(const float *m) { (void)m; return false; }
WEAK void pot_gold_tint(float tint[8]) { (void)tint; }
WEAK void escape_bottle_tint(int kind, const float *ui, uint32_t ui_size, float tint[8]) {
    (void)kind, (void)ui, (void)ui_size, (void)tint;
}

// ---- cosmetic colour choices kept in the settings file (none in the shared runtime besides "Default")
WEAK const char *const g_tunic_colours[N_TUNIC_COLOURS] = {"Default"};
WEAK const char *const g_heart_colours[N_HEART_COLOURS] = {"Default"};
