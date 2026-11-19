# Opt-in smoke harness

`run-smoke.sh` runs one bounded, isolated studio task. It creates a
temporary `acme-shop` repository with a one-file fixture and a unit
test, generates a neutral cast, validates it with `studio new --dry-run`,
then drives a scratch core through requirement, plan, dispatch, task,
cast, delivery, accepted review and `studio finish`.

The default runner is `fake`: no API keys, no network, no real agent.
Real runners are opt-in and need their own credentials.

## Local use

```sh
# Generate everything and validate it; starts no core.
scripts/smoke/run-smoke.sh --dry-run

# See the generated cast (stdout) while validating.
scripts/smoke/run-smoke.sh --dry-run --print-cast

# The offline end-to-end run.
scripts/smoke/run-smoke.sh --runner fake

# A real runner, bounded to 15 minutes.
scripts/smoke/run-smoke.sh \
  --runner claude --model claude-haiku-5-5 --timeout 900 \
  --artifacts /tmp/smoke-artifacts
```

`--root` selects the scratch directory; without it a fresh `mktemp -d`
under `TMPDIR` is used. `--keep-root` keeps it for inspection. On every
exit path (success, failure, timeout) the harness copies what it has to
`--artifacts` and removes the root.

Artifacts:

```
cast.toml         the cast that ran
dry-run.txt       studio new --dry-run output
events.jsonl      the studio ledger
status.txt        backlot status --all
smoke.txt         runner, model, effort, timeout, root
```

## Choices

- **Runner.** `fake` (default), `claude`, `codex`, `devin`, `cursor`,
  `cmd` or `opencode`.
- **Model.** `fake` uses `model-a`; `claude` defaults to
  `claude-haiku-5-5`, `codex` to `gpt-5.3-codex`, `cursor` to
  `composer-2.5`. `devin`, `cmd` and `opencode` require an explicit
  `--model`.
- **Effort.** Defaults to `low`. `cursor`, `devin` and `cmd` carry the
  effort in the model id, so `--effort` is refused for them.
- **Timeout.** 30–7200 seconds, default 900. A hard bound stops the run
  and still cleans up.
- **Limits.** The cast caps the runner at 3 live roles: the producer and
  director, plus either the writer or one actor. The stage refuses a
  lower cap because the fixed pair would leave no seat.

## GitHub Actions

`.github/workflows/nightly-smoke.yml` is deliberately opt-in:

- `workflow_dispatch` always runs, with `runner`, `model` and
  `timeout_seconds` inputs.
- The nightly `schedule` is a no-op unless the repository variable
  `ENABLE_NIGHTLY_SMOKE` is exactly `true`.

Enable it only when you want scheduled runs:

```sh
gh variable set ENABLE_NIGHTLY_SMOKE --body true
gh workflow run nightly-smoke.yml -f runner=fake
```

The job builds the pinned test core and `backlot`, then invokes the
harness with `--backlot target/debug/backlot` and
`--core target/test-core/herdr-core`. Artifacts upload with
`if: always()` and are kept for 7 days.

Real runners read credentials from repository secrets (empty when
unset): `ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, `CURSOR_API_KEY`.

## Tests

```sh
python3 -m unittest discover -s scripts/smoke -p 'test_*.py'
```

The suite checks shell syntax, the generated cast (neutral values,
runner/model/effort/timeout), input refusals, cleanup with no `TMPDIR`
leftovers, and the workflow's triggers, gate, permissions and bounded
timeout. It runs with no backlot on `PATH`, so it needs no core.
