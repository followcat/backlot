# Studio progress

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
- Measured: 28 model tests cover each validation rule accepted and refused, every "Evaluation rules"
  bullet, and replay-equals-apply at every prefix. 12 stage tests cover torn tails, corrupt lines,
  state-dir resolution and in-process concurrency; 3 binary tests cover the scripted studio loop
  (new → req → plan → task → cast → deliver → changes → deliver → accepted), 8 processes appending
  200 events with unique increasing seqs, and unchanged `--version`/`--session`/unknown-arg behavior.
  `status`/`records` output verified on the compiled binary. All gates green on every commit.
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
