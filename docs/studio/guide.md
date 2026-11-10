# Running a studio

A studio is one requirement worked on by a small cast of agents: a producer you talk to, a
director who assigns the work, a writer who writes the plan and the acceptance list, and actors
who do the tasks. Every role is an ordinary `hc` terminal running an agent CLI. A ledger records
what actually happened — talking never moves a task, a `backlot` command does — and a panel
inside `backlot` shows who is doing what.

This guide walks one studio end to end. [limits.md](limits.md) says plainly what has and has not
been verified; read it before trusting a studio with real work.

## What you need

- **`hc` installed and a core running.** `backlot` (the TUI) starts a local core itself when
  nothing answers on the session socket; `hc server start` does it without the UI. The studio
  commands (`backlot studio`, `backlot stage`, `backlot status`) do not start a core — they fail
  with "cannot reach the hc core" until one is up.
- **A git repository with at least one commit.** Actors and the director work in git worktrees
  created off it. It does not have to be clean.
- **At least one agent CLI installed and logged in:** Claude Code (`claude`), Codex (`codex`),
  Devin (`devin`) or OpenCode (`opencode`). The stage launches it with flags; nothing interactive
  runs. Each role in the cast file names its runner, so different roles can use different CLIs.

## The cast file

The cast file says who plays each role. One complete example:

```toml
# cast.toml — a studio's cast.
rework_cap = 2          # optional, default 2
stall_limit_s = 600     # optional, default 600
nudge_after_s = 180     # optional, default 180
escalate_after_s = 600  # optional, default 600

[producer]
runner = "claude"
model = "claude-sonnet-5-5"
effort = "high"
permission_profile = "standard"

[director]
runner = "claude"
model = "claude-sonnet-5-5"
effort = "high"
permission_profile = "standard"

[writer]
runner = "claude"
model = "claude-haiku-5-5"
effort = "medium"
permission_profile = "standard"

[[candidates]]
name = "actor-a"
runner = "claude"
model = "claude-haiku-5-5"
effort = "low"
permission_profile = "worktree"

[[candidates]]
name = "actor-b"
runner = "codex"
model = "gpt-5.3-codex"
effort = "medium"
permission_profile = "worktree"
```

| Key | Meaning |
|---|---|
| `[producer]` | Identity of the producer: the role you talk to. Required. |
| `[director]` | Identity of the director: splits the plan into tasks, casts actors, reviews. Required. |
| `[writer]` | Identity of the writer: drafts the plan and acceptance list with the producer. Required. |
| `[[candidates]]` | One table per actor the director may cast. `name` is what `backlot cast --actor` takes. Optional — but with none, `cast` refuses everyone and no task can run; only the fixed roles start. |
| `rework_cap` | Rework rounds the director may ask for per task before the call goes to you via the producer. Optional; default 2. |
| `stall_limit_s` | Seconds a live role's screen may stay unchanged while it counts as working before a `stalled` attention raises. Optional; default 600. Must be at least 1. |
| `nudge_after_s` | Seconds an owed step's owner may sit idle — live, waiting, nothing queued or in flight — before the stage queues it one `nudge` message (counted from the later of the step becoming owed and the idle stretch starting). Optional; default 180. Must be at least 1. |
| `escalate_after_s` | Seconds after that nudge was delivered during which the step may stay owed and the owner idle before an `owed` attention raises. Optional; default 600. Must be at least 1. |
| `runner` | The agent CLI that fills the role: `claude`, `codex`, `devin` or `opencode`. (`fake` is the test runner shipped for the suite — never put it in a real cast file.) |
| `model` | The model name, passed to the runner's own flag. |
| `effort` | Thinking depth, passed to the runner's own flag where it has one (Claude `--effort`; Codex `model_reasoning_effort`; Devin takes it as part of the model id; OpenCode has no flag). |
| `permission_profile` | `standard` or `worktree`. On Claude these map to `--permission-mode manual` and `acceptEdits`; the other runners accept the same two values. `worktree` is the right choice for actors working inside their own worktree. |
| `runner_version` | Optional. Recorded on the role's identity as the configuration that did the work; the stage fills it from `<runner> --version` when you leave it out. |
| `mode` | Optional. `standard` is the default and the only process that exists; `experimental` is a reserved name (R14, not built). |

The three fixed roles and every `[[candidates]]` entry each need `runner`, `model`, `effort` and
`permission_profile`. The candidates are the pool the director picks actors from — it cannot cast
anyone else.

## Starting

```sh
hc server start
backlot studio new --studio acme-shop --title "Shop" --repo /home/user/acme-shop --cast /home/user/cast.toml
backlot stage start
backlot
```

