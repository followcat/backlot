# Runners

A runner is a coding agent CLI that can fill a role. An adapter holds what differs between them: how to launch one with
a chosen model and thinking depth, how to resume it, and how to read its screen.

Measured on 2026-11-09 with real launches in an isolated hc session, unless a cell says otherwise; cursor-agent was
measured on 2026-11-11, cmd on 2026-11-13. Kimi 0.38.0 has not been measured.

| | Claude Code 2.1.319 | Codex 0.166.0 | Devin 3000.12.32 | OpenCode 2.2.5 | cursor-agent 2026.11.05 | cmd 1.88.5 |
|---|---|---|---|---|---|---|
| Model | `--model` | `-m` | `--model` | No flag on the interactive command. `OPENCODE_CONFIG_CONTENT={"model":"provider/model"}` with `--standalone` | `--model` | `-m`, session-scoped — the global `model`/`provider` config keys were untouched in the before/after snapshot |
| Thinking depth | `--effort` (low, medium, high, xhigh, max) | `-c model_reasoning_effort=<level>` | Part of the model ID | Not applied. `#variant` exists only on `opencode run` and has not been measured | Part of the model ID (`composer-2.5`, `gpt-5.3-codex-low`) | Not carried — `--effort` is accepted but writes the GLOBAL `reasoningEffort.<model>` config key (measured), so the adapter emits nothing and a cast effort is refused |
| First prompt, interactive | Positional | Positional | `--prompt-file` | `--prompt` | Positional; submits on open, fresh and resumed | Positional on a fresh launch only; a resume ignores it — send the prompt after the composer opens (the devin rule) |
| Resume by ID | `-r <id>`, prompt positional, context kept. On 2.1.321, after the hc core was stopped and started again, the same ID resumed and its transcript file kept growing | `codex resume <id> [prompt]`, same flags, context kept | `-r <id>`; ignores `--prompt-file`, so send the prompt after it opens | `-s <id>`; history shown and a new prompt accepted. Whether context is kept is not verified, because no model answered | `--resume <id>`, prompt positional, context kept (a code word was recalled) | `-r <id>` or `-r <name>` — a `-n <name>` at launch makes a resumable name; `--session <prefix>` and `-c` also work; context kept (a code word was recalled) |
| Choose the ID at launch | `--session-id <uuid>` | No. First line of `~/.codex/sessions/**/rollout-*.jsonl` | No. `devin list --format json` in the working directory | `-s ses_<anything>` creates it. The ID must start with `ses` | `create-chat` prints a uuid and `--resume <uuid>` starts in it; the stage instead finds the newest chat under `<config>/chats/<md5(cwd)>/<uuid>/` after launch — its `meta.json` holds the cwd | No — cmd mints the uuid. The stage finds the newest `<uuid>.jsonl` under `~/.commandcode/projects/<slug(cwd)>/` created at or after launch; line 1 `{"type":"session",...,"id","cwd"}` is authoritative |
| Permissions | `--permission-mode` | `-s`, `-a` | `--permission-mode`, `--respect-workspace-trust` | `--auto` approves all that is not denied; not used | `permissions.allow` `Shell(...)` rules, written per session into the role's own `cli-config.json` (see below) | `permissions.allow`/`deny` `Shell(...)` rules, written per launch into the workdir's `.commandcode/settings.json` (see below); `deny` carries `mcp__<server>` per global server |
| Folder trust | Dialog on first use inside a git repository — not seen in a plain folder — cursor on "No, exit": Down, then Enter, and the answer is saved in the user's Claude config. Keys sent as the dialog first paints are lost or can end the runner, so the stage waits 2 s and paces its keys | Dialog unless `-c 'projects={"<cwd>"={trust_level="trusted"}}'` is passed, which saves nothing | `--respect-workspace-trust false` | None seen | `--trust` skips the `⚠ Workspace Trust Required` dialog; the marker lands in the role's own `CURSOR_DATA_DIR/projects/<slug>/.workspace-trusted` | `--trust` skips `Do you trust the files in this folder?` (it only creates `projects/<slug>/`); an empty cwd auto-trusts. If seen, the stage never answers — it raises instead |
| hc status (OSC 7501) | `done` seen | `idle` seen | Not measured | `idle` seen | None — no `]7501;` bytes in a full-session typescript, `status: null` throughout | None — no `]7501;` bytes in a typescript capture; hc's screen source still classifies idle/working/blocked(permission) fine |

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
| Devin | an `Allow` picker line — an optional `❭` cursor, `← ◼◼◼◼ →` and any scope word after the arrows — plus a `Deny` option line, cursor optional on both | `to interrupt` | Live 3000.12.32 screens — `esc twice to interrupt`, the `❭ Allow ← ◼◼◼◼ → Once`/`Deny` prompt with the cursor on `Allow` |
| OpenCode | `Permission required` and `Allow once` | `esc interrupt` | Permission text from a capture; the working marker is not measured |
| cursor-agent | the workspace-trust dialog — `Workspace Trust Required` and `Trust this workspace` together; or a permission prompt — `Run this command?`, a line starting `Not in allowlist:` and `→ Run (once) (y)` | `ctrl+c to stop`, or a spinner line — one or two braille glyphs, a space, then `Composing` or `Running … tokens`. A transcript line starting `Running the` is not one | Measured on 2026.11.05 |
| cmd | the folder-trust dialog — `Do you trust the files in this folder?` and `Yes, proceed` together; or a permission prompt — `needs to execute`, `needs to run` or `make this edit to` plus a line starting `❯ 1.` | `esc to interrupt` on the status line — the rotating `*ing…` verb and the `✻ Worked for`/`✻ Thought for`/`⎿ Tip:` post-turn lines are not a turn | Measured on 1.88.5 |
| `fake` | the last `fake-work:` line is `blocked` | the last `fake-work:` line is `working` | The script itself |

