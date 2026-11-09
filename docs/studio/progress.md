# Studio progress

## S5a: The closed loop's mechanics (2026-11-09)

- Done: the stage closes the loop end to end on the fake runner. Role launches ensure git work
  areas first — `work/integration` on `backlot/<studio>/integration` for the director,
  `work/<task>` on `backlot/<studio>/<task>` for each actor, planned over injected git and reused
  on recast and reopen; a non-git repo raises `not a git repository` and starts neither, and a git
  failure raises `work area failed` once with the first stderr line. Producer and writer stay in
  the studio repo. Fresh starts render a role prompt from `prompts/<role>.md` into
  `prompts/<role key>.md` and hand the runner one line (`Read <path> and follow it.`; Devin gets
  the file form); resumes get none. Queued messages and reports deliver oldest-first per role as
  `[backlot] <from> → <to>: read <file>` behind a live/waiting/stable-screen/uptime/send-gap gate,
  one `terminal.send` per role at a time spawned off the loop; `to: user` is never sent.
  Stage-generated notices (delivery → caster, rework review → actor, blocked task and
  requirement revision above v1 → director, actor permission prompt → director) queue as ordinary
  messages deduped by `source_seq`. A role sampled blocked for two seconds raises one
  `permission prompt` attention per episode with the screen saved under `attention/`; `backlot
  approve` records `role.approval` (refused without an open prompt) and the stage sends the
  runner's key while the prompt still shows — never without a decision at or after the attention —
  then clears; a gone prompt clears with no key. `studio finish` refuses while tasks are open,
  then leaves `desired` holding only reopened people. The summary marks a behind actor's `doing`
  as `task t1 · behind vN`; the panel draws it verbatim.
- Measured: the model suite (65) covers the new events and fields; stage tests (122) cover
  work-area planning over a fake git runner and one real repo, prompt render and every prompt
  command through the real parser, each delivery gate on its own, oldest-first, a refused send,
  owed notices once per source event across restarts, approval keys per runner, episode dedup,
  and finish's reopen-only desired. `real_stage_closed_loop` drives the full loop on a scratch
  core — requirement, plan request (writer starts and is told), dispatch (director is told),
  cast into its own worktree and branch, delivery and rework notices, a midway revision notice,
  two blocked episodes answered by `backlot approve` (enter, then esc), a finish refusal naming
  the open task, acceptance, finish, and a reopen after finish. `real_stage_loop` still passes.
- Key findings: the light path needed no change — `task new`, `cast` and `review` have no caller
  restriction, and `cast.by` routes the delivery notice to the producer. The run exposed two
  bugs: `approve` was missing from the binary's command list, and an approval must be at or after
  the open attention or a stale decision answers a new prompt.
- Not verified: prompts are untested on real agents (S5b tunes them), Claude/Codex approval keys
  are convention not measurement (runners.md), the panel's behind-marker was drawn only in the
  row test, and worktree removal is out of scope — trees accumulate under `work/` until a later
  step.
- Rework 1 (2026-11-09): six fixes from reading the code against real runners. The folder-trust
  dialog no longer counts toward a permission episode — it neither raises nor holds one, and it
  restarts the two-second clock for a prompt right after it. A `terminal.send` that answers
  `sent: false` stays queued with its reason logged (a successful call's `SendResult` is the
  delivery — this core has no `sent` field). Send collection drains only the calling studio's
  finished sends, so `message.delivered` can no longer land in another studio's ledger. Task
  notices follow `cast.by` only for `producer` — a `user` cast now reaches the director instead
  of an undeliverable `to: user`. Producer and director prompts name `<studio dir>/req/` as the
  requirement's place instead of rendering an empty file at v0 (`backlot status` already shows
  the version). Devin's approval keys are mapped — `enter`/`esc`, observed on 3000.12.32.

## S4b: The studio panel (2026-11-09)

