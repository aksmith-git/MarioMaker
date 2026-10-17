// C interface to the Rust engine core (core/crates/ffi/src/lib.rs). Keep in sync with it;
// struct sizes are checked by the Rust test `layout_matches_header`.
#pragma once
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

typedef struct MmkAssets MmkAssets;
typedef struct MmkEngine MmkEngine;
typedef struct MmkAudio MmkAudio;
typedef struct MmkChallenge MmkChallenge;

enum {
    MMK_BTN_LEFT = 1, MMK_BTN_RIGHT = 2, MMK_BTN_UP = 4, MMK_BTN_DOWN = 8,
    MMK_BTN_JUMP = 16, MMK_BTN_RUN = 32, MMK_BTN_START = 64,
};

typedef struct { uint32_t buttons; } MmkInput;                       // 4 bytes

typedef struct {
    const char *model;
    const char *name;
    uint32_t width, height;
    uint8_t format;             // 1 rgba8, 2 bc1, 3 bc2, 4 bc3, 5 bc4, 6 bc5, 7 bc6h, 8 bc7
    uint8_t variant;            // 0 unorm, 1 snorm, 2 srgb
    uint8_t channel_select[4];  // 0 zero, 1 one, 2 R, 3 G, 4 B, 5 A
    const uint8_t *data;
    size_t len;
} MmkTextureInfo;                                                      // 48 bytes

#define MMK_TEXTURE_NONE 0xFFFFFFFFu   // MmkSprite.texture when the art is not in the cache

typedef struct {
    uint32_t texture;           // texture index, or MMK_TEXTURE_NONE
    float x, y, w, h;           // bottom-left in world px (y up), size px
    float u0, v0, u1, v1;       // texture coords, (0,0) top-left
    uint32_t layer;
    uint32_t flags;             // bit 0: flip x
} MmkSprite;                                                           // 44 bytes

typedef struct {
    const MmkSprite *sprites;
    size_t sprite_count;
    float camera_x, camera_y, view_w, view_h;
    uint64_t tick;
} MmkSnapshot;                                                         // 40 bytes

typedef struct {
    uint32_t score, coins, lives;
    int32_t time;               // seconds left, -1 = no timer
    uint32_t flags;             // bit 0: course cleared, bit 1: player dying
    uint32_t events;            // bit n: event n this tick (see mmk_sim::Event)
    uint32_t lives_gained;      // extra lives earned since load (monotonic; 100 Mario banks them)
} MmkHud;                                                              // 28 bytes

typedef struct {
    char name[128];             // UTF-8, NUL-terminated
    char description[320];      // UTF-8, NUL-terminated
    char style[4];              // "M1", "M3", "MW", "WU", "3W"
    uint8_t theme, sub_theme;   // 0 ground … 9 forest (docs/formats/smm2-course.md)
    uint8_t night;              // bit 0 main area, bit 1 sub area
    uint8_t playable;           // 1 = the engine can play it
    int16_t timer;              // seconds, 0 = none
    uint8_t encrypted;          // input was a .bcd
    uint8_t clear_condition_category;
    uint32_t clear_condition;
    uint32_t object_count;
    uint32_t unsupported;       // objects the engine can't run yet (0 until the sim reports them)
    uint64_t content_hash;      // FNV-1a 64 of the decoded course data (save-data key)
} MmkCourseInfo;                                                       // 480 bytes

// stage is a short name ("identify", "keys", "tables", "textures"); called on the import thread
typedef void (*MmkImportProgress)(void *user, const char *stage, size_t done, size_t total);

const char *mmk_last_error(void);

MmkAssets *mmk_assets_open(const char *path);
MmkAssets *mmk_assets_placeholder(void);   // no textures, no keys: run without a game dump
void mmk_assets_free(MmkAssets *assets);
size_t mmk_assets_texture_count(const MmkAssets *assets);
bool mmk_assets_texture(const MmkAssets *assets, size_t index, MmkTextureInfo *out);

MmkEngine *mmk_engine_new(const MmkAssets *assets);
void mmk_engine_free(MmkEngine *engine);
bool mmk_engine_load_course(MmkEngine *engine, const uint8_t *data, size_t len);
void mmk_engine_step(MmkEngine *engine, const MmkInput *input);
void mmk_engine_snapshot(MmkEngine *engine, MmkSnapshot *out);
void mmk_engine_hud(const MmkEngine *engine, MmkHud *out);

