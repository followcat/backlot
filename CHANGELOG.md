# Changelog

All notable changes to hc-tui are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## Unreleased

### Changed

- A shown float (`Alt+F` popup) takes every key except its popup chords: the
  `Alt+arrow` tab and agent shortcuts, `Ctrl+G` and keys typed while the
  keyboard is locked now reach the float's program instead of switching tabs,
  locking, or landing on the pane behind it. `Alt+F` still hides the float,
  even while locked.

### Fixed

- `PageUp` and `PageDown` inside a float page the float's own scrollback, under
  the same rule as a pane (normal screen, no mouse reporting); before, they
  reached the program, which usually ignored them.

## 0.1.19 — 2026-11-24

### Added

- `[ui] agent_panel_space_headers` (default `false`): head each space's run of
  agents with a dim space name and blank separator line when
  `agent_panel_sort = "spaces"`, across single-machine and multi-machine views
  with header-aware scroll reveal. (#59)

### Fixed

- Creating the first workspace (`+ new` / `new_workspace`) on an empty local or
  `--remote` core (or an empty remote machine) no longer fails with
  `No focused terminal`; `cwd` is omitted so the target core starts in its own
  default directory and the workspace takes its folder name from the spawned
  terminal. (#57)
- Unnamed `herdr-app` spaces and tabs now follow the live working folder and
  lead agent's stripped terminal title (and unnamed tabs fall back to the
  running foreground job's name when no agent title names them); finished
  terminals of closed `herdr-app` tabs stay hidden with immediate
  structure-cache invalidation on finish, and `tb`+32-hex app tab IDs are
  suppressed as opaque identities. (#58)

## 0.1.18 — 2026-11-24

### Changed

- The pinned official test core is official `hc` 0.1.22 plus `hc ls --table`
  (`7cb4de47164f1d65db84dfab294166994f143a15`). Verified alignment across the
  0.1.22 changes: chunked remote image paste (`put_chunks`) keeps two-hop
  `machine/tN` uploads on `terminal.put`, aborts on chunk errors, and surfaces
  documented storage/timeout errors without retrying; failed registry saves
  (`unavailable`) surface cleanly and are never retried blindly; remote viewer
  reattach fixes require no client workarounds.

### Fixed

- Child terminals ended because their parent closed (`reason = "killed"` with
  `ended_by.by = "die_with_parent"`) are now auto-reaped alongside normal exited
  terminals, while manual `terminal.kill` records stay untouched. (#56)
- `hc-proto` now treats document-only `events.subscribe` streams (non-empty
  `docs` without a terminal filter, which receive empty terminal snapshots and
  omit terminal events in `hc` 0.1.22) as filtered so sequence gaps and
  heartbeats do not trigger false resyncs.

## 0.1.17 — 2026-11-20

### Changed

- The test suites now create their temporary directories through a `TestDir`
  guard that removes the directory when the test ends, including while a
  panicking test unwinds; 30 sites in hc-tui and 4 in hc-proto use it. CI runs
  the workspace tests through `scripts/check-test-tmp.sh`, which points
  `TMPDIR` at a fresh directory and fails the run when the tests leave files
  behind. There is no behaviour change for users.

## 0.1.16 — 2026-11-19

### Fixed

- The agents list now scrolls to the focused agent row whenever the focused
  pane changes — a tab or workspace switch, a sidebar click, a focus key or a
  focus that arrives from the core — instead of leaving the highlighted row
  out of view. A manual scroll stays until the next focus change, and
  focusing a plain shell touches nothing.

## 0.1.15 — 2026-11-18

### Fixed

- A program that uses synchronized output (`CSI ? 2026 h` … `l`, as both
  Antigravity CLI and Claude Code do) no longer flickers: a frame's bytes are
  held until it closes, so the pane never paints a half-written line or a
  cursor sitting in the middle of the old text. The hold is bounded at one
  second and 256 KiB, a frame boundary inside one DATA chunk paints the frame
  before it, and markers split across chunks are bridged.

## 0.1.14 — 2026-11-18

### Added

- `[advanced] attach_history_lines` — scrollback a cold attach asks the core to
  replay on its first paint: 5000 by default, `0` for the screen only, clamped
  at 50000. A cold open of a 6000-line terminal now carries 5000 lines instead
  of the hard-coded 200.

### Changed

- Tab switches reuse the screen they already have. Leaving a tab hands the
  focused pane's VT to the observer that inherits the terminal, and switching
  back adopts that VT at the new wire's first paint while the observer's live
  wire keeps feeding it — a warm switch costs about one screen on the wire
  (5,958 bytes against 282,496 for a cold open of 5000 lines) and the cached
  scrollback, cursor style and palette survive. A resumed attach, a changed
  terminal incarnation, an evicted observer or a screen that never arrives
  each fall back to a cold attach with full history.
- `[advanced] scrollback_limit_bytes` (Herdr's own key, default 10000000) now
  sizes the client VT for the focused pane and every cached observer alike;
  the former 256 KiB / 64 KiB role split is gone, so a cached tab keeps the
  history it left.

### Fixed

- Leaving a tab now hands its screen to the cached observer. `sync_target`
  read the target after the outgoing wire had idled the pane, so the handover
  never ran and every switch dropped the screen it had.

## 0.1.13 — 2026-11-18

### Added

- Ctrl+V image paste over SSH sends the image in acknowledged pieces
  (`terminal.put_begin` / `put_chunk` / `put_finish`, `put_abort` on failure,
  64 KiB per piece) when the core advertises `put_chunks`; a core without it —
  and any terminal on another machine — keeps one `terminal.put`.
- `hc-proto` models `events.subscribe`'s `docs` (named documents, a trailing
  `*` is a prefix) and keeps the watcher's document subscription list in one
  place (`WatchParams::doc_patterns`) for forks such as backlot.

### Changed

- The pinned official test core is official `hc` 0.1.21
  (`c7517808d64f06609e4ecf0db1757005373cc3a3`), and the OpenRPC schema fixture
  is copied from the same commit. Older official cores keep working.
- `terminal.send`'s `cancelled` error kind reads as a refusal: the notice says
  nothing was typed and that the line can be sent again, never as success.
- `terminal.list`'s `machines` is read as an answer even when empty (`[]` means
  no machines on 0.1.21 and later); older cores that omit it read the same.
- Documentation links the fork's client contract,
  [docs/fork-features.md](https://github.com/followcat/herdr-core/blob/v0.1.21-followcat.1/docs/fork-features.md),
  from the compatibility and status pages.

### Fixed

- `tools/build-test-core.sh` stops git discovery at `target/`: ghostty's
  `build.zig` runs `git describe`, and from the fetched sources git walked up
  into this repository, where a release tag on `HEAD` made zig reject the
  checkout (`tagged releases must be in vX.Y.Z format matching build.zig`).
- The parity gate is meaningful again: the parity config copy selects herdr's
  own agent-panel sort (`spaces`, header label `grouped`) and pins the audited
  popup binding to a one-shot popup, so the default scenario reaches
  `popup-open`'s successors; both clients stand in one fixture directory so the
  navigator's path row compares; the harness excuses exactly the documented
  cells it cannot unify (the version badge, the navigator's pane id, and the
  cursor shape right after the pane command) and nothing else.
