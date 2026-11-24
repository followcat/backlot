# What has and has not been tried

This page covers what real agents have run, what only the test runner has run, and what nobody
has tried. `progress.md` is the entry-by-entry record behind it; `runners.md` has the per-runner
measurements.

## What real agents have run

Two casts have been through the whole loop: Claude Code in every role, and a mixed cast of
Devin, cursor-agent and cmd.

Claude Code ran four times on a
small sample repository — `claude-haiku-5-5` at low effort, on `hc` from the followcat fork,
five or six sessions each; Claude Code 2.1.319 for the first three, 2.1.321 for the fourth. The
first, third and fourth runs finished the loop, from the requirement through plan, tasks,
deliveries and reviews to an integration branch; the second stopped on a stage bug, fixed
before the third. Between them the runs covered permission prompts answered by the director and
by the person, a denied command, two actors at once, and merging two task branches into the
integration branch. The fourth stopped the hc core twice: once while both actors were working,
once while the director was merging.

The mixed cast ran twice on a small sample repository, on `hc` from the followcat fork: Devin
(`swe-2-max`) as the director, cursor-agent (`auto`) as the writer, cmd 1.88.5
(`deepseek/deepseek-v4.1-flash`) as the actors, and the producer on `fake`. The first run
(2026-11-13) had one actor and went from the requirement to an accepted, merged task, leaving
only the user's `finish` owed. The second (2026-11-14) had two actors working at once: 323
ledger events in 1 h 48 min, two deliveries, the stage's own check on each, two accepted
reviews, the merge, both reports and `studio.finished`. In both, the director answered the
actors' prompts with `backlot approve`, the person answered the director's and the writer's,
and every attention raised was cleared by the end of the second run. Neither run had a stall
to recover from or a rate limit to retry, and the producer was never a real agent.

