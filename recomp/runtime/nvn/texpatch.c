// Texture edits on upload (texpatch.h). LAS Recomp recognised Link's Awakening's UI textures here (button prompts,
// the Hero Mode icon, tunic and heart colours); that knowledge is the game's, so the shared runtime knows no
// textures: nothing is identified and nothing changes. A game can bring its own set later through the same interface.
#include "texpatch.h"

int texpatch_identify(const NvnTexture *t) { (void)t; return -1; }
int texpatch_bottle_kind(int id) { (void)id; return 0; }
bool texpatch_is_item_cursor(int id) { (void)id; return false; }
uint32_t texpatch_generation(void) { return 0; }
void texpatch_apply(int id, uint32_t lv, uint8_t *blocks, uint32_t w, uint32_t h, uint32_t row) {
    (void)id, (void)lv, (void)blocks, (void)w, (void)h, (void)row;
}
int texpatch_heart_kind(int id) { (void)id; return 0; }
void texpatch_heart_tint(int kind, float tint[8]) { (void)kind, (void)tint; }
