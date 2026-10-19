// The player's SMM1 dump and the files generated from it for the host-drawn 100 Mario screens (smm1_data.c).
#pragma once
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

// Generates the cache at the first call if needed and installs the layout loader; false: no SMM1 dump.
bool smm1_data_open(void);
uint8_t *smm1_read_file(const char *path, size_t *n);
uint8_t *smm1_common_msbt(const char *group_and_name, size_t *n);  // "Network/Nwk_Message" -> CommonMsg table
uint8_t *smm1_model(const char *name, size_t *n);  // "M1_Object_peach" -> its BFRES (malloc'd)
uint8_t *smm1_sound_file(const char *name, size_t *n);  // "BGM_100MarioMap" -> its BFSTM (or BARS archive)
