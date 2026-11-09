# Runners

A runner is a coding agent CLI that can fill a role. An adapter holds what differs between them: how to launch one with
a chosen model and thinking depth, how to resume it, and how to read its screen.

The table comes from each CLI's `--help` on 2026-11-09. **None of it has been exercised by a launch yet.** Milestone S3
measures each row and records the result here.

| | Claude Code 2.1.319 | Codex 0.166.0 | Devin 3000.12.28 | OpenCode 2.2.5 | Kimi 0.38.0 |
|---|---|---|---|---|---|
| Model | `--model` | `-m` | `--model` | `-m provider/model#variant` | `-m` (alias) |
| Thinking depth | `--effort` | Config key `model_reasoning_effort`, likely through `-c`; to measure | Part of the model ID | `#variant` in the model | None in `--help` |
| First prompt, interactive | Positional | Positional | `--prompt-file` | `--prompt` | Send after start; `-p` is non-interactive |
| Resume by ID | `-r <id>` | `codex resume <id>` | `-r <id>` | `-s <id>` | `-S <id>` |
| Choose the ID at launch | `--session-id <uuid>` | No; find it after launch | No; find it after launch | `-s` creates if missing | To measure |
| Permissions | `--permission-mode` | `-s`, `-a` | `--permission-mode`, `--respect-workspace-trust` | To measure | `-y` |
| hc status | OSC 7501 if configured; fork screen detection | Same | Same | Same | OSC 7501 only |

Screen detection exists only on the followcat core and covers each agent partially (`docs/screen-status.md` in that core).
On official hc, an agent that does not report status has none, so the stage's own screen sampling matters there. Running
backlot on official hc is intended but has not been verified.

## Rules for adapters

- **Per-session flags only.** Interactive model switchers such as `/model` can change the account default. An adapter
  never sends one.
- **Environment profiles.** An actor may need its own proxy or other settings. Profiles are named in the user's config and
  passed through `terminal.run`'s `env`. Values are never written to labels, the ledger, the summary document or logs.
- **Launch one at a time.** Starting several agents at once has failed for some CLIs.
- **Rate limits are per account.** Some runners share one limit across all sessions, so casting needs account-level limits
  (`max_concurrent` per runner account).

## Screen patterns

Observed on real runs. These are the first entries for each adapter's pattern table and need captured fixtures.

| Runner | Screen shows | Meaning | Response |
|---|---|---|---|
| Devin | `Connection error` | Stopped until it gets a message | One short continue message |
| Devin | `Reached free model rate limit ... reset in N minutes` | Rate limited, status reads `done` | Wait, then a short retry. Do not change model |
| Any | Permission dialog | Waiting on a person | Approval policy |
| Codex | Workspace trust prompt | First run in a folder | Answer for the actor's own work area only |

Fixtures use neutral data only: `host-a`, `alice`, `/home/user`, `acme-shop`.

## First version

Claude Code, Codex, Devin and OpenCode. Kimi and others follow the same adapter shape when needed.
