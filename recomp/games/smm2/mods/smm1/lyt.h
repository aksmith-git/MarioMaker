// SMM1 (Wii U) layout player for the host-drawn 100 Mario Challenge screens: the "Cafe" UI formats (Yaz0, SARC,
// BFLYT, BFLAN, BFLIM, BFFNT, MSBT), a player that evaluates a layout and its bound animations into textured quads,
// and a CPU rasterizer for those quads.
//
// A C port of SMCraft's layout player (our own Rust: core/crates/formats lyt/bflim/bffnt/gx2/msbt and the archived
// core/crates/ui player/text/font, branch smcraft-archive): same rules, same coordinate conventions. Layout space:
// 1280 x 720 for full-screen SMM1 layouts, origin at the centre, y up. Textures are RGBA8, top row first.
//
// Nothing here is Nintendo's: the files are read from the player's own SMM1 dump at run time (smm1_data.c).
#pragma once
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

// ---------------------------------------------------------------- containers
// Yaz0: the decompressed bytes (malloc'd; *n their size), or NULL (not Yaz0 / damaged).
uint8_t *lyt_yaz0(const uint8_t *d, size_t n, size_t *out_n);

typedef struct {
    char name[128];
    uint32_t start, end;
} LSarcEntry;
typedef struct {
    const uint8_t *data;  // not owned
    size_t n;
    int count;
    LSarcEntry *e;
} LSarc;
bool lyt_sarc_open(LSarc *s, const uint8_t *d, size_t n);
const uint8_t *lyt_sarc_get(const LSarc *s, const char *name, size_t *len);
void lyt_sarc_close(LSarc *s);

// ---------------------------------------------------------------- textures (one global table; ids from 0)
typedef struct {
    int w, h;
    uint8_t *px;  // RGBA8, straight alpha
} LTex;
int lyt_tex_add(int w, int h, uint8_t *px);  // takes px
const LTex *lyt_tex(int id);                 // NULL for -1 / unknown
// BFLIM -> RGBA8 (malloc'd), with the format's channel mapping (luminance / alpha formats replicate one channel).
uint8_t *lyt_flim_decode(const uint8_t *d, size_t n, int *w, int *h);
// A GX2 surface (Cafe format code as in BFLIM, tile mode, swizzle word) -> RGBA8 with the raw channels (BC4/BC5: R / RG)
uint8_t *lyt_surface_raw(const uint8_t *d, size_t n, uint32_t w, uint32_t h, int fmt, uint32_t mode, uint32_t swz);

// ---------------------------------------------------------------- fonts
typedef struct {
    int8_t left;
    uint8_t glyph_w, char_w;
} LCharW;
typedef struct LFont {
    char name[32];
    uint8_t height, width, ascent;
    uint16_t line_feed, alter;
    LCharW defw;
    uint8_t cell_w, cell_h;
    uint16_t baseline, cells_row, cells_col, sheet_w, sheet_h;
    int nsheets;
    int *sheet_tex;  // decoded on first use (lyt_font_sheet): SpecialFont has 36 large sheets
    uint8_t *raw;    // the file (kept for that)
    size_t raw_n, sheet_size, data_off;
    int sheet_fmt;
    uint32_t sheet_mode;
    int nglyphs;
    LCharW *widths;  // per glyph index (defw where the file has none)
    uint16_t *cmap;  // 0x10000 entries: code -> glyph, 0xFFFF none
    struct LFont *fallback;  // characters this font lacks (SpecialFont: kana, kanji, symbols)
} LFont;
LFont *lyt_font_parse(const char *name, const uint8_t *d, size_t n);  // copies d
int lyt_font_sheet(LFont *f, int sheet);                              // texture id (-1: none)
bool lyt_font_has(const LFont *f, uint32_t c);

// ---------------------------------------------------------------- messages (MSBT, UTF-16 files)
#define LYT_ARG 0xFFFCu  // stands for a value tag (groups 2, 4, 250) in a message
typedef struct {
    char label[64];
    uint32_t *text;  // UTF-32, other control tags removed
    int n;
} LMsg;
typedef struct {
    int count;
    LMsg *m;
} LMsbt;
bool lyt_msbt_parse(LMsbt *out, const uint8_t *d, size_t n);
const LMsg *lyt_msbt_get(const LMsbt *t, const char *label);

