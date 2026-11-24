# Changelog

All notable changes to backlot are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

backlot is a fork of [hc-tui](https://github.com/followcat/hc-tui). This file
records backlot's own releases; for the hc-tui releases merged in (up to
v0.1.17), see [hc-tui's changelog](https://github.com/followcat/hc-tui/blob/main/CHANGELOG.md).

## 0.4.0 — 2026-11-24

This release lets agents that never report progress serve as studio roles. The stage now raises a stall only when a role
shows no sign of life at all: no hook signal, no screen change, no busy child process, no work area change and no
ledger event. A dialog on a role's own screen is escalated, never answered by the stage, and an outward action such as a
push, `gh`, a deploy or an install is refused and escalated to the person. agy, cmd and Devin get hook signals. A role
may report one line with `backlot progress`, and the panel shows it next to the command the role is running. Around
that, studios gain lifecycle actions (restart or recast a role, free work areas, delete with an archive and restore),
notifications wherever the person is looking, one needs-you list, each studio's next step, and a keyboard-driven left
tree.

### Added

- **One-line role progress & live descendant command display (D5 / R7e).** Added
  `backlot progress "<line>"` (`--file`, `--text`, `--stdin`) recording `role.progress`
  (control chars stripped, collapsed to one line, capped at 200 chars) in `backlot-model`
  (`progress.rs`, `RoleProgress`, `RoleRunning`, `EventKind::RoleProgress`, `HollywoodKind::RoleProgress`)
  and `backlot-stage` (`args_studio.rs`, `commands.rs`, `cli.rs`, allowed across all role prompts
  and runner base rules; returns `Fail::Usage` naming `BACKLOT_ROLE`/`BACKLOT_STUDIO` outside a role env,
  with no stall/nudge obligations). Extended D4 process-tree probing (`liveness_diff.rs`, `liveness.rs`,
  `live.rs`, `daemon.rs`) to track `deepest_descendant` `argv` (capped at 60 chars) under
  `RoleSummary.running` / `LiveRole.running` / `RoleState.running` only while `work == working`;
  `RoleSummary.doing` and `LiveRole.doing` stay unchanged (`Option<String>`) for older clients.
  Updated `backlot status`, `backlot status --json` (per-role `progress` and `running`, once each),
  `hc-tui` panel (`panel/working_line.rs`, `PersonLine::Working`, client wall-clock ages via
  `update_state::now_ms()`), and studio docs to show `working <dur> · "<progress>" <age> ago · <running> <dur>`
  with progressive narrow-width fitting.

- **Unified six-source liveness evidence replacing separate stall clocks (D4 / R7d1).** Added
  `backlot-stage/src/liveness.rs`, `liveness_probe.rs`, `liveness_diff.rs`, `liveness_work.rs`, and
  `runners/normalise.rs` (+ `normalise_strip.rs`), plus `RoleLastEvent` / `last_by_role` in
  `backlot-model`: one `Evidence` track per `(studio, role)` follows hook signals, mid-turn
  `activity.revision` (`revision_stalls`), per-runner normalised screens (stripping spinners,
  timers, `Thought for …` headers, token counters, and status clocks while keeping the send gate on
  raw screens), local process trees (`term.pid` via `/proc` on Linux or `ps` CPU-only on macOS,
  10 s sample throttle, root CPU/I/O floors of `8 jiffies/s` and `16 KB/s` measured on `agy` and
  applied only without session hook signals, and exited `cutime+cstime`), local git work areas
  (`HEAD`, resolved ref, `.git/index` mtime, and 60 s `git status --porcelain` hash), and role
  ledger events (`by.role`). Kernel inode lock waits (`lock_inode_wait`) get a `3 × stall_limit_s`
  threshold (`waiting on: <argv> for <dur>`); pure screen animations stall with
  `screen animates only (spinner)`; any moving evidence clears `stalled` naming its source. Added
  `spin:` to `tools/fake-runner.sh` for spinner harness tests.

- **Signal & guard hooks for `agy`, `cmd`, and `devin` (R7d2).** Added `--runner <name>` to
  `backlot hook` normalizing `agy` (`conversationId`, `hookEventName`, `PreInvocation`/`PostInvocation`)
  into `signals.jsonl` and enforcing `guard_rules` on `agy` `PreToolUse` (`run_command`
  `CommandLine`) with `role.refused`, deduplicated `outward action` attention, and a 1.5 s fail-open
  budget returning `{"decision":"allow"}` / `{"decision":"deny","reason":...}`. Wired per-work-area
  byte-preserving hook files on every start and resume for `agy` (`.agents/hooks.json`), `cmd`
  (`.commandcode/settings.json`), and `devin` (`.devin/config.local.json`), excluding new files in
  `<git>/info/exclude` (following linked-worktree `commondir`) and leaving tracked files untouched with
  a `hooks unavailable` Info attention and launch log. Updated `caps.rs` (`agy`, `cmd`, `devin`
  `hooks`, and `devin` `prompt_detection.hook = true`) and regenerated `docs/studio/runners.md`.

- **Dialog escalation, `agy` work states, and turn-level revision stalls (R7b).** Disabled
  structured `activity.revision` stall tracking for runners whose revision only bumps on turn
  completion (`mid_turn_revision: false` for `agy`); wired `agy` 1.5.16 work states, prompt
  blocks, and `Native` status caps from R7a; added D2's 12-line generic dialog fallback rule in
  `runners/dialog.rs`; raised, routed (`actor` → reviewer, `director`/`writer` → `producer`,
  `producer`/Hollywood → `needs_user`), and cleared (`dialog closed`) `question` attentions for
  non-permission dialogs while keeping `gate_open` closed; and denied Claude `AskUserQuestion`
  while instructing role prompts to use `backlot ask` / `backlot tell`.

- **Outward-action guard (`backlot guard`, shims, git hooks, and `agy` deny rules).** Every launched role (including Hollywood) gets `<role>/bin` first on `PATH` (ahead of `exe_dir`) with `gh`, `hc`, `scp`, `rsync`, `ssh`, `npm` and `cargo` shims, plus `GIT_CONFIG_*` `core.hooksPath` pointing at `<role>/git-hooks` (appended without clobbering existing `GIT_CONFIG_*`, refusing `git push` via `pre-push` while passing client git hooks through to the repository's own hooks). Refused commands exit 126, record `role.refused` in studio ledgers and raise one `outward action` attention (`needs_user: true`) per episode. `agy` launches also splice the 14 `command(<prefix>)` entries (without trailing ` *`) into `.agy-settings.json` `permissions.deny`.

- **The lifecycle actions in `Ctrl+B ?`.** A `lifecycle` group lists `Restart <role> ·
  <studio>`, `Recast <role> · <studio>`, `Free work areas · <studio>` and `Archived studios ·
  <machine>`, filterable by role, studio or machine name.

- **Archived studios in the left tree.** When a machine's `studio list --archived --json` is
  non-empty the finished group gains an `Archived (n) …` row; Enter lists each entry (name,
  stamp, repo, requirement) and `Restore as <name>` runs `studio restore` with `--dry-run`
  first, then for real. The archived list is cached per machine like the ledger list.

- **`Free work areas (prune)…` and a finish guard.** A finished studio's menu fetches
  `studio prune --dry-run`, shows its plan, and then prunes for real. `Finish studio…` on a
  studio with no requirement and no tasks asks `Nothing was built yet (no requirement, no
  tasks). Finish anyway?` and passes `--force`.

- **Restart and recast from the role menu.** A live or stuck role's menu offers `Restart <name>`
  — a two-choice confirm, `Resume its session` (the default when its runner keeps a resumable
  session) or `Fresh start`, running `backlot restart --as user` on the studio's machine.
  Hollywood's own menu carries `Restart Hollywood`. The fixed seats — producer, director, writer
  — add `Recast…` (runner, then model, then effort from `backlot runners --json` and each
  runner's caps; a no-effort runner has no effort step, an optional one allows empty) and no
  longer offer the `Close` that the command refuses on them.

- **Studio events notify the person, wherever they are looking.** The client now derives
  notifications from the `backlot` documents, not only from pane status words: a role's
  unanswered question (`producer asks: <question>`), a user-owned prompt on a role or on
  Hollywood (`hollywood needs you: <command>`), a Hollywood reply
  (`hollywood replied: <headline>`), an open card (`card waiting: <action>`), a studio every
  task is done on (`acme-shop is ready to finish`) and a stage that stopped with live studios
  (`stage stopped on host-a`). Each event rides the existing settle → queue → TTL pipeline with
  the same `[ui.sound]` and `[ui.toast]` config, fires once per ledger seq (the first document
  after start seeds silently, and a reconnect reseeds), and is suppressed when its target is
  already focused, exactly as a watched pane is. The toast's key (`prefix+o`) or a click opens
  the row's target — the role's terminal, `bl-hollywood`, the Call Sheet on that card, the
  studio, or the dock's `stage stopped` row (selected, never started: `s` stays the person's
  press). A new `[ui.toast] studio = false` (default `true`, backlot-only) turns them off;
  `--plain` emits nothing.

- **The lot shows who runs on what.** Every person row in the studio dock now carries a
  dim runner tag before its state word (`claude`, `cmd`, `agy`, `codex`, `devin`,
  `cursor`, `cline`, `opencode`, `fake`) and, while the frame has room, the cast's short
  model form. The state word keeps the right edge and the name and task keep their cells:
  the badge uses only spare space, dropping the model first, then the tag, so the name
  truncates only when it alone overflows.

- **"Restart to update".** When the local stage's published `backlot_version` (with its
  published `backlot_build`, compared once the versions are equal) or an installed
  `backlot --version` from the six-hour machine probe is newer than the running client,
  one dim line closes the lot: `backlot <new> installed — restart the client to update
  (q, then backlot)`. Strict semver decides; an older stage never draws it. The build id is
  compiled in from git (`HEAD`, `-dirty` while the tree has changes), and
  `backlot --version` prints it as `backlot 0.3.0 (abc123def456)` so equal-version rebuilds
  are still distinguishable.

- **The left tree answers to the keyboard: the launcher's machine rows.** `Ctrl+B ?`'s
  `machines` group now carries, per machine, `Stage log · <machine>`,
  `Unfold finished studios (n) · <machine>` (or `Fold …`) and `Machine menu · <machine>` — the
  latter opens the same menu a right-click does — beside the existing stage rows, all filterable
  by machine name.

- **The left tree answers to the keyboard: focus, movement and row actions.** `Ctrl+B t` moves
  the keyboard into the left machines tree (opening the sidebar if it is collapsed) and
  highlights the row for the current place; `↑`/`↓` (and `k`/`j`) move the highlight and the
  section scrolls to it; `Enter` runs the same action a left-click does — a workspace row focuses
  its terminal, a stage-down studio starts the machine's stage, the `finished (n)` fold toggles,
  a finished studio opens its menu, a machine row folds its section — and `m` opens the row's
  context menu. `Esc` returns the keys to the terminal. `Ctrl+B t` is free in the default keymap
  (`Ctrl+B Tab`/`Ctrl+B h` would collide with the pane cycle/focus bindings).

- **The empty states say what to do.** With every stage down and no published studio, the lot
  now says `<n> studios on stopped stages — s to start` (starting that one stage when a single
  machine holds them all) instead of `no studios`, and the cast says `stage stopped — s to start`
  instead of drawing blank.

- **No truncated ids in the tree's stage-down row.** The tree's stage-down row puts the studio
  name first and shortens its tag to `down`, so a 10–12 character id fits whole at the default
  sidebar width instead of reading `beta-too…`.

- **A new studio starts with what to build.** `backlot studio new --ask <text>` (or
  `--ask-file <f>`) queues the person's first message to the new producer as the same
  `message.queued` a `backlot tell producer --as user` records, so the stage delivers it to the
  producer's terminal as its first `[backlot] user → producer: read <file>` line. An empty ask is
  refused and records nothing; `--dry-run` restates its first line. The New Studio dialog's step 2
  gains an optional **What should this studio build?** field — multi-line, `alt+⏎` adds a line,
  `Ctrl+E` edits it in `$EDITOR`, empty is today's behaviour — whose first line shows on step 4's
  summary card under `First ask`; a non-empty brief rides the create as `--ask-file`, written to a
  temp file on the target machine. The producer's prompt now reads a first `tell` from the user as
  the requirement brief and records it before asking "What would you like us to build?".

- **Every machine's studios in the left machines tree.** A machine whose stage is down — or that
  published no document — now lists its studios from `backlot studio list --json` (run locally or
  over `RemoteRunner`, cached and refreshed on a stage change or every 30 s at most), dimmed with
  a `stage down` tag; Enter or a click on one starts that machine's stage. Each machine also
  carries a folded `finished (n)` row under its live studios that unfolds with Enter/Space (or
  the machine menu's `Unfold finished studios (n)`), and a finished row opens the existing
  finished-studio menu. A machine that cannot be reached shows one `unreachable` line instead of
  an empty tree, and the tree scrolls to the machine row of the studio the selection lands on,
  so a row never drops out of a short sidebar. The cast explains itself: with no studio current
  it shows the only live studio's cast, or one dim `select a studio (Enter on it)` line. Starting
  the stage keeps the focus where it was (from the opening page it lands on a studio or the lot,
  never the stage tab); the stage's terminal is read-only to keystrokes, its bar reading
  `stage log — read only`, and the machine menu's `Stage log` opens it as the observe-only peek.

- **Restart a stuck role in place.** `backlot restart --person <role> [--fresh] [--reason <r>]`
  reloads a live or lost producer, director, writer or actor under the same identity: the stage
  stops its terminal deliberately (`role.closed`, reason `restarted` — never a loss) and launches it
  again, resuming the recorded session when the runner supports it, or starting a new one with
  `--fresh`. The role's ledger row carries `restarted`, which the summary and Hollywood read. The
  same request reloads Hollywood itself (`restart --person hollywood`, its own `role.restart`).

- **Archive list and restore.** `backlot studio list --archived` lists the studios `studio delete`
  retired — name, stamp, repo and last requirement title — and `backlot studio restore
  <archived-name> [--name <new-name>] [--dry-run]` copies one back under `studios/` as a finished
  studio, refusing a name already taken. `docs/studio/guide.md` gains a finish/prune/delete/restore
  table.

- **Hollywood's terminal starts at a readable size.** `bl-hollywood`'s `terminal.run` now carries
  an initial grid of at least 100 columns and hc's `200×120` default, so a person who peeks or
  focuses it sees its answers instead of a 21-column strip. Peeking stays
  strictly read-only: a peek attaches as an observer, which never sends a size, so no client view
  can narrow it — only focusing it into a pane does (H3).

- **Hollywood's AGENTS.md carries a generated CLI reference.** The stage writes one line per
  command Hollywood may run, with its flags, sliced from the CLI's own usage strings
  (`args_usage.rs`, `args_hwood.rs`) at every start, and substitutes it for `{{CLI_REFERENCE}}`
  in the prompt. A drift test regenerates the checked-in copy (`prompts/hollywood-cli.md`) and
  fails when a usage string changes without it. The prompt says to use the reference and run
  `--help` only for a command it does not list, so Hollywood never probes its own CLI or blocks
  on a prompt for it (H2, P5).

- **Hollywood is fed facts, not snapshots.** Every attention Hollywood can read — in
  `backlot status`, in a notice body, in the status commands it runs — now carries its class
  (`needs you` / `stuck` / `info`), its age (`raised 2h ago`) and, for a screen snapshot, when the
  screen was taken (`screen at 14:02, may be stale`). Each role line also carries its current live
  work state (`working`, `waiting`, `blocked`), read from `<state dir>/live.json`. The stage writes
  that file on a change and a heartbeat (at most every 5 s), and a reader drops a file older than
  15 s — after the stage stops, `backlot status` says `stage not running — live state unknown`
  instead of the last `· working`. Before this, Hollywood read a launch-time sign-in snapshot
  as the producer's current state and reported a healthy producer as "not signed in". Its prompt
  now says: decide from the role's current state, never from an old snapshot, and never report
  `info` bookkeeping as a problem. `AttentionClass::label` is the one wording every surface reads.

- **Hollywood's bar shows what it is doing, and asks you when it needs permission.**
  The summary's `hollywood` object now carries `doing` (the terminal's reported
  status, read and capped like a role's), and — while a permission prompt stands on
  `bl-hollywood`'s screen — `attention`, `attention_user` and `attention_detail`
  (the command the picker asks about). The expanded Call Sheet draws one live row
  under the newest `user ›` line while that line has no reply after it: `… sending
  to hollywood` while the message is still in the outbox, `⠶ hollywood: <doing>` (or
  `⠶ hollywood is working`) while it works, and `⚠ hollywood asks: <prompt>` at a
  prompt. The prompt row takes `y` allow once, `d` deny, `p` peek and Enter to focus
  `bl-hollywood`; `y`/`d` run `backlot approve --person hollywood --decision …`,
  which records `role.approval` in Hollywood's own ledger. The stage sends the
  runner's one-time key (`enter` on cmd's `❯ 1. Yes`), never "don't ask again", and
  only while the prompt the decision answered still shows. Hollywood's cmd launch
  also pre-allows the read-only `backlot --help` and `backlot help`, so it can read
  its own CLI without a prompt.
- **One attention class per reason.** `backlot_model::attention::AttentionClass::of` sorts every
  reason the stage can raise into `needs you`, `stuck` or `info`: a person's prompts, refusals,
  failed launches and budget overruns are theirs; a stall with nudges left, a rate limit or a
  connection error is `stuck` until the stage escalates it; `session id not found`,
  `inherited XDG_CONFIG_HOME` and `cursor dir was open` are stage bookkeeping. An unrecognised
  reason is `needs you`, so nothing important is hidden. The role summary carries the class as
  `attention_class`, and `docs/studio/panel.md` holds the full table.
- **One honest state per person, in plain words.** A person row now shows exactly one state, in
  this priority: `needs you · <what>`, `asks you · <question>`, `stuck · <reason>`,
  `working · <doing>`, `waiting`, `ready`. `info` attentions — the stage's own bookkeeping such as
  `session id not found` — never change the state; they show only in the role's drawer and the
  sheet's overview, dimmed. The old `stalled` label is gone: a `stuck` episode reads `stuck …`,
  peach, and a needs-you attention that is not a prompt reads `needs you · …`, red. A live
  producer with `work: waiting`, `doing: idle` and an open `session id not found` now reads
  `ready`, not `! stalled`.
- **The producer's questions reach the person.** The producer's prompt now says every question to
  the user — including the first "What would you like us to build?" when no requirement is
  recorded — also goes through `backlot tell user --text <question>`, while the terminal stays the
  conversation. The role summary carries the newest unanswered `tell user` as `asks`, which the
  person row shows as `asks you · <question>`; a user `tell` back to that role — or any later
  ledger action the role records itself, such as a revised requirement, report or delivery —
  answers it, while stage-written events do not.
  `backlot tell user` is covered by every runner's pre-allow list.
- **One needs-you list everywhere.** One function builds it from every machine's `backlot`
  document — user prompts, roles with a needs-you attention or an unanswered question, Hollywood
  with a user-owned prompt (`HOLLYWOOD asks: …`, taking the same `y` allow once / `d` deny /
  `p` peek / Enter as its live row) or a blocked screen without one, studios with no requirement,
  stopped stages, open cards and ready studios. The Call
  Sheet's rows, the lot pill's urgent count and the band's calm row read it, so
  `✓ nothing waiting on you` shows only when the list is empty; the misleading `!N flagged`
  suffix is gone, and a `stuck` role shows on its own row.
- **Every studio shows its next step.** The dock's studio row and the Hollywood frame carry one
  `next:` line from the studio's own state: `tell the producer what to build` when no requirement
  is recorded (Enter on the studio row or `t` opens the producer tell input), `director reviewing`
  while tasks are in review, and `finish (f)` once every task is accepted. Any other state draws
  nothing.
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

- **Archived-studios footer overlap, Opening Page prefix chords, and narrow stopped-stage hints.**
  The archived-studios restore list now places its `↑↓ move  ⏎ restore as…` hint on its own row
  above the `[ ↵ choose ]` / `[ esc cancel ]` buttons so they no longer overlap; the Opening Page
  now lets direct bindings and `Ctrl+B` prefix chords (`Ctrl+B t`, `Ctrl+B ?`, etc.) through while
  still consuming unhandled bare keys; and the stopped-stage rows in the 26-column cast and
  38-column lot dock switch to compact `· s start` phrasing when narrower so `start` is never
  truncated.
- **A prompt row promises only the keys that work.** A user prompt on a runner with no approval
  key (`opencode`, `cline`, `agy`) used to show the same `y`/`d` cue as cmd or claude, so the
  person answered and nothing went out. Those rows now read
  `answer in its terminal (Enter)` instead — their `y`/`d` record no decision, the Call Sheet's
  chips, footer and the role menu offer no Allow/Deny, and `p` (peek) and Enter (focus) stay. The
  needs-you list entry is kept. Driven by `can_answer_prompt` from the previous commit.
- **A permission prompt is found and named whatever the terminal's width.** A
  `bl-hollywood` resized to 21 columns, with cmd's picker on screen and its
  structured status `kind: permission` / `state: blocked`, published `work:
  blocked` but no prompt row: `hollywood_prompt` still gated on the
  width-sensitive screen classifier, and its detail took only the line under the
  sentence — a middle fragment of a wrapped command. The prompt now stands
  whenever the combined signal says blocked (structured status, hook signal or
  screen), the detail is unwrapped from the title or `needs to` sentence down to
  `Press [ctrl+e]` or the picker (cmd's whole command, the other runners'
  question), and a prompt no channel names publishes `permission prompt`. Picker
  detection joins the screen's lines first — cmd's wrapped `❯ 1. Yes` /
  `2. Yes, don't ask` / `3. No` is enough — for roles and Hollywood alike; cmd's
  and cline's prompt blocks also open at their sentence/banner when a narrow
  screen has scrolled the `─` rule away.
- **An agy role's session is found again on agy 1.5.14.** The conversation db no longer always
  embeds the workspace's `file://` URI, so the byte scan found nothing and the role raised
  `session not found` after 60 s. The stage now reads agy's own
  `~/.gemini/antigravity-cli/cache/last_conversations.json` first — a cwd → last-conversation map —
  and takes the id it names for the role's cwd when that `.db` exists, is new enough and unclaimed.
  A missing, unreadable or malformed map falls back to the old byte scan, so older agy is unchanged.
- **A restarted stage looks for a role's session again, so installing the fix repairs a stalled
  role.** The ten-minute discovery stop is measured from the later of the role's start and the stage
  process's own start. A `role.started` replays unchanged across a restart, so a role already past
  the stop previously kept its open `session not found` forever; now a restarted stage re-scans it,
  records `role.session` and clears the attention on its own.
- **A cmd role's extra directory really stops the outside-workspace prompt.**
  `--add-dir` only feeds the model's `<workspace>` context; the tool permission check
  builds its runtime without that list and reads
  `permissions.additionalDirectories` from the project settings file instead. The
  stage now writes the same directory there — the studio state directory for a role,
  `<base>/studios` for Hollywood — beside `allow` and `deny`, splicing into an
  existing file and keeping every other key. So a stage-sent message read no longer
  stops at `Outside the workspace — …`. The `--add-dir` flag stays: it still gives
  the model the directory as context.
- **Hollywood never resumes with another runner's session, and records its own
  cmd session.** A `role.state closed`, or a `started` that carries no session, now
  clears the recorded session id — after a runner change to `cmd`, the old Devin id
  no longer survived to `cmd -r <devin id>`, which answers `No session named …`. A
  resume also refuses any id whose recorded runner differs from the one being
  launched. Hollywood's post-launch discovery already ran for studio roles; with the
  stale id gone it now records `cmd`'s session from
  `~/.commandcode/projects/<slug(hollywood cwd)>/` on a `role.state started`, so a
  lost Hollywood resumes its own conversation, and falls back to the existing rule
  when discovery finds nothing.
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

- **Finishing an empty studio needs `--force`.** `studio finish` on a studio with no requirement and
  no tasks refuses (`nothing was built yet (no requirement, no tasks) — add --force to finish
  anyway`).
- **Finishing clears stale questions.** A finished studio shows no open `tell user`: the finish is
  the answering event, so the producer's old `What would you like us to build?` no longer lingers.
- **`close` is plain, and fixed seats stay open.** `close` on the producer, director or writer now
  refuses with `the <role> is a fixed seat — use restart, or finish the studio`, and on an actor
  that is not held with `the <role> has nothing to close — it closes with its task`. No more
  `role '...' is not held` jargon.
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

