# hc-tui

An unofficial terminal UI for [herdr-core](https://github.com/herdrdev/herdr-core) (`hc`). It is for
people who work in a terminal or on a headless machine and want the Herdr TUI experience on top of `hc`:

- a sidebar of spaces and agents with live status, including machines added with `hc machine add`;
- tabs, split panes, zoom and keyboard lock;
- Herdr's key bindings and configuration, read from your existing Herdr config;
- spaces and tabs read from the same `herdr-app` document the desktop app uses;
- local use, or remote use over plain SSH, with the session staying on the server.

## Status

Early, and not yet packaged:

- **No prebuilt release yet.** Build from source, as described below.
- **Official `hc` support is the current goal.** So far hc-tui has been developed and tested against
  [followcat/herdr-core](https://github.com/followcat/herdr-core), a fork of `hc`. CI runs the protocol
  integration tests against an official herdr-core commit, but the full UI has not been verified on
  official `hc` yet.
- **On official `hc`**, agents appear with a status only if they report it themselves (OSC 7501), and the
  sidebar's Git and hostname details are unavailable, because those facts exist only in the fork.
- **Not implemented yet:** mouse selection, copy mode and clipboard image paste.

hc-tui stays a plain terminal client for `hc`. Multi-agent coordination is being built as a separate
project.

## Build and run

Requires a stable Rust toolchain (edition 2024) and Zig **0.16.0** on `PATH` (or via `ZIG=`): the `hc-vt`
crate builds `libghostty-vt` from the pinned `vendor/ghostty` submodule.

```sh
git clone --recursive https://github.com/followcat/hc-tui
cd hc-tui
cargo build --release -p hc-tui
./target/release/hc-tui --help
```

Run it against a local core, or a remote one over SSH:

```sh
hc-tui --session <name>
hc-tui --remote <ssh-target> --session <name>
```

The default prefix is Ctrl+B: `q` detaches, `w` opens workspace navigation, `b` toggles the sidebar and
`?` opens searchable help.

## More

- [Runtime behaviour](docs/runtime.md)
- [Requirements](docs/requirements.md), [plan](docs/plan.md) and [progress](docs/progress.md)
- [Parity with Herdr](docs/parity.md) and the [owned-session parity harness](scripts/parity/README.md)

To run the tests: `git submodule update --init --depth 1`, then `cargo test`. The pinned revisions live in
`.gitmodules` and `NOTICE`.

Unofficial project, not affiliated with or endorsed by the Herdr project. Licensed under the
[Apache License 2.0](LICENSE).
