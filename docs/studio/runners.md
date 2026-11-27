# Runners

A runner is a coding agent CLI that can fill a role. An adapter holds what differs between them: how to launch one with
a chosen model and thinking depth, how to resume it, and how to read its screen.

Measured on 2026-11-09 with real launches in an isolated hc session, unless a cell says otherwise; cursor-agent was
measured on 2026-11-11, cmd on 2026-11-13 (hooks measured on 1.93.0 on 2026-11-24), Devin hooks on 3000.14.30821 on 2026-11-24, cline on 2026-11-17, and agy 1.5.16 on 2026-11-24. Kimi 0.38.0 has not been measured.

| | Claude Code 2.1.319 | Codex 0.166.0 | Devin 3000.12.32 | OpenCode 2.2.5 | cursor-agent 2026.11.05 | cmd 1.88.5 | cline 3.0.74 | agy 1.5.16 |
|---|---|---|---|---|---|---|---|---|
| Model | `--model` | `-m` | `--model` | No flag on the interactive command. `OPENCODE_CONFIG_CONTENT={"model":"provider/model"}` with `--standalone` | `--model` | `-m`, session-scoped — the global `model`/`provider` config keys were untouched in the before/after snapshot | `-m` beside `-P cline-pass` — the provider is taken from a `cline-pass/<model>` id, and any other model is a plan refusal (`model not supported`), because no config channel carries a provider | `--model` |
| Thinking depth | `--effort` (low, medium, high, xhigh, max) | `-c model_reasoning_effort=<level>` — low, medium, high, xhigh, max, ultra (the union across the model list; a per-model refusal is codex's own) | Part of the model ID | Not applied. `#variant` exists only on `opencode run` and has not been measured | Part of the model ID (`composer-2.5`, `gpt-5.3-codex-low`) | Not carried — `--effort` is accepted but writes the GLOBAL `reasoningEffort.<model>` config key (measured), so the adapter emits nothing and a cast effort is refused | `--thinking none`, `low`, `medium`, `high` or `xhigh`; there is no `max` | `--effort` (low, medium, high, xhigh, max) |
| First prompt, interactive | Positional | Positional | `--prompt-file` | `--prompt` | Positional; submits on open, fresh and resumed | Positional on a fresh launch only; a resume ignores it — send the prompt after the composer opens (the devin rule) | Positional on a fresh launch only; a resume ignores it — send the prompt after the composer opens (the devin rule) | `-i "<prompt>"` on a fresh launch only; a resume ignores it — send the prompt after the composer opens (the devin rule) |
| Resume by ID | `-r <id>`, prompt positional, context kept. On 2.1.321, after the hc core was stopped and started again, the same ID resumed and its transcript file kept growing | `codex resume <id> [prompt]`, same flags, context kept | `-r <id>`; ignores `--prompt-file`, so send the prompt after it opens | `-s <id>`; history shown and a new prompt accepted. Whether context is kept is not verified, because no model answered | `--resume <id>`, prompt positional, context kept (a code word was recalled) | `-r <id>` or `-r <name>` — a `-n <name>` at launch makes a resumable name; `--session <prefix>` and `-c` also work; context kept (a code word was recalled) | `--id <id>`, context kept (a recall question was answered correctly); the positional prompt is ignored | `--conversation <id>`; `-i` is not passed on a resume, so the prompt goes in after the composer opens; `--settings` `trustedWorkspaces` keeps the folder-trust dialog away when `[stage] answer_trust` is on, and shows it first when `answer_trust = false` on an untrusted cwd |
| Choose the ID at launch | `--session-id <uuid>` | No. First line of `~/.codex/sessions/**/rollout-*.jsonl` | No. `devin list --format json` in the working directory | `-s ses_<anything>` creates it. The ID must start with `ses` | `create-chat` prints a uuid and `--resume <uuid>` starts in it; the stage instead finds the newest chat under `<config>/chats/<md5(cwd)>/<uuid>/` after launch — its `meta.json` holds the cwd | No — cmd mints the uuid. The stage finds the newest `<uuid>.jsonl` under `~/.commandcode/projects/<slug(cwd)>/` created at or after launch; line 1 `{"type":"session",...,"id","cwd"}` is authoritative | No — cline mints it. The stage finds the manifest `<id>/<id>.json` in the user's session store, created at or after launch, whose `cwd` is the role's and whose `pid` is the engine child of the terminal's `node` wrapper | No — agy mints it. `cache/last_conversations.json` maps the cwd to its id; else the newest `conversations/<uuid>.db` whose bytes carry the workspace URI |
| Permissions | `--permission-mode` | `-s`, `-a` | `--permission-mode`, `--respect-workspace-trust` | `--auto` approves all that is not denied; not used | `permissions.allow` `Shell(...)` rules, written per session into the role's own `cli-config.json` (see below) | `permissions.allow`/`deny` `Shell(...)` rules, written per launch into the workdir's `.commandcode/settings.json` (see below); `deny` carries `mcp__<server>` per global server | a stage-written work-area hook, `<cwd>/.cline/hooks/PreToolUse.sh`: the role's command rules, default-deny, the network tools and MCP names always cancelled (see below) | `--settings <dir>/.agy-settings.json` with `toolPermission: always-proceed` (session-only; other valid values on 1.5.16: `request-review`, `strict`, `proceed-in-sandbox`); `permissions.allow`/`deny` use `command(<prefix>)` without `*` |
| Folder trust | Dialog on first use inside a git repository — not seen in a plain folder — cursor on "No, exit": Down, then Enter, and the answer is saved in the user's Claude config. Keys sent as the dialog first paints are lost or can end the runner, so the stage waits 2 s and paces its keys | Dialog unless `-c 'projects={"<cwd>"={trust_level="trusted"}}'` is passed, which saves nothing | `--respect-workspace-trust false` | None seen | `--trust` skips the `⚠ Workspace Trust Required` dialog; the marker lands in the role's own `CURSOR_DATA_DIR/projects/<slug>/.workspace-trusted` | `--trust` skips `Do you trust the files in this folder?` (it only creates `projects/<slug>/`); an empty cwd auto-trusts. If seen, the stage never answers — it raises instead | None seen — a role's work area sits inside an existing repository | launch handles: `--settings` `trustedWorkspaces` = the work area (exact path; the global settings are untouched; measured on 1.5.18 on 2026-11-27) |
| hc status (OSC 7501) | `done` seen | `idle` seen | Not measured | `idle` seen | None — no `]7501;` bytes in a full-session typescript, `status: null` throughout | None — no `]7501;` bytes in a typescript capture; hc's screen source still classifies idle/working/blocked(permission) fine | None — `hc get` showed `agent: null`; the stage's screen markers carry the role (S14a) | Native OSC 7501 (`app: "antigravity"`): `idle` on composer, `working` during turns and backgrounded tasks, `done` (with `msg`) after a turn, `blocked` with `kind: "permission"` or `"question"` (`waiting_for_person: false`) |

## Capabilities

One row per runner, generated from `RunnerCaps` in `backlot-model` — the stage reads the same facts,
and the docs test fails when this drifts.

<!-- BEGIN GENERATED runner-caps -->
| Runner | Effort | Model | First prompt | Resume | Session discovery | Folder trust | Prompt detection | Allow | Deny | Status source | Hooks |
|---|---|---|---|---|---|---|---|---|---|---|---|
| Claude Code | required (low, medium, high, xhigh, max) | `--model` | positional | --session-id, -r (keeps prompt) | argv | stage answers | screen block, hook, structured status | `enter` | `esc` | fork inference | UserPromptSubmit, PreToolUse, PostToolUse, PostToolUseFailure, PermissionRequest, Notification, Stop, StopFailure |
| Codex | required (low, medium, high, xhigh, max, ultra) | `-m` | positional | `resume <id>` (keeps prompt) | files | launch handles | screen block, structured status | `enter` | `esc` | fork inference | SessionStart, PreToolUse, PostToolUse, Stop |
| Devin | optional (ignored) | `--model` | `--prompt-file` (file) | `-r` (prompt after open; id not read from argv) | `devin list --format json` | launch handles | screen block, hook, structured status | `enter` | `esc` | native 7501 | SessionStart, UserPromptSubmit, PreToolUse, PostToolUse, PermissionRequest, Stop, SessionEnd |
| OpenCode | optional (ignored) | `OPENCODE_CONFIG_CONTENT` | `--prompt` | -s (keeps prompt) | argv | not applicable | structured status | none sent | none sent | fork inference | SessionStart, PreToolUse, PostToolUse, Stop |
| cursor-agent | none | `--model` | positional | `--resume` (keeps prompt; id not read from argv) | files | launch handles | screen block, structured status | `y` | none sent | fork inference | SessionStart, PreToolUse, PostToolUse, Stop |
| cmd | none | `-m` | positional | -r, --resume, --session (prompt after open) | files | launch handles | screen block, structured status | `enter` | `esc` | fork inference | SessionStart, PreToolUse, PostToolUse, Stop |
| cline | required (none, low, medium, high, xhigh) | `-P cline-pass -m` | positional | --id (prompt after open) | files | not applicable | screen block | none sent | none sent | screen only | PreToolUse |
| agy | required (low, medium, high, xhigh, max) | `--model` | `-i` | --conversation (prompt after open) | cwd map, then sqlite | launch handles | screen block, structured status | none sent | none sent | native 7501 | SessionStart, PreInvocation, PreToolUse, PostToolUse, PostInvocation, Stop |
| fake | required (low, medium, high) | `FAKE_MODEL` | not carried | none | terminal name | stage answers | screen block, hook | `enter` | `esc` | screen only | UserPromptSubmit, PreToolUse, PostToolUse, PostToolUseFailure, PermissionRequest, Notification, Stop, StopFailure |
<!-- END GENERATED runner-caps -->

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
| cursor-agent | the workspace-trust dialog — `Workspace Trust Required` and `Trust this workspace` together; or any approval picker below the last full-width `─` divider — a `→ ` option-cursor line, a `Run Everything (shift+tab)` footer, and `allowlist`/`approval` wording or a tool question (`… this file?`, `… this command?`, `Proceed with this edit?`). The question's own words are not the marker, so a picker kind not yet named still blocks | `ctrl+c to stop`, or a spinner line — one or two braille glyphs, a space, then `Composing` or `Running … tokens`. A transcript line starting `Running the` is not one | Shell picker measured on 2026.11.05; the generic picker shape — `Write to this file?` and the rest — from the S12e run and the fork's `cursor.toml` |
| cmd | the folder-trust dialog — `Do you trust the files in this folder?` and `Yes, proceed` together; or a permission prompt — `needs to execute`, `needs to run` or `make this edit to` plus a line starting `❯ 1.` | `esc to interrupt` on the status line — the rotating `*ing…` verb and the `✻ Worked for`/`✻ Thought for`/`⎿ Tip:` post-turn lines are not a turn | Measured on 1.88.5 |
| cline | `Cline needs permission` and `Approve tool call?` together — it must not appear under `--yolo`, which forces auto-approve on, so a screen showing it means the user switched the mode off | a braille spinner line from `⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏` and a word after it (`⠏ Thinking... (esc to cancel)`), or `(esc to cancel)` alone. The composer stays on screen while it works, so its presence is not a turn by itself; a `▶ Thinking:` transcript line is a finished thought | Measured on 3.0.74 — idle, working and approval captures (S14a) |
| agy | the folder-trust dialog (`Accessing workspace:` and `Do you trust the contents of this project?` with `> Yes, I trust this folder` and `↑/↓ Navigate · enter Confirm`); or the command approval picker (`Requesting permission for:` and `Run this command?` with `> 1. Yes, run command` and `↑/↓ Navigate · tab Amend`); or a plan-mode question (`Question 1/1:` with `> 1.` and `↑/↓ Navigate · enter Select · esc Skip`). `esc to cancel` stays at the bottom on pickers, so `blocked` beats `working` | `esc to cancel` on the bottom row, a braille spinner line (`\u{2801}..\u{28ff}` + two spaces + verb, e.g. `Generating...`, `Reading file...`, `Running command... Ns`, `Editing files...`), or a running background-task row (`● [HH:MM:SS] <cmd> running` with `1 task(s) · /tasks`, where `? for shortcuts` replaces `esc to cancel`). `▸ Thought for Ns, N tokens` and `? for shortcuts` alone are not working | Measured on 1.5.16 — trust dialog, normal turn, backgrounded long commands, plan-mode question, `request-review` and `strict` command pickers |
| `fake` | the last `fake-work:` line is `blocked` | the last `fake-work:` line is `working` | The script itself |

## Signal matrix

The screen is the fallback, not the first read. Each sample combines three sources in order —
the role's own hook signal (Claude only, S9f), the terminal's **structured state**, then the
screen markers above. `blocked` from any source wins; a source that reports nothing is skipped,
so an upstream core without screen inference still works on markers alone.

| Runner | `waiting_for_person` | `status.state` | `status.kind` | `activity.revision` | How it is filled |
|---|---|---|---|---|---|
| Devin | yes | yes — its own words | yes | yes | **Native OSC 7501** — Devin reports itself |
| Claude Code | yes | yes — the core's words | yes | yes | **Fork screen inference** via `claude.toml` (the runner emits no 7501 of its own; a native `done` record may coexist) |
| Codex | yes | yes | yes | yes | Fork inference via `codex.toml` |
| OpenCode | yes | yes | yes | yes | Fork inference via `opencode.toml` |
| cmd | yes | yes | yes | yes | Fork inference via `command-code.toml` |
| cline | no | no | no | no | None — no hook signal and no structured state; the screen markers carry it (S14a: `agent: null`) |
| cursor-agent | yes | yes | yes | yes | Fork inference via `cursor.toml` — lands with the core-side Repo-A work (`feat/cursor-manifest` on followcat/herdr-core); before that, the screen markers carry cursor alone |
| agy | no (`false` even when `status.state` is `blocked`) | yes (`idle`, `working`, `done`, `blocked`) | yes (`permission`, `question`) | yes — turn-level only (stays frozen during streaming, tool calls and backgrounded tasks; bumps on turn completion) | **Native OSC 7501** (`app: "antigravity"`) measured on 1.5.16 (`waiting_for_person` stays `false`; `work.rs` and `permit.rs` read the screen, and `activity.revision` stalls are disabled) |
| `fake` | yes | no | no | no | The script's own hook signal through the same `--settings` pair Claude gets; the screen markers carry the rest (S9f) |

How the structured fields are consumed:

- `waiting_for_person` → `blocked`, first. It catches prompt kinds no marker knows (cursor's
  file-write picker was the S12c gap). When the screen cannot identify the prompt's block, the
  episode raises `needs the user` and no approval key is ever bound to it.
- `status.state` → the work map (`working`, `blocked`, `idle`/`done`/`waiting` → waiting) with
  `status.msg` attached to the summary's doing line. `error` is the rate-limit taxonomy's input:
  it, or a **non-working** reporting core's screen naming a limit, raises `rate limited`, and
  the stage waits the parsed reset out and sends `continue` — three auto-retries, then the
  episode escalates. A weekly or monthly limit escalates at once. A non-working screen (or an
  `error` message) naming `Connection error` raises its own episode and gets the same bounded
  `continue`, on a 15-second wait; a rate limit on the same sample wins.
- `activity.revision` → feeds D4's unified liveness evidence track for runners whose revision advances
  mid-turn (`claude`, `codex`, `devin`, `opencode`, `cursor`, `cmd`): a change while `working`
  counts as activity and clears `stalled` with `activity resumed`. Runners whose revision is
  turn-level only (`agy`) or absent (`cline`, `fake`) never count revision freezes against them.
  Across all runners, `runners/normalise.rs` strips animated braille spinners (`U+2800..=U+28FF`),
  Claude/cmd spinner glyphs, elapsed durations, `Thought for Ns, N tokens` headers, token
  counters, and status-line clocks before comparing screens for stalls so spinner-only redraws
  still stall (`screen animates only (spinner)`), while the delivery gate continues to inspect the
  raw screen.
- On an **upstream hc core** (no screen inference, no fork manifests), cmd and cursor report
  nothing at all — every structured field stays empty and the screen markers carry them, which
  is what the stage did before this upgrade.

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
| cursor-agent | From the last `─` rule above the picker's `→ ` option-cursor line to the end — the `$ <command>` or the diff preview, the question, the ` in <path>`/`Add <Verb>(<path>)` target and the options. A diff preview's own `─` rule can open the block; the `Approval N of M` header stays out of it | Shell picker measured on 2026.11.05; file pickers from the S12e run and the fork's `cursor.toml` |
| cmd | From the last full-width `─` rule above the `❯ 1.` picker to the end — the tool title, the `needs to …` sentence, the command and the picker | Measured on 1.88.5 |
| cline | From the last full-width `─` rule above `Cline needs permission` to the end — the tool name, the `$ <command>` preview and the `[y] Approve   [n] Deny` line | Measured on 3.0.74 (S14a). No key is ever sent for it, so the block is compared for the record only: a prompt under `--yolo` is the user's |
| agy | Command picker: from `Command` and the full-width `─` rule above `Requesting permission for:` to the end (`Run this command?`, numbered options, `↑/↓ Navigate` footer; `esc to cancel` stays at the bottom). Plan question: from `Question` and the full-width `─` rule above `Question N/M:` to the end (`↑/↓ Navigate · enter Select · esc Skip`) | Measured on 1.5.16 (approval keys stay `none sent`, escalated) |
| Anything else | none | — |

## Approval keys

A recorded `role.approval` sends one key while the saved prompt block is still the one on show —
`allow` takes the highlighted allow option, `deny` dismisses it.

<!-- BEGIN GENERATED approval-keys -->
| Runner | `allow` | `deny` | Markers from |
|---|---|---|---|
| Claude Code | `enter` | `esc` | Both measured on 2.1.319 — `enter` takes the allow option; `esc` does not run the command and waits for input |
| Codex | `enter` | `esc` | From its `› 1.` picker — still to be measured |
| Devin | `enter` | `esc` | Observed on 3000.12.32 — `enter` takes the highlighted `Once`, `esc` cancels |
| OpenCode | none sent | none sent | Not measured — the attention stays open for the user |
| cursor-agent | `y` | none sent | `y` takes the highlighted allow option — shell measured on 2026.11.05, the file picker's `(y)` from the S12e capture. A denied command is retried, so no deny key is mapped and the attention stays open |
| cmd | `enter` | `esc` | Both measured on 1.88.5 — `enter` takes `❯ 1. Yes` (run-once); `esc` or `3` ends the turn. Option `2` is never sent — it writes a project allow rule |
| cline | none sent | none sent | The block only appears when `--yolo`'s auto-approve was switched off by hand; both keys are a mode the stage must not change (S14a) |
| agy | none sent | none sent | Unmeasured — the attention stays open for the user |
| fake | `enter` | `esc` | The script itself — byte-verified in `real_stage_closed_loop` |
<!-- END GENERATED approval-keys -->

### What a cursor file picker reaches for (S12f)

A `Write to this file?`-family picker names its target twice — the ` in <path>` line under the
question and the `Add <Verb>(<path>) to allowlist?` option — and the stage reads both (`Add` first).
An actor's `y` is the director's only while the displayed path is absolute and resolves — the deepest
existing ancestor canonicalised, so `..` and symlinks are real — strictly inside the role's own work
area, naming no `.git`, `.commandcode`, `.cursor` or `cursor` component. Anything else flags
`needs_user`: a target outside or escaping the area, a `~` or relative or empty path, a `…`-truncated
one, no path at all, and a picker whose question names nothing measured (`Run this mcp?`, a web
prompt). The shell picker is not path-checked — its command is read by the usual env-flag scan.

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

- **Every role** gets the shared read-only set — `git status`, `git log`, `git show`, `git diff`,
  `git branch --list`, `git branch --show-current`, `ls`, `rg`, `wc`, `head`, `backlot msg show`, each bare
  (`Bash(git log)`) and with arguments (`Bash(git log *)`). These are the forensics a role needs to
  understand a repo, a delivery or its own state, so a `git log` never asks a person (field notes
  1126 §5). `git branch` is never a bare rule — only its two listing forms, which git refuses to
  combine with a delete (measured: `git branch --list -D x` is a usage error).
- An **actor** adds the two writes its loop drives — `git add` and `git commit`, bare and starred —
  plus every declared check's `run` verbatim as `Bash(<run>)`.
- The **director** adds `git merge` and the declared checks.
- The **producer** and **writer** keep the base pair plus the read-only set; anything else still
  prompts and D9 answers it.

Nothing broader is pre-allowed: no `Bash(*)`, no bypass flag or mode, no `--add-dir`, no
`Edit`/`Write`, and the permission mode is unchanged. A `run` the rule syntax cannot hold — a
parenthesis or a comma — is never pre-allowed, though `studio new` accepts the cast; a `run`
reaching for `BACKLOT_*`, `env -i` or `--ignore-environment` is refused there outright. Whether
Claude Code actually matches these rules — and whether a wrapped `X=y git commit …` slips past them —
is not yet verified; `limits.md` keeps it for the S9f real-session run.

**A prefix rule sees the command, not its flags — this is a seatbelt, not a sandbox.** Every runner
here matches an allow rule on the leading words of one plain command, so a rule that names a
read-only command also permits that command's *write-capable* flags: `rg --pre <cmd>` runs `<cmd>`
for every file it searches, and `git diff --output=<file>`, `git log --output=<file>` and `git show
--output=<file>` write to `<file>` (measured: `git log --output=x` creates `x`). The read-only set is
sized to keep honest forensic commands off the prompts, not to stop a hostile role — the ledger's
role rules, the guard shims and `git` hooks are the layers that refuse outward writes, and they too
are a seatbelt over a cooperative agent, not an OS sandbox. `rg` stays in the set because its
`--pre` misuse is a read tool invoked oddly, not a foothold a role gains over the repo it already
edits; the honest limit is recorded here so nobody mistakes the allow list for containment.

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
rule is `Shell(backlot)`; **every** role adds the shared read-only set — `git status`, `git log`,
`git show`, `git diff`, `git branch --list`, `git branch --show-current`, `ls`, `rg`, `wc`, `head`,
`backlot msg show` — an actor also `git add` and `git commit`, the director also `git merge`, and both every declared
check's `run` verbatim. Only `git branch`'s two listing forms are rules; the bare family stays out
because the matcher's prefix form would also allow `git branch -D`. Nothing broader is written — no `Shell(*)`, no
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
`Shell(backlot)` covers `backlot status`; **every** role adds the shared read-only set
(`Shell(git status:*)`, `git log`, `git show`, `git diff`, `git branch --list`,
`git branch --show-current`, `ls`, `rg`, `wc`, `head`, `backlot msg show`); an actor also `Shell(git add:*)` and
`git commit`, the director also `git merge`, and both every declared check's `run` verbatim as
`Shell(<run>:*)`. Only `git branch`'s two listing forms are rules — the bare prefix would also
allow `git branch -D`. Every `&&`/`||`/`;`/pipe subcommand
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

The launch always passes `--add-dir <dir>` — the studio state directory for a role,
and `<base>/studios` for Hollywood, the sibling of its `<base>/hollywood` cwd. The flag
only feeds the model's `<workspace>` context: the tool permission check builds its runtime
without the add-dir list, so the flag alone does **not** stop the
`Outside the workspace — …` prompt (measured on 1.93). What stops it is
`permissions.additionalDirectories` in the same project settings file, written with the
same directory beside `allow` and `deny` (the stage owns those three keys and keeps every
other member). A role's cwd is its worktree and Hollywood's is `hollywood/`, so without
it a stage-sent message file (`<studio dir>/messages/<n>-….md`), prompt or requirement
read stayed at the prompt and the guarded send never delivered. Both the flag and the
key ride fresh and resumed launches alike.

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

Hooks (measured on `cmd 1.93.0`): `cmd::prepare` splices `"hooks"` beside `"permissions"` in the
same `.commandcode/settings.json` (preserving `authInfo`, unrelated keys, and any non-backlot user
hooks), wiring `SessionStart`, `PreToolUse`, `PostToolUse`, and `Stop` (with `matcher` omitted and
`timeout: 5`) to `<backlot> hook --runner cmd --file <signals.jsonl>`. Payloads carry snake_case
`hook_event_name` and `session_id` matching the `<uuid>.jsonl` transcript file id; empty stdout +
exit 0 allows cleanly, and no `PermissionRequest`/`Notification` hook exists
(`prompt_detection.hook` stays `false`). New files are added to `<git>/info/exclude`; if
`.commandcode/settings.json` is already tracked in git, `prepare` leaves it untouched, logs
`hooks unavailable`, and raises an Info `hooks unavailable` attention.

## Per-work-area hooks (Devin)

Measured on Devin 3000.14.30821 (with `swe-2-flash-medium`). Devin supports per-work-area
configuration in `<cwd>/.devin/config.local.json` alongside `--permission-mode` and
`--respect-workspace-trust false` without touching `~/.config/devin/config.json`.

Before every start and resume, `devin::prepare` splices top-level `"hooks"` (preserving existing
keys and non-backlot hook entries byte-for-byte) wiring `SessionStart`, `UserPromptSubmit`,
`PreToolUse`, `PostToolUse`, `PermissionRequest`, `Stop`, and `SessionEnd` (`"matcher": ""`,
`"timeout": 5`) to `<backlot> hook --runner devin --file <signals.jsonl>`. Payloads carry
snake_case `hook_event_name` and `session_id` matching `devin list --format json`'s `id`, and empty
stdout + exit 0 lets prompts proceed. `PermissionRequest` includes `tool_name` (such as `exec`) and
`tool_input` (`command` or `file_path`), enabling `prompt_detection.hook = true`. New
`.devin/config.local.json` files are added to `<git>/info/exclude` (Devin excludes
`.devin/mcp_config.local.json` itself, not `config.local.json`); if already tracked in git,
`devin::prepare` skips writing it, logs `hooks unavailable`, and raises an Info `hooks unavailable`
attention.

Devin takes no argv allow list; `--permission-mode smart` stays the gate. The role's shared
read-only set is written instead into the same per-work-area `permissions.allow` (the member the
file already carries), as one `Exec(<cmd>)` per command in Devin's own rule syntax — `Exec(git
status)`, `Exec(git log)`, `Exec(git show)`, `Exec(git diff)`, `Exec(git branch --list)`, `Exec(git
branch --show-current)`, `Exec(ls)`, `Exec(rg)`, `Exec(wc)`, `Exec(head)`, `Exec(backlot msg show)`. Devin's rules are prefix
matches (`Exec(ls)` permits `ls -a`, `Exec(grep)` permits `grep -n x`), so a single entry per
command is the whole rule — there is no ` *` form and no `Bash(...)`, unlike claude. The splice
replaces only `allow`; any other `permissions` member (`deny` and the rest) keeps its bytes, and
Hollywood, the overseer, gets none. Nothing a role's job adds (an actor's commits, the director's
merge, a check's `run`) is pre-allowed here. This rule set is measured only as far as the file
format; whether Devin honours `permissions.allow` under `smart` is not yet verified (see
`limits.md`).

## Per-session files (cline)

Cline shares the user's login, settings and session store — no per-role config, data or auth tree, the
same reality cmd has. The role boundary is the work area: a hook the stage writes before launch, plus a
mask file for MCP. Both are rewritten at every start and resume.

- **`<cwd>/.cline/hooks/PreToolUse.sh`** (mode 0755) — the permission layer (S14a section 5). Cline runs
  it before every tool call with `.tool_call.name` and `.tool_call.input` as JSON on stdin; printing
  `{"cancel":true,"errorMessage":"…"}` refuses the call and ends the agent's turn, `{"cancel":false}`
  lets it run. The script is plain POSIX `sh` — no `jq`, because nothing else here assumes it — and
  default-deny:
  - the payload's strings are decoded before any check: JSON escapes are unescaped, `\n` and `\r`
    become a command separator, and `\b`, `\f` and `\uXXXX` refuse the whole call — so an escaped
    quote, a backtick between escaped quotes or an escaped newline cannot slip a second command past
    a prefix match;
  - allowed: `run_commands` whose every segment is one of the role's rules — `backlot` plus the
    shared read-only set (`git status`, `git log`, `git show`, `git diff`, `git branch --list`,
    `git branch --show-current`, `ls`, `rg`, `wc`, `head`, `backlot msg show`) for **every** role; an actor's `git add`
    and `git commit`; the director's `git merge`; each declared check verbatim. Only `git branch`'s
    two listing forms are rules, never the bare family. A command is split on `;`, `&&`, `||`, `|` and a
    decoded newline, and every part must match on its own; `$`, a backtick, a redirection, or `(` `)`
    `{` `}` in a segment refuses it, so nothing reaches past the prefix that matched;
  - allowed: `read_files` for any path but the secret ones (`.ssh`, `.aws`, `.gnupg`, `.netrc`,
    `.git-credentials`, `id_rsa*`, `.credentials*`, `.env*`, `.npmrc`, `.pypirc`, the gh and backlot
    configs, the stage's own files, any `/proc/<pid>/environ`, the agent CLIs' own stores — `.claude/`,
    `.codex/auth.json`, `.cursor/`, `.config/cursor/`, `.config/devin/`, `.commandcode/` — plus
    `.docker/config.json`, `.kube/config`, `.config/secrets/` and the shell start-up files `.zshrc`,
    `.bashrc`, `.profile`, `.zshenv`, `.bash_profile`) and any `..` path; `editor` only for a path
    strictly inside the role's work area, never `.git/` or `.cline/`;
  - `search_codebase`'s input shape is unmeasured, so every string in it that looks like a path —
    absolute, `~`-relative or holding a `/` — goes through the same `..` and secret checks as a read.
  - quoting is understood, not stripped: a rule is plain unquoted words, so `git commit -m
    "fix(runners): tidy"` is one allowed segment while `"curl" x`, an unterminated quote, a `$(…)` or a
    backtick inside double quotes refuse. Every word of an allowed segment is checked for a secret
    too — `git diff --no-index /proc/self/environ /dev/null` prints the environment and a commit's
    `-F` would echo it — with an `--opt=value`'s value checked as well.
  - the rules limit which programs start, not what code runs: an allowed `cargo test` runs the
    project's build scripts and tests, which the role itself may have written. Shell env mutations
    (`env`, `export`, `unset`, `declare`, `typeset` altering `BACKLOT_*`, `GIT_CONFIG_*`, or `PATH`)
    and self-approval/mutation calls (`backlot card`, `backlot perms`, `--as user`) are also cancelled
    in `PreToolUse.sh`.
  - always refused: `web_search`, `fetch_web_content`, `composio_*`, every `<server>__<tool>` MCP name,
    and every tool the list above does not name (`ask_question`, `skills`, an unknown name).
  A cancel ends the turn exactly as cmd's `esc` deny does: the agent does not retry, and it is told the
  action is outside the role and should be asked for through `backlot` — the actor's next step is a
  `backlot ask`/`tell`, which the rules let through.
- **`roles/<role>/cline-signals.jsonl`** — one line per decision in the shape `signals.rs` reads:
  `ts_ms`, `event: PreToolUse`, `decision: allow|cancel`, `tool_name` and one bounded target (the
  command or path, cut to 200 characters). Never file contents — an `editor` call's `new_text` and
  `old_text` are not recorded. The line carries no session id (the hook cannot know it), so the stage's
  session-filtered read ignores it today: the record is for a person, ready to bind to an attention once
  cline exposes its own id.
- **`roles/<role>/cline-mcp.json`** — `{"mcpServers":{}}`, the role's whole MCP policy while the studio
  has none for roles. `CLINE_MCP_SETTINGS_PATH` points cline at it, which removes every global server
  from a `--yolo` session (measured). The stage's env reaches the agent's commands only because of
  `--yolo`.
- **`/.cline/` in the worktree's `info/exclude`** — the hook directory never shows up in the actor's
  `git status`. The stage appends the line once, following a linked worktree's `gitdir:` pointer to the
  real git directory.

Why `--yolo`, and what it costs:

- `cline -i` runs its session in a per-user hub daemon by default, and tool commands are children of the
  hub: they get the **hub's environment**, so `BACKLOT_STUDIO`, `BACKLOT_ROLE`, `BACKLOT_TASK`,
  `BACKLOT_STATE_DIR` and `PATH` never reach the actor. `--yolo` is the one switch that selects the local
  backend, which is why every argv carries it. `--data-dir` would too, but it relocates the login
  (`settings/providers.json`) and the TUI then opens on a sign-in screen, so it is never passed.
- Without `--data-dir` the launch flags may rewrite the user's global providers file (per-provider
  `settings.model` and `settings.reasoning`). That is the cursor `--model` class of side effect:
  documented, not restored.
- `Tab` (plan/act) and `Shift+Tab` (auto-approve) persist to the user's global settings file; the stage
  never presses them.
- `--hooks-dir` has no effect in any mode and is never passed; a workspace hook is found by its path.
- The signals file's lines carry no session id, so the stage's session-filtered signal read does not use
  them for the work state today: the screen markers carry it, as they do for cmd.

Discovery and resume: the id is cline's own (`<epoch ms>_<5 lowercase chars>`). The stage looks for the
manifest the launch wrote, at or after the role's start, whose `cwd` (or `workspace_root`) is the role's
and whose `pid` is the engine process — the `.cline` child of the foreground `node` wrapper the terminal
reports. Directories with no manifest, a manifest for another folder and another process's session are
skipped. `--id <id>` resumes; the positional prompt is ignored on a resume, so the stage sends the
prompt after the idle composer shows (the devin and cmd rule).

## Per-session file (agy)

Measured on agy 1.5.16 (with session-discovery layout from 1.5.14–1.5.16; `trustedWorkspaces` measured on 1.5.18 on 2026-11-27).

- `agy --help` documents `--mode` (`accept-edits`, `auto`, `plan`), `--effort`
  (`low`, `medium`, `high`, `xhigh`, `max`), `--model`, `--settings` (a JSON object or file path,
  session-only and never saved), `-i`/`--prompt-interactive`, `--conversation <id>` to resume, and
  `-c`/`--continue` as a boolean continue-most-recent. `agy models` prints the model ids —
  Gemini families plus orchestrator, claude and gpt-oss rows, the thinking depth in each id.
- The reference launch is `agy --mode accept-edits --effort high --settings <dir>/.agy-settings.json
  -i "<prompt>"`. The adapter's argv is that shape with `--model` added; `standard` and `worktree`
  take `accept-edits`, while `readonly` passes `--mode plan`, adds mutating `command(git ...)` prefixes
  to `.agy-settings.json` `permissions.deny`, and blocks file-write tools in `PreToolUse`.
- The session store is `~/.gemini/antigravity-cli/conversations/<uuid>.db` — one sqlite file per
  conversation whose bytes embed the workspace's `file://` URIs verbatim (percent-encoded when the
  path needs it). On agy 1.5.14+ the db does not always carry the workspace URI: agy keeps
  `~/.gemini/antigravity-cli/cache/last_conversations.json`, a JSON object mapping an absolute cwd
  to the id of the last conversation started there, so the stage first takes the id that file names
  for the role's exact cwd when `conversations/<id>.db` exists, is born at or after the role's start
  and no other role claims it. A missing, unreadable or malformed file — and an id that fails any of
  those checks — falls through to the byte scan below, which finds the newest `.db` at or after the
  role's start whose bytes carry the role cwd's URI — a plain scan, no sqlite dependency.
  `--conversation <id>` resumes; `-i` is not passed on a resume (the devin and cmd rule). When
  `[stage] answer_trust` is on (the default) or the cwd is already trusted, `--conversation <id>`
  restores the transcript straight to the idle composer with no dialog; with `answer_trust = false`
  in an untrusted folder it shows the folder-trust dialog first.
