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
    MMK_BTN_JUMP = 16, MMK_BTN_RUN = 32, MMK_BTN_START = 64, MMK_BTN_SPIN = 128,
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
// RGBA8 copy (width*height*4 bytes, channel select not applied) of a texture from
// mmk_assets_texture / mmk_ui_texture, for GPUs without BC support. False for BC6H.
bool mmk_texture_decode_rgba(const MmkTextureInfo *info, uint8_t *out, size_t out_len);

MmkEngine *mmk_engine_new(const MmkAssets *assets);
void mmk_engine_free(MmkEngine *engine);
bool mmk_engine_load_course(MmkEngine *engine, const uint8_t *data, size_t len);
void mmk_engine_step(MmkEngine *engine, const MmkInput *input);
// Local multiplayer: 1–4 players; then step with one input per player (player 1 first)
void mmk_engine_set_players(MmkEngine *engine, uint32_t players);
void mmk_engine_step_players(MmkEngine *engine, const MmkInput *inputs, size_t count);
void mmk_engine_snapshot(MmkEngine *engine, MmkSnapshot *out);
void mmk_engine_hud(const MmkEngine *engine, MmkHud *out);

typedef struct {
    uint32_t kind;              // 0 none, 1 defeat N (target = enemy class), 2 N 1-Ups, 3 N coins
                                // (target = coin value, 0 any), 4 N POWs, 5 N P Switches, 6 goal
                                // invincible, 7 as Super, 8 as Fire, 9 no damage, 10 no landing,
                                // 11 in a Goomba's Shoe, 12 in a Koopa Clown Car, 13 as Raccoon, 14 as Cape, 15 as Propeller, 16 holding a shell, 17 on Yoshi, 18 as Cat, 19 in a Koopa Troopa Car, 20 as Builder, 21 on a Lakitu's Cloud, 22 holding the stone, 23 shake N trees, 24 not held by a claw, 25 no water, 26 wearing a helmet (target 1 Buzzy, 2 Spiny), 27 in a Dry Bones Shell, 28 as Superball Mario, 29 as Big Mario, 29 as Big Mario,
                                // 255 not supported
    uint32_t target, count, needed;
    uint8_t met, violated, _pad[2];
} MmkCondition;                                                        // 20 bytes
bool mmk_engine_clear_condition(const MmkEngine *engine, MmkCondition *out);   // false: none

// What the SMM2 in-course screens show besides MmkHud (core/crates/ffi/src/play_status.rs)
typedef struct {
    uint32_t keys;              // keys held
    uint32_t pink_coins, pink_total;   // collected this attempt / in the course
    uint32_t style;             // 0 SMB1, 1 SMB3, 2 SMW, 3 NSMBU, 4 3D World
    uint32_t character;         // 0 Mario, 1 Luigi, 2 Toad, 3 Toadette
    int32_t time_frames;        // frames left, -1 = no timer
    uint32_t flags;             // bit 0 checkpoint reached, bit 1 dying because time ran out,
                                // bit 2 goal sequence running
    uint32_t area;              // 0 main, 1 sub
    float player_x, player_y;   // player 1 in the view (px from its bottom-left; feet)
    int32_t restart_in;         // frames until the death sequence restarts the course, -1 alive
    uint32_t _pad;
} MmkPlayStatus;                                                       // 48 bytes
void mmk_engine_play_status(const MmkEngine *engine, MmkPlayStatus *out);
// Player 1's character: 0 Mario, 1 Luigi, 2 Toad, 3 Toadette
void mmk_engine_set_character(MmkEngine *engine, uint32_t character);
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

// The open-source course server (core/crates/ffi/src/online.rs; blocking, off the main thread).
// JSON array of listings {code, name, maker, style, theme, plays, clears, likes, difficulty};
// returns its length (cap 0 sizes it), 0 on error (mmk_last_error). sort: new / popular / hard.
size_t mmk_online_search(const char *server, const char *query, const char *sort, uint32_t offset,
                         uint32_t limit, char *buf, size_t cap);
