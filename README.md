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

Requires a stable Rust toolchain (edition 2024):

```sh
cargo build
cargo test
cargo run -p hc-tui -- --help
```

Unofficial project, not affiliated with or endorsed by the Herdr project. Licensed under the
[Apache License 2.0](LICENSE).