typedef struct {
    uint32_t kind;              // 0 none, 1 defeat N (target = enemy class), 2 N 1-Ups, 3 N coins
                                // (target = coin value, 0 any), 4 N POWs, 5 N P Switches, 6 goal
                                // invincible, 7 as Super, 8 as Fire, 9 no damage, 10 no landing,
                                // 11 in a Goomba's Shoe, 12 in a Koopa Clown Car,
                                // 255 not supported
    uint32_t target, count, needed;
    uint8_t met, violated, _pad[2];
} MmkCondition;                                                        // 20 bytes
bool mmk_engine_clear_condition(const MmkEngine *engine, MmkCondition *out);   // false: none
// Background clear colour 0xRRGGBBAA (theme sky; black when the theme has no background art)
uint32_t mmk_engine_clear_color(const MmkEngine *engine);
// View flags: bit 0 = draw upside down (underground night)
uint32_t mmk_engine_view_flags(const MmkEngine *engine);
void mmk_engine_restart(MmkEngine *engine);
void mmk_engine_debug_teleport(MmkEngine *engine, float x, float y);

// assets may be NULL (then encrypted .bcd files fail). Thread-safe.
bool mmk_course_info(const MmkAssets *assets, const uint8_t *data, size_t len, MmkCourseInfo *out);
// SMM2 course code "XXX-XXX-XXX" of a server data id; returns its length (cap 0 sizes it)
size_t mmk_smm2_course_code(uint32_t data_id, char *buf, size_t cap);

// Blocks until done; run off the main thread. update_* may be NULL. Writes <out_root>/smm2-<build>/.
bool mmk_import_run(const char *romfs, const char *exefs, const char *update_romfs,
                    const char *update_exefs, const char *out_root,
                    MmkImportProgress progress, void *user);
// Audio: a mixer fed by an attached engine, rendered on the host's audio thread.
enum { MMK_AUDIO_RATE = 48000 };
MmkAudio *mmk_audio_open(const char *path);        // asset cache root; NULL if it has no audio
void mmk_audio_free(MmkAudio *audio);              // stop the audio thread first
uint32_t mmk_audio_sample_rate(void);
// interleaved stereo float (L, R), 2 * frames values; audio thread, no allocation
void mmk_audio_render(const MmkAudio *audio, float *out, size_t frames);
void mmk_audio_set_volumes(const MmkAudio *audio, float master, float music, float sfx);
void mmk_engine_set_audio(MmkEngine *engine, const MmkAudio *audio);   // NULL detaches

// 100 Mario Challenge session (core/crates/ffi/src/challenge.rs). Difficulty 0 Easy, 1 Normal,
// 2 Expert, 3 Super Expert.
typedef struct {
    char id[1024];              // as given to mmk_challenge_add_course (the app: file path)
    char name[128];
    char maker[128];
    uint8_t difficulty;         // 0–100 fail rate
    uint8_t _pad[7];
} MmkRunCourse;                                                        // 1288 bytes

typedef struct {
    uint32_t course_number, total, lives, coins, score, banked, deaths, skips;
} MmkRunStatus;                                                        // 32 bytes

enum { MMK_OUTCOME_CLEARED = 0, MMK_OUTCOME_DIED = 1, MMK_OUTCOME_SKIPPED = 2, MMK_OUTCOME_SAVE_QUIT = 3 };
enum { MMK_NEXT_PLAY = 0, MMK_NEXT_RETRY = 1, MMK_NEXT_GAME_OVER = 2, MMK_NEXT_COMPLETE = 3, MMK_NEXT_QUIT = 4 };

MmkChallenge *mmk_challenge_new(const char *save_json, uint64_t seed);   // save_json may be NULL
void mmk_challenge_free(MmkChallenge *c);
void mmk_challenge_add_course(MmkChallenge *c, const char *id, const char *name, const char *maker,
                              uint8_t difficulty, bool castle);
uint32_t mmk_challenge_available(const MmkChallenge *c, uint8_t difficulty);
bool mmk_challenge_unlocked(const MmkChallenge *c, uint8_t difficulty);
bool mmk_challenge_can_continue(const MmkChallenge *c, uint8_t difficulty);
uint32_t mmk_challenge_clears(const MmkChallenge *c, uint8_t difficulty);
bool mmk_challenge_start(MmkChallenge *c, uint8_t difficulty, bool resume);
bool mmk_challenge_course(const MmkChallenge *c, MmkRunCourse *out);
void mmk_challenge_course_started(MmkChallenge *c);
bool mmk_challenge_unloadable(MmkChallenge *c);
void mmk_challenge_extra_lives(MmkChallenge *c, uint32_t n);
uint8_t mmk_challenge_finish(MmkChallenge *c, uint8_t outcome, uint32_t score, uint32_t coins);
void mmk_challenge_status(const MmkChallenge *c, MmkRunStatus *out);
size_t mmk_challenge_cleared_count(const MmkChallenge *c);
bool mmk_challenge_cleared(const MmkChallenge *c, size_t i, MmkRunCourse *out);
int32_t mmk_challenge_take_unlock(MmkChallenge *c);                  // -1 = none
void mmk_challenge_end(MmkChallenge *c);
size_t mmk_challenge_save_json(const MmkChallenge *c, char *buf, size_t cap);  // length; cap 0 = size it
void mmk_engine_set_run_counters(MmkEngine *engine, uint32_t score, uint32_t coins, uint32_t lives);

