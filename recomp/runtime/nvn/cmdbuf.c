// NVN command buffers: commands are recorded as CmdRec records into host memory (see "recording"); the game's
// command / control memory only shows the usage it would have on the console.
// A command handle is the host address of a list's first record.
#include <pthread.h>
#include <stdlib.h>
#include <string.h>

#include "nvn.h"

static NvnCmdBuf *cb_of(uint64_t guest) { return HOST(NvnCmdBuf, guest); }

NVN(nvnCommandBufferInitialize) {
    NvnCmdBuf *cb = calloc(1, sizeof *cb);
    cb->guest = A(0);
    SET_HOST(A(0), cb);
    RET(1);
}
NVN(nvnCommandBufferFinalize) {
    NvnCmdBuf *cb = cb_of(A(0));
    if (cb) free(cb->buf);
    free(cb);  // (its calls-only lists are left to the graveyard's limit: they may still be called)
    SET_HOST(A(0), 0);
}
NVN(nvnCommandBufferSetDebugLabel) { (void)c; }

static CmdRec *raw_rec(uint64_t at, uint32_t op, uint32_t size) {
    CmdRec *r = (CmdRec *)(uintptr_t)at;
    memset(r, 0, sizeof *r);
    r->op = op;
    r->size = size;
    return r;
}

// ---- recording. A list is recorded into its command buffer's host buffer and copied out at EndRecording; the
// handle is the host address of the copy's first record (opaque to the game). The game's command and control memory
// are never written: our records are many times the size of the GPU's own command words, so a game that sizes its
// memory for those (or records one list while another, opened earlier, is still recording in the same memory, as
// SMM2 does) would be overrun. The game still sees its memory being used, at an estimated 16 bytes of command memory
// per state / draw command (calls of other lists go to control memory on the console: 0 here) and 4 bytes of
// control memory per command (the estimates of SuperMarioWonderRecomp, MIT), so its allocators behave as on the
// console; usage stays below the size it gave (it never needs to be called back for more).
//
// Lifetime: on the console a list stays valid until the game records over the memory it used. Each list remembers
// the guest command memory range it (by the estimate) used; a list recorded over part of that range retires it.
// Lists that used no command memory (only calls of other lists) are retired after 64 more such lists of the same
// command buffer. Retired lists are freed later still (a list being executed may hold them).
#define CMD_BYTES 16
#define CTL_BYTES 4

typedef struct ListInfo {
    uint64_t start, end;  // guest command memory it used (estimate)
    uint8_t *mem;         // the list (its handle)
    struct ListInfo *next;
} ListInfo;
#define LIST_BUCKETS 65536
#define PAGE_SHIFT_L 16
#define LOOKBACK 16  // pages before a range checked for lists reaching into it (lists over 1 MB: rare)
static ListInfo *list_buckets[LIST_BUCKETS];  // by 64 KB page of start
#define NGRAVE (1 << 16)
static struct { uint8_t *mem; uint64_t at; } graveyard[NGRAVE];
static uint64_t grave_head, grave_tail, list_clock;
static pthread_mutex_t list_lock = PTHREAD_MUTEX_INITIALIZER;
#define GRAVE_DELAY 20000  // lists recorded after one is retired before it is freed

static unsigned bucket(uint64_t page) { return (unsigned)(page * 0x9E3779B1u) & (LIST_BUCKETS - 1); }

// (list_lock held)
static void bury(uint8_t *mem) {
    if (grave_head - grave_tail == NGRAVE) free(graveyard[grave_tail++ % NGRAVE].mem);
    graveyard[grave_head % NGRAVE].mem = mem;
    graveyard[grave_head++ % NGRAVE].at = list_clock;
}
static void retire_overlapping(uint64_t start, uint64_t end) {
    uint64_t first = start >> PAGE_SHIFT_L, last = (end - 1) >> PAGE_SHIFT_L;
    first = first > LOOKBACK ? first - LOOKBACK : 0;
    for (uint64_t page = first; page <= last; page++)
        for (ListInfo **pp = &list_buckets[bucket(page)]; *pp;) {
            ListInfo *l = *pp;
            if ((l->start >> PAGE_SHIFT_L) == page && l->start < end && start < l->end) {
                *pp = l->next;
                bury(l->mem);
                free(l);
            } else {
                pp = &l->next;
            }
        }
    while (grave_tail < grave_head && list_clock - graveyard[grave_tail % NGRAVE].at > GRAVE_DELAY)
        free(graveyard[grave_tail++ % NGRAVE].mem);
}

static uint64_t est_used(uint64_t size, uint64_t base, uint64_t n, uint64_t per) {
    uint64_t u = base + n * per, cap = size > 64 ? size - 64 : 0;
    return u < cap ? u : cap;
}

