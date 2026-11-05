# hc-tui

A terminal UI for [herdr-core](https://github.com/herdrdev/herdr-core) (`hc`). Its keys,
interaction, layout and UI must match **herdr 0.9.5-followcat at `d00953e0`**:

- a sidebar of spaces and agents with live status;
- tabs and split panes;
- herdr's bindings and configuration;
- local use, or remote use over plain SSH.

**Status:** M2.1 is accepted for its available-fact fixtures. M3a adds shared
structure, tabs/splits, layout bindings and lock. Exact M2 parity still awaits
reviewed C3 facts in M2.2; first-attach appearance remains a client limitation.
Custom commands, notifications and remaining actions are deferred.
There is no release yet. See [requirements](docs/requirements.md), [plan](docs/plan.md),
[progress](docs/progress.md) and [parity](docs/parity.md).

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

Run `hc-tui --session <name>` locally, or
`hc-tui --remote <ssh-target> --session <name>` over SSH. The default
prefix is Ctrl+B: `q` detaches, `w` opens workspace navigation, `b` toggles
the sidebar and `?` opens searchable help. Existing herdr configuration is
read directly. See [runtime behavior](docs/runtime.md) and the
[owned-session parity harness](scripts/parity/README.md).

The pinned revisions live in `.gitmodules` and `NOTICE`.

Unofficial project, not affiliated with or endorsed by the Herdr project. Licensed under the
[Apache License 2.0](LICENSE).
