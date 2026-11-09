# hc-tui

An unofficial terminal UI for [herdr-core](https://github.com/herdrdev/herdr-core) (`hc`). Use it if you work in a
terminal or on a machine with no desktop, and want the Herdr TUI on top of `hc`.

![hc-tui with a sidebar of projects and agents, and three panes](docs/images/hc-tui.png)

## Features

- A sidebar of spaces and agents. Each agent shows whether it's working, blocked, done or idle. Machines added with
  `hc machine add` appear there too.
- Tabs, split panes and zoom. `Ctrl+G` locks the keyboard, so every key goes to the program.
- Select text with the mouse and it's copied when you let go. This uses OSC 52, so your terminal has to allow it.
- Your Herdr key bindings, theme and other settings carry over from your existing Herdr config.
- Spaces and tabs come from the same `herdr-app` document the desktop app uses.
- Works locally or over plain SSH. Your terminals stay on the server when you detach.

| Mouse selection | Settings |
|---|---|
| ![Text selected with the mouse in a pane](docs/images/selection.png) | ![The settings overlay with its list of themes](docs/images/settings.png) |

## Install

```sh
curl -fsSL https://followcat.github.io/hc-tui/install.sh | sh
```

This installs hc-tui and, when it's missing, the `hc` core it talks to. hc-tui goes into `~/.local/bin`, without
sudo, after a check against the release's `sha256sums.txt`. There are builds for Linux (x86_64 and aarch64) and
macOS (arm64 and Intel). To pin a version, skip the core install, upgrade or uninstall, see
[docs/install.md](docs/install.md).

## Usage

```sh
hc-tui                                  # the default session
hc-tui --session work
hc-tui --remote host-a --session work   # over SSH
```

hc-tui starts the `hc` core itself when nothing answers on the session socket. `Ctrl+B q` detaches and leaves the
core and your terminals running; run `hc-tui` again to reattach.

The prefix is `Ctrl+B`, as in Herdr:

| Keys | Action |
|---|---|
| `Ctrl+B w` | Navigate workspaces |
| `Ctrl+B c` | New tab |
| `Ctrl+B v`, `Ctrl+B -` | Split right, split down |
| `Ctrl+B h/j/k/l` | Move focus |
| `Ctrl+B z` | Zoom the pane |
| `Ctrl+B b` | Show or hide the sidebar |
| `Ctrl+B ?` | Search the key bindings |
| `Ctrl+B q` | Detach (the core and your terminals keep running) |

If you've changed these in your Herdr config, your bindings apply instead.

### Alt shortcuts

With a few lines in your Herdr config, you can switch without the prefix and open a floating shell. They aren't bound
by default: [docs/keys.md](docs/keys.md#alt-shortcuts) has the config to copy.

| Keys | Action |
|---|---|
| `Alt+Left`, `Alt+Right` | Previous or next tab |
| `Alt+Up`, `Alt+Down` | Previous or next agent, across tabs, workspaces and machines |
| `Alt+F` | A floating shell over the current tab |

| Switching with `Alt` and the arrow keys | A floating shell with `Alt+F` |
|---|---|
| ![Switching tabs and agents with Alt and the arrow keys](docs/images/alt-arrows.gif) | ![A floating shell opened with Alt+F](docs/images/alt-f.gif) |

## Status

hc-tui is early.

It runs on official `hc`. CI builds a pinned herdr-core from source and drives the real UI against it. Most day-to-day
use so far has been on [followcat/herdr-core](https://github.com/followcat/herdr-core), a fork of `hc`, so reports from
official `hc` are especially welcome.

On official `hc`, agents only show a status if they report it themselves (OSC 7501), and the sidebar has no Git or
hostname details. See [Core compatibility](docs/compatibility.md).

The macOS builds pass CI but haven't been tried on a real Mac yet.

Not done yet:

- keyboard copy mode

hc-tui will stay a plain terminal client for `hc`. Multi-agent coordination is being built as a separate project.

## Build from source

You need stable Rust (edition 2024) and Zig **0.16.0** on `PATH`, or set `ZIG=`. Zig builds `libghostty-vt` from the
pinned `vendor/ghostty` submodule.

```sh
git clone --recursive https://github.com/followcat/hc-tui
cd hc-tui
cargo build --release -p hc-tui
./target/release/hc-tui --help
```

To run the tests: `git submodule update --init --depth 1`, then `cargo test`.

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
