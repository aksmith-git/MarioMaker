// Mods (mods.h): a folder chosen with RECOMP_MOD_DIR. Nothing is active by default.
#include "mods.h"

#include <stdio.h>
#include <stdlib.h>
#include <sys/stat.h>

#include "rt.h"

const char *mods_dir(void) {
    const char *d = getenv("RECOMP_MOD_DIR");
    return d && *d ? d : NULL;
}

bool mods_romfs_override(const char *path, char *out, size_t cap) {
    const char *d = mods_dir();
    if (!d) return false;
    struct stat st;
    if ((size_t)snprintf(out, cap, "%s/romfs/%s", d, path) >= cap) return false;
    return stat(out, &st) == 0 && S_ISREG(st.st_mode);
}

void mods_boot(void) {
    if (mods_dir()) LOG("[mods] %s\n", mods_dir());
}
