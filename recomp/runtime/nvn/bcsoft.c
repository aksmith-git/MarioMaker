// BC1-7 textures decoded on the CPU, for GPUs without BC support (the iOS Simulator, older iPhones): runtime/nvn/bcsoft.h.
// The decoder is bcdec (third_party/bcdec, MIT).
#include "bcsoft.h"

#include <string.h>

#define BCDEC_IMPLEMENTATION
#define BCDEC_BC4BC5_PRECISE
#include "../../third_party/bcdec/bcdec.h"

int bcsoft_kind(uint32_t f) {
    switch (f) {
    case 0x41: case 0x42: case 0x45: case 0x46: return BCSOFT_BC1;
    case 0x43: case 0x47: return BCSOFT_BC2;
    case 0x44: case 0x48: return BCSOFT_BC3;
    case 0x49: return BCSOFT_BC4;
    case 0x4A: return BCSOFT_BC4S;
    case 0x4B: return BCSOFT_BC5;
    case 0x4C: return BCSOFT_BC5S;
    case 0x4D: case 0x4E: return BCSOFT_BC7;
    case 0x4F: return BCSOFT_BC6S;
    case 0x50: return BCSOFT_BC6;
    default: return 0;
    }
}

int bcsoft_texel_size(int kind) { return kind == BCSOFT_BC6 || kind == BCSOFT_BC6S ? 8 : 4; }

// blocks: (w + 3) / 4 x (h + 3) / 4 x d blocks, row by row; out: w x h x d texels (RGBA8 / RGBA8 snorm / RGBA16F)
void bcsoft_decode(int kind, const uint8_t *blocks, uint32_t w, uint32_t h, uint32_t d, uint8_t *out) {
    uint32_t bw = (w + 3) / 4, bh = (h + 3) / 4;
    int bs = kind == BCSOFT_BC1 || kind == BCSOFT_BC4 || kind == BCSOFT_BC4S ? 8 : 16;
    int ts = bcsoft_texel_size(kind);
    uint8_t tmp[4 * 4 * 8];
    for (uint32_t z = 0; z < d; z++)
        for (uint32_t by = 0; by < bh; by++)
            for (uint32_t bx = 0; bx < bw; bx++) {
                const uint8_t *b = blocks + (((uint64_t)z * bh + by) * bw + bx) * bs;
                memset(tmp, 0, sizeof tmp);
                switch (kind) {
                case BCSOFT_BC1: bcdec_bc1(b, tmp, 16); break;
                case BCSOFT_BC2: bcdec_bc2(b, tmp, 16); break;
                case BCSOFT_BC3: bcdec_bc3(b, tmp, 16); break;
                case BCSOFT_BC7: bcdec_bc7(b, tmp, 16); break;
                case BCSOFT_BC6: case BCSOFT_BC6S: {
                    uint16_t rgb[16 * 3];
                    bcdec_bc6h_half(b, rgb, 4 * 3, kind == BCSOFT_BC6S);
                    for (int i = 0; i < 16; i++) {
                        uint16_t *o = (uint16_t *)(tmp + i * 8);
                        o[0] = rgb[i * 3], o[1] = rgb[i * 3 + 1], o[2] = rgb[i * 3 + 2], o[3] = 0x3C00;  // (alpha 1.0)
                    }
                    break;
                }
                case BCSOFT_BC4: case BCSOFT_BC4S: {
                    uint8_t r[16];
                    bcdec_bc4(b, r, 4, kind == BCSOFT_BC4S);
                    for (int i = 0; i < 16; i++) tmp[i * 4] = r[i], tmp[i * 4 + 3] = kind == BCSOFT_BC4S ? 127 : 255;
                    break;
                }
                case BCSOFT_BC5: case BCSOFT_BC5S: {
                    uint8_t rg[32];
                    bcdec_bc5(b, rg, 8, kind == BCSOFT_BC5S);
                    for (int i = 0; i < 16; i++)
                        tmp[i * 4] = rg[i * 2], tmp[i * 4 + 1] = rg[i * 2 + 1], tmp[i * 4 + 3] = kind == BCSOFT_BC5S ? 127 : 255;
                    break;
                }
                default: break;
                }
                for (uint32_t y = 0; y < 4 && by * 4 + y < h; y++) {
                    uint32_t n = w - bx * 4 < 4 ? w - bx * 4 : 4;
                    memcpy(out + (((uint64_t)z * h + by * 4 + y) * w + bx * 4) * ts, tmp + y * 4 * ts, (size_t)n * ts);
                }
            }
}
