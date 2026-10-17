# Ghidra helper scripts

Run against a project you analysed yourself from your own game dump. Their output (pseudocode,
string listings) is derived from the game and belongs in the private `research/` folder, never
in this repository.

```sh
G=~/tools/ghidra_12.1.4_PUBLIC
# one-time import + full analysis (v1.0.0 loaded at 0x7100000000 to match community addresses)
GHIDRA_HEADLESS_MAXMEM=12G $G/support/analyzeHeadless <projdir> SMM2 \
  -import smm2-v100-main.elf -loader ElfLoader -loader-imagebase 7100000000

# queries (no re-analysis)
$G/support/analyzeHeadless <projdir> SMM2 -process smm2-v100-main.elf -noanalysis -readOnly \
  -scriptPath tools/ghidra -postScript Decompile.java research/decomp/player 1 0x71009b9dd0
$G/support/analyzeHeadless <projdir> SMM2 -process smm2-v100-main.elf -noanalysis -readOnly \
  -scriptPath tools/ghidra -postScript StringRefs.java research/strings.tsv 'Kuribo|Jump'
```
