# Testing backlot

Five tiers. The unit suite runs everywhere; the others are opt-in
because they need a real core, a reference build or a paid agent.

## 1. Unit and workspace suite

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

The leak-checked form points `TMPDIR` at a fresh directory and fails if
any test leaves a file behind (lock files excepted):

```sh
sh scripts/check-test-tmp.sh
```

Every test uses neutral fixtures only: `host-a`/`host-b`, `alice`,
`/home/user`, `acme-shop`.

## 2. Real-core integration tests

These are `#[ignore]` tests against a pinned `herdr-core`. Build it once
with `tools/build-test-core.sh`, then set `HC_TUI_TEST_CORE`:

```sh
tools/build-test-core.sh
export HC_TUI_TEST_CORE=$PWD/target/test-core/herdr-core

cargo test -p hc-tui --test real_core_smoke -- --ignored --test-threads=1
cargo test -p hc-tui --test real_stage      -- --ignored --test-threads=1
cargo test -p hc-tui --test real_hollywood  -- --ignored --test-threads=1
cargo test -p hc-tui --test real_two_studios -- --ignored --test-threads=1
```

Each test owns a scratch core: a `hctui-test-*` session with an explicit
socket, temporary `HOME`/`XDG_*` roots and a pre-seeded `host-a`
identity. They never touch an existing session. `real_stage` drives the
shipped `fake` runner; no real agent starts.

`real_agent_smoke_e2e` is a further opt-in tier: it launches real agent
CLIs and requires `HC_TUI_REAL_AGENTS=1` in addition to
`HC_TUI_TEST_CORE`. Do not run it in ordinary CI.

[docs/integration.md](../integration.md) has the pins and the
scratch-isolation details.

## 3. Herdr parity harness

The reference-comparison suite lives in `scripts/parity/`. It needs a
build of the pinned herdr revision, a private manifest with its SHA-256,
and a private config copy. Captures stay outside the repository.

```sh
python3 -m unittest discover -s scripts/parity -p 'test_*.py'
python3 scripts/parity/run.py --hc ... --hc-tui ... --out ...
```

[scripts/parity/README.md](../../scripts/parity/README.md) documents the
manifest, the scratch sessions and the exact cell/input comparisons.

## 4. Two-machine visual rig

`scripts/rig/` reproduces the usual manual setup: a local core, optional
`host-b`/`host-c` cores added with `hc machine add --via`, neutral
`acme-shop` and `beta-tools` repositories with fake-runner studios, and
optionally Xvfb + kitty running `backlot`.

```sh
scripts/rig/rig.sh plan --display :90
scripts/rig/rig.sh up --display :90
scripts/rig/rig.sh keys ctrl+b c
scripts/rig/rig.sh shot /tmp/backlot.png --crop 1600x800+0+0
scripts/rig/rig.sh down
```

Every process is recorded in `$ROOT/pids`; `down` stops those PIDs only
and removes the root. `--dry-run`/`plan` prints the exact isolated
paths and commands and spawns nothing, so it works on a headless CI
runner. `xclick.py` sends explicit-display clicks and drags and never
assumes `:0`.

```sh
python3 -m unittest discover -s scripts/rig -p 'test_*.py'
```

See [scripts/rig/README.md](../../scripts/rig/README.md).

## 5. Opt-in real-agent smoke

`scripts/smoke/run-smoke.sh` runs one bounded studio task on a scratch
core and temporary repository, then always copies the ledger and status
and removes the root.

```sh
# Offline: generated cast, studio new --dry-run, no core.
scripts/smoke/run-smoke.sh --dry-run

# Offline end to end on the fake runner.
scripts/smoke/run-smoke.sh --runner fake

# A real runner is opt-in and needs the caller's credentials.
scripts/smoke/run-smoke.sh --runner claude --model claude-haiku-5-5 --timeout 900
```

`.github/workflows/nightly-smoke.yml` runs it only on manual
`workflow_dispatch`, or on the nightly schedule when the repository
variable `ENABLE_NIGHTLY_SMOKE` is exactly `true`. Nothing runs on push
or pull requests. The workflow is bounded to 25 minutes and uploads
artifacts for 7 days with `if: always()`.

```sh
python3 -m unittest discover -s scripts/smoke -p 'test_*.py'
```

See [scripts/smoke/README.md](../../scripts/smoke/README.md).

## What stays opt-in

| Tier | Requires |
|---|---|
| Unit suite | nothing |
| Real-core integration | a pinned `herdr-core` |
| Parity | a pinned herdr build and a private manifest |
| Visual rig | `Xvfb`/`kitty` for the GUI half; a built `backlot` |
| Real-agent smoke | `workflow_dispatch` or `ENABLE_NIGHTLY_SMOKE=true`, plus the runner's credentials |

No tier connects to a named `hc` session it did not create, and none
uses real host names, home paths, tokens or email addresses in fixtures.