NVN(nvnCommandBufferAddCommandMemory) {
    NvnCmdBuf *cb = cb_of(A(0));
    cb->cmd_mem = HOST(NvnPool, A(1))->storage + A(2);
    cb->cmd_size = A(3);
    cb->ncmd_base = cb->ncmd;
    if (ENV("LARECOMP_LOG_CMDMEM"))
        LOG("[cmdmem] add cb %llx memory %llx size %llx\n", (unsigned long long)A(0), (unsigned long long)cb->cmd_mem,
            (unsigned long long)cb->cmd_size);
}
NVN(nvnCommandBufferAddControlMemory) {
    NvnCmdBuf *cb = cb_of(A(0));
    cb->ctl_size = A(2);
    cb->nctl_base = cb->nctl;
}
static uint64_t cmd_used(NvnCmdBuf *cb) { return est_used(cb->cmd_size, 0, cb->ncmd - cb->ncmd_base, CMD_BYTES); }
static uint64_t ctl_used(NvnCmdBuf *cb) { return est_used(cb->ctl_size, 0, cb->nctl - cb->nctl_base, CTL_BYTES); }
NVN(nvnCommandBufferGetCommandMemorySize) { RET(cb_of(A(0))->cmd_size); }
NVN(nvnCommandBufferGetCommandMemoryUsed) { RET(cmd_used(cb_of(A(0)))); }
NVN(nvnCommandBufferGetCommandMemoryFree) { NvnCmdBuf *cb = cb_of(A(0)); RET(cb->cmd_size - cmd_used(cb)); }
NVN(nvnCommandBufferGetControlMemorySize) { RET(cb_of(A(0))->ctl_size); }
NVN(nvnCommandBufferGetControlMemoryUsed) { RET(ctl_used(cb_of(A(0)))); }
NVN(nvnCommandBufferGetControlMemoryFree) { NvnCmdBuf *cb = cb_of(A(0)); RET(cb->ctl_size - ctl_used(cb)); }
NVN(nvnCommandBufferSetMemoryCallback) { cb_of(A(0))->callback = A(1); }
NVN(nvnCommandBufferGetMemoryCallback) { RET(cb_of(A(0))->callback); }
NVN(nvnCommandBufferSetMemoryCallbackData) { cb_of(A(0))->callback_data = A(1); }
NVN(nvnCommandBufferGetMemoryCallbackData) { RET(cb_of(A(0))->callback_data); }
NVN(nvnCommandBufferSetCommandMemoryCallbackEnabled) { (void)c; }

// Reserve a record in the list being recorded (valid until the next one is reserved).
static CmdRec *rec(Cpu *c, NvnCmdBuf *cb, uint32_t op, uint32_t extra) {
    (void)c;
    uint32_t size = (uint32_t)((sizeof(CmdRec) + extra + 7) & ~7u);
    if (cb->buf_used + size > cb->buf_cap) {
        uint64_t want = (cb->buf_used + size) * 2;
        cb->buf_cap = want > 65536 ? want : 65536;
        cb->buf = realloc(cb->buf, cb->buf_cap);
        if (!cb->buf) FATAL("nvn: out of host memory recording a command list\n");
    }
    CmdRec *r = raw_rec((uint64_t)(uintptr_t)(cb->buf + cb->buf_used), op, size);
    cb->buf_used += size;
    cb->nctl++;
    if (op != CMD_CALL && op != CMD_BARRIER && op != CMD_END) cb->ncmd++;  // (see "recording" above)
    return r;
}

// Debug: LARECOMP_TRACE_CALLS=<n> logs the first n recording / call / submit / render target / clear calls with their
// guest return address (which code records what, and in which command buffer).
static void trace_call(Cpu *c, const char *what) {
    static long left = -2;
    if (left == -2) left = getenv("LARECOMP_TRACE_CALLS") ? atol(getenv("LARECOMP_TRACE_CALLS")) : 0;
    if (left <= 0) return;
    left--;
    LOG("[trace] %s cb %llx x1 %llx x2 %llx x3 %llx lr %llx\n", what, (unsigned long long)c->x[0], (unsigned long long)c->x[1],
        (unsigned long long)c->x[2], (unsigned long long)c->x[3], (unsigned long long)c->x[30]);
}

