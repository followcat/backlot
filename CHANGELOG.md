# Changelog

All notable changes to backlot are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

backlot is a fork of [hc-tui](https://github.com/followcat/hc-tui). This file
records backlot's own releases; for the hc-tui releases merged in (up to
v0.1.14), see [hc-tui's changelog](https://github.com/followcat/hc-tui/blob/main/CHANGELOG.md).

## Unreleased

### Added

- **The studio dock is the unified inspection surface.** `Ctrl+B y` still opens
  it, but the panel is now the one place a studio's people, tasks and
  deliveries are visible and inspectable. Each studio block carries a health
  glyph (`●` live, `◌` stage stopped, `×` offline or ledger error, `✓`
  finished), `r<version> · <done>/<total>` progress and a `!<waiting>` count;
  the owed-step line names the role the step is owed to (`→ deliver t1 ·
  actor-a`) instead of the raw ledger key. People rows show the `USER` badge
  and the read-only `dir` tag on director-owned prompts, and a `[y/d/p]` cue
  line on user-owned ones. Tasks with no live actor get their own rows —
  uncast, dependency-gated, paused, at rework cap or blocked, and accepted —
  and accepted tasks fold under `Space` when there are six or more. `Space` on
  a person or task opens a three-line drawer; on a studio header it folds the
  block. The dock's width is fixed (38 columns at 160+, 36 at 100+, 28 under),
  it sits beside the panes while they keep 44 columns and overlays the pane
  surface below that; `Esc` unfocuses it beside the panes (borders mute,
  `Ctrl+B y` refocuses, `q`/`Ctrl+B Y`/`×` closes) and steps back over an
  overlay — to the 16-column strip when it fits, else closed — with the footer
  reading `esc back`. The collapsed strip shows health, a short studio/machine
  label and the active or blocked roles.
- **The `v` Task & Delivery Sheet.** `v` on a dock row opens a centered modal —
  inside the pane surface, never over a beside dock — with three tabs: Overview
  & Casts (acceptance, dependency chain, every cast), Deliveries & Reviews
  (commit range, actor-reported and stage-run check exit codes and durations,
  verdicts and findings) and Actor Records. The summary paints at once while
  `status --json` and `records --json` load asynchronously on the studio's own
  machine; `o` streams a delivery report or check log, locally or through
  `backlot report show` remotely.
- `backlot report show --studio <s> --file <relpath>` streams a studio-relative
  file under `reports/`, `checks/`, `attention/` or `requirements/` to stdout
  — `..`, absolute paths and symlinks that leave the studio are rejected, and
  the stream is capped at 1 MiB.
- **The dock's keys.** `Enter` focuses a live terminal (a closed person shows
  `closed — press o to reopen`), `o` reopens a resumable person, `p` peeks,
  `y`/`d` answer a user-owned prompt, `+`/`-` set a task's priority, `r`/`f`
  run the stage-start and studio-finish actions, `c` collapses, `q` closes,
  `?` opens help, and `m` is reserved for the S3 context menu.
- The `Ctrl+B ?` help gains a `studio dock (open)` group; dialogs (the sheet,
  the New Studio dialog, help) share one geometry and center inside the pane
  surface, so no dialog ever crosses an open dock's border.

- **The native New Studio dialog.** Every studio-mode new-studio entry —
  `Ctrl+B N` over a studio, `n` on the opening page or in the Call Sheet, the
  `+ new studio` frame (a click now opens it), and the sidebar's
  `new · <machine>` with that machine preselected — opens a four-step overlay
  in the client instead of the cooked-stdin wizard popup: **1 Machine & Repo**
  (Local plus every attached machine, probed for reachability and
  `backlot --version`, `Ctrl+R` re-checks; the repository browser has a
  breadcrumb, a `/` filter and `→`/`←` descend/ascend), **2 ID & Title** (live
  validation and a duplicate check against that machine's studios), **3 Cast**
  (per-role runner/model/effort cycled among what the target machine detected,
  actor candidates added with `a` and removed with `x`, `Ctrl+E` edits the
  generated cast.toml in `$EDITOR`), **4 Check & Start** (`studio new
  --dry-run`, then `Enter` creates on the target machine and optionally starts
  its stage, after which the new producer's terminal takes focus). `Esc` steps
  back through the steps and then closes; `Ctrl+C` cancels from any step. A
  studio can now be created on any attached machine: every question is answered
  by running `backlot … --json` there over `terminal.run`, remote repository
  paths are never checked against the local disk, and the generated cast
  travels on stdin (`studio new --cast -`) so no file is left behind. In a
  terminal, `backlot studio new --interactive` still asks the same questions.
- `backlot repos --json [--from <dir>]` lists a directory's entries for the
  dialog — each classified `git_ready`, `git_empty` or `dir`, with the studio
  that uses it — defaulting to `$HOME`.
- `backlot runners --json` prints the runner detection as JSON, with the
  models and efforts backlot knows per runner.

### Changed

- **A tab switch keeps its screen, and a cold open has more history.** A cold
  attach asks the core for 5000 lines instead of 200, every pane's VT takes the
  10 MB scrollback budget, and switching back to a tab continues the VT it left
  instead of replaying history — about one screen on the wire instead of 5000
  lines. `[advanced] attach_history_lines` (default 5000, clamped at 50000)
  and `[advanced] scrollback_limit_bytes` (default 10000000) tune both. A
  resumed attach, a changed terminal, an evicted observer or a screen that
  never arrives fall back to a cold open with full history. The studio's role
  terminals get the same cold open and the same warm handover.
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

### Fixed

- A program that redraws with synchronized output (`CSI ? 2026 h` … `l`, as
  Antigravity CLI and Claude Code do) no longer flickers: a frame's bytes are
  held until it closes, bounded at one second and 256 KiB, so the pane never
  paints a half-written line or a cursor in the middle of the old text.

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