// ---------------------------------------------------------------- layouts (BFLYT)
typedef struct {
    float t[2], r, s[2];
} LTexSrt;
typedef struct {
    uint16_t tex;
    uint8_t wrap_s, wrap_t, min_f, mag_f;
} LTexMap;
typedef struct {
    float t[2], s[2];
    uint8_t flags;
} LProj;
typedef struct {
    char name[29];
    uint8_t black[4], white[4];
    int ntex, nsrt, ngen, ntev, nproj;
    LTexMap tex[3];
    LTexSrt srt[3];
    uint8_t gen_src[3];
    uint8_t tev_c[8], tev_a[8];
    bool has_blend, texture_only;
    uint8_t blend_op, blend_src, blend_dst;
    LProj proj[3];
} LMat;

enum { LP_NULL, LP_PIC, LP_TXT, LP_WND, LP_BND, LP_PRT };
typedef struct {
    uint8_t vc[4][4];  // TL, TR, BL, BR
    uint16_t mat;
    int nuv;
    float uv[3][4][2];
} LPic;
typedef struct {
    uint16_t mat, font;
    uint8_t pos, lalign, flags;
    float italic;
    uint8_t top[4], bot[4];
    float fsize[2], cspace, lspace;
    char text_id[64];
    uint32_t *text;  // stored text (UTF-32)
    int ntext;
    float sh_off[2], sh_scale[2];
    uint8_t sh_top[4], sh_bot[4];
} LTxt;
typedef struct {
    int16_t infl[4];   // left, right, top, bottom
    uint16_t fsz[4];
    uint8_t flags;
    LPic content;
    int nframes;
    uint16_t fmat[8];
    uint8_t fflip[8];
} LWnd;
typedef struct LPane LPane;
typedef struct {
    char name[25];
    uint8_t usage, basic_usage, mat_usage;
    LPane *pane;  // replacement pane, or NULL
    bool has_basic;
    float t[3], r[3], s[2], size[2];
    uint8_t alpha;
} LProp;
typedef struct {
    char layout[64];
    float magnify[2];
    int nprops;
    LProp *props;
} LPrt;
struct LPane {
    int kind;
    char name[25];
    uint8_t flags, origin, alpha, mag;  // flags: bit 0 visible, bit 1 influenced alpha
    float t[3], r[3], s[2], size[2];
    int parent;  // index in the layout's pane list (-1: root)
    LPic pic;
    LTxt txt;
    LWnd wnd;
    LPrt prt;
};

// ---------------------------------------------------------------- animations (BFLAN)
typedef struct {
    float f, v, s;
} LKey;
typedef struct {
    uint8_t index, target, curve;  // curve 0 constant, 1 step, 2 hermite
    int nk;
    LKey *k;
} LAEntry;
typedef struct {
    char magic[5];
    int ne;
    LAEntry *e;
} LATag;
typedef struct {
    char name[29];
    uint8_t kind;  // 0 pane, 1 material
    int nt;
    LATag *t;
    int target;  // resolved pane / material index in the owning layout (-1 none)
} LACont;
typedef struct {
    char name[64];  // tag name (In, MapIn, ...)
    uint16_t frames;
    bool loop;
    int ntex;
    int *tex;  // texture ids for texture-pattern keys (-1 missing)
    int nc;
    LACont *c;
} LAnim;
float lyt_eval(const LAEntry *e, float frame);

// ---------------------------------------------------------------- a loaded layout with its parts, textures, fonts, texts
typedef struct LRes {
    char name[64];
    float w, h;
    int ntex;
    int *tex;  // per layout texture slot (-1 missing)
    int ntimg;
    char (*timg_name)[64];  // every texture of the archive by stem (texture-pattern keys)
    int *timg;
    int nfont;
    LFont **font;
    int nmat;
    LMat *mat;
    int npane;
    LPane *pane;
    int nanim;
    LAnim *anim;
    LMsbt msg;
    struct LRes **part;  // per pane: the part's layout (part panes), else NULL
} LRes;

// The loader the player asks for files (smm1_data.c): a layout archive (decompressed SARC of blyt/ anim/ timg/), a
// font by name (CafeStd_* and unknown names fall back to MessageFont) and a layout's message table.
typedef struct {
    uint8_t *(*layout_arc)(const char *name, size_t *n);
    LFont *(*font)(const char *name);
    uint8_t *(*msbt)(const char *layout, size_t *n);
} LLoader;
void lyt_set_loader(const LLoader *l);
LRes *lyt_load(const char *name);  // once per name (cached); NULL if missing
int lyt_anim_index(const LRes *r, const char *name);
int lyt_pane_index(const LRes *r, const char *name);