NVN(nvnCommandBufferBeginRecording) {
    trace_call(c, "BeginRecording");
    NvnCmdBuf *cb = cb_of(A(0));
    cb->recording = true;
    cb->buf_used = 0;
    cb->rec_start = cb->cmd_mem + cmd_used(cb);
    rec(c, cb, CMD_BARRIER, 0);  // (a list is never empty)
}
NVN(nvnCommandBufferEndRecording) {
    NvnCmdBuf *cb = cb_of(A(0));
    rec(c, cb, CMD_END, 0);
    cb->recording = false;
    uint8_t *list = malloc(cb->buf_used);
    memcpy(list, cb->buf, cb->buf_used);
    uint64_t start = cb->rec_start, end = cb->cmd_mem + cmd_used(cb);
    pthread_mutex_lock(&list_lock);
    list_clock++;
    if (end > start) {
        retire_overlapping(start, end);
        ListInfo *l = malloc(sizeof *l);
        l->start = start, l->end = end, l->mem = list;
        unsigned b = bucket(start >> PAGE_SHIFT_L);
        l->next = list_buckets[b];
        list_buckets[b] = l;
    } else {  // (calls only: retired after 64 more of these from this command buffer)
        if (cb->calls_only[cb->ncalls_only % 64]) bury(cb->calls_only[cb->ncalls_only % 64]);
        cb->calls_only[cb->ncalls_only++ % 64] = list;
    }
    pthread_mutex_unlock(&list_lock);
    if (ENV("LARECOMP_LOG_CMDMEM"))
        LOG("[cmdmem] end cb %llx list %p: guest %llx..%llx, %llu bytes\n", (unsigned long long)A(0), (void *)list,
            (unsigned long long)start, (unsigned long long)end, (unsigned long long)cb->buf_used);
    RET((uint64_t)(uintptr_t)list);
    trace_call(c, "EndRecording(ret x0)");
}
NVN(nvnCommandBufferIsRecording) { RET(cb_of(A(0))->recording); }

// ---- generic recorders
typedef struct { uint64_t x[8]; float f[4]; } Snap;
static Snap snap(Cpu *c) {
    Snap s;
    memcpy(s.x, c->x, sizeof s.x);
    for (int i = 0; i < 4; i++) s.f[i] = *(float *)&c->v[i];
    return s;
}
static CmdRec *record(Cpu *c, uint32_t op, uint32_t extra, Snap *s) {
    CmdRec *r = rec(c, cb_of(s->x[0]), op, extra);
    memcpy(r->x, s->x + 1, sizeof r->x);
    memcpy(r->f, s->f, sizeof r->f);
    return r;
}
// record with a copy of `len` bytes from the guest pointer in argument `arg`
static CmdRec *record_copy(Cpu *c, uint32_t op, int arg, uint32_t len) {
    Snap s = snap(c);
    CmdRec *r = record(c, op, len, &s);
    if (s.x[arg] && len) memcpy(r->extra, (void *)(uintptr_t)s.x[arg], len);
    return r;
}
#define PLAIN(name, op) NVN(name) { Snap s = snap(c); record(c, op, 0, &s); }

// ---- state binding
NVN(nvnCommandBufferBindProgram) {
    Snap s = snap(c);
    CmdRec *r = record(c, CMD_BIND_PROGRAM, 0, &s);
    r->x[0] = (uint64_t)(uintptr_t)HOST(NvnProgram, s.x[1]);  // host program
}
NVN(nvnCommandBufferBindBlendState) { record_copy(c, CMD_BIND_BLEND, 1, 8); }
NVN(nvnCommandBufferBindChannelMaskState) { record_copy(c, CMD_BIND_CHANNEL_MASK, 1, 4); }
NVN(nvnCommandBufferBindColorState) { record_copy(c, CMD_BIND_COLOR, 1, 4); }
NVN(nvnCommandBufferBindDepthStencilState) { record_copy(c, CMD_BIND_DEPTH_STENCIL, 1, 8); }
NVN(nvnCommandBufferBindMultisampleState) { record_copy(c, CMD_BIND_MULTISAMPLE, 1, 0x18); }
NVN(nvnCommandBufferBindPolygonState) { record_copy(c, CMD_BIND_POLYGON, 1, 4); }
NVN(nvnCommandBufferBindVertexAttribState) { record_copy(c, CMD_BIND_VERTEX_ATTRIB, 2, (uint32_t)A(1) * 4); }
NVN(nvnCommandBufferBindVertexStreamState) { record_copy(c, CMD_BIND_VERTEX_STREAM, 2, (uint32_t)A(1) * 8); }

