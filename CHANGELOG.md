# Changelog

All notable changes to backlot are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

backlot is a fork of [hc-tui](https://github.com/followcat/hc-tui). This file
records backlot's own releases; for the hc-tui releases merged in (up to
v0.1.17), see [hc-tui's changelog](https://github.com/followcat/hc-tui/blob/main/CHANGELOG.md).

## Unreleased

### Added

- **Recasting a fixed role.** `backlot recast --person producer|director|writer
  --runner <r> --model <m> [--effort <e>] [--permission-profile
  standard|worktree] [--reason <r>]` records `role.recast` in the studio's
  ledger. The reduced state uses the new identity for that role from then on;
  every earlier event keeps the identity it recorded, so deliveries and reviews
  stay attributed to whoever made them. The stage closes the old terminal with
  `role.closed` (reason `recast` — never a loss) and starts a fresh conversation
  under the same `bl-<studio>-<role>` name, never resuming the old runner's
  session; queued messages stay queued for the new terminal, and its first
  prompt says it takes the role over and must read `backlot status` and
  `backlot records` first. The command reuses `studio new --dry-run`'s runner,
  effort and cline-model rules and the studio's `[limits]` seat check, and is
  the user's alone. Hollywood gains a `recast` card (`backlot hollywood propose
  recast …`): its base is the role's current identity, its text reads `recast
  director → cmd/deepseek-v4.1-flash`, and its execution records `role.recast`
  `by: user` with the `c<n>` marker so a restart repeats nothing.

### Fixed

- **A cmd role's extra directory really stops the outside-workspace prompt.**
  `--add-dir` only feeds the model's `<workspace>` context; the tool permission check
  builds its runtime without that list and reads
  `permissions.additionalDirectories` from the project settings file instead. The
  stage now writes the same directory there — the studio state directory for a role,
  `<base>/studios` for Hollywood — beside `allow` and `deny`, splicing into an
  existing file and keeping every other key. So a stage-sent message read no longer
  stops at `Outside the workspace — …`. The `--add-dir` flag stays: it still gives
  the model the directory as context.
- **cmd roles and Hollywood read their state files without a prompt.** A `cmd`
  launch now passes `--add-dir`: the studio's state directory for a studio role,
  and `<base>/studios` for Hollywood, the sibling of its `<base>/hollywood` cwd.
  A role's cwd is its worktree and Hollywood's is `hollywood/`, so the message
  files the stage sends (`<studio dir>/messages/<n>-….md`) lay outside cmd's
  workspace and every read stopped at `Outside the workspace — …` — while the
  prompt is open the stage's guarded send never delivers. The flag is per run and
  rides fresh and resumed launches alike.
- **A user config's `[runner_env]` now reaches every role's launch.** `hollywood
  setup` writes the runner's `PATH` into `~/.config/backlot/config.toml`, but the
  stage read `[runner_env]` only from the per-core `config.toml`, so the next
  Hollywood launch failed with `cannot start "cmd": No such file or directory`.
  The stage now reads the user config's `[runner_env.<runner>]` as a lower layer
  than the per-core one — `agent.env` and `[env_file]` first, then the user
  config, then the per-core table, which wins on any key it sets. Only
  `[runner_env]` is read from the user config; its `[env_file]` is ignored, and a
  malformed user file refuses with the position only, like the per-core one.

### Changed

- **Stopping the stage moved to the machine's menu.** `Stop stage…` is gone
  from every studio's context menu: a stage is the scheduler of a whole
  machine, so stopping it halts every studio there while running roles and the
  ledger keep going. The machine row's menu now lists `Stop the stage…` right
  after `Restart the stage` while the stage runs, and its confirm names the
  machine and says the stop is machine-wide. A studio keeps `Start stage` while
  the stage is down, the one stage row that unblocks it.

## 0.3.0 — 2026-11-20

This release gives every machine its own `backlot` version, a machine menu that
installs and upgrades it, and a stage that survives failure — bounded git
calls, studio `prune`, producer alerts, rate-limit and connection-error
recovery, `stage watch --restart` and `stage service`. The ledger moves to
schema v1 with `studio migrate`, D17a hardening and factual usage and cost, and
casting gains a factual `cast --suggest` advisor and an experimental mode.

### Added

- **Factual casting advisor (L3).** `backlot cast --task <id> --suggest [--json]`
  lays the studio's `[[candidates]]` beside their cross-studio actor records for
  the task's kind: task and delivery counts, first-delivery outcomes, defects,
  stage checks, upstream changes, takeovers, final outcomes, wall time and
  usage — in declaration order, with `(not enough data: n/5)` and
  `(no history for '<kind>')` notices where the history is thin or absent.
  Matching tolerates runner-version differences and prints
  `[matched across runner versions]` when it applies. There is no score, rank
  or automatic pick (D8) — the director still casts with `--actor` and
  `--reason`. `backlot records --kind <task_kind>` filters the record view by
  task kind.
- **Experimental production mode (R14).** `mode = "experimental"` is a
  first-class studio mode: `backlot studio new --mode standard|experimental`
  overrides the cast file's `mode` (a bad value is a usage error), the
  `--interactive` wizard asks for the mode and the New Studio dialog's step 2
  toggles it. `backlot status` prints `[experimental]` on the studio header and
  the dock/sheet carry an `exp` badge — standard studios and `--plain` are
  unchanged. Each role's prompt gains a research-level appendix only in this
  mode (hypotheses, competing variants, honest negative findings), and an
  experimental studio may create a task with no recorded requirement for
  direct-to-task exploration.
- **Rate limits are recognized on reporting cores.** A terminal whose status
  reads `done`, `idle` or `waiting` — Devin's rate limit reports `done` — now
  has its fresh screen tail read like a silent core's, so Devin's
  `Reached free model rate limit … reset in 54 minutes` and Claude Code's
  measured screens raise `rate limited` instead of waiting for `stall_limit_s`.
  A `working` terminal is never matched, and an `error` keeps its existing
  message-then-screen order. `hit your limit`, `session limit`, `weekly limit`
  and `monthly limit` are recognized words.
- **A weekly or monthly quota escalates at once.** Limit text naming a weekly
  or monthly limit raises `rate limited` with `needs the user` immediately —
  no three futile `continue` retries — while a `rate_pace` sentinel keeps a
  cleared attention from re-raising on the same banner, and the banner going
  clears `resumed`.
- **Connection errors are recognized and retried.** Devin stops on
  `Connection error` until sent a short message; the stage now raises its own
  `connection error` attention, sends `continue` after 15 seconds, and after
  three failed attempts escalates `needs the user`. A rate limit on the same
  sample wins, and a reporting core's bare `agent error` is suppressed while
  the connection error holds. The banner disappearing clears `resumed`.
- `backlot stage watch --restart` supervises the stage: a dead
  `backlot-stage` is started again through the same idempotent path as
  `stage start`, under a bounded budget (five starts in ten minutes, an
  exponential 2→4→8→16→30 s backoff, reset after a stable minute), and pauses
  once per episode when the budget is spent. A dead core is reconnected with a
  2→5→15→30 s backoff, announcing the edge once, and the stage is checked
  again immediately. `stage watch` without `--restart` is unchanged.
- `backlot stage service [--print | --write] [--session <s>]` renders a
  systemd `--user` unit (`backlot-stage.service`, or
  `backlot-stage-<session>.service`) that runs `stage watch --restart`;
  `--write` installs it under `${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user/`
  and never invokes `systemctl`. Working roles are refused, `--print --write`
  is a usage error, and `--remote` is refused because a user unit runs on the
  local host.
- **Every machine shows its `backlot` version.** The stage publishes
  `backlot_version` in its `backlot/1` document, so a machine whose stage runs
  answers for itself; machines without a stage (or an older stage doc) are
  probed once — `command -v`, then a login-shell lookup, then `~/.local/bin` —
  and cached for six hours, on connect, on an offline → online return, and
  after an install. In studio mode the expanded sidebar's machine rows carry a
  second line: `hc <v>` (the endpoint's core version, dropped first at narrow
  widths) plus `backlot <v>`, an accent `↑<latest>` when the machine is behind
  the known release, `no backlot`, or a dim pending/error word. `--plain`
  keeps Herdr's one-line rows untouched.
- **The machine row has its own menu.** Right-click (or the launcher) on a
  machine offers only what is valid there: `Install backlot` when it is
  missing, `Upgrade backlot to v<latest>` when it is behind, `Start`/`Restart
  the stage` on the stage's state, `New studio here` when a `backlot` can
  answer, and the endpoint ops (`Connect`/`Rename`/`Remove machine`, `Add
  machine` on the local row). An unreachable machine dims the command rows —
  the endpoint ops stay live as the repair path — and `Ctrl+B ?` lists the
  same actions in a `machines` group when any apply.
- **Install and upgrade run on the machine.** The menu's install/upgrade first
  probes the target's `backlot` path and environment read-only; a binary
  managed by mise, Homebrew, Nix, cargo or a dev `target/` build is refused
  with the channel's own command — never an `install.sh` over a package
  manager's file. Otherwise a popup terminal opens on that machine
  (`terminal.run on: <machine>` remotely) running the official installer with
  `BACKLOT_SKIP_CORE=1` (hc is never touched), a semver-validated
  `BACKLOT_VERSION=v<latest>` pin and a shell-quoted `BACKLOT_INSTALL_DIR`.
  Closing the popup invalidates the located-path and version caches and
  re-probes; an upgraded machine whose stage was running is restarted —
  `stage stop` then `stage start` through the action tracker, and the ledger
  replays so no studio state is lost. The `stage not running` dock rows are
  now clickable too: Enter or a click starts that machine's stage.
- `backlot studio prune [<studio> | --all-finished] [--dry-run] [--force]` reaps a finished
  studio's worktrees and `backlot/<studio>/*` branches. A live studio is refused (exit 2);
  without `--force` a dirty worktree is kept and branches go only through `git branch -d`, so
  unmerged work is never lost. `--dry-run` reads git and prints the plan. The ledger,
  `reports/`, `messages/` and `attention/` are never removed, so `backlot records` and
  `backlot status` keep the full history.
- A producer that stalls or cannot resume now tells a live director once that it cannot restart
  the producer and only the user or Hollywood can intervene. `backlot status` and
  `status --all` lead the studio block with a `producer alert:` line, and Hollywood's notice
  names the producer outright (`producer stalled` / `producer cannot resume — needs attention`).
- **Ledger schema versioning and migration.** Every studio and Hollywood event now carries
  `v: 1`; unversioned lines still load as `v: 1`, and a `v` newer than the binary refuses
  with a clear "upgrade backlot" error. `backlot_model::migrate` is a pure step-by-step
  pipeline ready for future versions, and `backlot studio migrate
  [<studio> | --all] [--dry-run]` rewrites a log in place — backup kept, atomic rename,
  replay-verified — while a current log is a no-op.
- **Runner-reported usage and cost.** `backlot deliver` accepts `--tokens-in`,
  `--tokens-out`, `--cache-read-tokens` and `--cost-usd` (parsed to exact integer
  micro-dollars), recorded as an optional `usage` object on `task.delivered`. The sums are
  factual only — `StudioState`, `ActorRecord` and the `backlot/1` summary carry totals, and
  `backlot status`, `backlot records`, the studio dock (`∑ 12.4k/3.1k tok · $0.08`) and the
  studio sheet show them only while a counter is non-zero. No score, per D8; `--plain` is
  unchanged.

### Changed

- Synced with hc-tui v0.1.16: the agents list follows the focused pane, so a
  tab, workspace or core-driven focus change scrolls the focused agent row
  into view (hc-tui #53).
- Every stage `git` call is bounded at 60 seconds. A hung git is spawned in its own process
  group, gets SIGTERM then SIGKILL (one second later), is reaped, and surfaces
  `git timed out after <n>s` instead of blocking the stage tick for every studio.

### Security

- **D17a boundary hardening.** A permission prompt whose block reaches for `BACKLOT_`
  reads/`unset`/`export`/`declare`, `env -u`/`--unset`, `/proc/*/environ` reads or
  `env`/`printenv` dumps coupled with secret/`.env` paths is now flagged `needs_user`
  alongside the existing `env -i`/`--ignore-environment` checks. `report show --file`
  rejects NUL bytes and keeps its canonicalized-target containment; studio and card
  identifiers are validated before any path or argv is built (including `..` draft
  escapes and leading `-`), and the client's action runner refuses argv or machine
  values carrying control bytes.

## 0.2.4 — 2026-11-19

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
- **The dock's keys.** `Enter` focuses a live terminal (on a closed person it
  reopens a resumable or fixed seat through the action tracker; `o` does the
  same for a resumable one), `p` peeks,
  `y`/`d` answer a user-owned prompt, `+`/`-` set a task's priority, `r`/`f`
  run the stage-start and studio-finish actions, `c` collapses, `q` closes,
  `?` opens help, and `m` opens the row's context menu.
- **Studio context menus everywhere.** `m` on a dock or Call Sheet row,
  right-click on a dock row, a Call Sheet row, a sidebar cast row or the
  studio header, and `Ctrl+B m` from a focused pane (a role pane's role menu,
  a shell pane's studio menu) all open the same menu for that target —
  focus/peek/sheet navigation, prompt answers (the D9 director override goes
  through a confirm), task verbs, tells, stage and finish. Items a press
  cannot run are dimmed with their reason or hidden, the menu clamps inside
  the screen (`min(34, cols - 4)`), and each item's shown key activates it.
- **The native studio dialogs.** Menu `…` items open a one-line reason field
  (pause/resume/abandon/takeover; card cancel and studio finish keep the
  reason optional), a confirm card (close a role, stop the stage, clear
  stale attention, answer a director-routed prompt yourself), the four-row
  Tell editor (`tell <role> --text`), and the Revise editor prefilled from
  `requirements/v<n>.md` (`Ctrl+E` rewrites it in `$EDITOR` locally; a remote
  studio prefills through `backlot report show` and submits `req revise
  --stdin` over `terminal.run`). `Esc` cancels, `Enter`/`y` submits, and
  dialogs center in the modal band beside the dock.
- **One action lifecycle.** Every `backlot` call the TUI makes — dock keys,
  Call Sheet rows, menus and dialogs — runs through the ActionTracker: the
  target row spins `⠋ <verb>…` while it runs (a repeat of the same command
  waits), a success toast holds 3.5 s, and a failure card holds 8 s with the
  first `stderr` line, always naming a remote machine (`host-b: paused task
  t1` / `host-b failed: …`). `C` or `copy cmd` writes the exact command —
  `ssh <machine> -- backlot …` for a remote studio — to the clipboard
  through OSC 52.
- `backlot stage stop [--session <s>] [--remote <t>]` ends the stage's
  terminal gracefully: `stage stopped`, or `stage already stopped` when no
  stage is up.
- `backlot tell <role>` and `backlot req revise` take `--text <msg>` /
  `--stdin` alongside `--file` — exactly one source each, empty text
  refused.
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
- **`Ctrl+B ?` is the studio action launcher.** In studio mode the help
  overlay keeps Herdr's keybind reference and gains runnable rows: a
  `waiting on you` group first (your prompts, stopped stages, Hollywood
  cards and studios ready to finish), then `studio & task actions` and
  `hollywood`, then Herdr's own sections. `Enter` on a runnable row runs
  the action through the same context-menu path a right-click uses —
  opening a compact target picker or the S3 reason dialog when a target is
  needed — and `Enter` on a reference row focuses its surface. `/` filters
  (`/pause` finds `Pause task…`), the glyph legend stays pinned, and the
  sidebar's `menu` button opens the overlay. With no studios and no
  `[hollywood]` — or under `--plain` — the list stays Herdr's own,
  cell-identical.
- **Every studio surface answers the mouse.** Call Sheet rows select and
  act like `Enter`, chips run their action, the `tell hollywood` line
  opens for editing on click, and the bar's scrollbar drags while the
  wheel scrolls rows vertically or the frame strip horizontally;
  right-click on the title or a row opens its S3 menu. Clicking outside
  the expanded bar collapses it to the lot pill and hands the keys to the
  pane underneath. The lot pill's left-click toggles the bar and its
  right-click opens the studio header's menu; studio header clicks focus
  the studio; sidebar cast rows focus their person and the `others` row
  expands in place. In the dock, left-click selects and activates,
  right-click claims the dock's keys and opens the row menu, the scrollbar
  clicks, drags and wheels, and dragging the left border resizes the dock
  between 28 and 48 columns. Every path that opens an overlay or selects a
  row claims the keys for that surface; every path that focuses a pane
  gives them back.

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