- The workdir's `.agy-settings.json` is the per-session boundary like cmd's project file:
  before every start and resume, `agy::prepare` writes (or splices byte-preservingly)
  `toolPermission: "always-proceed"` together with policy-aware `permissions.deny` containing the outward-action
  prefixes (`command(git push)` when policy is `none`, `command(gh pr)` unless `recorded` with `allow_gh_pr`,
  `command(gh release)`, `command(gh repo)`, `command(gh issue)`, `command(gh api)`, `command(hc update)`,
  `command(hc machine)`, `command(hc server stop)`, `command(hc run)`, `command(hc exec)`,
  `command(hc send)`, `command(hc keys)`, `command(hc connect)`, `command(systemd-run)`,
  `command(tmux new -d)`, `command(tmux new-session -d)`, `command(scp)`, `command(rsync)`, `command(ssh)`,
  `command(npm publish)`, `command(cargo publish)`) — **without** trailing ` *` — while preserving
  any other top-level settings and any existing `permissions.allow`. When `[stage] answer_trust` is
  on (the default), `agy::prepare` also splices `"trustedWorkspaces"` keeping any existing entries and
  adding the role's canonical absolute cwd once; when `answer_trust = false`, `trustedWorkspaces` is
  not added. Feature-branch pushes under `recorded`/`any`
  pass `permissions.deny` and are checked at execution time by the per-role `pre-push` hook and `PreToolUse`
  (`backlot hook --runner agy`), which also rejects segment-level `BACKLOT_*`/`GIT_CONFIG_*`/`PATH` env
  mutations and `backlot card`/`perms`/`--as user` self-approval attempts. agy reads `.agy-settings.json` once at startup,
  so routine local tools proceed without prompts while outward prefixes are blocked ahead of the
  shims (`bash -c` still bypasses agy prefix rules and hits `PreToolUse`, role `PATH` shims, and git hook).
  Four `toolPermission` values are accepted on 1.5.16:
  `always-proceed`, `request-review`, `strict` and `proceed-in-sandbox` (`prompt`, `ask` and
  `default` are rejected as `unrecognized value`).
  - `request-review` (and `proceed-in-sandbox` when `--sandbox` is not passed) shows a 4-option
    command picker (`> 1. Yes, run command`, `2.` conversation allow-prefix, `3.` persist to
    `settings.json`, `4. No, cancel`); `enter` runs once, `esc` cancels (`Interrupted`), `4` declines.
  - `strict` shows a 2-option picker (`> 1. Yes, run command`, `2. No, cancel`) and still prompts
    even when `permissions.allow` matches the command.
  - `permissions.allow` and `permissions.deny` take `command(<prefix>)` **without** a trailing ` *`
    (measured on 1.5.16: `command(git push)` blocked `git push origin HEAD:refs/heads/denied-probe`
    under `always-proceed`, `request-review`, `strict` and `--sandbox` `proceed-in-sandbox`, while
    `command(git push *)` failed to match and let the push run; similarly `command(echo)`
    auto-allowed `echo …` under `request-review` while `command(echo *)` still prompted). A denied
    command never opens a picker and prints
    `Permission denied for unsandboxed(<command>). Matches user-configured deny rule.`
