// BC1-7 (S3TC / RGTC / BPTC) block decoding on the CPU, for GPUs without BC textures (runtime/nvn/bcsoft.c).
#pragma once
#include <stdint.h>

enum { BCSOFT_BC1 = 1, BCSOFT_BC2, BCSOFT_BC3, BCSOFT_BC4, BCSOFT_BC4S, BCSOFT_BC5, BCSOFT_BC5S, BCSOFT_BC6, BCSOFT_BC6S,
       BCSOFT_BC7 };

int bcsoft_kind(uint32_t nvn_format);  // 0: not a BC format
int bcsoft_texel_size(int kind);       // bytes per decoded texel: 4 (RGBA8 / RGBA8 snorm), 8 (RGBA16F: BC6H)
void bcsoft_decode(int kind, const uint8_t *blocks, uint32_t w, uint32_t h, uint32_t d, uint8_t *out);
