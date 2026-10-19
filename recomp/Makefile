# Recomp build (forked from LAS Recomp's Makefile; see UPSTREAM.md).
#
#   make smm2                      the whole pipeline for Super Mario Maker 2 v3.0.3 from your own dump:
#                                  set up the game files, recompile main + sdk to C, build, (then `make run-smm2`)
#   make GAME=smm2 <target>        one step: setup | gen | all | run | runtime-objs | clean-gen
#
# The shared runtime is runtime/ (kernel, IPC, services, NVN on Metal / Vulkan, platform layers) and the recompiler
# is tools/. A game is games/<game>/: game.mk (identity, modules, dump layout) and its own glue code.
# Everything made from the dump goes to private/ (game files), generated/ (recompiled C) and build/; git ignores
# them and ../scripts/check_no_proprietary.py blocks them. They may be symlinks to another disk.
#
# macOS: Homebrew clang/SDL3/FFmpeg, Metal renderer.  Linux: clang + Vulkan.  Windows: MSYS2 UCRT64 + Vulkan.
# Cross builds (from macOS or any host, with zig as the compiler: tools/fetch_zig.sh, or ZIG=<zig>):
#   make TARGET=linux-aarch64 | linux-x86_64 | windows-x86_64 GAME=smm2 all   (DEPS=<headers + link libraries>)
# iOS / iPadOS (Xcode's clang; the game code is linked into the app - nothing compiles on the device):
#   make TARGET=ios-arm64 | iossim-arm64 GAME=smm2 DEPS=<SDL3 for iOS> app   (tools/make_ios_app.sh signs it)
UNAME := $(shell uname -s)
comma := ,
TARGET ?=
ifneq ($(TARGET),)
PLATFORM := $(word 1,$(subst -, ,$(TARGET)))
ARCH     := $(word 2,$(subst -, ,$(TARGET)))
else ifneq ($(filter MINGW% MSYS% CYGWIN% Windows_NT,$(UNAME) $(OS)),)
PLATFORM := windows
else ifeq ($(UNAME),Linux)
PLATFORM := linux
else
PLATFORM := macos
endif

GAME     ?= smm2
GAMEDIR  := games/$(GAME)
GAME_MOVIES := 1  # (runtime/hle/movie.c + FFmpeg: games whose code uses the movie library; game.mk may say 0)
include $(GAMEDIR)/game.mk

# Game files (set up by `make setup` from the dump), recompiled code, build products: one folder each per game.
DATA     ?= private/$(GAME)
GAMEFILES := $(DATA)/extracted
GEN      ?= generated/$(GAME)
B        ?= build/$(GAME)

# The guest window's base (runtime/rt.h GUEST_ORIGIN): the game code is generated for it. Another base gets its own
# generated/ and build/ folders.
GUEST_ORIGIN ?= 0x7000000000

IOS := $(filter ios iossim,$(PLATFORM))
ifneq ($(IOS),)
# iOS / the Simulator: Xcode's clang for the SDK. The game code is generated/<game> (arm64, as macOS), compiled into
# the app's executable (iOS loads no code it didn't ship with: no game code library, no first-launch build).
IOS_SDK  := $(if $(filter iossim,$(PLATFORM)),iphonesimulator,iphoneos)
IOS_MIN  ?= 17.0
IOS_TRIPLE := arm64-apple-ios$(IOS_MIN)$(if $(filter iossim,$(PLATFORM)),-simulator)
CROSS_CC := xcrun --sdk $(IOS_SDK) clang -target $(IOS_TRIPLE)
CROSS_CXX := xcrun --sdk $(IOS_SDK) clang++ -target $(IOS_TRIPLE)
B        := build/$(GAME)-$(TARGET)
else ifneq ($(TARGET),)
# zig cc / c++ for the target; the generated code depends only on the CPU's FP/SIMD lowering (asm on aarch64, C on
# x86-64), so targets share it: generated/<game> (aarch64, as macOS) or generated/<game>-c (x86-64)
ZIG      ?= $(firstword $(wildcard build/zig/zig) zig)
ZTARGET  := $(ARCH)-$(if $(filter windows,$(PLATFORM)),windows-gnu,linux-gnu.2.36)
CROSS_CC := $(ZIG) cc -target $(ZTARGET)
CROSS_CXX := $(ZIG) c++ -target $(ZTARGET)
B        := build/$(GAME)-$(TARGET)
ifeq ($(ARCH),x86_64)
GEN      := generated/$(GAME)-c
endif
GENSIMD  := --simd $(if $(filter aarch64,$(ARCH)),asm,c)
else ifeq ($(PLATFORM),linux)
HOSTARCH := $(patsubst arm64,aarch64,$(shell uname -m))
ARCH     ?= $(HOSTARCH)
ifneq ($(ARCH),$(HOSTARCH))
CROSS    := --target=$(ARCH)-linux-gnu $(if $(SYSROOT),--sysroot=$(SYSROOT))
GEN      := generated/$(GAME)-$(ARCH)
B        := build/$(GAME)-$(ARCH)
endif
GENSIMD  := --simd $(if $(filter aarch64,$(ARCH)),asm,c)
endif

