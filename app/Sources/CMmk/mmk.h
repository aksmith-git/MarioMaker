// C interface to the Rust engine core (core/crates/ffi/src/lib.rs). Keep in sync with it;
// struct sizes are checked by the Rust test `layout_matches_header`.
#pragma once
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

typedef struct MmkAssets MmkAssets;
typedef struct MmkEngine MmkEngine;
typedef struct MmkAudio MmkAudio;

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
} MmkHud;                                                              // 24 bytes

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
// Background clear colour 0xRRGGBBAA (theme sky; black when the theme has no background art)
uint32_t mmk_engine_clear_color(const MmkEngine *engine);
void mmk_engine_restart(MmkEngine *engine);
void mmk_engine_debug_teleport(MmkEngine *engine, float x, float y);

// assets may be NULL (then encrypted .bcd files fail). Thread-safe.
bool mmk_course_info(const MmkAssets *assets, const uint8_t *data, size_t len, MmkCourseInfo *out);

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