- Folder trust: measured on agy 1.5.18 (2026-11-27) — passing `--settings` with
  `{"trustedWorkspaces": ["<cwd>"]}` (exact canonical absolute path; trusting a parent directory
  does not match) skips the dialog and goes straight to the prompt on both fresh and
  `--conversation` launches, while `~/.gemini/antigravity-cli/settings.json` stays byte-identical.
  Only when `[stage] answer_trust` is on (the default) is the cwd added; when `answer_trust = false`,
  `agy` renders `Accessing workspace: ... Do you trust the contents of this project?` with
  `> Yes, I trust this folder` highlighted (`status.state: "blocked"`, `kind: "permission"`,
  `waiting_for_person: false`), where `enter` accepts and persists trust and `down`+`enter` or `esc`
  exits 0.
- Plan mode (`--mode plan`): `ask_question` opens `Question 1/1:` with numbered options (`> 1.`)
  and `Write-in...` (`status.state: "blocked"`, `kind: "question"`, `waiting_for_person: false`),
  even while `esc to cancel` stays visible at the bottom. `up`/`down` move the cursor, `enter`
  submits, `esc` skips.
- Hooks (measured on `agy 1.5.16`): `--settings` rejects `"hooks"` (`Error: --settings: unknown setting "hooks"`),
  so `agy::prepare` writes `<workspace>/.agents/hooks.json` under top-level `"backlot"` (byte-preservingly
  spliced, keeping other named hooks intact). `SessionStart`, `PreInvocation`, `PostInvocation`, and `Stop`
  take flat command arrays, while `PreToolUse` and `PostToolUse` take matcher groups (`"matcher": ""`,
  `"timeout": 5`), each invoking `<backlot> hook --runner agy --file <signals.jsonl>`. Payloads use camelCase
  (`conversationId`, `hookEventName`, `toolCall.name`, `toolCall.args.CommandLine`); `hook` maps
  `conversationId` -> `session_id`, `PreInvocation` -> `UserPromptSubmit` (`working`),
  `PreToolUse`/`PostToolUse` (`working`), `PostInvocation`/`Stop` -> `Stop` (`waiting`), and
  `SessionStart` (`none`). On `PreToolUse`, empty stdout + exit 0 or `{"decision":"allow"}` allows,
  while `{"decision":"deny","reason":"refused by backlot: <command> is the person's to run; ask with backlot ask"}`
  blocks `run_command` before `.agy-settings.json` `permissions.deny` runs; nonzero exits, `{}`, or
  exceeding the per-hook timeout kill/fail and deny the tool, so `backlot hook --runner agy` enforces
  `guard_rules::refused_command_line` + `guard::record_refusal` inside a 1.5 s fail-open budget and
  answers `{"decision":"allow"}` on allowed calls, parse errors, internal errors, or timeouts.
  New `.agents/hooks.json` files are added to `<git>/info/exclude` (following linked-worktree `commondir`);
  if tracked in git, `.agents/hooks.json` is left untouched (`hooks unavailable`), keeping `.agy-settings.json`
  `permissions.deny` as fallback. On an untrusted workspace's turn 1, `agy` only loads user global hooks at
  `SessionStart` and reloads `<workspace>/.agents/hooks.json` for turn 2+ once trusted; on an already-trusted
  workspace `SessionStart` fires immediately at launch.
