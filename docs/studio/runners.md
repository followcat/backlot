# Runners

A runner is a coding agent CLI that can fill a role. An adapter holds what differs between them: how to launch one with
a chosen model and thinking depth, how to resume it, and how to read its screen.

Measured on 2026-11-09 with real launches in an isolated hc session, unless a cell says otherwise. Kimi 0.38.0 has not
been measured.

| | Claude Code 2.1.319 | Codex 0.166.0 | Devin 3000.12.32 | OpenCode 2.2.5 |
|---|---|---|---|---|
| Model | `--model` | `-m` | `--model` | No flag on the interactive command. `OPENCODE_CONFIG_CONTENT={"model":"provider/model"}` with `--standalone` |
| Thinking depth | `--effort` (low, medium, high, xhigh, max) | `-c model_reasoning_effort=<level>` | Part of the model ID | Not applied. `#variant` exists only on `opencode run` and has not been measured |
| First prompt, interactive | Positional | Positional | `--prompt-file` | `--prompt` |
| Resume by ID | `-r <id>`, prompt positional, context kept | `codex resume <id> [prompt]`, same flags, context kept | `-r <id>`; ignores `--prompt-file`, so send the prompt after it opens | `-s <id>`; history shown and a new prompt accepted. Whether context is kept is not verified, because no model answered |
| Choose the ID at launch | `--session-id <uuid>` | No. First line of `~/.codex/sessions/**/rollout-*.jsonl` | No. `devin list --format json` in the working directory | `-s ses_<anything>` creates it. The ID must start with `ses` |
| Permissions | `--permission-mode` | `-s`, `-a` | `--permission-mode`, `--respect-workspace-trust` | `--auto` approves all that is not denied; not used |
| Folder trust | Dialog on first use inside a git repository — not seen in a plain folder — cursor on "No, exit": Down, then Enter, and the answer is saved in the user's Claude config. Keys sent as the dialog first paints are lost or can end the runner, so the stage waits 2 s and paces its keys | Dialog unless `-c 'projects={"<cwd>"={trust_level="trusted"}}'` is passed, which saves nothing | `--respect-workspace-trust false` | None seen |
| hc status (OSC 7501) | `done` seen | `idle` seen | Not measured | `idle` seen |

Screen detection exists only on the followcat core and covers each agent partially (`docs/screen-status.md` in that core).
On official hc, an agent that does not report status has none, so the stage's own screen sampling matters there. Running
backlot on official hc is intended but has not been verified.

## Work states

The stage's watch pass reads each live role's visible screen (`terminal.read`, `lines: 0`, at most
once a second per role) and classifies it `working`, `waiting` or `blocked` for the summary. A
prompt is checked before the working marker — a prompt can sit on a screen that still shows the
working line — and anything unmatched is `waiting`, the role answering to nothing. The ledger never
sees it.

| Runner | `blocked` | `working` | Markers from |
|---|---|---|---|
| Claude Code | a line containing `Do you want to` and a line trimmed to `❯ 1.`; or the folder-trust dialog | `esc to interrupt`, or a spinner line: a trimmed line of one glyph from `·✢✳✶✻✽*`, a space, a word ending in `…` | Measured on 2.1.319 — trust dialog, permission prompt, spinner line |
| Codex | a line containing `Would you like to` and a line trimmed to `› 1.` | `esc to interrupt` | An older capture — not measured on 0.166.0 |
| Devin | a line trimmed to `Allow` containing `Once`, plus a line trimmed to `Deny` | `to interrupt` | Live 3000.12.32 screens — `esc twice to interrupt`, the `Allow … Once`/`Deny` prompt |
| OpenCode | `Permission required` and `Allow once` | `esc interrupt` | Permission text from a capture; the working marker is not measured |
| `fake` | the last `fake-work:` line is `blocked` | the last `fake-work:` line is `working` | The script itself |

## Approval keys

