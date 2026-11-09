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
- **Role to role:** `backlot tell` records the message and writes its body to a file. The stage sends the target one
  line — `[backlot] <from> → <to>: read <file>` — with `terminal.send`, oldest first, at most one send per role at a
  time. A send goes only while the target is live, its work sample is `waiting`, its screen has been unchanged for two
  samples at least two seconds apart, it has been up at least five seconds, and no send to it finished in the last five
  seconds. A refused send stays queued and is retried; a crash between send and record can repeat a line, which is
  harmless. `terminal.send` can hold for minutes, so it is spawned off the stage loop and collected on a later tick.
  `report.sent` to a role delivers the same way. The stage's own notices (see ledger.md `source_seq`) queue as ordinary
  messages with a generated body file under `messages/`.
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

Light path: the producer dispatches one task directly to one actor, or does the job itself — `task new`, `cast` and
`review` have no caller restriction, so `backlot … --as producer` runs the whole path. A producer's cast is recorded in
`task.cast`'s `by` and routes the delivery notice to the producer. It is recorded the same way, so the two paths can be
compared on real work.

When everything is accepted the user decides; `backlot studio finish` records `studio.finished` and the studio stops
scheduling — only a person explicitly reopened is desired again.

## Stop conditions

Agents reviewing agents can loop without end, so the stage enforces limits:

- Rework rounds per task. Default 2. At the cap the task goes to the producer to accept with documented gaps, rescope or
  recast.
- A running time budget per task, optional.
- A stall limit on a role's unchanged screen — `stall_limit_s` in the cast file, default 600 s — detailed under
  "Watching agents".

## Watching agents

Status alone cannot say what each person is doing. It can read `done` on an agent that has been stuck for hours, and on
official hc, agents that do not report status have none at all. The stage therefore combines:

| Source | Used for |
|---|---|
| Explicit `backlot` commands | Task state. This is the authority. |
| hc status and turn events | Working or stopped, when reported |
| `terminal.read` samples | Stalls: a live role's screen unchanged for `stall_limit_s` |
| Per-runner screen patterns | Attention reasons: permission prompt |

The stall rule reads the send gate's screen-stability track — a live role is stalled when its screen has not changed
for `stall_limit_s` (default 600 s) and its work counts: an actor only on a `running` task, where `working` and
`waiting` both count — an actor that went idle is exactly the case — while the producer, director and writer stall only
while `working`; an idle one is normal, and `blocked` never counts because a permission prompt has its own attention.
One `stalled` attention raises per frozen episode with the screen saved under `attention/<seq>.txt`; a changed screen
clears it and a later freeze raises its own. The notice goes to the actor's task reviewer and to the producer for the
other fixed roles — the producer's own stall is panel-only.

A permission prompt follows the approval policy. Rate-limit and connection-error screen patterns are not built —
nothing is measured — so a role that hits one simply stops, and reaches its reviewer as a stall like any other frozen
screen.

## State directory

`$XDG_STATE_HOME/backlot/<core id>/`, overridable with `BACKLOT_STATE_DIR`. Keying by core ID keeps scratch sessions
apart from real ones. See `ledger.md` for the contents.

## Summary document

The stage writes one core document named `backlot`, holding all studios. One fixed name is needed because the watcher's
document list is set when it starts, while studios come and go.

- The stage is the only writer, and it writes with the CAS version.
- It writes only on change, at most once every 2 seconds. Core rewrites all documents in one file, so writes stay rare.
- It stays small: target under 64 KiB, against the 1 MiB limit. Field lengths are capped and old finished studios drop
  off. Closed people stay listed while their studio is live (R19), with no "doing" text. The full history is in the
  ledger.
- No credentials, environment values or prompt text.

Whether the stage is running comes from the terminal inventory (`backlot-stage` live or not), not from a heartbeat, so a
liveness signal costs no document writes.

## Recovery

An hc server restart ends every terminal, including the stage; the person starts it again with
`backlot stage start` — hc never re-runs the original command, and `--keep` does not help.

When the stage starts, it replays each ledger and lists terminals by label. A role that should be live and is still
there is adopted. One that is gone is recorded `role.lost`, and the same pass clears the role's open attention — the
prompt or stall it named is gone with the terminal. A lost role that is still wanted is resumed with its runner's
resume flag into the recorded session ID as `role.resumed` with reason `recovered`, then told once, through the usual
notice path, that its terminal was lost, the session was resumed, anything running then has stopped and a
permission-prompted command must be run again.

A crash loop is bounded: the reducer keeps every `role.lost` time, so the guard survives the restart, and a third loss
inside ten minutes — or a lost role with no recorded session — raises a `cannot resume` attention instead of starting,
routed to the role's reviewer like a stalled one. Session discovery for a post-launch runner keeps looking past the
60-second `session id not found` attention until the discovery stop, so a slow first start still gets an id.

A studio whose ledger cannot be loaded, or whose act, watch or deliver pass fails, is skipped and shown with an
`error` line in the summary while the other studios carry on; the stage picks it up again when it loads. Only losing
the core ends the stage.

The panel shows a one-line "stage not running" while keeping the last document on screen. Starting
the stage from the panel is not implemented — the stage runs as its own process.

## Studio panel