// --- layout player (core/crates/ffi/src/ui.rs) -------------------------------------------------
// Draws SMM1 layouts imported by `mmk import-smm1` (BFLYT/BFLAN → quads). Layout units: origin
// at the centre, y up (1280×720 for full-screen layouts). Shading per quad: t = combine(tex0, tex1)
// (a missing texture samples as 1), c = mix(black, white, t), out = c * color, blended as flagged.

typedef struct MmkUi MmkUi;
typedef struct MmkUiLayout MmkUiLayout;

typedef struct {
    float pos[4][2];            // corners TL, TR, BL, BR
    float uv0[4][2];            // tex0 coords, (0,0) top-left of the texture
    float uv1[4][2];            // tex1 coords
    uint8_t color[4][4];        // per-vertex RGBA (pane alpha folded in)
    uint8_t black[4];           // material colour for texture value 0
    uint8_t white[4];           // material colour for texture value 1
    uint32_t tex0, tex1;        // mmk_ui_texture index, or MMK_TEXTURE_NONE
    uint32_t flags;             // bits 0-7 wrap s0,t0,s1,t1 (2 bits each: 0 clamp, 1 repeat, 2 mirror)
                                // bit 8 linear filter; 12-15 colour combine, 16-19 alpha combine
                                // (0 tex1, 1 multiply, 2 add, 5 subtract, 15 tex0 only);
                                // 20-23 blend op (0 off, 1 add, 2 sub, 3 rev sub, 4 min, 5 max),
                                // 24-27 src factor, 28-31 dst factor (0 zero, 1 one, 2 dst colour,
                                // 3 1-dst colour, 4 src alpha, 5 1-src alpha, 6 dst alpha,
                                // 7 1-dst alpha, 8 src colour, 9 1-src colour)
} MmkUiQuad;                                                           // 132 bytes

MmkUi *mmk_ui_open(const char *dir);   // folder with manifest.json + ui/ (e.g. .../smm1-v208)
void mmk_ui_free(MmkUi *ui);
size_t mmk_ui_texture_count(const MmkUi *ui);          // grows as layouts are opened
bool mmk_ui_texture(MmkUi *ui, size_t index, MmkTextureInfo *out);

MmkUiLayout *mmk_ui_layout(MmkUi *ui, const char *name);
void mmk_ui_layout_free(MmkUiLayout *layout);
void mmk_ui_layout_size(const MmkUiLayout *layout, float *w, float *h);
// anim = "[PartPane/…/]Name"; binds it (or moves it) to `frame`
bool mmk_ui_play(MmkUiLayout *layout, const char *anim, float frame);
void mmk_ui_stop(MmkUiLayout *layout, const char *anim);   // NULL = all
float mmk_ui_anim_length(MmkUiLayout *layout, const char *anim);   // frames, -1 if unknown
bool mmk_ui_set_text(MmkUiLayout *layout, const char *pane, const char *text);
// valid until the next call on this layout
size_t mmk_ui_draw_list(MmkUiLayout *layout, const MmkUiQuad **out);
typedef struct {
    float translate[3];         // in the parent pane's space, layout units, y up
    float rotate[3];            // degrees
    float scale[2];
    float size[2];
    float alpha;                // 0-255
    uint32_t visible;
} MmkUiPane;                                                           // 48 bytes
// visible: 0 hide, 1 show (over animations), -1 back to the animations
bool mmk_ui_set_visible(MmkUiLayout *layout, const char *pane, int32_t visible);
// pane values with the bound animations applied
bool mmk_ui_pane(const MmkUiLayout *layout, const char *pane, MmkUiPane *out);
// message by label (own table, else a part's); value tags become U+FFFC. Length; 0 = none
size_t mmk_ui_message(const MmkUiLayout *layout, const char *label, char *buf, size_t cap);