bool mmk_online_download(const char *server, const char *code, const char *path);
bool mmk_online_report(const char *server, const char *code, const char *what); // play / clear / like
// Accounts and the rest (JSON in and out; token NULL = anonymous). action: register {name} →
// {account, token}; me; vote {code, like}; clear {code, time_ms}; comments {code}; comment
// {code, text}; maker {maker_id}; rankings {kind, limit}; endless {difficulty, cleared};
// delete {code}; submit_replay {code, path}; record_replay {code, path}. Returns the result's
// length (cap 0 sizes it), 0 on error (mmk_last_error).
size_t mmk_online_call(const char *server, const char *token, const char *action, const char *args,
                       char *buf, size_t cap);

// SMM1 extras (options are off by default; core/crates/ffi/src/options.rs, costumes.rs).
// Options: smm1_shell_kicks 0/1, smm1_layering 0/1, smb1_mushroom 0 off / 1 mystery / 2 weird,
// costume -1 random or an SMM1 costume id. mmk_engine_option returns INT32_MIN for unknown names.
bool mmk_engine_set_option(MmkEngine *engine, const char *name, int32_t value);
int32_t mmk_engine_option(const MmkEngine *engine, const char *name);
// The SMM1 sprite cache (`<smm1 cache>/sprites`, made by mmk import-smm1): costumes, Lanky Mario.
// Add to the assets before reading the texture count or creating engines; returns textures added.
uint32_t mmk_assets_add_smm1_sprites(MmkAssets *assets, const char *path);
uint32_t mmk_assets_costume_count(const MmkAssets *assets);
int32_t mmk_assets_costume_id(const MmkAssets *assets, uint32_t index);
const char *mmk_assets_costume_name(const MmkAssets *assets, uint32_t index);
bool mmk_audio_add_smm1_sprites(MmkAudio *audio, const char *path);
void mmk_engine_use_costumes(MmkEngine *engine, const MmkAssets *assets);

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
// Where a pane is drawn: corners TL, TR, BL, BR (x, y) in layout units, global alpha 0-1.
typedef struct {
    float corners[8];
    float alpha;
    uint32_t visible;
} MmkUiRect;
bool mmk_ui_pane_rect(const MmkUiLayout *layout, const char *pane, MmkUiRect *out);
// Force values over the animations: flags 1 translate (tx, ty), 2 size (w, h), 4 scale (sx, sy),
// 8 alpha (0-255); flags 0 removes the override.
bool mmk_ui_set_pane(MmkUiLayout *layout, const char *pane, uint32_t flags, float tx, float ty,
                     float w, float h, float sx, float sy, float alpha);
// Copy part pane `part` as `name`, moved by (dx, dy); it follows the original's animations.
bool mmk_ui_add_clone(MmkUiLayout *layout, const char *part, const char *name, float dx, float dy);
// Message `label` of table `table` (a layout's, or a shared SMM2 one: GoalCondition,
// PartsIconName_plural, …), value tags as U+FFFC. Length; 0 = none; cap 0 sizes it.
size_t mmk_ui_table_message(MmkUi *ui, const char *table, const char *label, char *buf, size_t cap);
// Draw picture `pane` with the import's texture ui/textures/<group>/<name>.mtex (name NULL:
// its own texture again), e.g. group "MissionIcon" for the clear-condition icons.
bool mmk_ui_set_picture(MmkUi *ui, MmkUiLayout *layout, const char *pane, const char *group,
                        const char *name);
// Force a picture / window pane's material colour over its animations: which 0 black, 1 white;
// rgba (4 bytes) NULL gives it back to the animations.
bool mmk_ui_set_material_color(MmkUiLayout *layout, const char *pane, uint32_t which, const uint8_t *rgba);
// Width (pane units) of text pane `pane`'s widest line; -1 if there is no such text pane.
float mmk_ui_text_width(const MmkUiLayout *layout, const char *pane);

// --- front end (core/crates/ffi/src/front.rs) --------------------------------------------------
// The SMM2 screens of `mmk-front` (opening, title, Course World, 100 Mario, in-course screens),
// the same ones mmk-play shows. The app creates the engine and hands it over; the front end
// loads, steps and restarts courses on it. Per 60 Hz tick: mmk_front_tick, then draw
// mmk_front_frame (layout quads drawn like mmk_ui_draw_list's, the course view where an item says
// so) and play mmk_front_sounds. Main thread only.