- Done: the client reads the local `backlot` document through the watcher's new
  `local_documents` param (this core only; remote machines never carry the stage's document)
  and parses it into `backlot_model::summary::Summary` — `backlot/1` only, a missing, malformed
  or foreign-schema document reads as `no studios`, and an unknown `work` degrades to waiting.
  A right-column panel draws it: `Closed` (default, zero width, no keys, no timer),
  `Collapsed` (16 columns, working groups or `idle`), `Expanded` (configured width 24–60,
  default 30). Beside the panes only past the mobile threshold with 60+ columns left for them;
  otherwise it covers the frame and the pane geometry keeps the closed shape. All five
  `layout_config().layout(...)` call sites go through `chrome_layout`. Expanded shows studios,
  the four role groups (each collapsible, empty groups skipped) and people — mark, name,
  priority flag, runner/model, `doing`, attention — with five distinct mark+colour pairs from
  `chrome/status.rs`; working marks spin on a ~120 ms one-shot deadline armed only while a
  working mark is drawn. Selection, clicks and Enter act on stable targets, never indices.
  `prefix+y` toggles; focused keys move/collapse/flip/act; `+`/`-` run `backlot priority` and
  a two-step Enter runs `backlot reopen` on resumable closed people — off the event loop via an
  injectable runner (`current_exe()` in production), failures notice with the first stderr
  line. Mouse: title flips, rows act like Enter, wheel scrolls, the scrollbar drags, an outside
  click unfocuses. Without a live `backlot-stage` terminal the panel shows `stage not running`
  and keeps the last document. Panel settings live in `$XDG_CONFIG_HOME/backlot/config.toml`
  (`[panel] width`, `key`) — malformed files notice with line:column only; herdr's config is
  untouched.
- Measured: 45 new tests — the row model (grouping, order, collapse, titles, idle), document
  reading (missing/malformed/wrong schema/unknown `work`/stage liveness), config (defaults,
  clamp, malformed position, path), the renderer (distinct marks and colours, the animated
  working mark, overflow scrollbar, stage line, selection), and App wiring (geometry at
  200/120/80/40 beside and full-screen, the toggle cycle, every focused key, two-step reopen,
  injected priority argv, the failure notice, title/row/outside clicks, wheel and scrollbar
  drag, the deadline arming only while working, and a closed panel identical in geometry,
  forwarded keys and cells). `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`
  and `cargo test` (1039 tests) green; clippy also green on the CI-pinned 1.99.0 toolchain;
  `real_stage` passes on the real core.
- Key finding: `y` is unbound in the reference default keybinds and nothing in
  `scripts/parity/` sends it, so `prefix+y` is safe; a configured herdr binding on the same
  tail wins while the panel is closed or unfocused.
- Not verified: the full parity harness was not run (the reviewer runs it against a saved
  baseline). The panel landed inside `hc-tui` (`src/panel/` + `src/app/panel.rs`), not a
  separate `backlot-panel` crate — design updated. The stage-down state shows the line only;
  starting the stage from the panel is not implemented.
- Deviations from the design brief: none beyond the crate placement and the stage-down offer
  above.
- Rework 1 (2026-11-09): real-TUI fixes — a full-screen panel resets its rect before painting
  and owns the keyboard (`Esc`/Enter/one chord all close it; beside semantics unchanged), a
  beside reservation can no longer flip the chrome to the mobile layout, the spin deadline is
  armed only when none is pending, runner text advances by display width past wide-character
  names, an unparseable `panel.key` falls back to `y` with a file-naming notice, `Esc` or
  activating another row disarms a pending reopen, and the wheel bounds by the scroll
  metrics' line maximum.

## S4a: The data behind the panel (2026-11-09)

- Done: tasks carry a `priority` (`low`/`normal`/`high`, default `normal`) — `task.cast` gains an
  optional `priority` (absent keeps the current one, old logs load) and `task.priority` changes it
  on an existing task; it orders the panel's display only and nothing in reconcile reads it. A
  closed person can be held: `role.reopen`/`role.release` set and clear `RoleState.held`, and the
  stage wants every held role back — desired with `Start.resume = <its session>`, merged into one
  entry when the role is already desired — so `act` resumes it and records `role.resumed` (reason
  `reopened`, original session ID) rather than `role.started`. New commands `backlot cast
  --priority`, `backlot priority`, `backlot reopen` and `backlot close --person`, all gated to the
  user and the director. `runners::work_state` classifies a visible screen per runner — each
  adapter's blocked prompt before its mid-turn marker, else `waiting` — and the watch samples one
  `terminal.read` (`lines: 0`) at most once a second per live role into a `Stage` `(studio, role)`
  map, keeping the last value on a refused read and dropping the entry when the role stops being
  live; it never reaches the ledger. The summary (schema still `backlot/1`) gained `work`, `task`,
  `priority` and `resumable` on roles — session IDs stay out — and actors sort open-first, then by
  priority high-first, then role key. `backlot status --json` shows `priority` and `held`. The fake
  runner answers `work:<state>` lines for the whole path.
- Measured: 58 model + 91 stage tests cover priority reduction/validation/old logs/commands,
  every reopen refusal, held roles in `desired` (resume set, no duplicate, actorless skipped),
  `role.resumed` on resume (including post-launch runners), release into `Stop`, the `work_state`
  table per runner with blocked beating working, the summary's new fields and ordering with no
  session IDs, and the one-second sample throttle. `real_stage.rs` passes on a scratch core: an
  actor's `work` walks waiting → working → blocked on `work:` lines, `task abandon` closes it as
  `resumable`, `backlot reopen` resumes it in the same session ID under `role.resumed`, and
  `backlot close` closes it again. `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`
  and `cargo test` green on every commit; clippy also green on the CI-pinned 1.99.0 toolchain.
- Not verified: OpenCode's blocked and working markers are guessed, not measured (marked in
  `runners.md`); every other runner's markers come from captured screens but have not been watched
  live. Reopen/resume ran only on the fake runner — real adapters' resume-on-reopen is S6
  territory alongside lost roles. A held role's terminal loss restarts it through the same
  desired entry, unmeasured on a real core.

