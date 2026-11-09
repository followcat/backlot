# Studio design

Status: agreed for the first version. The defaults in the last section can still change.

## Interface and mechanism

The film studio is what the user sees and talks to. Under it are three plain mechanisms: a hierarchy with one user
entry point (the producer), a ledger that holds requirements, tasks and evidence, and independent work areas for actors.
Role conversations do not drive the work by themselves. A task moves only when the ledger records that it moved.

Alternatives considered for R03: a flat agent group chat loses the single entry point and has no owner for integration.
A pure task graph is good for ordering but gives the user nothing to talk to. A shared blackboard alone has no one
accountable for acceptance. The studio combines the useful parts of each.

## Layers

| What | Where | Core change |
|---|---|---|
| Producer, writer, director and actor sessions | Ordinary hc terminals | None |
| Scheduling and enforcement | `backlot stage`, run in an hc terminal | None |
| Ledger, requirement versions, delivery records | The project's state directory | None |
| Live summary for the panel | Core document `backlot` | None |

The TUI is a viewer. It detaches, restarts, runs as several clients and runs over `--remote`, so nothing the studio needs
to keep can live in it.

## The stage process

`backlot stage` is a deterministic process that runs in an hc terminal named `backlot-stage`. Terminal names are unique
among live terminals, so hc itself guarantees one stage per core.

It carries out and records decisions; it does not make them. Judgement belongs to the agents in the roles.

| Stage does | Roles do |
|---|---|
| Starts, resumes and stops role terminals | Decide what to do next |
| Enforces the candidate list and round caps | Choose actors from the candidates, within the caps |
| Delivers messages between roles | Write the content of those messages |
| Detects stalls and prompts | Decide how to respond when asked |
| Writes the summary document | Review deliveries and write findings |

It uses `hc-proto` directly, because `doc.get` and `doc.set` have no CLI verb.

## Role terminals

Each role is a terminal started through `terminal.run`:

- **Name:** `bl-<studio>-<role>`, within hc's limit of 1-32 characters of `a-z 0-9 - _`.
- **Labels:** `backlot.studio`, `backlot.role`, `backlot.task`, plus `herdr.space`, `herdr.tab` and `herdr.seen=0` so
  the terminal appears in the left sidebar like any other.
- **Environment:** `BACKLOT_STUDIO`, `BACKLOT_ROLE`, `BACKLOT_TASK`, plus the runner's environment profile.
- **No parent and no notes:** a terminal started with a parent ends with it by default, and a role must outlive whoever
  started it.

## How roles act

Every role calls the same `backlot` command through its shell. Any agent that can run a shell command can take part, with
no per-agent integration beyond the launch flags in `runners.md`.

| Role | Commands |
|---|---|
| Producer | `backlot req revise`, `backlot plan request`, `backlot dispatch`, `backlot report` |
| Writer | `backlot plan publish` |
| Director | `backlot task new`, `backlot cast`, `backlot review`, `backlot report`, `backlot ask` |
| Actor | `backlot deliver`, `backlot block` |
| Any | `backlot status`, `backlot tell <role>` |

**A delivery is only what `backlot deliver` records**, with a commit range and a report. An idle status, a finished turn
or a report file appearing are not deliveries. Status can read idle in the middle of a turn, and a report file can appear
before the agent has finished.

If an actor goes quiet without delivering, the stage reminds it once and then raises attention to the director.

## Messages

- **User to producer:** the user types in the producer's terminal. Selecting a studio focuses it.
- **Role to role:** `backlot tell` records the message and writes its body to a file. The stage sends the target a
  one-line pointer to that file with `terminal.send`, once the target is not working and shows no prompt.
- **Producer to user:** in the producer's terminal, with a one-line summary in the panel.

## Flow

Normal path:

1. The user and producer settle the goal and constraints. The producer records requirement version 1.
2. The producer starts the writer. They agree on a plan and acceptance list, and the writer publishes it.
3. The producer dispatches to the director.
4. The director splits the work into tasks that can each be delivered on their own, and casts actors from the candidates.
5. Each actor works in its own area and delivers.
6. The director reviews against the plan and acceptance list, requests rework or accepts, and integrates.
7. At nodes, the director reports to the producer, and the producer reports to the user.

Light path: the producer dispatches one task directly to one actor, or does the job itself. It is recorded the same way,
so the two paths can be compared on real work.

## Stop conditions

Agents reviewing agents can loop without end, so the stage enforces limits:

- Rework rounds per task. Default 2. At the cap the task goes to the producer to accept with documented gaps, rescope or
  recast.