What has not run on real agents: stall detection, the at-cap notice, going on after a block,
the rate-limit and connection-error retries (fixtures and fake-core tests only), and
everything of Hollywood — the layer, its cards and `bl-hollywood`
have only ever run on `fake` (H4's real-TUI proof is a fake-runner run; a real agent is H5).

Codex and OpenCode were launched and their session IDs captured, no more — `runners.md` says
exactly what was measured for each runner. Per runner:

- **Claude Code** — launch, resume, the base `--allowedTools` pair, the approval keys (`enter`/`esc`)
  and the work-state markers all measured on 2.1.319. The per-role git and check `Bash` rules added
  to `--allowedTools` since — and whether a wrapped `X=y git commit …` slips past them — are argv-only
  and not yet verified on a real session; that waits for the S9f run.
- **Codex 0.166.0** — launched; the launch arguments were checked. Its work-state markers come
  from an older capture and its approval keys are mapped from its picker, unmeasured.
- **Devin 3000.12.32** — launched; its permission prompt and `enter`/`esc` keys were observed
  live. `devin list`'s exact JSON field names are as documented, not re-measured, and its
  resume ignores `--prompt-file` (the stage sends the prompt after the session opens). As the
  director of both mixed-cast runs it created and cast the tasks, approved the actors' prompts,
  reviewed and merged; its session id was found within seconds and its state came from the
  structured status it reports. Every one of its own prompts needed the person.
- **OpenCode 2.2.5** — launched; its replies were not confirmed. Its working marker is guessed,
  not measured, and a recorded approval sends it **no key at all** — there is none mapped, so
  the attention stays open for the user regardless of the decision.
- **cursor-agent 2026.11.05** — ran one controlled director run with every other role on `fake`:
  the `CURSOR_*_DIR` isolation held (chats, projects and the `--trust` marker under
  `roles/<role>/cursor/`), `mcp list` under the role's env showed the user's MCP server
  `disabled` — the `mcp-disabled.json` list is verified to hide them — the `Shell(...)` rules
  pre-allowed `backlot`, `git merge …` and the declared check prompt-free, and a killed director
  resumed the same chat uuid with its context. The review still prompted five times —
  `git diff`, `git log` twice, `git branch` and a `rg`/`strings` pipe; the read-only set
  `git status`, `git diff`, `git log`, `git show` is now pre-allowed for the director, while
  `git branch` stays out since its prefix would allow `git branch -D`. The `api3.cursor.sh:443` direct connection was not re-measured at the
  director — the run closed before the probe — so the S11a finding stands (outside the proxy,
  `NODE_USE_ENV_PROXY=1` does not redirect it; model and API traffic go through the proxy). The
  agent's own writes take the umask — `cli-config.json` landed 664 — so the stage holds
  `roles/<role>/cursor/` at 700 as the boundary and raises `cursor dir was open` on drift.
  `CURSOR_SANDBOX_POLICY_DIR` relocation is unexercised, not broken — the bundle honors the
  variable; the director wrote no policy and the run's global `sandbox-policies` churn was the
  user's running Cursor IDE. `runner_version` used to ask `cursor --version` and record the IDE's
  version; it now asks the launch program (`cursor-agent --version`), cached per program. A
  denied command is retried by the runner, so deny sends no key and the attention stays open for
  the user. As the writer of both mixed-cast runs it published the plan and the acceptance list.
  It reports no structured status, so its state is read from the screen; in both runs its
  `Write to this file?` picker was not seen as a prompt and was answered by hand in its
  terminal. The stage now reads every picker of that shape as a prompt (`runners.md`), which has
  not been through a real run since.
- **cmd 1.88.5** (Command Code) — measured live, then the actor of both mixed-cast runs. The
  stage writes `.commandcode/settings.json` (mode 600) into the actor's worktree: `backlot`, the
  git subcommands an actor needs and the declared check ran with no prompt; reads outside the
  worktree and file edits prompted, and the director answered them. `enter` takes the picker's
  run-once option; `esc` ends the turn. cmd writes its session file only when its first turn
  starts, minutes late when that turn waits on a prompt: its discovery window is 240 s, and the
  attention raised at that mark cleared itself when the id was found (17 s and 53 s later in
  the second run). cmd has **no per-role isolation**: its transcripts and `file-history/` land
  under your real `~/.commandcode`; `auth.json`, `config.json` and `mcp.json` there were
  byte-identical after both runs.

Stall detection has only run on the fake runner. Restart recovery has run on Claude Code only;
whether Codex, Devin, OpenCode, cursor-agent or cmd pick a stopped core's session back up is untested.
A held role whose terminal is lost restarts through the same resume path — unmeasured on a real
core.

## The hc core

CI runs the studio's integration tests on a pinned official `herdr-core` build with the fake
runner, and installs a release build on clean Linux and macOS runners with official `hc` 0.1.19,
where a studio starts its producer and director against a stub `claude`. Real agents have only
ever run on the followcat fork, so a full studio on official `hc` has not been tried.

The fork's core reads screens to detect agent status; official hc has no such detection — an
agent shows a status there only if it reports OSC 7501 itself. That is the sidebar's status.
The stage's `working`/`waiting`/`blocked` classification is its own `terminal.read` sampling and
does not depend on either.

`stage run`'s polling fallback is unit-tested only — the scratch core always offers
`events.subscribe`.

The client reads every machine's `backlot` document through the watcher's `documents`, so a
studio whose stage runs on a machine added with `hc machine add` shows in the panel and in
Hollywood's frames, and its actions run on that machine through `terminal.run on:`. That path has
run against a second core on the same host (`hc machine add --via`, fake runner, real key
presses in kitty), not against a real remote host.

A remote studio needs `backlot` installed on the core's host: `stage start` runs that host's own
binary from its `PATH`, with `--session` alone, because the stage terminal lives there. A missing
binary is reported with the target and the one-line install; it leaves no `backlot-stage`
terminal behind. This path is unit-tested only — no test has driven a real remote host.

Each machine's `backlot` version shows under its sidebar row: the running stage publishes it in
the `backlot` document (`backlot_version`, added in this release — older stage documents lack
it and read as unprobed until the probe answers), and machines without it are probed through
`terminal.run` at most once per six hours. The build also compiles in a `backlot_build` — the
git `HEAD` short hash, with `-dirty` while the tracked tree has changes — so two installs of one
version stay distinguishable; `backlot --version` prints it as
`backlot 0.3.0 (abc123def456)`, and the probe reads the version before the parenthesised id.
The machine menu's install/upgrade opens a popup
running the official installer on that machine (`curl | sh` with `BACKLOT_SKIP_CORE=1`), refuses
package-managed binaries first, and restarts an upgraded machine's stage. The version read, the
menu items, the managed-install refusal, the pinned installer line, the re-probe and the
stop→start restart chain are all unit- and draw-tested; the installer popup's live download and
a real remote host's install have not run — the popup itself is `terminal.run`-launched the same
way the proven studio actions are.

## After an hc restart

By default the person starts the stage again with `backlot stage start` — the core never re-runs
the original commands. Anything a role was running at that moment is gone; the ledger replays and
a role with a recorded session id resumes into it. A role with no recorded session cannot be
resumed, and three losses inside ten minutes stops automatic recovery with a `cannot resume`
attention.

`backlot stage watch --restart` is the automated version: the watch also supervises the stage.
When `backlot-stage` is not live it starts it again through the same idempotent path as
`stage start`, at most five times in ten minutes, waiting 2, 4, 8, 16 then 30 seconds between
repeated failures and resetting that backoff once the stage has stayed live for a minute. When
the ten-minute budget is spent it says `stage restart paused: too many restarts (5 in 10m)` once
and waits for the oldest attempt to leave the window. A core that went away with the `hc` restart
is reconnected with its own 2→5→15→30 second backoff (`core disconnected, reconnecting`, then
`core reconnected`), after which the stage is checked again at once. The watch without
`--restart` still only observes.

`backlot stage service [--print | --write] [--session <s>]` renders the systemd `--user` unit
that runs `stage watch --restart`: `backlot-stage.service`, or
`backlot-stage-<session>.service` when a session is named. `--print` (the default) writes the
unit to stdout; `--write` puts it under `${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user/` and
prints the `systemctl --user daemon-reload && systemctl --user enable --now <unit>` command,
but never runs `systemctl` itself. The unit carries `Restart=on-failure`, `RestartSec=5`, the
executable's directory first on `PATH`, and `BACKLOT_STATE_DIR` when the stage environment has
one. What has **not** been run: the unit under a real systemd user manager — its rendering,
writing and refusal rules are unit-tested, but no host init started it. The reconnect path has
likewise only met a closed wire in tests, not a real `hc` restart.

On Claude Code 2.1.321 this held for both restarts of the fourth run: every live role came back
in its own session within a second of `stage start`, closed roles stayed closed, and a prompt
open at the stop was cleared. A command that was waiting at a permission prompt is gone; the
recovered role has to notice that and run it again — the director did, after first checking
whether its interrupted merge had landed.

A stage restart loses the in-memory screen track, so an open `stalled` attention clears and can
re-raise one `stall_limit_s` later.

## Attention and detection limits

- **Rate limits and connection errors are recognized from measured screens only.** The stage
  reads a reporting core's fresh screen tail when it is not `working`: Devin's rate limit
  (`done` with `Reached free model rate limit … reset in 54 minutes`) and its
  `Connection error` stop, and Claude Code's measured `hit your limit` / `session limit` /
  `weekly limit` / `monthly limit` screens. A short limit waits the parsed reset and sends
  `continue` three times before escalating; a weekly or monthly quota escalates
  `needs the user` at once instead of burning those retries. A connection error gets the same
  bounded treatment on a 15-second wait. A `working` terminal is never matched — the words may
  merely be printed — and a banner deeper than the last 12 non-blank lines is scrollback, not
  evidence. What is not verified: none of these paths has met a real agent since they were
  written. The unit and fake-core tests cover the detection and retry cycles; Devin's,
  Claude's and every other runner's exact live wording beyond the captured lines waits for a
  real run. A rate limit or connection error in an ordinary frozen screen still reaches its
  reviewer as a `stalled` once the taxonomy's words are absent.
- **Stall detection watches an *unchanged* screen — and, for a hook-wired Claude, a silent
  signal.** A runner that keeps animating while it is stuck — a live spinner, a clock in its
  status line — never looks frozen on screen, but a Claude role whose last hook signal
  reported working and then went quiet past `stall_limit_s` raises `stalled` anyway, `no hook
  signal for <n>s`; any newer signal clears it. Every other runner still needs its screen to
  freeze.
- **Hook signals are only as good as the wiring.** `--settings` layers merge, so the user's own
  hooks keep running beside ours — and `disableAllHooks` in the user's settings turns ours off:
  the role then reads from its screen, as before. If the hook command itself stops working
  mid-session — the backlot binary moved, the signals path vanished — the role reads as its
  last signal until its next start or resume rewrites the files. Still unverified on a real
  Claude Code: the exact event payloads (the sink already reads `error` and `error_type` for
  StopFailure's error), that `--settings` hooks fire without a trust or review prompt, that
  `-r` keeps the session id across a resume, and how the S9c pre-allow list behaves beside real
  prompts — a real run checks them later.
- **An answered request has no hook of its own.** Claude Code emits nothing between the answer
  and the tool's end, so a bound `PermissionRequest` stays the tail's pending line while the tool
  runs. The stage marks a request answered only when *it* sent the key (`attention.cleared` with
  `sent <key>` on a `<seq>.signal`-bound attention, rebuilt from the ledger each sample) — until
  the tool's own signal the role reads Working, not Blocked. A prompt answered at the role's
  terminal instead leaves the pending line too, so `approve` requires the screen to still read
  Blocked and refuses `prompt gone` otherwise. If Claude's screen markers ever break, approvals
  are refused and the attention stays with the user — the safe direction.
- **A notice can still arrive late — a stale one is dropped, not sent.** Delivery waits for the
  role to be up five seconds, its screen to hold still two, and a five-second gap since the last
  send to it, and several items owed at once now merge into one `messages/digest-<seq>.md`
  rather than several sends. At send time every source-backed queued item is checked against
  the event it relays (`design.md` has the table): one whose source has moved on is recorded
  `message.dropped` and never reaches the terminal. Two windows remain: a send already in
  flight cannot be un-sent, so a notice that went stale while `terminal.send` held still
  lands; and a crash between the send and the ledger write can send the same text twice.
- **The approval race is closed on the fake runner only.** Before a recorded decision's key goes out,
  the screen on show is compared with the attention's saved screen through a per-runner prompt block
  (`runners.md`); a changed prompt sends nothing, clears `prompt changed before the decision was
  sent` and raises its own attention, and a prompt answered in the terminal itself clears `prompt
  answered elsewhere` once the role has been out of the episode for a whole window. Both fixes ran
  on a real core against `fake`; the claude block is built from the measured 2.1.319 layout and
  devin's from a live 3000.12.32 prompt read, while codex's is unmeasured and neither codex's nor
  devin's has run as a studio role — a region that misses a moving part of a real screen could
  refuse a legitimate send, in which case the fresh attention waits for the person.
- **Screen-level dialog escalation (`question`) has run on fixtures and the fake runner only, not
  on real agents.** Role prompts instruct agents never to ask on their own screen (using
  `backlot ask` or `backlot tell`), Claude session settings deny `AskUserQuestion`, and the
  stage now catches interactive dialogs on screen via `agy`'s `Question N/M:` picker
  (`kind: "question"`), structured `kind: "question"`, or D2's 12-line generic fallback rule
  (option cursor `❯`/`>`/`→`/`›` + sibling option or dialog footer) — holding `gate_open`
  closed, raising one `question` attention after two seconds (`attention/<seq>.txt`), routing
  actor questions to the task's reviewer and `director`/`writer` questions to `producer`,
  surfacing `producer`/Hollywood questions with `needs_user`, and clearing `dialog closed` two
  seconds after exit. Unverified against real live agents: non-standard TUI pickers that omit
  those cursor glyphs/footers, free-form prose questions with no picker, and real `claude` or
  `agy` sessions exercising these paths end-to-end.
- **The producer's own problems reach a live director and Hollywood, but only the user can
  fix them.** An actor's stall or `cannot resume` reaches its task's reviewer, the writer's and
  director's reach the producer. A stalled or unresumable producer now tells a live director
  once — with a body that says the director cannot restart it and only the user or Hollywood
  can intervene — and `backlot status` puts a `producer alert:` line above everything else.
  With `[hollywood]` configured the layer receives a `stalled` notice headed
  `<studio>: producer stalled / cannot resume — needs attention`. The direct notice is dropped
  when the producer's attention clears. None of this has run on real agents: the fake runner
  covers the routing and staleness rules only.
- **Every cursor-agent run keeps one direct connection.** `api3.cursor.sh:443` is reached outside
  whatever proxy the environment carries — `NODE_USE_ENV_PROXY=1` does not redirect it — while
  the model and API traffic go through the configured proxy. Measured in S11a and accepted.

## The stage's loop

- **Every `git` call in the stage's loop is bounded (60 s).** Work-area, delivery-range,
  wizard and repo probes spawn `git` in its own process group, drain both pipes on reader
  threads and time out at 60 s: SIGTERM to the group, a one-second grace, then SIGKILL, and
  the child is reaped. A hung git raises one `work area failed` line instead of blocking every
  studio. The bound has only been exercised by unit tests — a real hung git under a network
  file system has not been reproduced.
- **One real runner starts per tick.** Casting several actors spreads the launches over several
  ticks; `fake` is exempt. A post-launch runner (`codex`, `devin`, `cursor`) also defers its start while a
  live role in the same runner-and-folder pair still lacks a session id.
- **`[limits]` caps live roles per runner, per studio.** The cast table's `<runner> = <n>` holds a
  wanted start back once `<n>` live roles — fixed and actor alike, a being-started one included —
  sit on that runner. Held starts retry each tick in a fixed order (producer, director, writer,
  then actors by task priority and seq) and the wait writes nothing to the ledger. The scope is
  the studio: two studios on one account each count only their own roles, so a shared account's
  real ceiling is the sum of the studios' caps, set by the person who writes the casts. `studio
  new` refuses a cap that can never sit its fixed roles — under the producer-and-director count on
  the runner, or at their count while the writer or a candidate shares it. **Hollywood is outside
  every cap** — `bl-hollywood` is not a studio role and no cast counts it — but its start still
  spends the tick's one-real-runner slot like any other launch.
- **A held role owes nothing until it starts.** A start a `[limits]` cap holds back accrues no
  task budget (the clock opens at the first `role.started`), owes no step, draws no nudge and no
  `owed` attention; once the role starts, an owed step ages from the later of its ordinary
  `since` and that start.
- **The summary document is written at most once every two seconds** and only when it changed —
  `backlot status` reads the ledger, but the panel can be a couple of seconds behind it.
- **The summary document has a 1 MiB hard limit.** A live studio is written even past the 64 KiB
  target, but past 1 MiB the document is not; a finished studio is dropped for size instead.

## The Hollywood layer

- **Everything of Hollywood runs on `fake` only.** The layer, `hollywood setup`, `card show`,
  card apply/cancel from the panel and `tell hollywood` from its input line are unit-, draw- and
  real-TUI-tested against the fake runner; no real agent has been Hollywood yet (H5).
- **The layer reads only the summary document** — its studios, headline and open cards inherit
  the document's ≤2 s lag and its caps (≤10 open cards, ≤200 headline chars). A headline or card
  that just landed can lag the ledger by a couple of seconds.
- **`Alt+Y` is a direct key while `[hollywood]` exists.** While bound it no longer reaches the
  pane (readline `M-y`); without the kitty keyboard protocol an Esc then `y` inside the escape
  window reads as Alt+Y; macOS needs Option-as-Meta. `key = "prefix+h"` or removing the section
  is the way out — `[hollywood]` absent takes no key at all.
- **Card actions from the layer run as the user** through spawned `backlot` commands — the same
  D17a honesty note applies: the commands are exactly what you could type, and a failure lands
  as a notice in the client, not in the ledger.

## The client (U2)

- **Everything of U2 runs on `fake` only.** The opening page, `backlot studio new --interactive`,
  studio shells, the per-studio space and the `shells`/`others` groups are unit-, draw- and
  real-TUI-tested against the fake runner; no real-agent studio has run under U2 yet.
- **The relabel pass is once-and-idempotent, and only where the core allows.** Live role
  terminals get `herdr.space=<studio>` and `herdr.tab=<person>` on the stage's next reconcile —
  only when they already carry `backlot.studio`+`backlot.role` of a ledger studio, and only when
  the core can change labels on a live terminal (`terminal.update`). Where it cannot — official
  `hc` — the pass logs once and backs off: roles launched before U2 keep their old `backlot`
  space and `bl-…` tab until they are next launched. Terminal *names* never change.
- **The stage never touches a shell.** No message, nudge, sample, limit count, reap or relabel
  reaches a `backlot.shell` terminal, and a shell never becomes a role — proven in the stage's
  tests on `fake`. Only the user closes a shell; a finished studio keeps its shells while it
  stays in the document.
- **The wizard writes nothing before the final accept** — repository, id, title and cast are
  answers and a temporary file until then, and a refused cast leaves no ledger and no studio
  directory.
- **`--plain` is untested against a real agent** — it is the pre-U2 multiplexer, and parity
  covers its cells; a real cast under `--plain` simply shows as `others` in studio clients.

## Files and records

- **Who may record what is enforced at append; the contents are still trusted.** The ledger
  refuses an event outside the recording role's whitelist: an actor records only its own task's
  delivery, its own `blocked` and plain `tell`/`ask` messages — it can no longer accept its own
  delivery or correct a verdict. The writer records plans and messages only, `task.delivered`
  from anyone but the task's current actor is the user's alone, and `stage start`/`stage run`
  refuse working-role callers. `backlot deliver --commits A..B` resolves both ends in
  `work/<task>` and refuses an empty range or a tip off the task branch, so the recorded
  `sha..sha` is real. The stage also runs the declared checks itself (S9b part 2): each
  `[[checks]]` entry in the cast runs once per delivery in a detached worktree at the delivery's
  end sha, `sh -c` under an environment stripped to a passthrough list plus the declared
  `check_env` names, with the timeout escalated TERM then KILL and the log capped — the result
  is `task.checked`, recorded by `stage`. An accepting review waits for every declared check —
  the user may accept over pending ones — and names each failing check in its conclusion; a
  correction is gated the same. `--check name=pass` on `deliver` stays the actor's own claim,
  kept apart from the stage's records in notices, `status` and `records`. A check is
  actor-written code run as the user with a stripped environment and a timeout — it is not a
  sandbox. Processes a check leaves behind are killed by their group or, when their tree is
  swept, by a `/proc/<pid>/cwd` scan of the user's own processes; on a platform without
  `/proc` that sweep is skipped. A permission prompt whose own block reaches for
  `BACKLOT_*` — read, `unset`, `export`/`declare` — `env -i`, `--ignore-environment`,
  `env -u`/`--unset`, a `/proc/` `environ` read, or an `env`/`printenv` dump coupled with a
  secret or `.env` path is the user's: the director gets no notice and cannot approve it.
  `report show --file` and the file readers stay inside their directory — absolute paths,
  `..`, NUL bytes and symlinks that resolve outside are refused — and studio, card and task
  identifiers are validated before any path or argv is built. The limit: every role runs
  `backlot` as the same user, so this stops a role
  doing another role's step *through backlot*, not a determined process — a script that sets
  the variable inside shows only `bash x.sh` on the prompt screen.
- **The outward-action guard stops agent mistakes; it is not a sandbox.** Shims and
  `GIT_CONFIG_*` hooks catch accidental outward writes (`git push`, `gh` writes, `hc`
  updates/machine/server-stop, `scp`, remote `rsync`, `ssh <host> <cmd>`, `npm publish`,
  `cargo publish`), not a determined escape: invoking an absolute binary path (for example
  `/usr/bin/git` or `/usr/bin/gh`) or clearing `PATH` / `GIT_CONFIG_*` bypasses the shims and
  hook directory; when `agy`'s `.agents/hooks.json` is active inside a role env (`BACKLOT_*`), its
  `PreToolUse` hook intercepts `run_command` (including `bash -c` and chained commands) before
  `.agy-settings.json` `permissions.deny`, records `role.refused` and `outward action`, and returns
  `refused by backlot: …` (outside a role env, `PreToolUse` always answers allow, and the
  `PreToolUse` shell fallback answers `{"decision":"allow"}` if the recorded `backlot` binary is
  missing or exits nonzero). In a not-yet-trusted work area, `agy` loads the work-area hooks only
  after the trust dialog is answered, so the turn that started before it runs without the hook guard;
  the shims (`roles/<role>/bin` and `GIT_CONFIG_*` git hook) and `.agy-settings.json`
  `permissions.deny` still apply on that turn (direct prefix denials stop inside `agy` without
  reaching a shim or appending `role.refused`, as do skipped/tracked `hooks unavailable` cases);
  `claude-settings.json` stays hooks-only and relies on the shims and git hook;
  `cmd` and `devin` signal hooks run as lifecycle hooks rather than a command guard (relying on
  the shims and git hook, and their work-area hook setups are unit-tested and CLI-measured rather
  than proven in a full studio run, whereas `agy` was live-checked end-to-end); and
  `curl`/`wget` piped into `sh` or `bash` is not covered by a `PATH` shim (checking `/proc`
  races pipeline `execve`).
- **Hollywood's exact pre-allow list is wired for `claude`, `cursor` and `cmd` only.** On `devin`,
  `codex` and `opencode` the runner's own permission mode is the gate — the ledger's role rules
  still refuse what they must, but nothing trims the prompt set at launch.
- **A cursor role's `cli-config.json` holds the account's `authInfo`** once the agent has run —
  the agent writes it into the role's isolated `roles/<role>/cursor/config/`. The studio state
  directory is private: do not share or publish it.
- **A finished studio's worktrees and branches are reaped by `backlot studio prune`.** It
  refuses a live studio, keeps a worktree with uncommitted changes unless `--force`, deletes
  `backlot/<studio>/*` branches only through `git branch -d` unless `--force`, and never
  removes `events.jsonl`, `reports/`, `messages/` or `attention/`. It follows the binding: a
  workspace-bound studio prunes each member the ledger recorded (a member gone or broken is
  kept with its reason while the others run), and a no-git studio prunes its state with no
  git step at all. `--dry-run` reads git and prints the plan; `--all-finished` walks every
  finished studio. The rules run in unit tests against an injected git and real-git scratch
  repositories — single repository, workspace and no-git — and the workspace dry run was
  checked against a real finished workspace studio.
- **Panel rows name the person, not the task.** Several closed rows for one person look alike in
  a narrow panel.
- **`backlot records` is the factual record** — deliveries, wall time, defects per actor
  identity — with a written conclusion and no score. `backlot cast --task <id> --suggest`
  reads the same records per candidate for the task's kind — factual counts and honest
  "not enough data" / "no history" notices, never a score or a pick (D8). Neither has run
  against a real agent's history yet — every record the tests show is a seeded one.
- **A studio with no candidates cannot run tasks.** `backlot cast` refuses anyone not in
  `[[candidates]]` — only the fixed roles ever start.
- **A `stage stopped` row starts the stage.** Enter, a click or `s` on the line runs
  `backlot stage start` on that row's machine through the action tracker — the panel still
  shows the last document while it comes up.

## Scale and platforms

- **Two live studios on one core ran the whole loop side by side — on the fake runner.** The
  `real_two_studios_side_by_side` integration test drives `acme-shop` and `acme` — names that
  share a prefix, the worst case for name-keyed ownership — each on its own repository with
  the same task id and the same role names, steps interleaved through requirement, plan,
  dispatch, cast, a blocked prompt, delivery, the declared checks, review, merge and
  `studio.finished`, with a stage-only restart mid-production. It holds: every terminal
  belongs to exactly one studio by name and `backlot.studio` label and none is shared,
  reused or closed by the other studio's events; each ledger holds only its own events; a
  `tell` lands in one studio's `messages/` and one producer's inbox only; a blocked prompt
  in one studio leaves the other delivering; an `approve` key reaches only that studio's
  terminal; `[limits]` count per studio — six `fake` roles live over two `fake = 3` caps;
  checks run in the delivery's own repository and log so; merges land on each repo's own
  `backlot/<studio>/` branches; one studio finishing leaves the other live and intact; the
  restart adopts every role of both with no `role.lost`/`role.resumed`; and the summary
  document plus `status --all` show both studios with the right state at each step. What
  has not run: real agents in two studios — the fake runner is all the proof carries — and
  a third live studio has never run.
- **Experimental production (`mode = "experimental"`, R14) is wired but unproven.** The
  CLI flag, the cast key, the wizard/TUI selection, the `[experimental]`/`exp` badges and
  the per-role research guidance all exist, and an experimental studio may create a task
  with no recorded requirement; no real agent has run a studio in it yet.
- **macOS builds and tests pass in CI; nobody has run it on a real Mac.** `docs/troubleshooting.md`
  has the open macOS issue — `Ctrl+B q` does not exit under automation there, cause unknown.
- **Usage and cost** are only what a runner reports itself; nothing in backlot meters it.
  `backlot deliver` accepts `--tokens-in`, `--tokens-out`, `--cache-read-tokens` and
  `--cost-usd`, the ledger stores them on `task.delivered`, and `status`, `records` and the
  panel show the sums only while a counter is non-zero — strictly factual, no score (D8).
  Whether a runner can report them at all, and how they reach the `deliver` call, is still
  runner-dependent: backlot records what it is told, it does not meter.

## The fake runner

`fake` is the test runner the suite drives. The binary carries the path to `tools/fake-runner.sh`
compiled in from its build tree — on an installed release that path usually does not even exist —
and a cast file should not use it either way.