## S3: Runner adapters (2026-11-09)

- Done: `runners.rs` is now a `runners/` module, one adapter file per runner plus `fake`. `plan`
  takes the identity, terminal name, working directory, an optional first prompt and an optional
  session ID to resume, and returns argv, env and how the session ID is known (`Chosen` now, or
  `PostLaunch`). Claude Code, Codex, Devin and OpenCode launch and resume with the measured
  commands in `docs/studio/runners.md`; `standard` and `worktree` permission profiles map to the
  right flags per runner, and an unknown runner or profile is an `attention.raised`, never a
  launch. Claude Code argv is prefixed `env -u` for each inherited `CLAUDECODE`, `CLAUDE_EFFORT`,
  `CLAUDE_PID`, `AI_AGENT` or `CLAUDE_CODE_*` marker; OpenCode gets its model through
  `OPENCODE_CONFIG_CONTENT` and no effort flag. `role.session { role, session_id }` joined the
  model: codex IDs are found in `~/.codex/sessions/**/rollout-*.jsonl` (newest day first, matching
  cwd, created after the role started, unclaimed), devin's via `devin list --format json` in the
  role's cwd, both through an injected exec; claude and opencode IDs read back off the terminal's
  argv for adopted roles. A live role still without a session after 60s raises one
  `attention.raised` ("session id not found"), ever. At most one real runner launches per tick
  (`fake` exempt); `runner_version` comes from `<runner> --version` once per runner per run.
  `[runner_env.<runner>]` in `<state base>/config.toml` reaches the terminal env only — malformed
  config refuses the role. Claude Code's folder-trust dialog is answered with Down then Enter
  once per role when the screen shows it inside the role's own cwd in its first 60s.
- Measured: 45 model + 57 stage tests cover the exact argv per runner/profile/prompt/resume,
  refusals, marker scrubbing, codex/devin capture edge cases (claimed, pre-start, wrong cwd,
  failures), the 60s attention once, `runner_env` parsing and confidentiality, and the trust
  parser. `real_stage.rs` passed on a scratch core with `model = "trust"` making the fake runner
  show the dialog — the stage answered, the role announced itself, the ledger stayed unchanged.
  `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` and `cargo test` green on every
  commit; clippy also green on the CI-pinned 1.99.0 toolchain.
- Not verified: no real `claude`/`codex`/`devin`/`opencode` launch was run — forbidden by the
  milestone; every argv is unit-tested, not observed. Resume argv is built but nothing calls it
  for a lost or closed role yet (S6). Codex/devin session discovery ran only against fake trees
  and injected output; `devin list`'s exact field names are as specified, not re-measured. The
  trust answer is verified through the fake's trust mode, not a real Claude Code screen. Running
  on official hc remains unverified.
