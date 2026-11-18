# Changelog

All notable changes to backlot are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

backlot is a fork of [hc-tui](https://github.com/followcat/hc-tui). This file
records backlot's own releases; for the hc-tui releases merged in (up to
v0.1.13), see [hc-tui's changelog](https://github.com/followcat/hc-tui/blob/main/CHANGELOG.md).

## Unreleased

### Changed

- **The expanded Hollywood bar is the Call Sheet.** One list of everything
  waiting on the user across every machine — user-owned prompts first, then
  stopped stages and offline machines with live studios, then open cards, then
  studios ready to finish — each row answered from the bar with keys: `y`/`d`/`p`
  on a prompt, `r` on a stage, `a`/`x`/`s` on a card, `f` with an inline `y/n`
  confirm on a ready studio, `i` for `tell hollywood`, `?` for help. Remote
  prompt, stage and finish actions run on their own machine; cards and `tell`
  run on the local core. Clicks select; activation stays on the keys.
- Frames carry their own machine's stage and connectivity, and a
  machine-qualified label (`beta-tools · host-b`) that truncates the machine
  before the id. Under height pressure the frames fold into a one-row chip
  strip, details and the headline go before the waiting rows do, and the list
  scrolls with a `i/N` marker.
- The summary document gains `attention_seq`/`attention_detail` on roles and
  `tasks_done`/`tasks_total` on studios — additive, `backlot/1` unchanged.
- `x` in the expanded bar is the card cancel; `Esc` or the toggle collapses.

## 0.2.3 — 2026-11-18

### Added

- The in-client update, on backlot's own release: `prefix+u` (or a click on
  the `↑v…` hint) opens the update card with the release notes of every newer
  release, `↵ update` runs backlot's `install.sh` for the latest release,
  verifies `backlot --version`, and `↵ restart` re-executes the installed
  binary in place. A package-managed or dev build is refused with its own
  command, and `BACKLOT_INSTALL_DIR` names another directory.
- A running-version badge in the sidebar header (and in the mobile header),
  with an accent `↑v…` hint for a newer release and a green `↻v…` once an
  install waits for its restart. `BACKLOT_NO_UPDATE_CHECK=1` still opts out.
- `◌ waiting` for an agent whose own turn is quiet while work it started is
  still running.
- The sidebar and panel chrome the user changed by hand — width, the
  spaces/agents split, collapse state, the agent-panel sort — is remembered
  per endpoint across restarts.
- An opt-in diagnostics snapshot.

### Changed

- **Studio workspaces drop the tab bar.** A one-row studio header
  (`studio › role t1 ◐ ↑`) takes its place. `Alt+↑/↓` switch studios, and `Alt+←/→` step
  through the cast. Elsewhere Herdr's tab keys apply. Other workspaces keep Herdr's tab bar.
- **The lot pill** (`✦ N · !U you · C card`) replaces the floating collapsed
  strip. It turns red when something waits on you. The Hollywood bar starts
  collapsed (`[hollywood] start`).
- **One glyph and colour vocabulary** across the panel, the cast sidebar and
  Hollywood. `idle` is a grey `○`, and only `working` animates.
- **Panel selection** starts on the first actionable row. `j`/`k` skip
  dividers. The highlight stays inside the block's borders and hides while the
  panel is unfocused. Hints never cut a word.
- **`Ctrl+B ?` in studio mode** leads with backlot's sections. The glyph legend
  is pinned above the footer.
- **Settings:** a `studio` tab with call-sheet start, direct `Alt+Y`, dock
  width and the overseer. An unset overseer shows the setup command.
- The agent panel's default sort is `priority` (waiting first), and a sort
  the user picked is remembered.
- Official `hc` 0.1.21 is the tested core; older 0.1.x cores keep working,
  with each missing capability switched off. The pinned test core and the
  schema fixture move with it, and the fork's client contract is linked from
  the README and [core compatibility](docs/compatibility.md).
- The studio keeps naming the `backlot` document in exactly one place
  (`WatchParams.documents`) and names no documents in `events.subscribe`:
  on 0.1.21 and later cores that would silence terminal events.
- Image paste over SSH sends acknowledged pieces when the core advertises
  `put_chunks`, and one `terminal.put` otherwise.

### Fixed

- The studio cast stays in the sidebar when the core has several machines.
- A `cancelled` `terminal.send` is a refusal: the stage and the client both
  say nothing was typed, the message stays queued, and a timed-out send never
  reads as "not typed" (the core may still type it).
- Pane output follows the tail when a key is pressed (not row 0), attaches
  release their call registrations when they close early, and a narrow mobile
  header keeps the studio's name before the update hint takes its cells.
- The parity gate compares again: herdr's own agent-panel sort in the
  reference config, a one-shot popup pin, one fixture directory for both
  clients, the fixture's cursor shape, and exact-cell known differences with
  their reasons.

