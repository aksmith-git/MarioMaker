// SMM2 course codes <-> data ids (the same arithmetic as core/crates/formats course_id.rs and smmfmt/course_id.py).
#include <ctype.h>
#include <stdio.h>
#include <string.h>

#include "mod.h"

static const char ALPHABET[] = "0123456789BCDFGHJKLMNPQRSTVWXY";
#define XOR_KEY 0x1680E07Cull

bool code_to_data_id(const char *code, uint32_t *id) {
    char s[9];
    int n = 0;
    for (const char *p = code; *p && n <= 9; p++)
        if (*p != '-') {
            if (n == 9) return false;
            s[n++] = (char)toupper((unsigned char)*p);
        }
    if (n != 9) return false;
    uint64_t v = 0;
    for (int i = 8; i >= 0; i--) {
        const char *d = strchr(ALPHABET, s[i]);
        if (!d || !*d) return false;
        v = v * 30 + (uint64_t)(d - ALPHABET);
    }
    uint64_t low = v & 0xFFF, one = (v >> 12) & 1, mid = (v >> 14) & 0xFFFFF, check = (v >> 34) & 0x3F,
             eight = (v >> 40) & 0xF;
    uint64_t data = ((low << 20) | mid) ^ XOR_KEY;
    if (one != 1 || eight != 8 || check != (uint64_t)(((int64_t)data - 31) % 64 + 64) % 64) return false;
    *id = (uint32_t)data;
    return true;
}

void data_id_to_code(uint32_t id, char out[12]) {
    uint64_t x = id ^ XOR_KEY;
    uint64_t check = (uint64_t)((((int64_t)id - 31) % 64 + 64) % 64);
    uint64_t n = (8ull << 40) | (check << 34) | ((x & 0xFFFFF) << 14) | (1ull << 12) | (x >> 20);
    char c[9];
    for (int i = 0; i < 9; i++) c[i] = ALPHABET[n % 30], n /= 30;
    snprintf(out, 12, "%.3s-%.3s-%.3s", c, c + 3, c + 6);
}
