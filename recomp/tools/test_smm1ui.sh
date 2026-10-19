#!/bin/sh
# Builds tools/test_smm1ui (the SMM1 layout player alone, no game): build/test_smm1ui. See tools/test_smm1ui.c.
set -e
cd "$(dirname "$0")/.."
mkdir -p build
FLOW=""
[ -f games/smm2/mods/smm1/flow.c ] && FLOW="games/smm2/mods/smm1/flow.c games/smm2/mods/smm1/ending.c -DSMM1UI_STANDALONE"
cc -O2 -g -Wall -Wno-unused-function -Iruntime -Iruntime/nvn -o build/test_smm1ui tools/test_smm1ui.c \
    games/smm2/mods/smm1/lyt_formats.c games/smm2/mods/smm1/lyt_player.c games/smm2/mods/smm1/lyt_raster.c \
    games/smm2/mods/smm1/smm1_data.c runtime/nvn/bcsoft.c $FLOW -lpthread -lm
echo build/test_smm1ui