A recorded `role.approval` sends one key while the prompt still shows — `allow` takes the highlighted
allow option, `deny` dismisses it.

| Runner | `allow` | `deny` | Markers from |
|---|---|---|---|
| `fake` | `enter` | `esc` | The script itself — byte-verified in `real_stage_closed_loop` |
| Claude Code | `enter` | `esc` | From its `❯ 1.` picker — still to be measured in S5b |
| Codex | `enter` | `esc` | From its `› 1.` picker — still to be measured in S5b |
| Devin | `enter` | `esc` | Observed on 3000.12.32 — `enter` takes the highlighted `Once`, `esc` cancels |
| OpenCode | none sent | none sent | Not measured — the attention stays open for the user |
| Anything else | none sent | none sent | — |

## Rules for adapters

- **Per-session flags only.** Interactive model switchers such as `/model` can change the account default. An adapter
  never sends one.
- **Environment profiles.** An actor may need its own proxy or other settings. Profiles are named in the user's config and
  passed through `terminal.run`'s `env`. Values are never written to labels, the ledger, the summary document or logs.
- **Launch one at a time.** Starting several agents at once has failed for some CLIs.
- **Rate limits are per account.** Some runners share one limit across all sessions, so casting needs account-level limits
  (`max_concurrent` per runner account).

## Pre-allowed tools (Claude Code)

Every Claude Code launch — fresh or resumed — carries one `--allowedTools` pair before `--model`:
`Bash(backlot *)` and `Read(//<studio state dir>/**)` joined into one comma-separated value. The list
is variadic, so another `--flag` must follow the value or the positional prompt is swallowed as a
tool name. The pair is the minimum a role needs: its first step is a `backlot` command or reading its
prompt or a message file, and those live in the studio's state directory, outside its work folder —
without the rules every one prompts and the producer's and director's prompts reach the user. Nothing
else is pre-allowed: no `Bash(*)`, no bypass flag or mode, no `--add-dir`, no `Edit`/`Write`, and the
permission mode is unchanged — any other command still prompts and D9 answers it.

In the `Read` rule `//` is the rule syntax's absolute-path marker; a single leading `/` given on the
command line anchors at the primary working directory per the permissions documentation — it is not
an absolute path — so the studio dir's leading `/` is dropped and the `//` is written instead. The
rule syntax comes from Claude Code's permissions documentation and is still to be measured on
2.1.319 in S5b.

Two edges keep the `Bash` rule narrow. A role cannot change its identity: a `--as`, `--studio` or
`--task` flag that disagrees with the terminal's `BACKLOT_ROLE`, `BACKLOT_STUDIO` or `BACKLOT_TASK`
is refused before the command runs. And a command prefixed with an environment assignment or a
wrapper does not match `Bash(backlot *)` — per the same documentation, to be measured in S5b — so it
still prompts.

## Screen patterns

Observed on real runs. These are the first entries for each adapter's pattern table and need captured fixtures.

| Runner | Screen shows | Meaning | Response |
|---|---|---|---|
| Claude Code | `Quick safety check` with `Yes, I trust this folder`, cursor on `❯ No, exit` | First use of a folder that is a git repository | One key per tick from the cursor's line: Down on `No`, Enter on `Yes`, only when the terminal's cwd is the role's own folder |
| Codex | Workspace trust prompt | First run in a folder | Launch-time `-c 'projects={"<cwd>"={trust_level="trusted"}}'` keeps it away; if seen, answer for the actor's own work area only |
| Devin | `Connection error` | Stopped until it gets a message | One short continue message |
| Devin | `Reached free model rate limit ... reset in N minutes` | Rate limited, status reads `done` | Wait, then a short retry. Do not change model |
| Any | Permission dialog | Waiting on a person | Approval policy |

Fixtures use neutral data only: `host-a`, `alice`, `/home/user`, `acme-shop`.

## First version

Claude Code, Codex, Devin and OpenCode. Kimi and others follow the same adapter shape when needed.
