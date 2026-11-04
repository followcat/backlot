# hc-tui requirements

hc-tui is a terminal UI client for [herdr-core](https://github.com/herdrdev/herdr-core) (`hc`).
It aims to feel like the herdr 0.9 TUI: a sidebar of spaces and agents with live status, tabs,
split panes, and herdr-style keys. It works on the local machine and over plain SSH.

It answers the gap described in herdrdev/herdr-core#25. hc itself draws nothing, the herdr 0.9 TUI
runs its own separate runtime, and herdr-app is a desktop window that cannot run over SSH.

Unofficial. Not affiliated with or endorsed by the Herdr project.

## Setups to serve

1. **Remote-first:** an always-on Linux box runs hc and every agent. The user reaches it over SSH
   from a laptop or desktop, like `herdr --remote <box>`.
2. **Local:** the same machine runs hc and hc-tui, possibly next to herdr-app.
3. **Narrow terminals:** phone SSH clients, and split terminal windows of 40 columns or more.

## Functional requirements

### R1 Connection
- **Local:** connect to hc's Unix socket. Resolve it the way hc does: `HC_SOCKET` if set, else
  the session given by `--session` / `HC_SESSION` (default `default`) under hc's runtime and state
  directories.
- **Remote:** `hc-tui --remote <ssh-target> [--session <s>]` speaks the same JSON-RPC through
  `ssh <target> hc connect [--session <s>]`.
  - Attach streams use framed mode on their own connection, so each stream gets its own
    `hc connect`.
  - Recommend SSH ControlMaster in the docs. Never ask for or store passwords.
- **Handshake:** check `hello` for protocol generation 1 and the methods hc-tui uses.
  - A missing method disables the feature that needs it and shows why. It never crashes.
- **Reconnect:** reconnect with backoff. Resubscribe, then refetch documents after a reconnect or
  a `missed` event. The last inventory stays visible, marked offline.

### R2 Inventory
- Machines: this core plus `machine.list`, with each machine's state.
- Terminals: `terminal.list` and `events.subscribe`, covering:
  - name, title, labels, cwd, foreground;
  - parent/child (workers);
  - status records and turn;
  - live or finished.
- Status comes only from core. That includes OSC 7501 records, and the screen-detected records
  that a patched core may add. hc-tui never guesses status from screen text.

### R3 Spaces, tabs and panes
- **Structure source, in this order:**
  1. The herdr-app document: `doc.get {name: "herdr-app", on}` on each machine. Read it only;
     the parser must tolerate versions and unknown fields.
  2. The terminal labels `herdr.space` and `herdr.tab`.
  3. Grouping by machine and cwd.
- **hc-tui's own layout state** lives in a core document named `hc-tui`, read with `doc.get` and
  written with versioned `doc.set`; a conflict means re-read, then retry. It holds splits, focus,
  zoom, and tabs created in hc-tui. Every hc-tui client of the same core then shows the same
  layout.
- **Never write the `herdr-app` document** (herdrdev/herdr-app#43 is unresolved).
- **New terminals carry labels** (`herdr.space`, `herdr.tab`, `herdr.seen=0`) so herdr-app adopts
  them in the right place.

### R4 Sidebar
- **Spaces view:** spaces grouped by machine. Each space shows its most urgent state, in the order
  blocked > working > done > idle.
- **Agents view:** every agent across all machines, "your turn" (blocked) first.
- **Counters** for your turn, working and ready. Offline machines stay listed.
- **Collapsing:** below about 80 columns the sidebar collapses to a toggle.

### R5 Panes
- Each visible pane is one `terminal.attach` stream in framed mode.
  - It has VT emulation. Prefer the `ghostty-vt` crate that herdr-core and herdr use.
  - It renders into a ratatui buffer.
- **Size ownership follows hc's rules:**
  - the focused pane attaches as the active viewer;
  - background and peek panes use `observe`;
  - when another client owns the size, show a banner and offer "take control";
  - respond to RESIZE, FOCUS and FACTS.
- **Scrollback:** history from the first paint, plus local history. A copy mode with keyboard and
  mouse selection copies through OSC 52.
- **Terminal features:** truecolor and 256 colors, cursor shapes, bracketed paste, and
  kitty-keyboard passthrough. Automatic query replies go as AUTOMATIC_INPUT when core advertises
  it, otherwise as DATA.

### R6 Keys
- A herdr-like prefix-key model with herdr 0.9's default bindings. Bindings are configurable in a
  TOML file whose action names match herdr's `config.toml` where possible, and an import command
  reads a herdr config.
- `Ctrl+G` toggles **lock**: every key passes to the terminal until it is pressed again.

### R7 Actions
- **Terminals:**
  - new terminal or agent with `terminal.run` (machine, cwd, labels, optional program);
  - split; close with confirmation (`terminal.kill`); rename (`terminal.label herdr.title`);
  - mark seen (`herdr.seen`); keep (`terminal.keep`).
- **Input:** send a line (`terminal.send`), interrupt (`terminal.interrupt`), keys
  (`terminal.keys`).
- **Workers:** peek at a worker in a popup.
- **Machines:** list, add (`machine.add`; SSH must already work), connect, rename, remove.

### R8 Notifications
- Ring the bell or send OSC 9/777 when a terminal becomes blocked or finishes, following
  herdr-app's rules:
  - not for sub-agents;
  - not while the terminal is viewed;
  - one alert per turn, after a short settle.

### R9 Popups and clipboard
- A floating terminal popup with text selection and copy.
- Clipboard image paste in a pane, including over SSH and with kitty press/release events.
  - The image goes to the agent's machine with `terminal.put`, and the path is typed into the
    pane. Port this from herdr's clipboard image forwarding.

## Non-functional requirements
- **Rust**, ratatui and crossterm. One static binary (musl) for Linux x86_64/aarch64, plus a macOS
  build.
- **Performance:**
  - local start within 200 ms;
  - near-zero idle CPU (event-driven, no polling loops);
  - redraws capped at about 60 per second.
- **Robustness:** a broken or unreachable machine never blocks the others.
- **Tests:**
  - unit tests;
  - a scripted fake core (JSON-RPC server) for UI and state tests;
  - integration tests against a real `hc` built from source. They run in a scratch session with
    temporary XDG directories and never touch a user's sessions.
- **CI:** GitHub Actions running `cargo fmt --check`, `cargo clippy -D warnings` and `cargo test`.
- **License:** Apache-2.0. Code derived from herdr or herdr-core keeps a header naming its origin,
  and NOTICE lists it.
- **English everywhere.**

## Out of scope for v1
- herdr mods and plugins, worktrees, the file panel.
- Writing the herdr-app document.
- Inline images (kitty graphics), which come later.
- Windows.