// (cmd, first, count, const NVNbufferRange*) and the single-binding forms
NVN(nvnCommandBufferBindVertexBuffers) { record_copy(c, CMD_BIND_VERTEX_BUFFERS, 3, (uint32_t)A(2) * 16); }
NVN(nvnCommandBufferBindVertexBuffer) {
    Snap s = snap(c);
    CmdRec *r = record(c, CMD_BIND_VERTEX_BUFFERS, 16, &s);
    r->x[0] = s.x[1]; r->x[1] = 1;
    memcpy(r->extra, &s.x[2], 16);
}
// (cmd, stage, first, count, const NVNbufferRange*)
NVN(nvnCommandBufferBindUniformBuffers) { record_copy(c, CMD_BIND_UNIFORM_BUFFERS, 4, (uint32_t)A(3) * 16); }
NVN(nvnCommandBufferBindUniformBuffer) {
    Snap s = snap(c);
    CmdRec *r = record(c, CMD_BIND_UNIFORM_BUFFERS, 16, &s);
    r->x[0] = s.x[1]; r->x[1] = s.x[2]; r->x[2] = 1;
    memcpy(r->extra, &s.x[3], 16);
}
NVN(nvnCommandBufferBindStorageBuffers) { record_copy(c, CMD_BIND_STORAGE_BUFFERS, 4, (uint32_t)A(3) * 16); }
NVN(nvnCommandBufferBindStorageBuffer) {
    Snap s = snap(c);
    CmdRec *r = record(c, CMD_BIND_STORAGE_BUFFERS, 16, &s);
    r->x[0] = s.x[1]; r->x[1] = s.x[2]; r->x[2] = 1;
    memcpy(r->extra, &s.x[3], 16);
}
// (cmd, stage, first, count, const handle*)
NVN(nvnCommandBufferBindTextures) { record_copy(c, CMD_BIND_TEXTURES, 4, (uint32_t)A(3) * 8); }
NVN(nvnCommandBufferBindTexture) {
    Snap s = snap(c);
    CmdRec *r = record(c, CMD_BIND_TEXTURES, 8, &s);
    r->x[0] = s.x[1]; r->x[1] = s.x[2]; r->x[2] = 1;
    memcpy(r->extra, &s.x[3], 8);
}
NVN(nvnCommandBufferBindImages) { record_copy(c, CMD_BIND_IMAGES, 4, (uint32_t)A(3) * 8); }
NVN(nvnCommandBufferBindImage) {
    Snap s = snap(c);
    CmdRec *r = record(c, CMD_BIND_IMAGES, 8, &s);
    r->x[0] = s.x[1]; r->x[1] = s.x[2]; r->x[2] = 1;
    memcpy(r->extra, &s.x[3], 8);
}
NVN(nvnCommandBufferBindSeparateTextures) { record_copy(c, CMD_BIND_TEXTURES, 4, (uint32_t)A(3) * 8); }
NVN(nvnCommandBufferBindSeparateTexture) {
    Snap s = snap(c);
    CmdRec *r = record(c, CMD_BIND_TEXTURES, 8, &s);
    r->x[0] = s.x[1]; r->x[1] = s.x[2]; r->x[2] = 1;
    memcpy(r->extra, &s.x[3], 8);
}
NVN(nvnCommandBufferBindSeparateSamplers) { (void)c; }
NVN(nvnCommandBufferBindSeparateSampler) { (void)c; }
NVN(nvnCommandBufferBindTransformFeedbackBuffers) { (void)c; }
NVN(nvnCommandBufferBindTransformFeedbackBuffer) { (void)c; }
NVN(nvnCommandBufferBindCoverageModulationTable) { (void)c; }

NVN(nvnCommandBufferSetTexturePool) {
    Snap s = snap(c);
    CmdRec *r = record(c, CMD_SET_TEXTURE_POOL, 0, &s);
    r->x[0] = (uint64_t)(uintptr_t)HOST(NvnDescPool, s.x[1]);
}
NVN(nvnCommandBufferSetSamplerPool) {
    Snap s = snap(c);
    CmdRec *r = record(c, CMD_SET_SAMPLER_POOL, 0, &s);
    r->x[0] = (uint64_t)(uintptr_t)HOST(NvnDescPool, s.x[1]);
}

// SetRenderTargets(cmd, n, colors[], colorViews[], depth, depthView)
// extra: { u64 tex[8]; NvnTextureView views[8]; u8 has_view[8]; u64 depth; NvnTextureView dview; u8 has_dview }
typedef struct {
    uint64_t tex[8];
    NvnTextureView views[8];
    uint8_t has_view[8];
    uint64_t depth;
    NvnTextureView dview;
    uint8_t has_dview;
} RtRec;
NVN(nvnCommandBufferSetRenderTargets) {
    trace_call(c, "SetRenderTargets");
    Snap s = snap(c);
    int n = (int)s.x[1];
    if (n > 8) n = 8;
    RtRec rt = {0};
    const uint64_t *colors = (const uint64_t *)(uintptr_t)s.x[2], *views = (const uint64_t *)(uintptr_t)s.x[3];
    for (int i = 0; i < n; i++) {
        rt.tex[i] = colors && colors[i] ? (uint64_t)(uintptr_t)HOST(NvnTexture, colors[i]) : 0;
        if (views && views[i]) { rt.has_view[i] = 1; memcpy(&rt.views[i], (void *)(uintptr_t)views[i], sizeof(NvnTextureView)); }
    }
    rt.depth = s.x[4] ? (uint64_t)(uintptr_t)HOST(NvnTexture, s.x[4]) : 0;
    if (s.x[5]) { rt.has_dview = 1; memcpy(&rt.dview, (void *)(uintptr_t)s.x[5], sizeof(NvnTextureView)); }
    CmdRec *r = record(c, CMD_SET_RENDER_TARGETS, sizeof rt, &s);
    r->x[0] = (uint64_t)n;
    memcpy(r->extra, &rt, sizeof rt);
}