typedef struct MmkFront MmkFront;

typedef struct {
    void *user;
    // value `key` (save data "saves", 100 Mario "hundred", settings): write it NUL-terminated
    // into buf when it fits in cap and return its length; -1 when there is none
    int64_t (*load_value)(void *user, const char *key, char *buf, size_t cap);
    void (*store_value)(void *user, const char *key, const char *value);
    void (*set_paused)(void *user, bool paused);   // music down while paused
} MmkFrontHost;                                                        // 32 bytes

enum { MMK_FRONT_BOOT = 1, MMK_FRONT_INSTANT_FADES = 2, MMK_FRONT_HUNDRED_SMM1 = 4 };
typedef struct {
    const char *smm2_ui;        // SMM2 UI import folder (required)
    const char *smm1_ui;        // SMM1 import folder or NULL
    const char *server;         // course server URL, NULL / "" none
    const char *downloads;      // folder for server courses (NULL: temp)
    const char *opening;        // force an opening (InDefault, InLink, …) or NULL
    const char *script;         // captures: MMK_UI_SCRIPT presses, or NULL
    const char *title_input;    // captures: MMK_UI_INPUT presses, or NULL
    uint32_t flags;             // MMK_FRONT_BOOT | _INSTANT_FADES | _HUNDRED_SMM1
    uint32_t title_start;       // 0 press-a-button, 1 Make / Play, 2 Play menu
    int32_t title_select;       // Play menu button, -1 none
    int32_t title_course;       // title demo course, -1 random
} MmkFrontOptions;                                                     // 72 bytes

typedef struct {
    uint32_t menu;              // held: 1 up, 2 down, 4 confirm, 8 back, 16 left, 32 right
    uint32_t buttons;           // engine buttons (MMK_BTN_*) for the course
    uint32_t flags;             // bit 0 pause held, bit 1 skip held, bit 2 click this tick
    float click_x, click_y;     // layout units (1280x720 view, origin centre, y up)
    uint32_t text;              // typed key while MMK_FRONT_TYPING: a Unicode scalar, 8 erase,
                                // 10 finish, 27 cancel, 0 none (one per tick)
} MmkFrontInput;                                                       // 24 bytes

typedef struct {
    uint32_t kind;              // 0 layout quads, 1 the course view (the whole 16:9 view)
    uint32_t ui;                // 0 SMM2 import's textures, 1 SMM1 import's
    float width, height;        // layout size in layout units (origin centre, y up)
    const MmkUiQuad *quads;
    size_t quad_count;
} MmkFrontItem;                                                        // 32 bytes

typedef struct {
    const char *name;           // <smm2 ui>/ui/sound/<name>.wav
    float volume;
    float delay;                // frames
} MmkFrontSound;                                                       // 16 bytes

enum { MMK_FRONT_COURSE_SHOWN = 1, MMK_FRONT_COURSE_DONE = 2, MMK_FRONT_PLAYING = 4, MMK_FRONT_NO_SCREEN = 8,
       MMK_FRONT_TYPING = 16 };
typedef struct {
    uint32_t flags;             // MMK_FRONT_COURSE_SHOWN | _COURSE_DONE | _PLAYING | _NO_SCREEN | _TYPING
    uint32_t _pad;
    uint64_t engine_tick;
} MmkFrontStatus;                                                      // 16 bytes

// engine / assets (and audio, NULL = silent) must outlive the front end. NULL on error.
MmkFront *mmk_front_new(const MmkFrontOptions *opts, MmkEngine *engine, const MmkAssets *assets,
                        const MmkAudio *audio, MmkFrontHost host);
