# Studio progress

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
