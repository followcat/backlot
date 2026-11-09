# backlot

An unofficial terminal client for [herdr-core](https://github.com/herdrdev/herdr-core) (`hc`), with a
studio layer for directing coding agents. Use it if you work in a terminal or on a machine with no
desktop, and want the Herdr TUI on top of `hc`:

- a sidebar of spaces and agents with live status, including machines added with `hc machine add`;
- tabs, split panes, zoom and keyboard copy mode; `Ctrl+G` locks the keyboard so every key goes to the
  program;
- mouse selection that copies on release (through OSC 52, so your terminal has to allow it);
- Herdr's key bindings, theme and other settings, read from your existing Herdr config;
- spaces and tabs read from the same `herdr-app` document the desktop app uses;
- local use, or remote use over plain SSH — where `Ctrl+V` pastes an image from your local clipboard
  into the agent's machine — with the session staying on the server.

The studio layer is not implemented yet — see [docs/studio/](docs/studio/). backlot started as a fork
of [hc-tui](https://github.com/followcat/hc-tui), which stays a plain terminal client. Unofficial
project, not affiliated with or endorsed by the Herdr project.

![backlot showing a sidebar and a split pane](docs/images/hc-tui.png)

## Install

backlot is one static binary. The script installs it and, when it's missing, the `hc` core it talks
to — see [herdr-core](https://github.com/herdrdev/herdr-core). Prebuilt archives cover Linux
(x86_64 and aarch64, static musl) and macOS (arm64 and Intel).

```sh
curl -fsSL https://github.com/followcat/backlot/releases/latest/download/install.sh | sh
```

The script verifies the archive against the release's `sha256sums.txt` and installs to
`~/.local/bin` without sudo. Pin a release with `BACKLOT_VERSION=v…`, skip the core install with
`BACKLOT_SKIP_CORE=1`, or fetch an archive from the
[Releases page](https://github.com/followcat/backlot/releases) and check it yourself. Upgrade by
running the script again; uninstall by deleting the binary. Details: [docs/install.md](docs/install.md).
The first tagged release is pending — if the Releases page is still empty, build from source below.

## Status

Early, and the first tagged release is pending:

- **It runs on official `hc`.** CI builds a pinned official herdr-core from source and drives the real
  UI against it: sidebar, attach, switching, tabs, splits, status and detach. Day-to-day use is still
  mostly on [followcat/herdr-core](https://github.com/followcat/herdr-core), a fork of `hc`, so
  reports from official `hc` are especially welcome.
- **On official `hc`**, agents appear with a status only if they report it themselves (OSC 7501), and
  the sidebar's Git and hostname details are unavailable, because those facts exist only in the fork.
  See [Core compatibility](docs/compatibility.md) for the full per-core picture.
- **The macOS builds** pass CI but haven't been tried on a real Mac yet.

## Build from source

You need stable Rust (edition 2024) and Zig **0.16.0** on `PATH`, or set `ZIG=`. Zig builds `libghostty-vt` from the
pinned `vendor/ghostty` submodule.

```sh
git clone --recursive https://github.com/followcat/backlot
cd backlot
cargo build --release -p hc-tui
./target/release/backlot --help
```

To run the tests: `git submodule update --init --depth 1`, then `cargo test`.

Run it against a local core, or a remote one over SSH — backlot starts the `hc` core itself when
nothing answers on the session socket:

```sh
backlot --session <name>
backlot --remote <ssh-target> --session <name>
```

The default prefix is Ctrl+B: `q` detaches, `w` opens workspace navigation, `b` toggles the sidebar and
`?` opens searchable help.

## Docs

- [Install, upgrade, uninstall](docs/install.md)
- [Keys](docs/keys.md)
- [Remote use over SSH](docs/remote.md)
- [Troubleshooting](docs/troubleshooting.md)
- [Core compatibility](docs/compatibility.md)
- [Runtime behaviour](docs/runtime.md)

For development: [requirements](docs/requirements.md), [plan](docs/plan.md), [progress](docs/progress.md),
[parity with Herdr](docs/parity.md) and the [parity harness](scripts/parity/README.md).

## License

[Apache License 2.0](LICENSE). Not affiliated with or endorsed by the Herdr project.
