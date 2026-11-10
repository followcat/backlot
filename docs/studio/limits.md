# What has and has not been tried

This page covers what real agents have run, what only the test runner has run, and what nobody
has tried. `progress.md` is the entry-by-entry record behind it; `runners.md` has the per-runner
measurements.

## What real agents have run

Claude Code is the only runner that has been through the whole loop. It ran four times on a
small sample repository — `claude-haiku-5-5` at low effort, on `hc` from the followcat fork,
five or six sessions each; Claude Code 2.1.319 for the first three, 2.1.321 for the fourth. The
first, third and fourth runs finished the loop, from the requirement through plan, tasks,
deliveries and reviews to an integration branch; the second stopped on a stage bug, fixed
before the third. Between them the runs covered permission prompts answered by the director and
by the person, a denied command, two actors at once, and merging two task branches into the
integration branch. The fourth stopped the hc core twice: once while both actors were working,
once while the director was merging. What has not run on real agents: stall detection, the
at-cap notice and going on after a block.

Codex, Devin and OpenCode were launched and their session IDs captured — `runners.md` says
exactly what was measured for each. Per runner:

- **Claude Code** — launch, resume, the base `--allowedTools` pair, the approval keys (`enter`/`esc`)
  and the work-state markers all measured on 2.1.319. The per-role git and check `Bash` rules added
  to `--allowedTools` since — and whether a wrapped `X=y git commit …` slips past them — are argv-only
  and not yet verified on a real session; that waits for the S9f run.
- **Codex 0.166.0** — launched; the launch arguments were checked. Its work-state markers come
  from an older capture and its approval keys are mapped from its picker, unmeasured.
- **Devin 3000.12.32** — launched; its permission prompt and `enter`/`esc` keys were observed
  live. `devin list`'s exact JSON field names are as documented, not re-measured, and its
  resume ignores `--prompt-file` (the stage sends the prompt after the session opens).
- **OpenCode 2.2.5** — launched; its replies were not confirmed. Its working marker is guessed,
  not measured, and a recorded approval sends it **no key at all** — there is none mapped, so
  the attention stays open for the user regardless of the decision.

Stall detection has only run on the fake runner. Restart recovery has run on Claude Code only;
whether Codex, Devin or OpenCode pick a stopped core's session back up is untested. A held role
whose terminal is lost restarts through the same resume path — unmeasured on a real core.

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

The panel reads the stage's document through the local watcher's `local_documents` — remote
machines never carry it, so a studio running on a remote core shows nothing in the panel.

## After an hc restart

The person starts the stage again with `backlot stage start` — the core never re-runs the
original commands. Anything a role was running at that moment is gone; the ledger replays and a
role with a recorded session id resumes into it. A role with no recorded session cannot be
resumed, and three losses inside ten minutes stops automatic recovery with a `cannot resume`
attention.

On Claude Code 2.1.321 this held for both restarts of the fourth run: every live role came back
in its own session within a second of `stage start`, closed roles stayed closed, and a prompt
open at the stop was cleared. A command that was waiting at a permission prompt is gone; the
recovered role has to notice that and run it again — the director did, after first checking
whether its interrupted merge had landed.

A stage restart loses the in-memory screen track, so an open `stalled` attention clears and can
re-raise one `stall_limit_s` later.

## Attention and detection limits

- **Rate limits and connection errors are not recognized.** `runners.md` lists the observed
  screens (Devin's rate-limit line, its connection-error stop) but nothing matches on them — such
  a role reaches its reviewer as a stall, after `stall_limit_s` (600 by default).
- **Stall detection watches an *unchanged* screen.** A runner that keeps animating while it is
  stuck — a live spinner, a clock in its status line — never looks frozen and never raises
  `stalled`.
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
- **A question on a role's own screen reaches nobody.** Only the producer's terminal is read by
  the person. In the fourth run the director asked there which report node to use and waited
  about four minutes; its prompt now names the `integration` node and sends questions through
  `backlot tell`, but nothing detects a role waiting on an answer that never comes — stall
  detection counts an idle producer, director or writer as normal.
- **The producer's own problems notify nobody.** An actor's stall or `cannot resume` reaches its
  task's reviewer, the writer's and director's reach the producer — the producer's shows only in
  the panel and `backlot status`.

## The stage's loop

- **`git` calls have no timeout.** Work-area git steps run inside the stage's loop; a hung git
  blocks every studio the stage runs until it returns. Runner `--version` (5 s) and the session
  probes — codex's `~/.codex/sessions` scan and `devin list` (10 s) — do time out, but a slow one
  still delays that tick for every studio.
- **One real runner starts per tick.** Casting several actors spreads the launches over several
  ticks; `fake` is exempt. A post-launch runner (`codex`, `devin`) also defers its start while a
  live role in the same runner-and-folder pair still lacks a session id.
- **`[limits]` caps live roles per runner, per studio.** The cast table's `<runner> = <n>` holds a
  wanted start back once `<n>` live roles — fixed and actor alike, a being-started one included —
  sit on that runner. Held starts retry each tick in a fixed order (producer, director, writer,
  then actors by task priority and seq) and the wait writes nothing to the ledger. The scope is
  the studio: two studios on one account each count only their own roles, so a shared account's
  real ceiling is the sum of the studios' caps, set by the person who writes the casts. `studio
  new` refuses a cap that can never sit its fixed roles — under the producer-and-director count on
  the runner, or at their count while the writer or a candidate shares it.
- **The summary document is written at most once every two seconds** and only when it changed —
  `backlot status` reads the ledger, but the panel can be a couple of seconds behind it.
- **The summary document has a 1 MiB hard limit.** A live studio is written even past the 64 KiB
  target, but past 1 MiB the document is not; a finished studio is dropped for size instead.

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
  `BACKLOT_*`, `env -i` or `--ignore-environment` is the user's: the director gets no notice and
  cannot approve it. The limit: every role runs `backlot` as the same user, so this stops a role
  doing another role's step *through backlot*, not a determined process — a script that sets
  the variable inside shows only `bash x.sh` on the prompt screen.
- **Worktrees and branches are not removed** when a studio ends — `work/` trees and `backlot/…`
  branches accumulate — and state directories are not pruned. Removal is yours; the guide shows
  the commands.
- **Panel rows name the person, not the task.** Several closed rows for one person look alike in
  a narrow panel.
- **`backlot records` is the factual record** — deliveries, wall time, defects per actor
  identity — with a written conclusion and no score. Casting *from* actor records is not built.
- **A studio with no candidates cannot run tasks.** `backlot cast` refuses anyone not in
  `[[candidates]]` — only the fixed roles ever start.
- **`stage not running` is a report, not a button.** The panel shows the line and keeps the last
  document; starting the stage from the panel is not implemented.

## Scale and platforms

- **One studio at a time is what has been tried.** The stage loop reconciles every studio it
  finds and the panel scrolls, but a second live studio has never run.
- **Later, not built:** experimental production (`mode = "experimental"`, R14) is a reserved
  name only.
- **macOS builds and tests pass in CI; nobody has run it on a real Mac.** `docs/troubleshooting.md`
  has the open macOS issue — `Ctrl+B q` does not exit under automation there, cause unknown.
- **Usage and cost** are only what a runner reports itself; nothing in backlot meters it.

## The fake runner

`fake` is the test runner the suite drives. The binary carries the path to `tools/fake-runner.sh`
compiled in from its build tree — on an installed release that path usually does not even exist —
and a cast file should not use it either way.
