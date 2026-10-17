# MarioMaker (working title)

An open-source, native macOS (Apple Silicon) engine that plays Super Mario Maker 2 courses and
adds Super Mario Maker 1's 100 Mario Challenge.

**Status:** Phase 1 (environment and extraction). No engine code yet. See [PROGRESS.md](PROGRESS.md).

## What is and isn't in this repository

This repo contains only original code, original placeholder art, docs and tooling.

- No Nintendo code, assets, text, audio, executables or keys are committed, ever.
  `scripts/check_no_proprietary.py` enforces this in CI and as a pre-commit hook.
- The original game's assets are extracted at first launch from **your own** dump of the games.
- Without a dump, the engine runs on placeholder assets.

Reverse-engineering notes live in a private, gitignored `research/` folder. They are used as a
behavioural reference only; the engine is a clean implementation.

## Development setup

```sh
scripts/install-hooks.sh                         # pre-commit guard against proprietary files
python3 -m unittest scripts/test_check_no_proprietary.py
```

Tooling for working with your own dumps is described in [docs/TOOLCHAIN.md](docs/TOOLCHAIN.md).