// viewports/scissors/depth ranges
NVN(nvnCommandBufferSetViewports) { record_copy(c, CMD_SET_VIEWPORTS, 3, (uint32_t)A(2) * 16); }
NVN(nvnCommandBufferSetViewport) {
    Snap s = snap(c);
    CmdRec *r = record(c, CMD_SET_VIEWPORTS, 16, &s);
    float v[4] = {(float)(int32_t)s.x[1], (float)(int32_t)s.x[2], (float)(int32_t)s.x[3], (float)(int32_t)s.x[4]};
    r->x[0] = 0; r->x[1] = 1;
    memcpy(r->extra, v, 16);
}
NVN(nvnCommandBufferSetScissors) { record_copy(c, CMD_SET_SCISSORS, 3, (uint32_t)A(2) * 16); }
NVN(nvnCommandBufferSetScissor) {
    Snap s = snap(c);
    CmdRec *r = record(c, CMD_SET_SCISSORS, 16, &s);
    int32_t v[4] = {(int32_t)s.x[1], (int32_t)s.x[2], (int32_t)s.x[3], (int32_t)s.x[4]};
    r->x[0] = 0; r->x[1] = 1;
    memcpy(r->extra, v, 16);
}
NVN(nvnCommandBufferSetDepthRanges) { record_copy(c, CMD_SET_DEPTH_RANGES, 3, (uint32_t)A(2) * 8); }
NVN(nvnCommandBufferSetDepthRange) {
    Snap s = snap(c);
    CmdRec *r = record(c, CMD_SET_DEPTH_RANGES, 8, &s);
    r->x[0] = 0; r->x[1] = 1;
    memcpy(r->extra, s.f, 8);
}
NVN(nvnCommandBufferSetBlendColor) { record_copy(c, CMD_SET_BLEND_COLOR, 1, 16); }
NVN(nvnCommandBufferSetStencilRef) { Snap s = snap(c); CmdRec *r = record(c, CMD_SET_STENCIL, 0, &s); r->x[6] = 0; }
NVN(nvnCommandBufferSetStencilMask) { Snap s = snap(c); CmdRec *r = record(c, CMD_SET_STENCIL, 0, &s); r->x[6] = 1; }
NVN(nvnCommandBufferSetStencilValueMask) { Snap s = snap(c); CmdRec *r = record(c, CMD_SET_STENCIL, 0, &s); r->x[6] = 2; }
PLAIN(nvnCommandBufferSetPrimitiveRestart, CMD_SET_PRIMITIVE_RESTART)
PLAIN(nvnCommandBufferSetAlphaRef, CMD_SET_ALPHA_REF)
PLAIN(nvnCommandBufferSetDepthBounds, CMD_SET_DEPTH_BOUNDS)
PLAIN(nvnCommandBufferSetPolygonOffsetClamp, CMD_SET_POLYGON_OFFSET)
PLAIN(nvnCommandBufferSetLineWidth, CMD_SET_LINE_WIDTH)
PLAIN(nvnCommandBufferSetPointSize, CMD_SET_POINT_SIZE)
PLAIN(nvnCommandBufferSetSampleMask, CMD_SET_SAMPLE_MASK)
// Render enable: SetRenderEnable(enable) / SetRenderEnableConditional(mode, counters). The condition compares the
// two 64-bit counter values at counters and counters + 16 (mode 0: render if equal, 1: if not equal) when executed,
// so reports written earlier in the same submission count (runtime/nvn/renderer.m). SMM2 clears a render target
// on its first bind each frame this way: every bind records a clear guarded by a report that the clear resets.
NVN(nvnCommandBufferSetRenderEnable) {
    Snap s = snap(c);
    CmdRec *r = record(c, CMD_SET_RENDER_ENABLE, 0, &s);
    r->x[0] = (uint32_t)s.x[1] != 0; r->x[1] = 0; r->x[2] = 0;
}
NVN(nvnCommandBufferSetRenderEnableConditional) {
    Snap s = snap(c);
    CmdRec *r = record(c, CMD_SET_RENDER_ENABLE, 0, &s);
    r->x[0] = (uint32_t)s.x[1]; r->x[1] = s.x[2]; r->x[2] = 1;
}
NVN(nvnCommandBufferSetDepthClamp) { (void)c; }
NVN(nvnCommandBufferSetRasterizerDiscard) { (void)c; }
NVN(nvnCommandBufferSetConservativeRasterEnable) { (void)c; }
NVN(nvnCommandBufferSetConservativeRasterDilate) { (void)c; }
NVN(nvnCommandBufferSetSubpixelPrecisionBias) { (void)c; }
NVN(nvnCommandBufferSetViewportSwizzles) { (void)c; }
NVN(nvnCommandBufferSetStencilCullCriteria) { (void)c; }
NVN(nvnCommandBufferSetColorReductionEnable) { (void)c; }
NVN(nvnCommandBufferSetColorReductionThresholds) { (void)c; }
NVN(nvnCommandBufferSetTiledCacheAction) { (void)c; }
NVN(nvnCommandBufferSetTiledCacheTileSize) { (void)c; }
NVN(nvnCommandBufferSetShaderScratchMemory) { (void)c; }
NVN(nvnCommandBufferSetPatchSize) { (void)c; }
NVN(nvnCommandBufferSetInnerTessellationLevels) { (void)c; }
NVN(nvnCommandBufferSetOuterTessellationLevels) { (void)c; }
NVN(nvnCommandBufferSetProgramSubroutines) { (void)c; }
NVN(nvnCommandBufferSaveZCullData) { (void)c; }
NVN(nvnCommandBufferRestoreZCullData) { (void)c; }