- Long turns and backgrounded commands: during a normal turn (`status.state: "working"`), the
  braille spinner animates (`⣷`/`⢿` + two spaces + `Generating...`, `Reading file...`,
  `Running command... Ns`) and `activity.revision` stays frozen until the turn reaches `done`.
  Long shell commands (`sleep 660`, `timeout 660 sh -c 'while :; do :; done'`,
  `flock /home/user/x.lock sleep 1`) auto-background after ~10 s into `● [HH:MM:SS] <cmd> running`
  with `? for shortcuts` and `1 task(s) · /tasks`: `flock` logged one check-in at ~60 s, after
  which all three kept the screen and `activity.revision` frozen for 600+ s while `status.state`
  stayed `"working"`. Meanwhile `agy` itself still ticks ~26–42 jiffies per 10 s and reads/writes
  several KB per 10 s, whereas `sleep 660` (`wchan=hrtimer_nanosleep`) and `flock`
  (`wchan=locks_lock_inode_wait`) sit at 0 CPU / 0 I/O and `sh -c 'while :; do :; done'` burns
  ~100 jiffies/s (`deepest`: `['sh', '-c', 'while :; do :; done']`, `['sleep', '660']`,
  `['flock', …]`). When a CPU-heavy child exits and is reaped, live descendant `utime+stime` drops
  unless `root_cutime+cstime` is included.
