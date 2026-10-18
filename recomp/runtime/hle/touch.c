// The console's touch screen (nn::hid::GetTouchScreenState<N>): touches on iOS / iPadOS (and any touch screen SDL
// reports), and the left mouse button on the desktop - SMM2's Course Maker is made for the touch screen. Positions
// are in the game's 1280x720 screen space, through where the game image is drawn in the window (the presenters call
// input_set_game_rect every frame). RECOMP_NO_MOUSE_TOUCH=1: the mouse doesn't touch.
#include <SDL3/SDL.h>
#include <pthread.h>
#include <stdlib.h>
#include <string.h>

#include "../hle.h"

#define MAX_TOUCH 16
#define MOUSE_FINGER_ID ((SDL_FingerID)0x4d4f555345ull)  // ("MOUSE": the desktop mouse as a finger)
typedef struct {
    SDL_FingerID id;  // 0 = free slot
    int32_t x, y;
    uint8_t started;  // not reported yet: the next sample carries the Start attribute
    uint8_t ended;    // released: reported once more with the End attribute, then freed
    uint32_t finger;  // finger id the game sees (small, stable while the touch lasts)
} Touch;
static Touch touches[MAX_TOUCH];
static pthread_mutex_t lock = PTHREAD_MUTEX_INITIALIZER;
static double rect[6] = {0, 0, 1, 1, 1, 1};  // game image x, y, w, h in the drawable, drawable w, h
static uint32_t next_finger;

void input_set_game_rect(double x, double y, double w, double h, double dw, double dh) {
    pthread_mutex_lock(&lock);
    rect[0] = x, rect[1] = y, rect[2] = w, rect[3] = h, rect[4] = dw, rect[5] = dh;
    pthread_mutex_unlock(&lock);
}

// (lock held) a point in the window, as a fraction of its size -> game screen pixels (clamped to the screen)
static void to_game(float fx, float fy, int32_t *gx, int32_t *gy) {
    double px = fx * rect[4], py = fy * rect[5];
    double x = rect[2] > 0 ? (px - rect[0]) / rect[2] * 1280.0 : 0, y = rect[3] > 0 ? (py - rect[1]) / rect[3] * 720.0 : 0;
    *gx = x < 0 ? 0 : x > 1279 ? 1279 : (int32_t)x;
    *gy = y < 0 ? 0 : y > 719 ? 719 : (int32_t)y;
}

// A two-finger tap (both fingers down together, up within 0.4 s): L + R for a few frames. Without a controller (iPad,
// iPhone) that is what the title screen asks for; the menus after it take touches themselves.
static int g_fingers, g_max_fingers;
static uint64_t g_first_down_ns;
static _Atomic int g_lr_frames;
uint64_t touch_gesture_buttons(void) {
    int n = g_lr_frames;
    if (n <= 0) return 0;
    g_lr_frames = n - 1;
    return (1ull << 6) | (1ull << 7);  // L, R
}
static void gesture(int kind) {  // (lock held)
    uint64_t now = SDL_GetTicksNS();
    if (kind == 0) {
        if (g_fingers++ == 0) { g_first_down_ns = now; g_max_fingers = 0; }
        if (g_fingers > g_max_fingers) g_max_fingers = g_fingers;
    } else if (kind == 2 && g_fingers > 0 && --g_fingers == 0) {
        if (g_max_fingers == 2 && now - g_first_down_ns < 400000000ull) { g_lr_frames = 8; LOG("[touch] two-finger tap: L + R\n"); }
    }
}

static void touch_event(SDL_FingerID id, float fx, float fy, int kind) {  // kind 0 down, 1 move, 2 up
    pthread_mutex_lock(&lock);
    if (id != MOUSE_FINGER_ID) gesture(kind);
    Touch *t = NULL;
    for (int i = 0; i < MAX_TOUCH && !t; i++)
        if (touches[i].id == id && !touches[i].ended) t = &touches[i];
    if (kind == 0 && !t)
        for (int i = 0; i < MAX_TOUCH && !t; i++)
            if (!touches[i].id) { t = &touches[i]; memset(t, 0, sizeof *t); t->id = id; t->started = 1; t->finger = next_finger++; }
    if (t) {
        to_game(fx, fy, &t->x, &t->y);
        if (kind == 2) t->ended = 1;
    }
    pthread_mutex_unlock(&lock);
}

// (tests: LARECOMP_HOST_INPUT "frame:tap:x/y:frames", runtime/hle/input.c) a touch at game pixels
#define TEST_FINGER ((SDL_FingerID)0x54455354ull)
void touch_test(int32_t x, int32_t y, bool down) {
    pthread_mutex_lock(&lock);
    Touch *t = NULL;
    for (int i = 0; i < MAX_TOUCH && !t; i++)
        if (touches[i].id == TEST_FINGER && !touches[i].ended) t = &touches[i];
    if (down && !t)
        for (int i = 0; i < MAX_TOUCH && !t; i++)
            if (!touches[i].id) { t = &touches[i]; memset(t, 0, sizeof *t); t->id = TEST_FINGER; t->started = 1; t->finger = next_finger++; }
    if (t) { t->x = x, t->y = y; if (!down) t->ended = 1; }
    pthread_mutex_unlock(&lock);
    LOG("[touch] test %s at %d,%d\n", down ? "down" : "up", x, y);
}