## Prompt blocks

Before a recorded approval's key goes out, the screen on show is compared with the screen the
attention saved — through a *prompt block*, the whitespace-normalised region that names the prompt.
A mismatch sends nothing; so does a screen with no block on either side, for a runner the stage can
answer. Lines that move while the prompt waits are dropped first.

| Runner | Block | Status |
|---|---|---|
| Claude Code | From the last full-width `─` rule above `Do you want to proceed?` to the end of the screen — the ` Bash command` header, the description, the command between `╌` rules and the picker; spinner lines dropped | Built from the measured 2.1.319 layout; the comparison itself ran on `fake` only |
| `fake` | From the last `fake-work: blocked` line to the end — a `prompt:<text>` input echoes `fake-prompt: <text>` under it so two prompts differ | The script itself |
| Codex | From the last `Would you like to` line to the end | Unmeasured — best effort from its `blocked` markers |
| Devin | From the last `●` line above `Approval required` to the end — the command block, the judge's sentence and the picker with its `❭` cursor and scope word | Built from a live 3000.12.32 prompt read; Devin has not yet run as a studio role — only Claude has |
| OpenCode | none | No approval key is ever sent, so no block is mapped |
| cursor-agent | From the last full-width `─` rule above `Run this command?` to the end — the `$ <command>` line, the allowlist sentence and the picker | Measured on 2026.11.05 |
| cmd | From the last full-width `─` rule above the `❯ 1.` picker to the end — the tool title, the `needs to …` sentence, the command and the picker | Measured on 1.88.5 |
| Anything else | none | — |

## Approval keys

A recorded `role.approval` sends one key while the saved prompt block is still the one on show —
`allow` takes the highlighted allow option, `deny` dismisses it.

| Runner | `allow` | `deny` | Markers from |
|---|---|---|---|
| `fake` | `enter` | `esc` | The script itself — byte-verified in `real_stage_closed_loop` |
| Claude Code | `enter` | `esc` | Both measured on 2.1.319 — `enter` takes the allow option; `esc` does not run the command, shows `Interrupted · What should Claude do instead?` and waits for input |
| Codex | `enter` | `esc` | From its `› 1.` picker — still to be measured |
| Devin | `enter` | `esc` | Observed on 3000.12.32 — `enter` takes the highlighted `Once`, `esc` cancels |
| OpenCode | none sent | none sent | Not measured — the attention stays open for the user |
| cursor-agent | `y` | none sent | `y` takes `Run (once)` — measured on 2026.11.05. A denied command is retried by the runner (it re-asked four times, then read the file itself), so no deny key is mapped and the attention stays open for the user |
| cmd | `enter` | `esc` | Both measured on 1.88.5 — `enter` takes `❯ 1. Yes` (run-once); `esc` or `3` ends the turn and the agent does not retry. Option `2` ("don't ask again") is never sent — it writes a project allow rule |
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