- Rework 1: nine fixes, all found reviewing the first real Claude Code launch or auditing the
  new code. The trust answer now sends one key per tick chosen from the cursor's screen line —
  the two-key burst raced the real dialog — with the once-only latch gone (the 60s window still
  bounds it); the fake's trust mode models the redraw for real. Post-launch runners defer a
  start while a live role in the same `(runner, folder)` pair still lacks its session id,
  across studios, and codex takes the oldest matching rollout so a person's later file cannot
  win a role's claim. Discovery is throttled to once per 5s per role, stops after ten minutes,
  and codex opens only `YYYY/MM/DD` day directories from the day before the role's start.
  `real_exec` drains probe stdout on a thread so a chatty `--version` cannot deadlock. A
  malformed `config.toml` records only line and column — never the secret-bearing line. Claude's
  `env -u` list is the exact set of inherited markers, so `CLAUDE_CODE_OAUTH_TOKEN` and friends
  survive. A configured `[runner_env] PATH` now leads after the exe's own directory instead of
  being overwritten. A `terminal.run` refusal raises `launch refused` attention (deduped per
  role and reason, retry kept). `studio new` refuses a relative `--repo` (exit 2) and
  canonicalizes an existing one.
- Rework 2: the real launch in a git folder showed the trust answer needs pacing, and review
  found two more defects. Keys now wait for the dialog's measured unsafe start — first key only
  after the screen has held two seconds, then one per 1.5 seconds — because a key at first
  paint is lost and an early Enter ended the real runner; the fake models that window. Probe
  output is read into a shared buffer with a bounded post-exit grace, so a grandchild holding
  the pipe (a runner's background service) can no longer stop the stage loop. A refused
  `terminal.run` hands the tick's real-launch slot back, so a role whose spawn always fails
  cannot starve the roles behind it. Session discovery also ends the moment the 60 second
  attention is raised — a role can never claim a file from one that starts after it — which
  makes the ten-minute stop only a backstop; a runner that first writes its session later than
  that stays without an ID.

## S2: The stage process (2026-11-09)

- Done: `backlot stage start` launches a `backlot-stage` terminal whose child runs
  `backlot stage run` — a reconcile loop that watches ledgers and hc events (subscription with
  polling fallback) and exits non-zero on connection loss. `crates/backlot-stage/src/reconcile.rs`
  is a pure function from ledger state + live terminals to ordered actions (stops before starts so
  a recast frees the name); `act.rs` executes them through the public `terminal.run`/`kill`/`rm`,
  recording `role.started`/`role.lost`/`role.closed` around each. Desired roles: producer and
  director always, the writer from a `plan` message until `dispatch`, and `actor-<task>-c<n>` per
  open task's current cast under terminal name `bl-<studio>-<task>`. Stray `bl-<studio>-*`
  terminals are removed, orphan live terminals are adopted. `runners.rs` maps cast identities to
  launch plans — only the `fake` runner exists (`tools/fake-runner.sh`); anything else raises one
  `attention.raised`. `backlot-model` gained `role.closed`, keeping actor and session ID after a
  deliberate stop, and `summary.rs` — the pure, versioned `backlot` document schema — which
  `doc.rs` publishes with change-sensitive CAS writes at most once every two seconds. Default
  state dirs key on `hello().core.id` (`--session`/`--remote` resolve the core once;
  `BACKLOT_STATE_DIR` still wins and needs no core). hc core is untouched.
- Measured: 41 model + 33 stage tests cover reconciliation rules (loss, deliberate stop, recast,
  adoption, strays), `role.closed` semantics, summary shape/caps/drop-off, write throttling on an
  injected clock, state-dir selection and CLI refusals. `cargo fmt --check`,
  `cargo clippy --all-targets -- -D warnings` and `cargo test` green on every commit.
  `real_stage.rs` exercises the whole loop on a scratch core: idempotent `stage start`, labelled
  role terminals, writer across request/dispatch, actor close with no `role.lost`, session ID
  surviving the close, and the document's throttled writes.
- Rework (2026-11-09): three defect fixes. A closed role now finishes a crashed stop only at its
  own recorded terminal address — a recast no longer kills the new actor under the shared name.
  Terminal ownership comes from the `backlot.studio` label (`reconcile::studio_view`), never the
  `bl-<studio>-` name prefix, so one studio cannot stop another's roles or a user's lookalike
  terminal; an unlabelled terminal is never stopped, adopted or dropped. The summary drops the
  role cap: fixed roles list first, then actors by key, every recorded person stays while the
  studio is live (R19), and only finished studios are dropped for size — a live one is written
  even over the 64 KiB target (the hard limit is 1 MiB).
- Not verified: no stall/restart recovery (S6 — a lost role is never relaunched), no real runners
  (S3), and `stage run`'s polling fallback path is unit-tested only — the scratch core always
  offers `events.subscribe`.

## S1: Ledger and recording commands (2026-11-09)

- Done: `crates/backlot-model` holds the ledger's pure model — serde event types, `apply`/`replay`
  reduction, append-time `validate` (candidate-only casts, numbered deliveries pinned to their
  requirement version, latest-delivery-only reviews, the rework cap and `at_cap`), corrections that
  change the view while the original event stays, and cross-studio `records` with no numeric score
  (D8). `crates/backlot-stage` holds the state directory, the `events.jsonl` append-only store
  serialized by `flock` on `events.lock` with torn-tail truncation, and all studio commands with
  `BACKLOT_*`/`--studio`/`--as`/`--task` caller resolution and 0/1/2 exit codes. `main` dispatches
  studio command words to `backlot_stage::maybe_run` before TUI parsing; help points at them. hc
  core is untouched.
- Rework (2026-11-09): studio names are `[a-z0-9_-]{1,16}` checked before any path is built, and
  only `studio new` may create a studio directory — other commands on a studio with no log are
  refused without touching the disk. `task.finding` records defects on any existing delivery,
  including on closed tasks, without changing status; `later_defects` counts only those and is
  attributed to the delivery's actor, while review-first-seen defects moved to `new_in_rework`.
  `wall_time_ms` follows each actor's cast interval instead of the whole task span. At the cap,
  `review` accepts only `accepted`/`partially-accepted` on the latest delivery (a recast restarts
  the budget; `rework_rounds` keeps the task's total), and `backlot task
  pause|resume|abandon|takeover` record the matching `task.state` changes.
- Measured: 32 model tests cover each validation rule accepted and refused, every "Evaluation rules"
  bullet, replay-equals-apply at every prefix, cap exits and findings attribution across a recast.
  16 stage tests cover torn tails, corrupt lines, state-dir resolution, in-process concurrency, the
  name rule, `finding`, the four task-state commands and an at-cap review end to end; 4 binary tests
  cover the scripted studio loop, 8 processes appending 200 events with unique increasing seqs,
  traversal/uppercase/over-length names exiting 2 with nothing on disk, and unchanged
  `--version`/`--session`/unknown-arg behavior. `status`/`records` output verified on the compiled
  binary. All gates green on every commit.
- Not verified: no real stage loop exists yet — messages and reports are recorded, not delivered.
  Role lifecycle events (`role.*`, `message.delivered`, `attention.*`) are modelled and validated but
  nothing appends them until S2's stage. The hc connection, runner launching and the summary
  document are all still absent by design.

## S0: Identity (2026-11-09)

- Done: the `hc-tui` package builds only the `backlot` binary. Version, help and the parse-error
  hint say `backlot`; release packaging, `scripts/install.sh` (`BACKLOT_*` variables), install docs
  and the README follow the new name, and the README records the hc-tui origin and the planned
  studio layer.
- Measured: `backlot --version` prints `backlot 0.1.0` and `backlot --help` shows `Usage: backlot`.
  Every commit passed `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` and
  `cargo test` (all suites green); `cargo test -p hc-tui --example parity_cells`, the parity Python
  unit tests and `sh scripts/test-install.sh` (20 checks) pass, as does clippy on the CI-pinned
  1.99.0 toolchain. At review, the real-core PTY smoke test passed on the renamed binary in an
  isolated session.
- Not verified: the native parity harness was not run. This milestone only renames the binary and
  user-visible strings, so the harness is first required at S4, where the panel changes the screen.
  No release was tagged, so the renamed install path is exercised only by the offline test script.