void mmk_front_free(MmkFront *front);
// library JSON: [{id (path), name, description, style, theme, playable, built_in, collection,
// key, modified, meta: {clear_rate, bucket, maker, code, world_record_ms, plays, likes, clears,
// attempts, uploaded, tags}}]
void mmk_front_set_library(MmkFront *front, const char *json);
void mmk_front_set_server(MmkFront *front, const char *server);
void mmk_front_tick(MmkFront *front, const MmkFrontInput *input);
// valid until the next mmk_front_frame
size_t mmk_front_frame(MmkFront *front, const MmkFrontItem **out);
size_t mmk_front_toast(const MmkFront *front, char *buf, size_t cap);   // 0 = none; cap 0 sizes it
// the UI sounds started since the last call; valid until the next call
size_t mmk_front_sounds(MmkFront *front, const MmkFrontSound **out);
void mmk_front_status(const MmkFront *front, MmkFrontStatus *out);
// textures of import ui (0 SMM2, 1 SMM1); the list grows as screens open layouts
size_t mmk_front_texture_count(const MmkFront *front, uint32_t ui);
bool mmk_front_texture(MmkFront *front, uint32_t ui, size_t index, MmkTextureInfo *out);
// play a course file with the SMM2 screens (name NULL: the course's own); then status
// MMK_FRONT_COURSE_DONE when the player leaves it
bool mmk_front_play_course(MmkFront *front, const char *path, const char *name, bool intro);
// captures (mmk-play --front-screen names): boot, title…, courseworld…, challenge…; course-*
// over mmk_front_play_course; then run ticks at once (course_screen: until the menu has been
// up `ticks` ticks; scripted: the smoke test's input)
bool mmk_front_capture(MmkFront *front, const char *screen, uint32_t difficulty, uint32_t cleared);
bool mmk_front_debug_course(MmkFront *front, const char *screen);
void mmk_front_run_ticks(MmkFront *front, uint64_t ticks, bool course_screen, bool scripted);

// --- meshes (core/crates/ffi/src/mesh.rs) ------------------------------------------------------
// NSMBU / 3D World models from the asset cache (meshes/, anims/), loaded on first use and kept
// with the assets handle (mesh pointers stay valid until mmk_assets_free). Model units = world px,
// y up, z toward the viewer. Vertices are in the bind pose: skin them with mmk_mesh_pose's
// matrices (sum of weight * skin[joint] * position). One render thread.

typedef struct {
    float position[3], normal[3], uv[2];   // uv (0,0) = top-left of the texture
    uint16_t joints[4];                    // bone indices
    float weights[4];
} MmkMeshVertex;                                                       // 56 bytes

typedef struct {
    const char *name;
    const MmkMeshVertex *vertices;
    size_t vertex_count;
    const uint32_t *indices;               // triangle list
    size_t index_count;
    uint32_t bone_count, shape_count;
    float bounds[6];                       // rest pose: min xyz, max xyz
    const uint8_t *colors;                 // vertex colours RGBA8, NULL if none
} MmkMeshModel;                                                        // 80 bytes

enum {
    MMK_MESH_UNLIT = 1,         // pre-rendered card: cut out by alpha, no lighting
    MMK_MESH_SELF_LIT = 2,      // no lighting (sky, clouds, glowing parts)
    MMK_MESH_FOG = 4,           // the environment's fogs apply
    MMK_MESH_ALPHA_TEST = 8,    // discard below alpha_ref
    MMK_MESH_BLEND_SHIFT = 4,   // bits 4-5: 0 opaque, 1 alpha blended, 2 additive
    MMK_MESH_FACES_SHIFT = 6,   // bits 6-7: faces drawn 0 front, 1 back, 2 both, 3 none
    MMK_MESH_EXTRA_SHIFT = 12,  // bits 12-13: extra texture 0 none, 1 emission (added),
                                // 2 alpha mask, 3 sky base under the albedo's cloud layer
    MMK_MESH_FOG1 = 1 << 14,    // the second distance fog applies
    MMK_MESH_FOG_Y = 1 << 15,   // the height fog applies
    MMK_MESH_WORLD_UV = 1 << 16, // uv from the world position: (x, -y) / 64, then srt
};
typedef struct {
    uint32_t first_index, index_count, material, bone;
    uint32_t flags;             // MMK_MESH_* bits 0-7; bits 8-9 wrap u, 10-11 wrap v
                                // (0 repeat, 1 mirror, 2 clamp)
    float alpha_ref;
    float srt[5];               // albedo uv: scale u, scale v, rotate, translate u, translate v
    float vertex_color_ratio;   // vertex colour tint strength (albedo * mix(1, colour, ratio))
    uint32_t extra_texture;     // MMK_MESH_EXTRA_SHIFT texture, or MMK_TEXTURE_NONE
    float emission;             // emission strength (extra mode 1)
    float tint[3];              // albedo multiplied by this colour
} MmkMeshShape;                                                        // 68 bytes

