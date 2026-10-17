# Toolchain

Everything here lives **outside the repo** under `~/smm-work/tools/` (binaries in `~/smm-work/tools/bin/`).
Extracted game data goes to `~/smm-work/extracted/`. Nothing in either folder is ever committed.

All versions are pinned. Downloaded release artefacts are hashed in `~/smm-work/tools/SHA256SUMS`.

| Tool | Version / pin | Source | Purpose |
|---|---|---|---|
| OpenJDK | 21.0.12 (Homebrew `openjdk@21`) | Homebrew core | Runtime for Ghidra and JWUDTool |
| Ghidra | 12.1.4 PUBLIC (`~/tools/ghidra_12.1.4_PUBLIC`) | NationalSecurityAgency/ghidra | Disassembly/decompilation backend for REA |
| REA | 6.4.0 (`/opt/homebrew/bin/rea`, MCP) | morluto/rea | Primary RE interface |
| Ghidra Switch Loader | 1.6.1 @ `5d80d195`, built against Ghidra 12.1.4 | Adubbz/Ghidra-Switch-Loader | Load NSO/NRO/KIP directly in Ghidra (upstream release targets 12.1.2, so built from source) |
| hactool | 1.4.0 @ `3121a5bf` (built from source) | SciresM/hactool | Switch XCI/NSP/NCA decryption |
| nstool | v1.9.2 macOS arm64 release | jakcron/nstool | Switch container inspection/extraction |
| nx2elf | `a1760ba5` (2026-04-09, built from source) | shuffle2/nx2elf | NSO → ELF fallback for tools that can't load NSO |
| JWUDTool | 0.4 | Maschell/JWUDTool | Wii U WUD/WUX disc decryption |
| CDecrypt | v4.8 @ `a4338402` + local patch | VitaSmith/cdecrypt | Wii U NUS (tmd/tik/app) decryption, used for the update bundled on the disc |
| oead | 1.3.0 (pip, venv) | zeldamods/oead | SARC, Yaz0, BYML, AAMP |
| zstandard | 0.25.0 (pip, venv) | indygreg/python-zstandard | SMM2 `.zs` decompression |
| cryptography | 50.0.1 (pip, venv; `requirements.txt`) | pyca/cryptography | AES-CBC / AES-CMAC for SMM2 `.bcd` |
| DuckDB | 1.5.6 (pip, venv) | duckdb/duckdb | Remote parquet queries for `tools/fetch_courses.py` |
| vgmstream | r2117 (Homebrew) | vgmstream/vgmstream | BFSTM/BWAV/BFWAV decoding |

Python venv: `~/smm-work/tools/venv` (run scripts that read extracted data with `python -I`).

## Local patch to CDecrypt

Upstream CDecrypt compiles the Wii U common key into the binary. The local build has that
removed and reads the key at runtime from `$WIIU_COMMON_KEY_FILE` instead, so the only key
material used is what was dumped from the owner's console. The patch is kept at
`~/smm-work/tools/cdecrypt-runtime-key.patch`.

## Not installed

- **hactoolnet / LibHac**: the GitHub repo no longer exists. hactool + nstool cover the same ground.
- **Switch Toolbox**: Windows-only. BNTX/BFRES decoding will be handled in-tree or by small
  scripts when Phase 4/5 needs them.

## Rebuilding

```sh
# hactool
git clone --branch 1.4.0 https://github.com/SciresM/hactool && cd hactool && cp config.mk.template config.mk && make
# nx2elf
git clone https://github.com/shuffle2/nx2elf && git -C nx2elf checkout a1760ba5 && cmake -S nx2elf -B nx2elf/build -G Ninja && ninja -C nx2elf/build
# Switch Loader
git clone https://github.com/Adubbz/Ghidra-Switch-Loader && cd Ghidra-Switch-Loader && \
  JAVA_HOME=$(brew --prefix openjdk@21)/libexec/openjdk.jdk/Contents/Home ./gradlew -PGHIDRA_INSTALL_DIR=$HOME/tools/ghidra_12.1.4_PUBLIC
# then unzip dist/*.zip into $GHIDRA_INSTALL_DIR/Ghidra/Extensions/
```