// ---------------------------------------------------------------- the player
typedef struct {
    float pos[4][2], uv0[4][2], uv1[4][2];  // TL, TR, BL, BR
    uint8_t col[4][4], black[4], white[4];
    int tex0, tex1;
    uint32_t flags;
} LQuad;
// flags: wrap s0 t0 s1 t1 (2 bits each: clamp, repeat, mirror), linear filter, tex0/tex1 combine, blend
#define LQ_WRAP_S0 0
#define LQ_WRAP_T0 2
#define LQ_WRAP_S1 4
#define LQ_WRAP_T1 6
#define LQ_LINEAR (1u << 8)
#define LQ_TRI (1u << 9)  // a triangle: corners TL, TR, BL (BR = TR + BL - TL); only the half s + t < 1 is drawn
#define LQ_COMB_C 12  // 0 replace (tex1), 1 modulate, 2 add, 5 subtract, 15 none
#define LQ_COMB_A 16
#define LQ_BLEND_OP 20  // 0 off, 1 add, 2 subtract, 3 reverse subtract, 4 min, 5 max
#define LQ_BLEND_SRC 24  // factors: 0 zero, 1 one, 2 dst, 3 1-dst, 4 src a, 5 1-src a, 6 dst a, 7 1-dst a, 8 src, 9 1-src
#define LQ_BLEND_DST 28
#define LQ_COMB_NONE 15u
#define LQ_BLEND_NORMAL ((1u << LQ_BLEND_OP) | (4u << LQ_BLEND_SRC) | (5u << LQ_BLEND_DST))

typedef struct {
    LQuad *q;
    int n, cap;
} LQuads;

typedef struct LInst LInst;
LInst *lyt_inst(LRes *r);  // a playing instance (its part panes get their own)
void lyt_inst_free(LInst *i);
const LRes *lyt_inst_res(const LInst *i);
// Paths are "[PartPane/…/]Name".
bool lyt_bind(LInst *i, const char *anim_path, float frame);  // bind (or move) an animation at a frame
void lyt_unbind(LInst *i, const char *anim_path);
void lyt_unbind_all(LInst *i);
float lyt_anim_length(LInst *i, const char *anim_path);  // frames, -1 if unknown
bool lyt_set_text(LInst *i, const char *pane_path, const uint32_t *s, int n);
bool lyt_set_text8(LInst *i, const char *pane_path, const char *utf8);
bool lyt_clear_text(LInst *i, const char *pane_path);
bool lyt_set_visible(LInst *i, const char *pane_path, int vis);  // 0 hide, 1 show, -1 back to the animations
// Force a pane's translate (x, y) / alpha (0-255) over its animations (NULL / < 0: keep the animated value).
bool lyt_set_pane(LInst *i, const char *pane_path, const float *xy, float alpha);
// A pane's values after the bound animations (in its parent's space).
typedef struct {
    float t[3], r[3], s[2], size[2], alpha;
    bool visible;
} LPaneInfo;
bool lyt_pane_info(LInst *i, const char *pane_path, LPaneInfo *out);
// Where a pane is drawn: its rectangle's corners in layout space (TL, TR, BL, BR).
bool lyt_pane_global(LInst *i, const char *pane_path, float corners[4][2], float *alpha, bool *visible);
// A message of this layout's table (or its parts'), UTF-32; NULL if none.
const LMsg *lyt_message(LInst *i, const char *label);
float lyt_text_width(LInst *i, const char *pane_path);  // widest line of the pane's current text (pane units)
void lyt_draw(LInst *i, LQuads *out);
void lyt_quads_reset(LQuads *q);

// UTF-8 -> UTF-32 (malloc'd; *n code points)
uint32_t *lyt_utf8(const char *s, int *n);

// ---------------------------------------------------------------- rasterizer
// Draw quads (layout units, centre origin, y up; lw x lh layout units fill the target) into an RGBA8 target of
// w x h pixels, top row first, blending over what is there. Multi-threaded by row bands.
void lyt_raster(const LQuads *q, uint8_t *rgba, int w, int h, float lw, float lh);
void lyt_clear(uint8_t *rgba, int w, int h, const uint8_t c[4]);