MACOS_MIN ?=
DEPS      ?=
MINFLAG  := $(if $(MACOS_MIN),-mmacosx-version-min=$(MACOS_MIN))
CC       := $(or $(CROSS_CC),clang $(MINFLAG) $(CROSS))
CXX      := $(or $(CROSS_CXX),clang++ $(MINFLAG) $(CROSS))
LLD      := $(if $(CROSS_CC),,-fuse-ld=lld)  # (zig links with its own lld)
PLATFORM_SRC := runtime/platform/$(if $(IOS),macos,$(PLATFORM)).c  # (iOS: the macOS layer, TARGET_OS_IPHONE inside)
IMGUI    := third_party/imgui
GENFLAGS := -O1 -fno-strict-aliasing -Wno-return-type -Wno-unused-label -fvisibility=hidden -Iruntime
GAMEDEFS := -DGAME_TITLE_ID=$(GAME_TITLE_ID)ull -DGAME_ID='"$(GAME_ID)"' -DGAME_TITLE='"$(GAME_TITLE)"' -DGAME_BUNDLE_ID='"$(GAME_BUNDLE_ID)"' \
            -DGAME_DISPLAY_VERSION='"$(GAME_DISPLAY_VERSION)"'

ifneq ($(GUEST_ORIGIN),0x7000000000)
GEN      := $(GEN)-$(GUEST_ORIGIN)
B        := $(B)-$(GUEST_ORIGIN)
GAMEDEFS += -DGUEST_ORIGIN=$(GUEST_ORIGIN)ull
endif
GEN_SRC  := $(wildcard $(GEN)/main/*.c) $(wildcard $(GEN)/sdk/*.c)
GEN_OBJ  := $(patsubst %.c,$(B)/obj/%.o,$(patsubst $(GEN)/%,%,$(GEN_SRC))) $(B)/obj/tools/gamelib_entry.o
RT_SRC   := $(wildcard runtime/*.c) $(wildcard runtime/hle/*.c) $(wildcard runtime/services/*.c) \
            $(wildcard runtime/nvn/*.c) $(wildcard runtime/nvn/maxwell/*.c) $(PLATFORM_SRC) \
            $(wildcard runtime/net/*.c) \
            $(GAME_SRC)
ifeq ($(PLATFORM),windows)
RT_SRC   += runtime/platform/windows_exports.c  # (what the game code DLL calls in the exe)
endif
ifeq ($(strip $(GAME_MOVIES)),0)
RT_SRC   := $(filter-out runtime/hle/movie.c,$(RT_SRC))
FFMPEG_LIBS :=
else
FFMPEG_LIBS := -lavformat -lavcodec -lavutil -lswscale
endif
RT_HDRS  := $(wildcard runtime/*.h) $(wildcard runtime/platform/*.h) $(wildcard runtime/nvn/*.h) \
            $(wildcard runtime/nvn/maxwell/*.h) $(wildcard runtime/nvn/vulkan/*.h) $(wildcard runtime/ui/*.h) \
            $(wildcard runtime/net/*.h) \
            $(GAMEDIR)/game.mk
UI_SRC   := $(wildcard runtime/ui/*.mm)
UI_OBJ   := $(patsubst runtime/%.mm,$(B)/rt/%.o,$(UI_SRC))
IMGUI_OBJ := $(B)/imgui/imgui.o $(B)/imgui/imgui_draw.o $(B)/imgui/imgui_tables.o $(B)/imgui/imgui_widgets.o \
             $(B)/imgui/imgui_impl_sdl3.o

ifneq ($(IOS),)
SDL      := $(or $(DEPS),/Volumes/SSD/smm-work/deps/ios-$(if $(filter iossim,$(PLATFORM)),sim,device))
EXE      := $(B)/$(GAME_ID)recomp
DEPINC   := -I$(SDL)/include
OSFLAGS  := -DGAMELIB_STATIC
RT_SRC   := $(filter-out runtime/gamebuild.c,$(RT_SRC))
LDFLAGS  := -L$(SDL)/lib -lSDL3 -framework CoreFoundation -framework Foundation -framework Metal $(if $(filter ios,$(PLATFORM)),-framework MetalFX) \
            -framework QuartzCore -framework UIKit -framework ImageIO -framework CoreGraphics -framework UniformTypeIdentifiers \
            -framework AVFoundation -framework AudioToolbox -framework CoreAudio -framework CoreHaptics -framework CoreMotion \
            -framework GameController -framework CoreBluetooth -framework OpenGLES -framework CoreVideo -framework CoreMedia -framework CoreServices
BACKEND_SRC := $(wildcard runtime/nvn/*.m)
BACKEND_OBJ := $(patsubst runtime/%.m,$(B)/rt/%.o,$(BACKEND_SRC))
IMGUI_OBJ += $(B)/imgui/imgui_impl_metal.o
UIFLAGS  := -fobjc-arc
GAMELIB  :=
EXE_GAME_OBJ := $(GEN_OBJ)  # (linked in)
APP      := $(B)/SMM2Recomp.app
else ifeq ($(PLATFORM),macos)
SDL      := $(or $(DEPS),/opt/homebrew/opt/sdl3)
FFMPEG   := $(or $(DEPS),/opt/homebrew/opt/ffmpeg)
EXE      := $(B)/$(GAME_ID)recomp
DEPINC   := -I$(SDL)/include -I$(FFMPEG)/include
LDFLAGS  := -L$(SDL)/lib -lSDL3 -Wl,-rpath,$(SDL)/lib $(if $(FFMPEG_LIBS),-L$(FFMPEG)/lib $(FFMPEG_LIBS) -Wl$(comma)-rpath$(comma)$(FFMPEG)/lib) \
            -framework CoreFoundation -framework Foundation -framework Metal -framework MetalFX -framework QuartzCore -framework AppKit -framework ImageIO -framework CoreGraphics -framework UniformTypeIdentifiers
BACKEND_SRC := $(wildcard runtime/nvn/*.m)
BACKEND_OBJ := $(patsubst runtime/%.m,$(B)/rt/%.o,$(BACKEND_SRC))
IMGUI_OBJ += $(B)/imgui/imgui_impl_metal.o
UIFLAGS  := -fobjc-arc
GAMELIB  := $(B)/$(GAME_ID)game.dylib
GAMELIB_LDFLAGS := -dynamiclib -undefined dynamic_lookup  # (the runtime functions it calls: the app's own)
GAMELIB_SHARED := -shared -Wl,-undefined,dynamic_lookup
else ifeq ($(PLATFORM),linux)
EXE      := $(B)/$(GAME_ID)recomp
DEPINC   := $(if $(DEPS),-I$(DEPS)/include)
ifeq ($(ARCH),x86_64)
GENARCH  ?= $(if $(CROSS_CC),-mcpu=x86_64_v3,-march=x86-64-v3)  # AVX2 + FMA for the lifted FP/SIMD code (checked at startup; zig: -mcpu)
endif
GENFLAGS += $(GENARCH) -fPIC
OSFLAGS  := -Wno-format
SHADERC  ?= -lshaderc
LDFLAGS  := $(LLD) -rdynamic $(if $(DEPS),-L$(DEPS)/lib) -lSDL3 $(FFMPEG_LIBS) \
            -lvulkan $(SHADERC) -lpthread -ldl -lm $(if $(CROSS_CC),-Wl$(comma)--allow-shlib-undefined -Wl$(comma)-rpath$(comma)'$$ORIGIN/lib')
BACKEND_SRC := $(wildcard runtime/nvn/vulkan/*.c) $(wildcard runtime/nvn/vulkan/*.cpp)
BACKEND_OBJ := $(patsubst runtime/%.cpp,$(B)/rt/%.o,$(patsubst runtime/%.c,$(B)/rt/%.o,$(BACKEND_SRC)))
IMGUI_OBJ += $(B)/imgui/imgui_impl_vulkan.o
UIFLAGS  := -x c++
GAMELIB  := $(B)/$(GAME_ID)game.so
GAMELIB_LDFLAGS := -shared $(LLD)
GAMELIB_SHARED := -shared
else
EXE      := $(B)/$(GAME_ID)recomp.exe
DEPINC   := $(if $(DEPS),-I$(DEPS)/include)
OSFLAGS  := -D_FILE_OFFSET_BITS=64 -D_POSIX_THREAD_SAFE_FUNCTIONS -D_CRT_RAND_S -DWIN32_LEAN_AND_MEAN -DNOMINMAX
GENARCH  ?= $(if $(CROSS_CC),-mcpu=x86_64_v3,-march=x86-64-v3)
GENFLAGS += -D_FILE_OFFSET_BITS=64 $(GENARCH)
LDFLAGS  := $(LLD) -Wl,--stack,16777216 $(if $(DEPS),-L$(DEPS)/lib) -lSDL3 $(FFMPEG_LIBS) \
            -lvulkan-1 -lwinmm -lole32 -lshell32 -lcomdlg32 -luuid -lshaderc_shared
BACKEND_SRC := $(wildcard runtime/nvn/vulkan/*.c) $(wildcard runtime/nvn/vulkan/*.cpp)
BACKEND_OBJ := $(patsubst runtime/%.cpp,$(B)/rt/%.o,$(patsubst runtime/%.c,$(B)/rt/%.o,$(BACKEND_SRC)))
IMGUI_OBJ += $(B)/imgui/imgui_impl_vulkan.o
UIFLAGS  := -x c++ $(OSFLAGS)
GAMELIB  := $(B)/$(GAME_ID)game.dll
LDFLAGS  += -Wl,--export-all-symbols -Wl,--out-implib,$(B)/$(GAME_ID)recomp.dll.a
GAMELIB_LDFLAGS := -shared $(LLD) $(B)/$(GAME_ID)recomp.dll.a
GAMELIB_SHARED := -shared
endif

# GAMELIB_ID: what the game code library is built from (the generator, the headers it is compiled against, the
# flags). A library with another id is never loaded.
GAMELIB_SRCS := tools/gen.py tools/lifter.py tools/analyze.py tools/nso.py tools/passes.py tools/patches.py \
                tools/simd_c.py tools/gamelib_entry.c $(GAME_HLE) $(wildcard $(GAME_PATCHES)) $(GAME_PATCH_DEPS) \
                runtime/cpu.h runtime/simd.h runtime/simd_fp.h runtime/gamelib.h
GAMELIB_ID := $(shell (cat $(GAMELIB_SRCS); echo '$(GENFLAGS)') | (shasum -a 256 2>/dev/null || sha256sum) | cut -c1-16)

RT_OBJ   := $(patsubst %.c,$(B)/rt/%.o,$(patsubst runtime/%,%,$(RT_SRC)))
# The course server inside the app (core/crates/embed, runtime/net/embed_server.h): linked when its library is built
# (`make embed-server`: cargo) or EMBED_SERVER=1, Apple builds for now; a build never needs Rust otherwise.
EMBED_TRIPLE := $(if $(filter ios,$(PLATFORM)),aarch64-apple-ios,$(if $(filter iossim,$(PLATFORM)),aarch64-apple-ios-sim,$(if $(filter macos,$(PLATFORM)),aarch64-apple-darwin)))
EMBED_LIB := $(if $(EMBED_TRIPLE),../core/target/$(EMBED_TRIPLE)/release/libmmk_embed.a)
EMBED_SERVER ?= $(if $(wildcard $(EMBED_LIB)),1,0)
ifeq ($(EMBED_SERVER),1)
OSFLAGS  += -DRECOMP_EMBED_SERVER
LDFLAGS  += $(EMBED_LIB)
endif

RTFLAGS  := -O2 -g -fno-strict-aliasing -Wall -Wno-unused-function -Iruntime -Iruntime/nvn $(DEPINC) $(OSFLAGS) $(GAMEDEFS)
CXXFLAGS := -O2 -g -std=c++17 -I$(IMGUI) -I$(IMGUI)/backends -Iruntime -Iruntime/nvn $(DEPINC) $(GAMEDEFS)

all: $(EXE) $(if $(GEN_SRC),$(GAMELIB))

$(B)/obj/%.o: $(GEN)/%.c runtime/cpu.h
	@mkdir -p $(dir $@)
	@$(CC) $(GENFLAGS) -c $< -o $@

$(B)/obj/tools/gamelib_entry.o: tools/gamelib_entry.c $(GAMELIB_SRCS)
	@mkdir -p $(dir $@)
	$(CC) $(GENFLAGS) -DGAMELIB_ID='"$(GAMELIB_ID)"' $(GAMEDEFS) -c $< -o $@
$(B)/rt/gamelib.o: $(GAMELIB_SRCS)
$(B)/rt/gamelib.o: RTFLAGS += -DGAMELIB_ID='"$(GAMELIB_ID)"'
$(B)/rt/gamebuild.o: $(GAMELIB_SRCS) Makefile
$(B)/rt/gamebuild.o: RTFLAGS += -DGAMELIB_ID='"$(GAMELIB_ID)"' -DGAMELIB_CFLAGS='"$(strip $(filter-out -Iruntime,$(GENFLAGS)))"' \
                                -DGAMELIB_SHARED='"$(GAMELIB_SHARED)"' -DGAMEBUILD_HLE='"$(GAME_HLE)"' \
                                -DGAMEBUILD_PATCHES='"$(GAME_PATCHES)"'

$(B)/rt/net/embed_server.o: $(wildcard $(EMBED_LIB))  # (built again when the library appears)

$(B)/rt/%.o: runtime/%.c $(RT_HDRS)
	@mkdir -p $(dir $@)
	$(CC) $(RTFLAGS) -c $< -o $@

# a game's own glue (games/<game>/*.c), compiled like the runtime
$(B)/rt/games/%.o: games/%.c $(RT_HDRS)
	@mkdir -p $(dir $@)
	$(CC) $(RTFLAGS) -c $< -o $@

# SMAA (third_party/smaa, MIT): the reference shader as a C string for runtime/nvn/smaa_data.c
$(B)/inc/smaa_hlsl.inc: third_party/smaa/SMAA.hlsl
	@mkdir -p $(dir $@)
	LC_ALL=C tr -d '\r' < $< | LC_ALL=C tr '\200-\377' '?' | sed -e 's/\\/\\\\/g' -e 's/"/\\"/g' -e 's/^/"/' -e 's/$$/\\n"/' > $@
$(B)/rt/nvn/smaa_data.o: $(B)/inc/smaa_hlsl.inc
$(B)/rt/nvn/smaa_data.o: RTFLAGS += -I$(B)/inc -Ithird_party/smaa/Textures

$(B)/rt/%.o: runtime/%.cpp $(RT_HDRS)
	@mkdir -p $(dir $@)
	$(CXX) $(CXXFLAGS) $(OSFLAGS) -c $< -o $@

$(B)/rt/%.o: runtime/%.m $(RT_HDRS)
	@mkdir -p $(dir $@)
	$(CC) $(RTFLAGS) -fobjc-arc -c $< -o $@

# linked to a temp file, then renamed over: a copy still running keeps its (old) file
$(EXE): $(RT_OBJ) $(BACKEND_OBJ) $(UI_OBJ) $(IMGUI_OBJ) $(EXE_GAME_OBJ)
	@mkdir -p $(dir $@)link
	$(CXX) -o $(dir $@)link/$(notdir $@) $^ $(LDFLAGS)
	mv -f $(dir $@)link/$(notdir $@) $@
	$(if $(and $(CROSS_CC),$(filter windows,$(PLATFORM))),(echo LIBRARY $(notdir $@); cat runtime/platform/windows_exports.def) > $(B)/exports.def && \
	    $(ZIG) dlltool -m i386:x86-64 -d $(B)/exports.def -l $(B)/$(GAME_ID)recomp.dll.a)

# its id file (<library>.id) says what it was built from without loading it (runtime/gamelib.c)
$(GAMELIB): $(GEN_OBJ) | $(EXE)
	$(CC) $(GAMELIB_LDFLAGS) -o $@.tmp $^
	mv -f $@.tmp $@
	echo $(GAMELIB_ID) > $@.id

$(B)/rt/ui/%.o: runtime/ui/%.mm $(RT_HDRS)
	@mkdir -p $(dir $@)
	$(CXX) $(CXXFLAGS) $(UIFLAGS) -c $< -o $@

$(B)/imgui/%.o: $(IMGUI)/%.cpp
	@mkdir -p $(dir $@)
	$(CXX) $(CXXFLAGS) -c $< -o $@

$(B)/imgui/imgui_impl_%.o: $(IMGUI)/backends/imgui_impl_%.cpp
	@mkdir -p $(dir $@)
	$(CXX) $(CXXFLAGS) -c $< -o $@

$(B)/imgui/imgui_impl_metal.o: $(IMGUI)/backends/imgui_impl_metal.mm
	@mkdir -p $(dir $@)
	$(CXX) $(CXXFLAGS) -fobjc-arc -c $< -o $@

# ------------------------------------------------------------------ the pipeline
# Python for the recompiler: tools/venv (made by `make venv`: capstone, unicorn, lz4)
PY := $(firstword $(wildcard tools/venv/bin/python) $(wildcard tools/venv/Scripts/python.exe))
venv:
	@[ -n "$(PY)" ] || python3 -m venv tools/venv
	tools/venv/bin/pip install -q lz4 capstone unicorn

# Game files from the dump: games/<game>/setup.py checks them and lays them out in $(GAMEFILES) (exefs/ and romfs/;
# an update's romfs over the base game's, as links). Paths come from DUMP_* (game.mk defaults; override on the
# command line).
setup:
	$(if $(PY),,$(error run `make venv` first))
	$(PY) $(GAMEDIR)/setup.py $(SETUP_ARGS) --out $(GAMEFILES)

# Recompile every module to C (AArch64 -> C, on this machine, from the player's own files).
gen:
	$(if $(PY),,$(error run `make venv` first))
	cd tools && ../$(PY) gen.py sdk ../$(GAMEFILES)/exefs/sdk $(shell printf '%#x' $$(($(GUEST_ORIGIN) + 0x4000000))) ../$(GEN)/sdk --hle ../$(GAME_HLE) $(GENSIMD)
	cd tools && ../$(PY) gen.py main ../$(GAMEFILES)/exefs/main $(GUEST_ORIGIN) ../$(GEN)/main \
	    $(if $(wildcard $(GAME_PATCHES)),--patches ../$(GAME_PATCHES)) $(GENSIMD)

clean-gen:
	rm -rf $(GEN) $(B)/obj

# iOS: the .app bundle around the executable (Info.plist, icon, signing: tools/make_ios_app.sh)
app: $(EXE)
	$(if $(IOS),,$(error app: pass TARGET=ios-arm64 | iossim-arm64))
	tools/make_ios_app.sh $(PLATFORM) $(EXE) $(APP)

# A player package of a cross build (runtime, compiler, generator; nothing from the game): dist/SMM2Recomp-<target>.zip
#   make TARGET=windows-x86_64 GAME=smm2 DEPS=<deps> all package
package: all
	$(if $(TARGET),,$(error package: pass TARGET=windows-x86_64 | linux-x86_64 | linux-aarch64))
	tools/package.sh $(TARGET) $(B) $(GAME_ID) $(GAME_HLE) $(DEPS)

# The course server inside the app (core/crates/embed) for this target, with cargo
embed-server:
	$(if $(EMBED_TRIPLE),,$(error embed-server: macOS / iOS builds only for now))
	cd ../core && cargo build -p mmk-embed --release --target $(EMBED_TRIPLE)

# Run from the data folder (saves, settings and shader caches go there); RUN_ARGS / env vars as the runtime reads them
run: all
	cd $(DATA) && $(CURDIR)/$(EXE) extracted $(RUN_ARGS)

gen-objs: $(GEN_OBJ)

# Runtime objects only (no generated code, no link): quick compile check, needs no game files
runtime-objs: $(RT_OBJ) $(BACKEND_OBJ) $(UI_OBJ)

# Recompiler unit tests (no game files)
test-tools:
	cd tools && ../$(PY) test_lift.py

# One command per game: `make smm2` = setup + gen + build. (Then `make run-smm2`.)
smm2:
	$(MAKE) GAME=smm2 setup
	$(MAKE) GAME=smm2 gen
	$(MAKE) GAME=smm2 -j$(JOBS) all
run-smm2:
	$(MAKE) GAME=smm2 run
JOBS ?= $(shell sysctl -n hw.ncpu 2>/dev/null || nproc 2>/dev/null || echo 4)

build/test_shaders: tools/test_shaders.m runtime/nvn/maxwell/translate.c runtime/nvn/maxwell/shader.h
	clang -O1 -fobjc-arc -framework Metal -framework Foundation -o $@ tools/test_shaders.m runtime/nvn/maxwell/translate.c

.PHONY: all venv setup gen clean-gen run gen-objs runtime-objs test-tools smm2 run-smm2 package embed-server app
