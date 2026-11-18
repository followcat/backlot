# Changelog

All notable changes to hc-tui are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## Unreleased

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
