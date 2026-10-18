// RomFS image from a folder of game files (runtime/romfs_build.c). The game reads its data through one raw RomFS
// image (romfs.bin, runtime/services/fs.c); players bring their own game files, and the start screen builds the image
// from their romfs/ folder.
#pragma once
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

// Writes the RomFS image of the folder `dir` to `out`. progress (may be NULL): bytes of file data written so far and
// the total, now and then. false = failed (why: a short message).
bool romfs_build(const char *dir, const char *out, void (*progress)(uint64_t done, uint64_t total), char *why,
                 size_t why_size);

// The same image without writing it: its header and tables in memory and, for each file, where its data sits in the
// image and which host file holds it (sorted by image offset). services/fs.c serves the game's RomFS reads from this
// when there is no romfs.bin, so nothing is copied: a merged tree of symlinks (an update over the base game) works as
// it is.
typedef struct {
    uint64_t off, size;  // image offset of the file's data, its size
    char *host;          // the host file holding it
    char *path;          // its path in the RomFS ("Folder/name")
    int fd;              // (opened on first read; -1 until then)
} RomfsIndexFile;
typedef struct {
    uint8_t header[0x200];  // image bytes 0..0x200 (the 0x50-byte header, then zeros)
    uint64_t tables_off, tables_size;
    uint8_t *tables;        // directory hash, directory entries, file hash, file entries (contiguous, at tables_off)
    uint64_t size;          // of the whole image
    RomfsIndexFile *files;
    int nfiles;
} RomfsIndex;
// overlay (may be NULL): a folder whose files replace or join those of dir (a mod's romfs/).
bool romfs_index(const char *dir, const char *overlay, RomfsIndex *ix, char *why, size_t why_size);

// Bytes of file data under dir (the image is about this plus its tables).
uint64_t romfs_dir_bytes(const char *dir);

// Copies the folder tree src to dst (hard links where the file system allows: no second copy of 6 GB). progress as
// above. false = failed.
bool romfs_copy_tree(const char *src, const char *dst, void (*progress)(uint64_t done, uint64_t total));

#ifdef __cplusplus
}
#endif