- **Geometry:** a right column, default 30 columns. The panel sits beside the panes only when they keep at least 60
  columns. Otherwise, including at or below the mobile threshold of 64, it is a full-screen view.
- **States (R17, D6, D12):** closed, collapsed or expanded. Closed draws nothing and is the state the parity harness
  checks. Collapsed is a narrow column of about 16 columns listing only the working roles, each with its status mark and
  the number of people working, such as `director` and `actors 2`. Expanded shows the people, and each role group can
  collapse on its own. The 60-column rule above applies to whichever width the panel has.
- **Content (R19):** studios, each with its roles and the people in each role: runner and model, state, a one-line
  "doing", and what needs attention.
- **Work states (R17):** working, waiting for input, blocked, lost and closed each have their own colour and mark, from
  the palette in `chrome/status.rs`. Working is animated (spinning or a slow blink). The animation runs only while
  something is working, so an idle panel causes no redraws. The stage derives the state from per-runner screen patterns
  and publishes it in the summary, separately from whether the role is live, lost or closed.
- **Scrolling (R13, R18):** a visible scrollbar, reusing `chrome/scroll`, with wheel, drag and keys.
- **Order (R20, D11):** several people in one role are ordered by priority, and closed people come after open ones. The
  director sets priority per task with `backlot cast --priority`, the default is normal, and the user can change it in
  the panel, which records a `task.priority` event. It affects display order only, never scheduling.
- **Closed people (R19):** a person whose work is finished stays listed as closed. Closing frees the terminal and keeps
  the session ID in the ledger. Selecting a closed person offers to reopen them: `backlot reopen` records the request,
  and the stage resumes the session with the runner's resume flag and records `role.resumed`.
- **Interaction:** click or keys. Selecting an open person focuses their terminal. Selecting a studio focuses the
  producer.
- **Parity:** one new action toggles the panel, bound to `prefix+y` by default (`y` is unbound in
  herdr's defaults and parity scripts never send it). A user-configured herdr binding on the same
  tail wins while the panel is closed or unfocused; a focused panel owns every key. With the panel
  closed nothing else changes. Panel settings live in backlot's own config file, so herdr's config
  is read exactly as before.
- **Local state:** whether the panel is closed, collapsed or expanded, and which role groups are collapsed, is per
  client. The content is shared by every client through the document.

## Repository layout

| Path | Contents |
|---|---|
| `crates/hc-proto`, `crates/hc-vt` | Unchanged names, shared with upstream |
| `crates/hc-tui` | The TUI, with small hooks only. Keeps its directory while upstream work is landing |
| `crates/backlot-model` | Events, reducer, summary schema. No I/O |
| `crates/backlot-stage` | The stage process and runner adapters |

The panel landed inside `hc-tui` — `src/panel/` holds the model, document reading, config, row
builder, renderer and action runner; `src/app/panel.rs` holds the geometry, key, mouse and runtime
wiring. A separate `backlot-panel` crate was planned but the widget needs the client's hit maps,
geometry helper, prefix dispatch and scheduler deadlines, and extracting it would mean exporting
most of the App's internals.

The binary is `backlot`. `main.rs` sends studio subcommands to the new crates before TUI argument parsing. The `hc-tui`
layout document key and the herdr config path stay as they are, so backlot and hc-tui on one core share a layout.

Pull from `upstream` often. Keeping new code in new crates limits conflicts to the hook files.

## Actor work areas and integration (D7, D10)

Each actor takes its own task, in its own git worktree on `backlot/<studio>/<task>` — created from the integration
branch's current tip — at `<studio dir>/work/<task>`. The director integrates accepted deliveries into
`backlot/<studio>/integration` in `<studio dir>/work/integration` and runs the project's checks there. Producer and
writer run in the studio repo itself. Merging into the user's own branches stays with the user.

A studio repo that is not a git work tree raises `not a git repository` and starts no actor or director. Every step is
idempotent: an existing branch or worktree is reused, a failure raises `work area failed` once with git's first stderr
line and is retried on a later tick, and a recast or a reopen reuses the task's tree — work continues on top. Worktrees
stay after a task closes; removing them is a later step.

## Permission prompts (D9)

Each role starts with a permission profile from `runners.md`. A live role sampled `blocked` on two samples two seconds
apart raises one `permission prompt` attention per blocked episode, with the visible screen saved to
`attention/<seq>.txt`. An actor's episode notices the director, who may approve it when it stays inside the actor's own
work area; `backlot approve --person <role> --decision allow|deny` records `role.approval` and the stage sends the
runner's approval key while the prompt still shows — never without a recorded decision — then clears the attention. A
prompt that touches real sessions, production, accounts or global settings, or is destructive outside the work area,
goes to the user and shows in the panel; so does a prompt on any role that is not an actor.

## Defaults that can still change

**Requirement changes.** Each task is pinned to the requirement version it was dispatched under. A new version marks
unfinished tasks as behind, and the panel shows it. The director decides per task to continue, pause or redo, and the
producer tells the user what is affected. Nothing is killed automatically.

**Usage.** Show only what a runner reports. Record wall time and rework rounds for every task, and show the rest as
unknown rather than estimating.

**Experimental production.** Designed after the first version. The studio record already carries a `mode` field.