static uint64_t copy_row_stride, copy_image_stride;
RT_ON_RESET(cmdbuf_reset) { copy_row_stride = copy_image_stride = 0; }
NVN(nvnCommandBufferSetCopyRowStride) {
    Snap s = snap(c);
    copy_row_stride = s.x[1];
    CmdRec *r = record(c, CMD_SET_COPY_STRIDES, 0, &s);
    r->x[0] = copy_row_stride; r->x[1] = copy_image_stride;
}
NVN(nvnCommandBufferSetCopyImageStride) {
    Snap s = snap(c);
    copy_image_stride = s.x[1];
    CmdRec *r = record(c, CMD_SET_COPY_STRIDES, 0, &s);
    r->x[0] = copy_row_stride; r->x[1] = copy_image_stride;
}
NVN(nvnCommandBufferGetCopyRowStride) { RET(copy_row_stride); }
NVN(nvnCommandBufferGetCopyImageStride) { RET(copy_image_stride); }

// ---- clears
NVN(nvnCommandBufferClearColor) { trace_call(c, "ClearColor"); CmdRec *r = record_copy(c, CMD_CLEAR_COLOR, 2, 16); r->x[5] = 0; }  // (kind: x6 is not an argument)
NVN(nvnCommandBufferClearColori) { CmdRec *r = record_copy(c, CMD_CLEAR_COLOR, 2, 16); r->x[5] = 1; }
NVN(nvnCommandBufferClearColorui) { CmdRec *r = record_copy(c, CMD_CLEAR_COLOR, 2, 16); r->x[5] = 2; }
PLAIN(nvnCommandBufferClearDepthStencil, CMD_CLEAR_DEPTH_STENCIL)
// ClearTexture(cmd, texture, view, region, color, mask)
typedef struct { uint64_t tex; NvnTextureView view; uint8_t has_view; int32_t region[6]; float color[4]; } ClearTexRec;
static void clear_texture(Cpu *c, int kind) {
    Snap s = snap(c);
    ClearTexRec ct = {0};
    ct.tex = (uint64_t)(uintptr_t)HOST(NvnTexture, s.x[1]);
    if (s.x[2]) { ct.has_view = 1; memcpy(&ct.view, (void *)(uintptr_t)s.x[2], sizeof ct.view); }
    if (s.x[3]) memcpy(ct.region, (void *)(uintptr_t)s.x[3], sizeof ct.region);
    if (s.x[4]) memcpy(ct.color, (void *)(uintptr_t)s.x[4], 16);
    CmdRec *r = record(c, CMD_CLEAR_TEXTURE, sizeof ct, &s);
    r->x[6] = (uint64_t)kind;
    memcpy(r->extra, &ct, sizeof ct);
}
NVN(nvnCommandBufferClearTexture) { clear_texture(c, 0); }
NVN(nvnCommandBufferClearTexturei) { clear_texture(c, 1); }
NVN(nvnCommandBufferClearTextureui) { clear_texture(c, 2); }
PLAIN(nvnCommandBufferClearBuffer, CMD_CLEAR_BUFFER)
// DiscardColor(cmdbuf, index): the target's contents become undefined. On the Switch a discarded (compressed)
// target reads back cleared; areas the game then leaves undrawn (e.g. a pit after a floor tile falls away) stay
// black there. Treated as a clear to (0,0,0,0) (LARECOMP_KEEP_DISCARD=1: old no-op behaviour).
NVN(nvnCommandBufferDiscardColor) {
    trace_call(c, "DiscardColor");
    static int keep = -1, logged;
    if (keep < 0) keep = getenv("LARECOMP_KEEP_DISCARD") != NULL;
    if (logged < 5) { logged++; LOG("[nvn] DiscardColor index %llu%s\n", (unsigned long long)c->x[1], keep ? " (kept)" : " -> clear"); }
    if (keep) return;
    Snap s = snap(c);
    CmdRec *r = record(c, CMD_CLEAR_COLOR, 16, &s);
    memset(r->extra, 0, 16);
    r->x[2] = 0xF;  // all channels
    r->x[5] = 0;    // float clear
}
NVN(nvnCommandBufferDiscardDepthStencil) { (void)c; }