#define MOUSE_FINGER MOUSE_FINGER_ID

bool touch_handle_event(const SDL_Event *e) {
    static int no_mouse = -1;
    if (no_mouse < 0) no_mouse = getenv("RECOMP_NO_MOUSE_TOUCH") != NULL;
    switch (e->type) {
    case SDL_EVENT_FINGER_DOWN: case SDL_EVENT_FINGER_MOTION: case SDL_EVENT_FINGER_UP: case SDL_EVENT_FINGER_CANCELED:
        touch_event(e->tfinger.fingerID + 1, e->tfinger.x, e->tfinger.y,
                    e->type == SDL_EVENT_FINGER_DOWN ? 0 : e->type == SDL_EVENT_FINGER_MOTION ? 1 : 2);
        return true;
    case SDL_EVENT_MOUSE_BUTTON_DOWN: case SDL_EVENT_MOUSE_BUTTON_UP: case SDL_EVENT_MOUSE_MOTION: {
        if (no_mouse || e->button.which == SDL_TOUCH_MOUSEID) return false;  // (mice SDL makes up from touches)
        if (e->type != SDL_EVENT_MOUSE_MOTION && e->button.button != SDL_BUTTON_LEFT) return false;
        if (e->type == SDL_EVENT_MOUSE_MOTION && !(e->motion.state & SDL_BUTTON_LMASK)) return false;
        SDL_Window *w = SDL_GetWindowFromID(e->type == SDL_EVENT_MOUSE_MOTION ? e->motion.windowID : e->button.windowID);
        int ww = 1, wh = 1;
        if (w) SDL_GetWindowSize(w, &ww, &wh);
        float x = e->type == SDL_EVENT_MOUSE_MOTION ? e->motion.x : e->button.x;
        float y = e->type == SDL_EVENT_MOUSE_MOTION ? e->motion.y : e->button.y;
        touch_event(MOUSE_FINGER, x / (ww > 0 ? ww : 1), y / (wh > 0 ? wh : 1),
                    e->type == SDL_EVENT_MOUSE_BUTTON_DOWN ? 0 : e->type == SDL_EVENT_MOUSE_MOTION ? 1 : 2);
        return true;
    }
    default:
        return false;
    }
}

// TouchScreenState<N> {s64 sampling_number; s32 count; u32 reserved; TouchState touches[N]}, TouchState (0x28) {s64
// delta_time; u32 attributes (1 start, 2 end); u32 finger_id; u32 x, y; u32 diameter_x, diameter_y; u32 rotation; u32
// reserved} (switchbrew HidTouchScreenState). Each call is one new sample.
static void fill_touch(Cpu *c, int n) {
    uint8_t *out = (uint8_t *)(uintptr_t)c->x[0];
    static int64_t sampling;
    static int first;
    if (!first++) LOG("[touch] the game reads the touch screen (%d touches per sample)\n", n);
    memset(out, 0, 16 + 0x28 * (size_t)n);
    pthread_mutex_lock(&lock);
    *(int64_t *)out = ++sampling;
    int k = 0;
    for (int i = 0; i < MAX_TOUCH && k < n; i++) {
        Touch *t = &touches[i];
        if (!t->id) continue;
        uint8_t *s = out + 16 + 0x28 * k++;
        *(uint32_t *)(s + 8) = (t->started ? 1u : 0u) | (t->ended ? 2u : 0u);
        *(uint32_t *)(s + 12) = t->finger;
        *(uint32_t *)(s + 16) = (uint32_t)t->x;
        *(uint32_t *)(s + 20) = (uint32_t)t->y;
        *(uint32_t *)(s + 24) = 15;  // (a fingertip)
        *(uint32_t *)(s + 28) = 15;
        t->started = 0;
        if (t->ended) t->id = 0;
    }
    *(int32_t *)(out + 8) = k;
    pthread_mutex_unlock(&lock);
    static int nlog;
    if (k && nlog < 8) { nlog++; LOG("[touch] %d touch(es), first at %u,%u\n", k, *(uint32_t *)(out + 32), *(uint32_t *)(out + 36)); }
}

HLE_FUNC("_ZN2nn3hid21InitializeTouchScreenEv", hid_touch_init) { (void)c; LOG("[touch] InitializeTouchScreen\n"); }
HLE_FUNC("_ZN2nn3hid19GetTouchScreenStateILm1EEEvPNS0_16TouchScreenStateIXT_EEE", hid_touch_state1) { fill_touch(c, 1); }
HLE_FUNC("_ZN2nn3hid19GetTouchScreenStateILm5EEEvPNS0_16TouchScreenStateIXT_EEE", hid_touch_state5) { fill_touch(c, 5); }
HLE_FUNC("_ZN2nn3hid19GetTouchScreenStateILm16EEEvPNS0_16TouchScreenStateIXT_EEE", hid_touch_state16) { fill_touch(c, 16); }