`hc server start` brings a core up first, because the studio commands need one and do not start
it themselves. If `backlot` (the TUI) is already open, a core is already running and you can skip
that line.

`backlot studio new` records `studio.created` in the studio's ledger and prints it back.
`--studio` names the studio — `[a-z0-9_-]{1,16}` — and every later command takes the same
`--studio` flag or reads `BACKLOT_STUDIO`. `--repo` must be an absolute path. `--title` is a
display name.

`backlot stage start` starts `backlot-stage`, a terminal on the same core that runs the
scheduling loop. It is idempotent — a second `stage start` says it is already running. The stage
then reads the ledger and starts the producer and the director as `bl-acme-shop-producer` and
`bl-acme-shop-director`; the writer joins when a plan is wanted, and an actor's terminal is named
for its task (`bl-acme-shop-t1`). Role terminals are ordinary `hc` terminals: they show in the
sidebar under the `backlot` space, and `hc ls` lists them.

Run `backlot` to open the UI. Press `Ctrl+B y` — prefix, then `y` — to open the studio panel on
the right, expanded and focused. Pressing the chord again while it is focused closes it; pressing
it while the panel is open but unfocused focuses it again. A herdr binding on `y` still wins
while the panel does not own the keys.

Every studio command also accepts `--session <s>` and `--remote <t>` to reach another core, the
same flags the TUI takes.

## Talking to the producer

Focus the producer's terminal — its sidebar row or pane, or Enter on the studio's row in the
panel — and type, exactly like talking to the agent yourself. Tell it what you want built, in as
much detail as you would give a person.

