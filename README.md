# hc-tui

A terminal UI for [herdr-core](https://github.com/herdrdev/herdr-core) (`hc`). It aims to feel
like the herdr 0.9 TUI:
- a sidebar of spaces and agents with live status;
- tabs and split panes;
- herdr-style keys;
- local use, or remote use over plain SSH.

**Status:** early development, not usable yet. See [requirements](docs/requirements.md) and
[plan](docs/plan.md).

## Build and test

Requires a stable Rust toolchain (edition 2024) and Zig **0.16.0** on `PATH` (or via `ZIG=`):
the `hc-vt` crate builds `libghostty-vt` from the pinned `vendor/ghostty` submodule. Fetch the
submodule on first checkout:

```sh
git submodule update --init --depth 1   # or clone with --recursive
cargo build
cargo test
cargo run -p hc-tui -- --help
```

The pinned revisions live in `.gitmodules` and `NOTICE`.

Unofficial project, not affiliated with or endorsed by the Herdr project. Licensed under the
[Apache License 2.0](LICENSE).