typedef struct {
    const float *skin;          // bone_count x 12 floats (row-major 3x4)
    const uint32_t *shape_texture;   // per shape: texture index or MMK_TEXTURE_NONE
    const uint8_t *shape_visible;    // per shape: 1 / 0
    uint32_t bone_count, shape_count;
} MmkMeshPose;                                                         // 32 bytes

enum { MMK_MESH_DRAW_CARD = 1, MMK_MESH_DRAW_TILE = 2 };
typedef struct {
    int32_t mesh;
    uint32_t model;
    int32_t anim;               // -1 = rest pose
    float time;                 // frames since the animation started
    float transform[12];        // model -> world px, row-major 3x4
    uint32_t layer;
    uint32_t order;             // draw before snapshot sprite #order
    uint32_t flags;             // MMK_MESH_DRAW_*
    uint32_t variant;           // pass to mmk_mesh_pose (1 = ".1" textures, e.g. Fire Mario)
} MmkMeshDraw;                                                         // 80 bytes

int32_t mmk_mesh_find(const MmkAssets *assets, const char *name);    // -1 = no mesh
size_t mmk_mesh_model_count(const MmkAssets *assets, int32_t mesh);
bool mmk_mesh_model(const MmkAssets *assets, int32_t mesh, size_t model, MmkMeshModel *out);
int32_t mmk_mesh_model_index(const MmkAssets *assets, int32_t mesh, const char *name);
bool mmk_mesh_shape(const MmkAssets *assets, int32_t mesh, size_t model, size_t shape, MmkMeshShape *out);
int32_t mmk_mesh_anim(const MmkAssets *assets, int32_t mesh, const char *name);   // -1 = none
size_t mmk_mesh_anim_count(const MmkAssets *assets, int32_t mesh);
size_t mmk_mesh_anim_name(const MmkAssets *assets, int32_t mesh, size_t index, char *buf, size_t cap);
float mmk_mesh_anim_frames(const MmkAssets *assets, int32_t anim);
// anim -1 = rest; buffers valid until the next pose call on this assets handle
bool mmk_mesh_pose(const MmkAssets *assets, int32_t mesh, size_t model, int32_t anim, float time,
                   uint32_t variant, MmkMeshPose *out);
// This frame's mesh draws (3D styles; filled by mmk_engine_snapshot), valid until the next snapshot
size_t mmk_engine_meshes(const MmkEngine *engine, const MmkMeshDraw **out);
// The 3D styles' scene this frame: cameras (column-major 4x4, world px -> clip, depth 0..1) and
// light uniforms (30 x float4: eye; 3 directional lights; hemisphere; ambient; 3 fogs; exposure;
// the sky's 6 constant colours)
typedef struct {
    float field_view_proj[16];
    float background_view_proj[16];
    float field_light[120];
    float background_light[120];
    bool has_background;
} MmkScene3d;                                                          // 1092 bytes
bool mmk_engine_scene3d(const MmkEngine *engine, MmkScene3d *out);    // false: 2D style
// The background scene's draws (drawn first, with background_view_proj and its own depth)
size_t mmk_engine_background(const MmkEngine *engine, const MmkMeshDraw **out);
void mmk_engine_set_meshes(MmkEngine *engine, bool enabled);          // default on
// Model viewer: the draws the engine makes for a sim sprite (model "M1_…", frame "walk.0") in
// style "WU"/"3W", origin (x, y) px, scale x SMB1 size, facing right unless flip_x. 0 = a sprite.
size_t mmk_mesh_sprite_draws(const MmkAssets *assets, const char *style, const char *model,
                             const char *frame, float x, float y, float scale, bool flip_x,
                             float tick, const MmkMeshDraw **out);