The other roles are reached the same way: focus their terminals (`bl-acme-shop-director`,
`bl-acme-shop-writer`, `bl-acme-shop-t1` for task t1's actor) and type. For a recorded message
the ledger keeps, use `backlot tell`:

```sh
backlot tell director --file /home/user/note.md --studio acme-shop --as user
```

The stage delivers it as `[backlot] user → director: read <file>` once the role reads idle and
its screen has held still — it can wait a while, and it can arrive after the situation it names
has moved on. In the ledger you are the role `user`, so person-facing commands take `--as user`.

`backlot status` shows the studio's recorded state at any time:

```sh
backlot status --studio acme-shop
```

The `next:` lines are what the ledger still owes — one per outstanding step, naming the step, the
role that owes it and how long it has been owed (`next: dispatch — producer, owed 4m`). `user` as
the owner means the step is yours (for example `finish` once every task is terminal and reported).
While a step sits with an idle owner the stage nudges it once after `nudge_after_s` and raises an
`owed` attention `escalate_after_s` after the nudge was delivered — so a `next:` line that survives
for minutes is a real hold-up, not a refresh artifact. The panel shows the first owed step as the
`→` line under the studio.

## What happens next

In the normal path the steps run in this order:

| Step | Who | What |
|---|---|---|
| plan | producer + writer | The producer records requirement v1 (`backlot req revise`), asks for a plan (`backlot plan request`); the writer joins, they agree, the writer publishes it (`backlot plan publish`). |
| dispatch | producer | `backlot dispatch` hands the agreed plan to the director; the writer leaves. |
| cast | director | `backlot task new` splits the plan into tasks; `backlot cast` puts a candidate on each — an actor terminal starts in its own worktree. |
| deliver | actor | The actor works in `work/<task>` on branch `backlot/<studio>/<task>`, commits, and records the delivery (`backlot deliver` with a commit range, a report file and the checks it ran). |
| review | director | The director reviews the delivery against the acceptance list and records a verdict (`backlot review`): `accepted`, `partially-accepted`, `changes-requested` or `blocked`. |
| rework | actor | A `changes-requested` review goes back to the actor as a `[backlot]` message; it fixes and delivers again. The rework cap counts the rounds. |
| report | director → producer → you | `backlot report` at agreed nodes; the producer reports to you in its own terminal. |

The light path skips the ceremony: for a small job the producer can create and cast a task itself
and review the delivery — the same commands, recorded the same way.

### What the panel shows

Expanded, the panel lists each studio, then the groups `producer`, `director`, `writer`,
`actors`, then each person with a mark, a priority flag, `runner/model`, a one-line "doing" and
any attention text. The marks:

| Mark | Meaning |
|---|---|
| spinning glyph | `working` — the role's screen shows its runner's working marker |
| `○` | `waiting` — live, not known to be working or blocked (an unmatched screen reads as waiting) |
| `×` | `blocked` — the role's screen shows a permission prompt |
| `!` | `lost` — the role's terminal vanished without a deliberate close |
| `✓` | `closed` — the stage released the role on purpose; it stays listed and can be reopened |

The title line names what is working (`working: director, actors 2`, else `idle`). `stage not
running` means no live `backlot-stage` terminal — the panel keeps the last document. With the
panel collapsed, a narrow strip lists only the groups with someone working.

Panel keys while it is focused: `j`/`k` or the arrows move the selection, `Enter` acts on the row
(person → focus its terminal, studio → focus the producer, closed person → arm `enter: reopen`
then a second Enter runs `backlot reopen`, group → collapse), `Space` collapses a group, `c`
switches collapsed and expanded, `+`/`-` raise and lower a task's priority, `Esc` unfocuses it
(closes it outright when the panel covers the whole frame). While the panel is focused it owns
the keys — `Ctrl+B y` again closes it.

## Permission prompts

A role whose screen sits on a permission prompt for two seconds raises a `permission prompt`
attention — one per blocked episode — and the visible screen is saved under `attention/<seq>.txt`
in the state directory.

- An **actor's** prompt goes to the director, who may approve it when the command stays inside
  that actor's own work area.
- A prompt touching **real sessions, production, accounts or global settings**, or destructive
  outside the work area, is yours — and so is any prompt on a role that is not an actor (the
  producer, the director, the writer).

Answer with `backlot approve`, naming the role's key — `producer`, `director`, `writer` or
`actor-<task>-c<n>` (the panel and `backlot status` show it):

```sh
backlot approve --person actor-t1-c1 --decision allow --studio acme-shop --as user
```

`--decision deny` answers no. The stage sends the runner's approval key only while the prompt
still shows, never without your recorded decision; a prompt already gone clears itself. A denied
role gets a notice telling it the command did not run.

## Changing the requirement midway

Requirements are versioned. The producer records a new version — you can record one directly too:

```sh
backlot req revise --file /home/user/req-v2.md --summary "second version" --studio acme-shop --as user
```

Each task stays pinned to the version it was dispatched under, so unfinished tasks become
*behind* the new version — the panel marks them `task t1 · behind vN`. Nothing is killed
automatically: the director is told which tasks are behind and chooses per task to continue,
pause or redo, and the producer tells you what is affected.

## What needs attention

Open attention shows in the panel's attention line and in `backlot status`. Every reason the
stage can raise:

| Reason | Meaning | What to do |
|---|---|---|
| `permission prompt` | A role's screen has sat on a permission prompt for two seconds. | An actor's is the director's; anything else is yours. Answer with `backlot approve`; the saved screen is under `attention/<seq>.txt`. |
| `stalled` | A live role's screen did not change for `stall_limit_s` while it counts as working: an actor whose task is `running` and whose screen reads working or waiting (`blocked` never counts — a prompt has its own attention), or a fixed role sampled `working` — an idle one is normal and never stalls. | Look at the saved screen file. The role's reviewer is told; a changed screen clears the attention. |
| `cannot resume` | A lost role could not be brought back: no recorded session id, or three losses inside ten minutes. | For an actor, its reviewer is told to abandon or recast the task. The writer's and director's losses notice the producer; the producer's is on you — there is no automatic way on for a fixed role. |
| `session id not found` | A post-launch runner (`codex`, `devin`) gave no session id within 60 seconds of the role starting. | The role still runs and the stage keeps looking for up to ten minutes. If it never resolves, the role cannot be resumed after a restart — finish its work in this run. |
| `launch refused` | `terminal.run` refused the role's launch — a taken terminal name or an argv the core rejects. | The stage retries each tick; the detail names the refusal. Check the runner exists and the terminal name is free. |
| `not a git repository` | The studio's `--repo` path is not a git work tree — no director or actor starts. | Point the studio at a real work tree, or make the repo one; the stage retries each tick. |
| `work area failed` | A git step failed while preparing `work/` — the detail is git's first error line. | Fix what git complained about; the stage retries each tick. |
| `owed` | A role was nudged about a step it owes — the detail names the step — and `escalate_after_s` later the step is still owed and the role is still idle. | Do the step yourself or talk to the role with `backlot tell <role> --file <file>`; the producer is told for other roles. It clears once the nudged step is done — when none of the role's owed steps carries a delivered nudge — or when the role is no longer wanted; a newer step then earns its own nudge and its own `owed` attention. |
| `runner not supported` | The cast's `runner` has no adapter — a typo, or a runner other than `claude`, `codex`, `devin`, `opencode`. | The role never starts. `backlot correct` or a new studio is the way out — see below. |
| `permission profile not supported` | The cast's `permission_profile` is not `standard` or `worktree`. | Same: the role never starts; fix the cast for the next studio. |
| `config malformed` | `<state base>/config.toml` did not parse, or a `[runner_env.<runner>]` table held a non-string — the role gets no runner env and does not start. | Fix the file's TOML; the stage retries each tick. Only the error's line and column are reported, never the file's contents. |

The ledger is append-only — a `correction` event changes the view, not the past. To repair a
recorded fact use `backlot correct --event <seq> --reason <r> --set <key=value> --as user`.

## After an hc restart

An hc restart ends every terminal, including the stage — the core never re-runs the original
commands. Start the stage again:

```sh
backlot stage start
```

The ledger replays, so everything recorded — requirements, tasks, casts, deliveries, reviews,
approvals — comes back, and `backlot status` shows it. What does not come back: every terminal
process the core was running is gone, so anything a role was doing at that moment stopped.

A role the studio still wants is then handled one of two ways. With a recorded session id, the
stage starts it again resuming that session (`role.resumed`, reason `recovered`) and tells the
role once what happened — a command stopped at a permission prompt must be run again. A role with
no recorded session id, or one lost three times inside ten minutes, raises a `cannot resume`
attention instead.

A stalled role's screen track is lost with the stage: an open `stalled` attention clears and can
raise again one limit later.

## Finishing

When everything is accepted, tell the producer; on your word it records — or you record:

```sh
backlot studio finish --reason "all tasks accepted" --studio acme-shop --as user
```

`studio finish` refuses while any task is open and names them. From then on the stage stops
scheduling the studio; producer and director close like any released role, and only a person you
explicitly reopen comes back. Reopen — `backlot reopen --person <role key> --as user`, or Enter
twice on a closed row — brings the person back into its recorded runner session; a person whose
session id was never found has nothing to resume into, and the panel says so.

What is left: the accepted work sits on the `backlot/acme-shop/integration` branch, checked out
in the `work/integration` worktree — the director integrated there, not into your own branches.
Merging it into your branches is yours:

```sh
git -C /home/user/acme-shop merge backlot/acme-shop/integration
```

backlot removes nothing: the worktrees under `work/` and the `backlot/...` branches stay, so a
closed person can be reopened into the same folder. To remove them yourself once the merge is
done:

```sh
git -C /home/user/acme-shop worktree list
git -C /home/user/acme-shop worktree remove ~/.local/state/backlot/<core-id>/studios/acme-shop/work/t1
git -C /home/user/acme-shop worktree remove ~/.local/state/backlot/<core-id>/studios/acme-shop/work/integration
git -C /home/user/acme-shop branch -D backlot/acme-shop/t1 backlot/acme-shop/integration
```

Check each worktree with `git status` inside it before removing — `worktree remove` refuses a
dirty tree. State directories are not pruned either; the ledger is the record, and removing the
studio's directory under `studios/` removes it.

## Where the files live

The state root is `$XDG_STATE_HOME/backlot/<core id>/` — `~/.local/state/backlot/<core id>/` by
default — one per core, overridable with `BACKLOT_STATE_DIR`:

```
config.toml                  optional [runner_env.<runner>] env for role terminals
studios/acme-shop/
  events.jsonl               the ledger: append-only, the source of truth
  events.lock                the flock every append takes
  req/v<N>.md                requirement versions
  plan/                      the writer's plans and acceptance lists
  reports/                   delivery reports, review findings, node reports
  messages/                  role-to-role messages and stage notice bodies
  attention/                 screens saved when attention was raised
  prompts/                   the rendered role prompt each fresh start got
  work/integration           the director's worktree, branch backlot/acme-shop/integration
  work/<task>                each task's actor worktree, branch backlot/acme-shop/<task>
```

`config.toml` holds per-runner environment — profiles, proxy settings — as `[runner_env.<runner>]`
tables. Values reach the role terminal's environment only; they are never written to the ledger,
the summary or logs. A malformed file raises `config malformed`.

`backlot records` reads every studio's ledger and summarizes each actor identity's deliveries —
the factual record, with no score.

## The panel's config

`$XDG_CONFIG_HOME/backlot/config.toml` — `~/.config/backlot/config.toml` by default:

```toml
[panel]
width = 30   # expanded column width, clamped to 24-60
key = "y"    # the chord tail after the prefix
```

A missing file means defaults; a malformed one means defaults plus a notice carrying only the
line and column. The panel's open/closed state is per client and in memory only — nothing is
saved.