Every Claude Code launch — fresh or resumed — carries one `--allowedTools` pair before `--model`: one
comma-separated value, so another `--flag` must follow it or the positional prompt is swallowed as a
tool name. The base pair is `Bash(backlot *)` and `Read(//<studio state dir>/**)` — the minimum a role
needs: its first step is a `backlot` command or reading its prompt or a message file, and those live
in the studio's state directory, outside its work folder — without the rules every one prompts and the
producer's and director's prompts reach the user.

Per role, on top of the base pair, and only in the argv — nothing is written to any settings file:

- An **actor** gets the git commands its loop drives — `git add`, `git commit`, `git status`,
  `git diff`, `git log`, `git show`, each bare (`Bash(git add)`) and with arguments
  (`Bash(git add *)`) — plus every declared check's `run` verbatim as `Bash(<run>)`.
- The **director** gets the read-only set it reviews with — `git status`, `git diff`, `git log`,
  `git show`, bare and starred — plus `git merge` and the declared checks.
- The **producer** and **writer** keep the base pair; anything else still prompts and D9 answers it.

Nothing broader is pre-allowed: no `Bash(*)`, no bypass flag or mode, no `--add-dir`, no
`Edit`/`Write`, and the permission mode is unchanged. A `run` the rule syntax cannot hold — a
parenthesis or a comma — is never pre-allowed, though `studio new` accepts the cast; a `run`
reaching for `BACKLOT_*`, `env -i` or `--ignore-environment` is refused there outright. Whether
Claude Code actually matches these rules — and whether a wrapped `X=y git commit …` slips past them —
is not yet verified; `limits.md` keeps it for the S9f real-session run.

In the `Read` rule `//` is the rule syntax's absolute-path marker; a single leading `/` given on the
command line anchors at the primary working directory per the permissions documentation — it is not
an absolute path — so the studio dir's leading `/` is dropped and the `//` is written instead. The
base pair is measured on 2.1.319: the first real run's plain `backlot` calls and studio-dir reads ran
without prompting.

Two edges keep the `Bash` rule narrow. A role cannot change its identity: a `--as`, `--studio` or
`--task` flag that disagrees with the terminal's `BACKLOT_ROLE`, `BACKLOT_STUDIO` or `BACKLOT_TASK`
is refused before the command runs. And a chained, env-prefixed or wrapped command does not match
`Bash(backlot *)` — measured in the same run, every one prompted — so the role prompts demand one
plain command per shell call.

## Per-session files (cursor-agent)

cursor-agent has no allowed-tools flag, so the stage writes what each launch needs into the role's
own files instead — before every start and resume, and a failure there is attention, never a
half-configured role.

Every cursor role runs isolated: `CURSOR_CONFIG_DIR`, `CURSOR_DATA_DIR` and
`CURSOR_SANDBOX_POLICY_DIR` point under `roles/<role>/cursor/` in the studio state directory
(directories mode 700, stage-written files 600). The isolation is not total — the global
`~/.cursor/mcp.json` is still read from a hard-coded path (below), and a daily empty
`.agent-data-cleanup-<date>` marker still lands in the global `~/.cursor/projects/` — and one
network constant was measured: every run keeps a connection to `api3.cursor.sh:443` outside the
configured proxy, which `NODE_USE_ENV_PROXY=1` does not redirect, while the model and API traffic
go through the proxy. The login does not move — `auth.json` reads `XDG_CONFIG_HOME`, not
`CURSOR_CONFIG_DIR` — which is why a stage environment carrying a nonempty `XDG_CONFIG_HOME` is a
plan refusal rather than something the adapter overrides: `XDG_CONFIG_HOME` itself is never set.

