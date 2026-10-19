#!/bin/sh
# Builds tools/test_smm1ui (the SMM1 layout player and the 100 Mario screens alone, no game): build/test_smm1ui.
# See tools/test_smm1ui.c.
set -e
cd "$(dirname "$0")/.."
mkdir -p build
cc -O2 -g -Wall -Wno-unused-function -Iruntime -Iruntime/nvn -DSMM1UI_STANDALONE -o build/test_smm1ui \
    tools/test_smm1ui.c games/smm2/mods/smm1/*.c runtime/nvn/bcsoft.c -lpthread -lm
echo build/test_smm1ui