- `work.rs`, `permit.rs`, `caps.rs`, `hook.rs`, and `agy.rs` wire `agy`'s screen states, prompt block,
  `Native` status source, turn-level revision rule, and `.agents/hooks.json` signals + `PreToolUse` guard.

## Read-only profile (`readonly`)

When a role runs under `permission_profile = "readonly"` (such as an `--kind audit` task cast with `backlot cast --readonly`), the stage enforces read-only boundaries in two layers:

1. **Guarantees (ledger, git shims, hooks, and delivery check):** `guard` refuses every git write (`add`, `commit`, `checkout`, `reset`, `clean`, `merge`, `rebase`, `cherry-pick`, `stash`, `push`, `tag`, `branch -d/-D/-m`, unknown subcommands, and `!` aliases) while allowing read-only git inspection; `backlot deliver` on a `readonly`/`audit` task refuses `--commits` and verifies a clean worktree with unmoved `HEAD` across single-repo and workspace members; and `director` merges referencing an audit branch (`backlot/<studio>/<audit>`) are refused.
2. **Runner-native settings (seatbelt / second layer — NOT OS-level containment):** Each adapter configures native read-only restrictions on top of the guarantees above, with honest limits per CLI:

| Runner | Native `readonly` configuration | Pre-allow changes | Honest limits |
|---|---|---|---|
| Claude Code | `--permission-mode plan`; `claude-settings.json` adds `"Edit"`, `"Write"`, `"NotebookEdit"` to `permissions.deny` | Drops `Bash(git add)` and `Bash(git commit)` | Read-only shell prefixes (such as `rg --pre` or `git diff --output=...`) are seatbelt, not an OS sandbox; git writes and worktree dirtiness are stopped by `guard` and `deliver` |
| Codex | `-s read-only` (keeps `-a on-request`) | None (no allow list) | Codex runs under its native read-only sandbox for workspace files; ledger writes (`backlot deliver --report`) sit outside the worktree under state dir and prompt unless approved |
| Devin | `--permission-mode auto`; `.devin/config.local.json` adds `"Write(*)"` to `permissions.deny` | Keeps read-only `Exec(...)` allow rules | `--permission-mode auto` only auto-approves read-only tools and `"Write(*)"` denies Devin's write tool; arbitrary shell mutations still depend on `guard` and `deliver` |
| OpenCode | `OPENCODE_CONFIG_CONTENT` sets `"permission": {"edit": "deny"}` (covers `edit`, `patch`, `write`) | None | Shell commands follow OpenCode defaults; file/git cleanliness at delivery is guaranteed by `guard` and `deliver` |
| cursor-agent | `--mode plan`; `cli-config.json` sets `"deny": ["Write(*)", "Write(**)"]` | Drops `Shell(git add)` and `Shell(git commit)` | Shell writes not matched by `Write(*)`/`Shell(...)` still hit prompts, `guard`, and `deliver` |
| cmd | Default mode + `.commandcode/settings.json` `permissions.deny` adds `"Edit"` and `"Write"` (on top of `mcp__*`) | Drops `Shell(git add:*)` and `Shell(git commit:*)` | Deliberately avoids `--permission-mode plan`: `cmd`'s `isPlanSafe` rejects `Shell(backlot)` before allow rules and would block `backlot deliver`. `Edit`/`Write` are denied at Rung 1; shell writes are stopped by `guard` and `deliver` |
| cline | `--plan` + `.cline/hooks/PreToolUse.sh` removes `editor` branch | Drops `git add` and `git commit` from `allowed` | `--plan` natively disables `Editor` and `ApplyPatch`, and `PreToolUse.sh` cancels `editor` before execution; because `forceLocalBackend` checks `mode === "yolo"`, ensure no conflicting shared Cline Hub daemon overrides local env/hooks |
| agy | `--mode plan` + `.agy-settings.json` `permissions.deny` adds mutating `command(git ...)` prefixes + `PreToolUse` hook denies file-writing tools | None | Native `command(...)` is prefix-matched (`bash -c` hits `PreToolUse` `guard_shell` and `deliver`), and `PreToolUse` (`backlot hook --runner agy`) denies file-write tools whenever hooks are loaded |