// ---- draws (all integer arguments fit in x1..x7)
NVN(nvnCommandBufferDrawArrays) { Snap s = snap(c); CmdRec *r = record(c, CMD_DRAW_ARRAYS, 0, &s); r->x[3] = 0; r->x[4] = 1; }
NVN(nvnCommandBufferDrawArraysInstanced) { Snap s = snap(c); record(c, CMD_DRAW_ARRAYS, 0, &s); }
// Debug: LARECOMP_TRACE_COUNT=<n> logs the guest call chain of indexed draws with n indices.
static void trace_draw(Cpu *c, uint64_t count) {
    static long want = -2;
    if (want == -2) want = getenv("LARECOMP_TRACE_COUNT") ? atol(getenv("LARECOMP_TRACE_COUNT")) : -1;
    if (want < 0 || (long)count != want) return;
    static int n;
    if (n++ > 40) return;
    LOG("[trace] draw count %llu lr %llx chain:", (unsigned long long)count, (unsigned long long)c->x[30]);
    uint64_t fp = c->x[29];
    for (int i = 0; i < 14 && fp >= GUEST_LO && fp < GUEST_HI && (fp & 7) == 0; i++) {
        uint64_t next = *(uint64_t *)(uintptr_t)fp, lr = *(uint64_t *)(uintptr_t)(fp + 8);
        LOG(" %llx", (unsigned long long)lr);
        if (next <= fp) break;
        fp = next;
    }
    LOG("\n");
}

NVN(nvnCommandBufferDrawElements) {
    trace_call(c, "DrawElements");
    trace_draw(c, c->x[3]);
    Snap s = snap(c);
    CmdRec *r = record(c, CMD_DRAW_ELEMENTS, 0, &s);
    r->x[4] = 0; r->x[5] = 0; r->x[6] = 1;  // base vertex, base instance, instances
}
NVN(nvnCommandBufferDrawElementsBaseVertex) {
    trace_draw(c, c->x[3]);
    Snap s = snap(c);
    CmdRec *r = record(c, CMD_DRAW_ELEMENTS, 0, &s);
    r->x[5] = 0; r->x[6] = 1;
}
NVN(nvnCommandBufferDrawElementsInstanced) { trace_draw(c, c->x[3]); Snap s = snap(c); record(c, CMD_DRAW_ELEMENTS, 0, &s); }
PLAIN(nvnCommandBufferDrawArraysIndirect, CMD_DRAW_ARRAYS_INDIRECT)
PLAIN(nvnCommandBufferDrawElementsIndirect, CMD_DRAW_ELEMENTS_INDIRECT)
NVN(nvnCommandBufferMultiDrawArraysIndirectCount) { (void)c; }
NVN(nvnCommandBufferMultiDrawElementsIndirectCount) { (void)c; }
NVN(nvnCommandBufferDrawTransformFeedback) { (void)c; }
NVN(nvnCommandBufferDrawTexture) { (void)c; }
PLAIN(nvnCommandBufferDispatchCompute, CMD_DISPATCH)
PLAIN(nvnCommandBufferDispatchComputeIndirect, CMD_DISPATCH_INDIRECT)
NVN(nvnCommandBufferBeginTransformFeedback) { (void)c; }
NVN(nvnCommandBufferEndTransformFeedback) { (void)c; }
NVN(nvnCommandBufferPauseTransformFeedback) { (void)c; }
NVN(nvnCommandBufferResumeTransformFeedback) { (void)c; }

