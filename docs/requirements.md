# hc-tui requirements

hc-tui is a terminal UI client for [herdr-core](https://github.com/herdrdev/herdr-core) (`hc`).
Its keys, interaction, layout and UI must match herdr **0.9.5-followcat** at
`d00953e0`. This is a hard requirement: with the same terminal size, configuration,
terminals and states, matching surfaces must be identical cell by cell, including
glyphs, colors and attributes. It works on the local machine and over plain SSH.

It answers the gap described in herdrdev/herdr-core#25. hc itself draws nothing, the herdr 0.9 TUI
runs its own separate runtime, and herdr-app is a desktop window that cannot run over SSH.

Unofficial. Not affiliated with or endorsed by the Herdr project.

## Setups to serve

1. **Remote-first:** an always-on Linux box runs hc and every agent. The user reaches it over SSH
   from a laptop or desktop, like `herdr --remote <box>`.
2. **Local:** the same machine runs hc and hc-tui, possibly next to herdr-app.
3. **Narrow terminals:** phone SSH clients, and split terminal windows of 40 columns or more.

## Functional requirements

### R0 Herdr parity
- Port the reference herdr UI, configuration and input code directly, retaining source-file
  attribution and Apache-2.0 licensing. Keep its structure where practical for future ports.
- Match sidebar width, sections, order, tokens, dimming, truncation, selection, tab bar,
  pane borders/titles, status line, popups, confirmations, scrollbars and copy-mode visuals.
- The same key and mouse sequences must have the same effects, including modes, focus,
  sidebar clicks, dragging, scrolling, and prompt text.
- Read the herdr configuration by default with herdr's parsing and defaults; an hc-tui
  configuration may override it. No import step is required for existing customizations.
- Adapt core data to herdr's presentation. Document each unavailable/different core fact,
  its reason and exact replacement in `docs/parity.md`. Do not invent hc-tui display formats.
- Missing required core facts remain explicit `pending-core-fact` results, never passes.
  M2.2 adopts reviewed C3 facts after M2.1. M3a may implement structure, layout/tab operations, bindings and lock independently; M3b uses reviewed C3 per-record activity for chronology.
- A committed neutral parity harness runs both clients in owned scratch sessions at
  80x24, 120x40, 200x50, and the 65/64/40-column boundary/mobile layouts, replays matching key/mouse scripts, and diffs screen cells and
  attributes. Every difference is a bug unless explicitly documented in `docs/parity.md`.

### R1 Connection
- **Local:** connect to hc's Unix socket. Resolve it the way hc does: `HC_SOCKET` if set, else
  the session given by `--session` / `HC_SESSION` (default `default`) under hc's runtime and state
  directories.
- **Remote:** `hc-tui --remote <ssh-target> [--session <s>]` speaks the same JSON-RPC through
  `ssh <target> hc connect [--session <s>]`.
  - Attach streams multiplex in framed mode on one shared `hc connect`: each attach gets its
    own channel, so a burst of attaches costs one transport and one `hello`. RPC and
    subscription streams keep their own connections.
  - On Unix the connections share one ssh login per target through hc-tui's own control
    socket (`$XDG_RUNTIME_DIR/hc-tui-ssh`, kept 10 minutes past the last stream), so a
    burst of connections costs one login instead of one each. Never ask for or store passwords.
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
- Presentation and interaction follow the pinned herdr sidebar exactly, including configured
  width, sections, ordering, glyphs/colors, truncation and highlight rules.
- **Spaces view:** spaces grouped by machine, using the reference's aggregate status,
  layout and configured token rules over mapped core facts.
- **Agents view:** every agent across all machines, using the reference layout and ordering.
- **Ordering and counters:** use the reference configuration and definitions, adapting
  core-reported state/seen facts as documented in `docs/parity.md`. The reference default
  agent ordering is by space; configured alternatives must retain their herdr behavior.
  Offline machines stay listed.
- **Narrow terminals:** follow herdr's collapse/toggle behavior and retain access to selection.

### R5 Panes
- Each visible pane retains an observe `terminal.attach` stream in framed mode,
  plus a silent size holder. The focused pane also has its input connection.
  - It has VT emulation. Prefer the `ghostty-vt` crate that herdr-core and herdr use.
  - It renders into a ratatui buffer.
- **Size ownership follows hc's rules:**
  - the focused pane attaches as the active viewer;
  - visible background panes hold size leases, with no human input or focus claims;
  - transport recovery is passive (`resumed`), including visible size holders;
    real tab entry, focus or allocation changes may reclaim size;
  - off-tab recent panes and peek panes stay strictly `observe`;
  - when another client owns the size, show a banner and offer "take control";
  - respond to RESIZE, FOCUS and FACTS.
- **Scrollback:** history from the first paint, plus local history. A copy mode with keyboard and
  mouse selection copies through OSC 52.
- **Terminal features:** truecolor and 256 colors, cursor shapes, bracketed paste, and
  kitty-keyboard passthrough. Automatic query replies go as AUTOMATIC_INPUT when core advertises
  it, otherwise as DATA.

### R6 Keys
- Use the pinned herdr's prefix model, defaults, action names and the user's configured
  bindings. Read the herdr config by default (normally `~/.config/herdr/config.toml`),
  preserving its parsing/default behavior; hc-tui config only overrides it.
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

### R7a Completion and attention

- Completion comes only from authoritative per-record `completion_ordinal`;
  initial idle/done is readiness, with no turn/time/transition inference.
- Keep acknowledgement client-local with the focused, composed fresh-paint
  gate; baseline activity origin/epoch/scope and incarnation changes.
- Compare state/completion ordinals only within their origin/epoch/scope
  when ordering a newly ingested batch. Allocate display receipts before
  watcher coalescing; retain unchanged ranks and prune absent agents.
  Across domains retain ingestion order without comparing foreign counters.
  A delayed A10 arriving after A20 and B displays A10 > B > A20; historical
  within-domain ordinal order does not override reference receipt behavior.
- The stalled-output acknowledgement race remains a documented limitation
  until an ordered output-consumption fence exists.

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
- herdr mods and plugins, and the file panel.
- Creating/removing worktrees; read-only repository/worktree facts are required for parity.
- Writing the herdr-app document.
- Inline images (kitty graphics), which come later.
- Windows.