The agent's own writes take the process umask — measured on the controlled run, where
`cli-config.json` landed group-readable and directories at 775 — so `roles/<role>/cursor/`
itself is the privacy boundary: every prepare forces it, `config/`, `data/` and `sandbox/` back
to 700, session discovery re-checks the root, and a dir found open raises one informational
`cursor dir was open` attention naming the path suffix. The bundle's own mkdir modes match:
only its sandbox-policies, agent-store and socket paths pin a mode; everything else inherits.
`CURSOR_SANDBOX_POLICY_DIR` relocation is unexercised, not broken — the bundle honors the
variable (`process.env.CURSOR_SANDBOX_POLICY_DIR?.trim()`, else `~/.cursor/sandbox-policies`),
but the run's director wrote no policy; the global policy churn seen then was the user's own
running Cursor IDE.

Two files are written on each launch. `config/cli-config.json` gets the role's `permissions.allow`
— `Shell(...)` rules in the narrowest form the bundle's matcher takes: a rule without `*` or `:`
matches only the command equal to it or starting with it plus a space, the command also tried as
its executable's basename — so `Shell(backlot)` covers `backlot status`, `Shell(git merge)`
reaches `git merge …` alone, and there is no starred variant. An existing file keeps every other
key byte-for-byte — `authInfo` above all — with only the `permissions` value spliced. The base
rule is `Shell(backlot)`; an actor adds `git add`, `git commit`, `git status`, `git diff`,
`git log`, `git show`, the director the read-only set it reviews with — `git status`,
`git diff`, `git log`, `git show` — plus `git merge`, and both add every declared check's `run`
verbatim. `git branch` stays out: the matcher's prefix form would also allow `git branch -D`.
Producer and writer keep the base only. Nothing broader is written — no `Shell(*)`, no
`--force`, `--yolo` or `--approve-mcps` — and a `run` holding `()` `,` `*?[]{}:` or `\` is not
expressible (`,` splits rules, `:` splits a command from its argument pattern, the rest are glob
or syntax characters), so it is simply never pre-allowed.

`data/projects/<slug>/mcp-disabled.json` holds the names of the user's global MCP servers —
`~/.cursor/mcp.json` is read from a hard-coded path regardless of `CURSOR_CONFIG_DIR` and holds
paid tools a role must never see; only the server names are read, never the values. Measured on
the controlled run: `cursor-agent mcp list` under the role's env listed the user's server as
`disabled`. The slug is cursor's own: non-alphanumeric runs collapse to one `-`, ends trimmed.
`--trust` at launch writes the workspace-trust marker into the same isolated `projects/<slug>/`,
so it stays per role.

cursor-agent assigns the chat id itself, so a session is found after launch: the newest uuid
directory under `config/chats/<md5(absolute cwd)>/` whose `meta.json` names the role's cwd and was
created at or after the launch. `--resume <id>` then restarts it with context — measured
resuming the same uuid with its full transcript after a kill. The `runner_version` the ledger
records on `role.started`/`role.resumed` comes from the launch program itself —
`cursor-agent --version`, never `cursor --version`, which is the IDE launcher — cached per
program. Effort lives in the model id (`gpt-5.3-codex-low`), so `--model` carries both, a cast
with `effort` is refused at `studio new`, and none is recorded. Never used: `--print`/`-p` (a
different run mode the stage cannot watch), `--output-format` (print mode only) and
`--workspace` (the terminal's cwd already picks the workspace).

## Per-session files (cmd)

cmd has no per-role config or auth isolation — `~/.commandcode` hangs off `HOME` and roles share
the user's account — so the boundary is the workdir's own `.commandcode/settings.json`, written
before every start and resume (cmd reads the project file once at launch), plus the project slug
directory under `~/.commandcode/projects/` that `--trust` creates.

The file carries `{"permissions":{"allow":..,"deny":..}}`; an existing file keeps every other
byte — only the `permissions` value is spliced (the same byte-exact splice as cursor's
`cli-config.json`). `allow` holds `Shell(...)` rules in the bundle's prefix form — the base
`Shell(backlot)` covers `backlot status`; an actor adds `Shell(git add:*)`, `git commit`,
`git status`, `git diff`, `git log`, `git show`; the director the read-only set plus
`git merge`; and both add every declared check's `run` verbatim as `Shell(<run>:*)`. `git branch`
stays out — the prefix form would also allow `git branch -D`. Every `&&`/`||`/`;`/pipe subcommand
must match an allow rule and an env-assignment prefix breaks the match (measured), so a declared
check stays a single plain command — the same constraint the cursor rules document. A `run`
holding `()` `\` `*?[]{}` `:` or `,` is not expressible — the first groups are glob or syntax,
`:` is the rule's own command/argument split and `,` is filtered like cursor — so it is simply
never pre-allowed, though the cast accepted it.

`deny` carries `mcp__<server>` once per server the global `~/.commandcode/mcp.json` names — names
only are read, never the values — because a project `mcp.json` is additive and cannot disable a
user server (measured). Nothing broader is written — no `Shell(*)`, no `--yolo`,
`--dangerously-skip-permissions`, `--permission-mode`, `--tools-*`, `--config`, `-p`,
`--output-format`, `--no-session`, `-w` or `--cloud` — and `--effort` is never passed either: it
rewrites the global `reasoningEffort.<model>` key, so a `cmd` cast with `effort` is refused at
`studio new` and the adapter emits nothing.

cmd mints the session uuid itself, so a session is found after launch: the newest
`<uuid>.jsonl` under `~/.commandcode/projects/<slug(cwd)>/` created at or after the launch —
line 1 `{"type":"session","version":3,"id","cwd",...}` is authoritative; `.checkpoints.jsonl`
companions do not count. The slug is cmd's own: lowercase, camel boundaries split
(`AControl` → `a-control`), non-alphanumeric runs collapse to `-`, empty → `root`. A fresh
launch also passes `-n <terminal>` (`bl-<studio>-<role>`), so the session keeps a resumable name
alongside the recorded uuid. `-r <id>` resumes by either; the positional prompt is ignored on
resume, so the prompt goes in after the composer opens — the same rule the devin adapter follows.
`runner_version` comes from `cmd --version`. `NODE_USE_ENV_PROXY=1` always rides in the launch
env — measured: without it the model traffic bypasses the company proxy (two persistent direct
connections stay regardless). Quirks recorded, not handled: `--no-auto-update` does not stop the
background self-update; read-only commands (`git status|log|diff`) never prompt; a leading
`sleep N` is refused by cmd's own guard above the permission layer — a `Blocked:` line is a
refusal, not a permission prompt; and non-TTY invocation dies cleanly
(`Interactive mode requires a TTY`).

## Screen patterns

Observed on real runs. These are the first entries for each adapter's pattern table and need captured fixtures.

| Runner | Screen shows | Meaning | Response |
|---|---|---|---|
| Claude Code | `Quick safety check` with `Yes, I trust this folder`, cursor on `❯ No, exit` | First use of a folder that is a git repository | One key per tick from the cursor's line: Down on `No`, Enter on `Yes`, only when the terminal's cwd is the role's own folder |
| Codex | Workspace trust prompt | First run in a folder | Launch-time `-c 'projects={"<cwd>"={trust_level="trusted"}}'` keeps it away; if seen, answer for the actor's own work area only |
| Devin | `Connection error` | Stopped until it gets a message | One short continue message |
| Devin | `Reached free model rate limit ... reset in N minutes` | Rate limited, status reads `done` | Wait, then a short retry. Do not change model |
| cursor-agent | `Workspace Trust Required` with `Trust this workspace` | First launch in an untrusted folder | `--trust` is passed at launch, so the dialog should never appear; if it does it is the user's — the stage never answers it, even for an actor |
| cmd | `Do you trust the files in this folder?` with `❯ 1. Yes, proceed` | First launch in a non-empty untrusted folder | `--trust` is passed at launch, so the dialog should never appear; if it does it is the user's — the stage never answers it, even for an actor |
| Any | Permission dialog | Waiting on a person | Approval policy |

Fixtures use neutral data only: `host-a`, `alice`, `/home/user`, `acme-shop`.

## First version

Claude Code, Codex, Devin, OpenCode, cursor-agent and cmd. Kimi and others follow the same
adapter shape when needed.