// ---- copies
typedef struct { uint64_t tex; NvnTextureView view; uint8_t has_view; int32_t region[6]; } TexRef;
static void tex_ref(TexRef *t, uint64_t tex, uint64_t view, uint64_t region) {
    t->tex = tex ? (uint64_t)(uintptr_t)HOST(NvnTexture, tex) : 0;
    if (view) { t->has_view = 1; memcpy(&t->view, (void *)(uintptr_t)view, sizeof t->view); }
    if (region) memcpy(t->region, (void *)(uintptr_t)region, sizeof t->region);
}
NVN(nvnCommandBufferCopyBufferToTexture) {  // (cmd, src, dst, dstView, dstRegion, flags)
    Snap s = snap(c);
    TexRef t = {0};
    tex_ref(&t, s.x[2], s.x[3], s.x[4]);
    CmdRec *r = record(c, CMD_COPY_BUFFER_TO_TEXTURE, sizeof t, &s);
    memcpy(r->extra, &t, sizeof t);
}
NVN(nvnCommandBufferCopyTextureToBuffer) {  // (cmd, src, srcView, srcRegion, dst, flags)
    Snap s = snap(c);
    TexRef t = {0};
    tex_ref(&t, s.x[1], s.x[2], s.x[3]);
    CmdRec *r = record(c, CMD_COPY_TEXTURE_TO_BUFFER, sizeof t, &s);
    memcpy(r->extra, &t, sizeof t);
}
NVN(nvnCommandBufferCopyTextureToTexture) {  // (cmd, src, srcView, srcRegion, dst, dstView, dstRegion, flags)
    Snap s = snap(c);
    TexRef t[2] = {{0}};
    tex_ref(&t[0], s.x[1], s.x[2], s.x[3]);
    tex_ref(&t[1], s.x[4], s.x[5], s.x[6]);
    CmdRec *r = record(c, CMD_COPY_TEXTURE_TO_TEXTURE, sizeof t, &s);
    memcpy(r->extra, t, sizeof t);
}
PLAIN(nvnCommandBufferCopyBufferToBuffer, CMD_COPY_BUFFER_TO_BUFFER)
NVN(nvnCommandBufferDownsample) {
    Snap s = snap(c);
    TexRef t[2] = {{0}};
    tex_ref(&t[0], s.x[1], 0, 0);
    tex_ref(&t[1], s.x[2], 0, 0);
    CmdRec *r = record(c, CMD_DOWNSAMPLE, sizeof t, &s);
    memcpy(r->extra, t, sizeof t);
}
NVN(nvnCommandBufferDownsampleTextureView) {
    Snap s = snap(c);
    TexRef t[2] = {{0}};
    tex_ref(&t[0], s.x[1], s.x[2], 0);
    tex_ref(&t[1], s.x[3], s.x[4], 0);
    CmdRec *r = record(c, CMD_DOWNSAMPLE, sizeof t, &s);
    memcpy(r->extra, t, sizeof t);
}
NVN(nvnCommandBufferTiledDownsample) { (void)c; }
NVN(nvnCommandBufferTiledDownsampleTextureView) { (void)c; }
NVN(nvnCommandBufferResolveDepthBuffer) { (void)c; }
// UpdateUniformBuffer(cmd, buffer, size, offset, dataSize, data)
NVN(nvnCommandBufferUpdateUniformBuffer) { record_copy(c, CMD_UPDATE_UNIFORM_BUFFER, 5, (uint32_t)A(4)); }

// ---- synchronization and misc
NVN(nvnCommandBufferFenceSync) {
    // work executes at submit time, so the sync is satisfied once this list has run
    Snap s = snap(c);
    record(c, CMD_FENCE_SYNC, 0, &s);
}
PLAIN(nvnCommandBufferWaitSync, CMD_WAIT_SYNC)
PLAIN(nvnCommandBufferBarrier, CMD_BARRIER)
PLAIN(nvnCommandBufferSignalEvent, CMD_SIGNAL_EVENT)
PLAIN(nvnCommandBufferWaitEvent, CMD_WAIT_EVENT)
PLAIN(nvnCommandBufferReportCounter, CMD_REPORT_COUNTER)
// ReportValue(value, address): a 16-byte report {value, timestamp}, written when executed (in order with the
// conditional render checks that read it).
PLAIN(nvnCommandBufferReportValue, CMD_REPORT_VALUE)
NVN(nvnCommandBufferResetCounter) { (void)c; }
NVN(nvnCommandBufferCallCommands) { trace_call(c, "CallCommands"); record_copy(c, CMD_CALL, 2, (uint32_t)A(1) * 8); }
// CopyCommands copies the lists' commands into this one now: the game may reuse their memory right after (LAS
// recorded a call, which then ran whatever had been written there since)
NVN(nvnCommandBufferCopyCommands) {
    trace_call(c, "CopyCommands");
    Snap s = snap(c);
    NvnCmdBuf *cb = cb_of(s.x[0]);
    uint64_t n = s.x[1];
    uint64_t *handles = malloc(n * 8 + 1);
    memcpy(handles, (void *)(uintptr_t)s.x[2], n * 8);
    for (uint64_t i = 0; i < n; i++) {
        if (!handles[i]) continue;
        for (uint64_t p = handles[i], guard = 0; guard < 1000000; guard++) {
            CmdRec *r = (CmdRec *)(uintptr_t)p;
            if (r->op == CMD_END) break;
            if (r->op == CMD_JUMP) { p = r->x[0]; continue; }
            if (r->op >= CMD_COUNT || r->size < sizeof(CmdRec)) break;
            uint32_t size = r->size;
            CmdRec *d = rec(c, cb, r->op, size - (uint32_t)sizeof(CmdRec));
            memcpy(d, (void *)(uintptr_t)p, size);
            p += size;
        }
    }
    free(handles);
}
NVN(nvnCommandBufferInsertDebugMarker) { (void)c; }
NVN(nvnCommandBufferInsertDebugMarkerDynamic) { (void)c; }
NVN(nvnCommandBufferInsertDebugMarkerStatic) { (void)c; }
NVN(nvnCommandBufferPushDebugGroup) { (void)c; }
NVN(nvnCommandBufferPushDebugGroupDynamic) { (void)c; }
NVN(nvnCommandBufferPushDebugGroupStatic) { (void)c; }
NVN(nvnCommandBufferPopDebugGroup) { (void)c; }
NVN(nvnCommandBufferPopDebugGroupId) { (void)c; }
