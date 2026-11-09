# What has and has not been tried

The studio layer's every claim here is measured in the ledger's own tests or in a run against a
real core. This page is the honest version: what real agents have done, what only the fake runner
has done, and what nobody has done. `progress.md` is the entry-by-entry record behind it;
`runners.md` has the per-runner measurements.

## What real agents have run

Claude Code is the only runner that has been through the whole loop. It ran three times on a
small sample repository — `claude-haiku-5-5` at low effort, Claude Code 2.1.319, on `hc` from
the followcat fork, five or six sessions each. The first and third runs finished the loop, from
the requirement through plan, tasks, deliveries and reviews to an integration branch; the
second stopped on a stage bug, fixed before the third. Between them the runs covered permission
prompts answered by the director and by the person, a denied command, two actors at once, and
merging two task branches into the integration branch. What has not run on real agents is
everything built after the third run: stall detection, the at-cap notice, going on after a
block, and restart recovery.

Codex, Devin and OpenCode were launched and their session IDs captured — `runners.md` says
exactly what was measured for each. Per runner:

- **Claude Code** — launch, resume, the `--allowedTools` pair, the approval keys (`enter`/`esc`)
  and the work-state markers all measured on 2.1.319.
- **Codex 0.166.0** — launched; the launch arguments were checked. Its work-state markers come
  from an older capture and its approval keys are mapped from its picker, unmeasured.
- **Devin 3000.12.32** — launched; its permission prompt and `enter`/`esc` keys were observed
  live. `devin list`'s exact JSON field names are as documented, not re-measured, and its
  resume ignores `--prompt-file` (the stage sends the prompt after the session opens).
- **OpenCode 2.2.5** — launched; its replies were not confirmed. Its working marker is guessed,
  not measured, and a recorded approval sends it **no key at all** — there is none mapped, so
  the attention stays open for the user regardless of the decision.
- **Kimi** — not measured.

Stall detection and restart recovery have only run on the fake runner. No real agent has been
resumed after a restart, and whether a real runner picks a stopped core's session back up is
untested. A held role whose terminal is lost restarts through the same resume path — unmeasured
on a real core.

## The hc core

CI runs the studio's integration tests on a pinned official `herdr-core` build with the fake
runner. Real agents have only ever run on the followcat fork. Running backlot on official hc is
intended but has not been verified end to end.

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

A stage restart loses the in-memory screen track, so an open `stalled` attention clears and can
re-raise one `stall_limit_s` later.

## Attention and detection limits

- **Rate limits and connection errors are not recognized.** `runners.md` lists the observed
  screens (Devin's rate-limit line, its connection-error stop) but nothing matches on them — such
  a role reaches its reviewer as a stall, after `stall_limit_s` (600 by default).
- **Stall detection watches an *unchanged* screen.** A runner that keeps animating while it is
  stuck — a live spinner, a clock in its status line — never looks frozen and never raises
  `stalled`.
- **A notice can arrive late or stale.** Delivery waits for the role to be up five seconds, its
  screen to hold still two, and a five-second gap since the last send to it — a notice can land
  after what it names has moved on. A crash between `terminal.send` and the ledger write can
  send the same notice twice.
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
- **The summary document is written at most once every two seconds** and only when it changed —
  `backlot status` reads the ledger, but the panel can be a couple of seconds behind it.
- **The summary document has a 1 MiB hard limit.** A live studio is written even past the 64 KiB
  target, but past 1 MiB the document is not; a finished studio is dropped for size instead.

## Files and records

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
