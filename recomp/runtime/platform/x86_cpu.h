// x86-64: does this CPU (and OS) run code built for x86-64-v3's AVX2 / FMA / BMI2 (the recompiled game code, Makefile
// GENARCH)? From CPUID and XGETBV directly: __builtin_cpu_supports needs libgcc's __cpu_model, which zig's compiler
// runtime (the cross builds' and players' compiler) doesn't have.
#pragma once
#include <cpuid.h>
#include <stdbool.h>

static inline bool x86_cpu_has_v3(void) {
    unsigned a, b, c, d;
    if (!__get_cpuid(1, &a, &b, &c, &d)) return false;
    bool fma = (c >> 12) & 1, osxsave = (c >> 27) & 1, avx = (c >> 28) & 1;
    if (!fma || !osxsave || !avx) return false;
    unsigned lo, hi;
    __asm__ volatile("xgetbv" : "=a"(lo), "=d"(hi) : "c"(0));  // the OS saves the YMM registers (XCR0 bits 1, 2)
    (void)hi;
    if ((lo & 6) != 6) return false;
    if (!__get_cpuid_count(7, 0, &a, &b, &c, &d)) return false;
    return ((b >> 5) & 1) && ((b >> 8) & 1);  // AVX2, BMI2
}