- A running time budget per task, optional.
- A stall limit, below.

## Watching agents

Status alone cannot say what each person is doing. It can read `done` on an agent that has been stuck for hours, and on
official hc, agents that do not report status have none at all. The stage therefore combines:

| Source | Used for |
|---|---|
| Explicit `backlot` commands | Task state. This is the authority. |
| hc status and turn events | Working or stopped, when reported |
| `terminal.read` samples, about every 30 s | Stalls: the screen has not changed for a limit, default 10 minutes |
| Per-runner screen patterns | Attention reasons: connection error, rate limit, permission prompt |

Known conditions have known responses: a connection error gets one short continue message; a rate limit waits for its
reset; a permission prompt follows the approval policy. Anything else goes to the director as attention.

## State directory

`$XDG_STATE_HOME/backlot/<core id>/`, overridable with `BACKLOT_STATE_DIR`. Keying by core ID keeps scratch sessions
apart from real ones. See `ledger.md` for the contents.

## Summary document

The stage writes one core document named `backlot`, holding all studios. One fixed name is needed because the watcher's
document list is set when it starts, while studios come and go.

- The stage is the only writer, and it writes with the CAS version.
- It writes only on change, at most once every 2 seconds. Core rewrites all documents in one file, so writes stay rare.
- It stays small: target under 64 KiB, against the 1 MiB limit. Field lengths are capped and old finished studios drop
  off. The full history is in the ledger.
- No credentials, environment values or prompt text.

Whether the stage is running comes from the terminal inventory (`backlot-stage` live or not), not from a heartbeat, so a
liveness signal costs no document writes.

## Recovery

An hc server restart ends every terminal, including the stage. `--keep` does not help, because hc never re-runs the
original command.

When the stage starts, it replays the ledger and lists terminals by label. A role that should be live and is still there
is adopted. One that is gone is recorded as lost and resumed with its runner's resume flag and recorded session ID, then
told it was resumed. A role that cannot be resumed is reported to the director or producer.

The panel shows when the stage is not running and offers to start it.

## Studio panel

- **Geometry:** a right column, default 30 columns. The panel sits beside the panes only when they keep at least 60
  columns. Otherwise, including at or below the mobile threshold of 64, it is a full-screen view.
- **Content:** studios, each with its roles: role, runner and model, state, a one-line "doing", and what needs attention.
  Built to scroll for R13.
- **Interaction:** click or keys. Selecting a role focuses its terminal. Selecting a studio focuses the producer.
- **Parity:** one new action toggles the panel, bound to a key herdr does not use. With the panel closed nothing else
  changes. Panel settings live in backlot's own config file, so herdr's config is read exactly as before.
- **Local state:** whether the panel is open is per client. The content is shared by every client through the document.

## Repository layout

| Path | Contents |
|---|---|
| `crates/hc-proto`, `crates/hc-vt` | Unchanged names, shared with upstream |
| `crates/hc-tui` | The TUI, with small hooks only. Keeps its directory while upstream work is landing |
| `crates/backlot-model` | Events, reducer, summary schema. No I/O |
| `crates/backlot-stage` | The stage process and runner adapters |
| `crates/backlot-panel` | The ratatui panel widget |

The binary is `backlot`. `main.rs` sends studio subcommands to the new crates before TUI argument parsing. The `hc-tui`
layout document key and the herdr config path stay as they are, so backlot and hc-tui on one core share a layout.

Pull from `upstream` often. Keeping new code in new crates limits conflicts to the hook files.

## Actor work areas and integration (D7, D10)

Each actor takes its own task, in its own git worktree on `backlot/<studio>/<task>`. The director integrates accepted
deliveries into `backlot/<studio>/integration` and runs the project's checks there. Merging into the user's own branches
stays with the user.

## Permission prompts (D9)

Each role starts with a permission profile from `runners.md`. A prompt that still appears is raised to the director, who
may approve it when it stays inside the actor's own work area. A prompt that touches real sessions, production, accounts
or global settings, or is destructive outside the work area, goes to the user and shows in the panel.

## Defaults that can still change

**Requirement changes.** Each task is pinned to the requirement version it was dispatched under. A new version marks
unfinished tasks as behind, and the panel shows it. The director decides per task to continue, pause or redo, and the
producer tells the user what is affected. Nothing is killed automatically.

**Usage.** Show only what a runner reports. Record wall time and rework rounds for every task, and show the rest as
unknown rather than estimating.

**Experimental production.** Designed after the first version. The studio record already carries a `mode` field.
