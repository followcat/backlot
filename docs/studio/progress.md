# Studio progress

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