## Screen patterns

Observed on real runs. These are the first entries for each adapter's pattern table and need captured fixtures.

| Runner | Screen shows | Meaning | Response |
|---|---|---|---|
| Claude Code | `Quick safety check` with `Yes, I trust this folder`, cursor on `❯ No, exit` | First use of a folder that is a git repository | One key per tick from the cursor's line: Down on `No`, Enter on `Yes`, only when the terminal's cwd is the role's own folder |
| Codex | Workspace trust prompt | First run in a folder | Launch-time `-c 'projects={"<cwd>"={trust_level="trusted"}}'` keeps it away; if seen, answer for the actor's own work area only |
| Devin | `Connection error` | Stopped until it gets a message | **Recognized** — the stage raises `connection error` and sends a short `continue` after 15 s, three times, then escalates |
| Devin | `Reached free model rate limit ... reset in N minutes` | Rate limited, status reads `done` | **Recognized** — a `done`/`idle`/`waiting` tail is read; wait the parsed reset, then a short retry. Do not change model |
| cursor-agent | `Workspace Trust Required` with `Trust this workspace` | First launch in an untrusted folder | `--trust` is passed at launch, so the dialog should never appear; if it does it is the user's — the stage never answers it, even for an actor |
| cmd | `Do you trust the files in this folder?` with `❯ 1. Yes, proceed` | First launch in a non-empty untrusted folder | `--trust` is passed at launch, so the dialog should never appear; if it does it is the user's — the stage never answers it, even for an actor |
| cline | `Cline needs permission` with `Approve tool call?` and `[y] Approve   [n] Deny` | Auto-approve was switched off by hand — `--yolo` forces it on, so the prompt should never appear | The stage presses nothing: `y` and `n` are both a mode it must not change. The attention stays open for the user |
| agy | `Accessing workspace:` with `Do you trust the contents of this project?` and `> Yes, I trust this folder` | First launch (or `--conversation`) in an untrusted folder when `[stage] answer_trust = false` (1.5.16; 1.5.18 skips via `--settings` `trustedWorkspaces` when on) | Stage writes `trustedWorkspaces` at launch when `answer_trust` is on; when off or seen, cursor starts on `Yes` and the stage leaves it for the user |
| agy | `Requesting permission for:` / `Run this command?` with `> 1. Yes, run command` | Command needing approval under `request-review`, `strict` or unsandboxed `proceed-in-sandbox` (1.5.16) | `enter` runs once (`1.`); `esc` cancels (`Interrupted`); `4` (`request-review`) or `2` (`strict`) declines. Escalated under D2 |
| agy | `Question 1/1:` with `> 1.` options and `↑/↓ Navigate · enter Select · esc Skip` | Plan-mode question picker (`--mode plan`, 1.5.16) | `up`/`down` + `enter` selects; `esc` skips. Escalated under D2 |
| Any | Permission dialog | Waiting on a person | Approval policy |

Fixtures use neutral data only: `host-a`, `alice`, `/home/user`, `acme-shop`.

## First version

Claude Code, Codex, Devin, OpenCode, cursor-agent, cmd, cline and agy. Kimi and others follow the
same adapter shape when needed.
