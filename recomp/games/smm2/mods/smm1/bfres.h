// Wii U BFRES (FRES 3.x, big-endian) read for SMM1's ending scene (bfres.c): textures, models (skeleton rest pose,
// shapes with positions / texture coordinates / indices, material texture names) and texture-pattern animations.
// Written from public descriptions of the Wii U BFRES layout (offsets relative to their own field).
#pragma once
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

typedef struct {
    char name[32];
    int parent;
    float s[3], r[4], t[3];  // rest pose (r: euler radians, or a quaternion when the skeleton says so)
    float world[3][4];       // rest pose in model space
} BBone;

typedef struct {
    char name[32];
    int mat, bone, nverts, nidx, skin_count;
    float (*pos)[3];
    float (*uv)[2];
    uint8_t (*idx0)[4];  // skinning bone indices (first set), when present
    uint32_t *idx;       // triangles
} BShape;

typedef struct {
    char name[32];
    int ntex;
    char tex[4][32];  // texture names (slot 0: albedo)
    uint8_t wrap_s[4], wrap_t[4];
    float srt[5];  // tex_srt0: scale x, y, rotation, translation x, y
} BMat;

typedef struct {
    char name[32];
    int nbones, nshapes, nmats;
    bool quat;
    uint32_t skl_flags;
    BBone *bones;
    int *smooth;  // smooth matrix index -> bone
    int nsmooth;
    BShape *shapes;
    BMat *mats;
} BModel;

typedef struct {
    int nkeys;
    float *frame, *value;  // texture-list index per key (step)
} BCurve;
typedef struct {
    char mat[32];
    int ncurves;
    BCurve *curve;     // one per pattern target (sampler)
    int *base;         // base texture index per pattern target
    int *pat_curve;    // its curve (-1: the base value)
    int npat;
} BMatPat;
typedef struct {
    char name[32];
    int frames;
    bool loop;
    int ntex;
    char (*tex)[32];
    int nmat;
    BMatPat *mat;
} BTexPat;

// skeletal animations (FSKA): per bone a base transform and curves over its components
typedef struct {
    int target;  // byte offset of the component: scale 0x4-0xC, translate 0x10-0x18, rotate 0x20-0x2C
    int type;    // 0 cubic, 1 linear
    int nk;
    float *frame, (*coef)[4];
    float scale, offset;
} BFCurve;
typedef struct {
    char name[32];
    uint32_t flags;  // bit 3 base scale, 4 base rotate, 5 base translate
    float base_s[3], base_r[4], base_t[3];
    int nc;
    BFCurve *c;
} BBoneAnim;
typedef struct {
    char name[32];
    int frames;
    bool loop, euler;
    int nb;
    BBoneAnim *b;
} BSkelAnim;

typedef struct {
    int nmodels, ntex, npat, nskel;
    BSkelAnim *skels;
    BModel *models;
    char (*tex_name)[32];
    int *tex;  // lyt texture ids (decoded RGBA)
    BTexPat *pats;
} Bfres;

Bfres *bfres_load(const uint8_t *d, size_t n);  // NULL if not FRES
int bfres_texture(const Bfres *b, const char *name);  // lyt texture id, -1
const BModel *bfres_model(const Bfres *b, const char *name);
const BTexPat *bfres_texpat(const Bfres *b, const char *name);
// the texture a pattern animation shows on material `mat` (slot 0) at `frame`; NULL if it doesn't animate it
const char *bfres_texpat_at(const BTexPat *p, const char *mat, float frame);
void bfres_dump(const Bfres *b);
const BSkelAnim *bfres_skelanim(const Bfres *b, const char *name);
// the model's bone matrices (model space, 3x4 each) with a skeletal animation at a frame (NULL: the rest pose)
void bfres_pose(const BModel *m, const BSkelAnim *a, float frame, float (*world)[3][4]);
// bone of a vertex's skinning index (the skeleton's smooth + rigid matrix table)
int bfres_skin_bone(const BModel *m, int index);
