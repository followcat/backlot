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
  Devin (`devin`), OpenCode (`opencode`) or cursor-agent (`cursor`). The stage launches it with
  flags; nothing interactive runs. Each role in the cast file names its runner, so different roles
  can use different CLIs.

## The cast file

The cast file says who plays each role. One complete example:

```toml
# cast.toml — a studio's cast.
rework_cap = 2          # optional, default 2
stall_limit_s = 600     # optional, default 600
nudge_after_s = 180     # optional, default 180
escalate_after_s = 600  # optional, default 600
task_budget_s = 7200    # optional running-time budget per task — unset means none
check_env = ["CI"]      # stage env names a check may see — optional

# Live-role cap per runner in this studio — optional; a runner with
# no entry is unlimited.
[limits]
claude = 3
devin = 1

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

# Checks the stage itself runs on every delivery, in a detached
# worktree at the delivery's tip.
[[checks]]
name = "units"
run = "cargo test --lib"
timeout_s = 120
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
| `task_budget_s` | Running-time budget per task in seconds, counted from the task's first actor `role.started` to its close, leaving out paused stretches — a cast a `[limits]` cap still holds has used nothing. A running or delivered task past it raises one `over budget` attention — the director and the producer are told; nothing is killed or paused. Optional; unset means no budget. Must be at least 1. |
| `[limits]` | Live-role cap per runner in this studio — `claude = 3` starts no fourth live role on `claude` until a slot frees. Fixed roles and actors count alike, a role being started counts, and a runner with no entry is unlimited. Held starts go producer, director, writer, then actors by task priority and seq, and the waiting role shows `waiting for a <runner> slot`. Per studio: two studios on one account each count only their own roles. A `0` is refused — a runner is limited by a real number or absent. The cap must seat its fixed roles first — the producer and the director stay live for the studio's whole run — so `studio new` refuses a cap under their count on the runner, or at their count while the writer or a candidate shares it: nobody else could ever sit. |
| `runner` | The agent CLI that fills the role: `claude`, `codex`, `devin`, `opencode`, `cursor`, `cmd` or `cline`. (`fake` is the test runner shipped for the suite — never put it in a real cast file.) |
| `model` | The model name, passed to the runner's own flag. A `cline` model must be a `cline-pass/<model>` id: the provider is taken from it, and any other id is refused. |
| `effort` | Optional. Thinking depth, passed to the runner's own flag where it has one (Claude `--effort`; Codex `model_reasoning_effort`; cline `--thinking` — `none`, `low`, `medium`, `high` or `xhigh`; OpenCode has no flag). `cursor` and `devin` carry the depth in the model id and `cmd`'s flag writes global config, so all three omit it — a set `effort` on any of them is refused. |
| `permission_profile` | `standard` or `worktree`. On Claude these map to `--permission-mode manual` and `acceptEdits`; the other runners accept the same two values. `worktree` is the right choice for actors working inside their own worktree. |
| `runner_version` | Optional. Recorded on the role's identity as the configuration that did the work; the stage fills it from the launch program's own `--version` (cursor's is `cursor-agent`, not the `cursor` IDE launcher) when you leave it out. |
| `mode` | Optional. `standard` is the default. `experimental` (R14) marks the studio an experimental production — `backlot studio new --mode` overrides it, the wizard and the New Studio dialog offer both, each role's prompt gains research-level guidance (hypotheses, competing variants, honest negatives), a task may be created with no recorded requirement for direct-to-task exploration, and `status` plus the dock/sheet flag it `[experimental]`/`exp`. |
| `check_env` | Optional list of stage environment names a check may see, valued from the stage's own environment. Names that could carry identity or credentials — `BACKLOT_*`, `HC_*`, proxies — are refused at `studio new`. |
| `[[checks]]` | One table per check the stage runs itself on every delivery: `name`, the `run` command (`sh -c` in a detached worktree at the delivery's tip, under a stripped environment), and `timeout_s` — past it the run is TERM'd, then KILL'd five seconds later. Results land as `task.checked`; an accepting review waits for them and names each failure in its conclusion. |

The three fixed roles and every `[[candidates]]` entry each need `runner`, `model` and
`permission_profile`; `effort` is required only on a runner that takes one. The candidates are the
pool the director picks actors from — it cannot cast anyone else.

## Starting

The easiest start is the client itself:

```sh
hc server start
backlot
```

With no studio, `backlot` opens on the **opening page**: one line of what it is, then its
actions. Press `n` — the **New Studio dialog** opens, a four-step overlay that runs in the
client itself: **1 Machine & Repo** (Local plus every attached machine, each checked for
reachability and `backlot --version`; a machine without backlot or offline says why and cannot
be picked), **2 ID & Title** (the repository's name, validated live), **3 Cast** (the runners
the target machine's `backlot runners --json` found, cycled per role; `Ctrl+E` edits the
generated cast.toml in `$EDITOR`), **4 Check & Start** (`studio new --dry-run` on the target
machine, then `Enter` creates — over hc's `terminal.run` when the machine is remote — and
optionally starts the stage before the new producer's terminal takes focus). `Esc` steps back
and then closes; `Ctrl+C` cancels from any step. When a studio exists, the page is gone and the
studio is current.

The repository list comes from `backlot repos --json` on the target machine — a breadcrumb, a
`/` filter, `→` to descend and `←` to ascend. A git repository without commits is disabled with
its hint, a repository another studio uses warns, and a remote path is never checked against the
local disk.

In a terminal, `backlot studio new --interactive` asks the same questions. Its repository
question is a full-screen picker when stdin and stdout are terminals (the plain
`repository:` line otherwise, so scripted input still works):

```
 New studio · repository
 path › ~/code/▏                               Tab completes

   ~/code
 ▶ ├   acme-shop                  ● main · 2 h ago
   ├   backlot                    ● main · 5 min ago
   ├   clients
   └ ▸ scratch

 ● git repository with a commit — Enter picks it
 ↑↓ move   → open   ← close/up   Tab complete   Enter pick
```

- While the input is untouched, a "repositories in use" group leads the tree: up to eight
  repositories the core's live terminals have as their working directory — usually the one you
  wanted anyway. It is simply absent when the core cannot be reached.
- Typing filters live: the tree's root follows the deepest existing directory of what you typed
  and the rest of the last component filters its children, repositories first. `Tab` completes
  the longest common prefix and descends when one match is left.
- `↑`/`↓` (and `PgUp`/`PgDn`, `Home`/`End`) move, `→` expands a directory or enters it, `←`
  collapses or climbs to the parent, `Enter` on a `●` row picks it. `Esc` returns to the input; a
  second `Esc` cancels. `Ctrl+C` cancels at once — nothing is written.
- `●` is a repository with a commit — its branch and last-commit age sit at the right; `○` has no
  commits and cannot be picked. Hidden directories stay hidden unless the filter starts with `.`;
  `node_modules`, `target` and `.git` are always skipped.

The draft's runner block comes from a detector that looks past the inherited `PATH`: the login
shell's `PATH`, then well-known install dirs (`~/.local/bin`, every `~/.nvm/versions/node/*/bin`
newest first, Volta, bun, cargo, npm-global, deno, mise and asdf shims, Homebrew and
`/usr/local/bin`). Each runner is listed with its version and where it was found; a binary only
the fallback reached is marked `needs PATH`, and accepting the cast writes
`[runner_env.<runner>] PATH = "<binary dir>:<inherited PATH>"` in the state root's `config.toml`
— the lines are shown before they are written, a missing file is born `0600`, and a PATH you set
yourself is never rewritten (a warning instead when it still cannot reach the binary).

`backlot runners` prints the same table whenever a role says "not found" — runner, status,
version, path, needs PATH — read-only, with no core needed. `backlot runners --json` prints the
detection as JSON, with the models and efforts backlot knows per runner — it is what the New
Studio dialog's cast table cycles. `backlot repos --json [--from <dir>]` lists a directory's
entries the same way — each classified `git_ready`, `git_empty` or `dir`, with the studio that
uses it — and is how the dialog browses a remote machine's repositories.

The page only ever appears when there is no studio. `backlot` waits for its first `backlot`
read before drawing it (two seconds at most), so a client with studios never flashes the page
on its way in; with studios it opens the Hollywood bar instead (see below). Once the page has
gone it never comes back over a studio: the `new_workspace` action — the sidebar's `new` row,
the workspace keybind and the mobile switcher's target — opens the new-studio dialog directly
when a studio exists, and shows the page only when none does. `Esc` hides the page for the
session.

`hc server start` brings a core up first, because the studio commands need one and do not start
it themselves. If `backlot` (the TUI) is already open, a core is already running and you can skip
that line.

The command-line path is unchanged:

```sh
backlot studio new --studio acme-shop --title "Shop" --repo /home/user/acme-shop --cast /home/user/cast.toml
backlot stage start
```

`backlot studio new` records `studio.created` in the studio's ledger and prints it back.
`--studio` names the studio — `[a-z0-9_-]{1,16}` — and every later command takes the same
`--studio` flag or reads `BACKLOT_STUDIO`. `--repo` must be an absolute path. `--title` is a
display name. With `--interactive` instead, whatever the flags leave out is asked for on the
terminal with the same defaults as the native dialog. `--cast -` reads the cast file from
stdin — how the client's dialog hands a generated cast to a remote `studio new` without leaving
a file on that machine. `--mode standard|experimental` overrides the cast file's `mode` — an
invalid value is a usage error.

`backlot stage start` starts `backlot-stage`, a terminal on the same core that runs the
scheduling loop. It is idempotent — a second `stage start` says it is already running. The stage
then reads the ledger and starts the producer and the director as `bl-acme-shop-producer` and
`bl-acme-shop-director`; the writer joins when a plan is wanted, and an actor's terminal is named
for its task (`bl-acme-shop-t1`). Role terminals are ordinary `hc` terminals: each studio is its
own space in the client with its people as tabs, and `hc ls` lists them all.

On a remote core (`--remote <target>`) the stage terminal runs on **that host**, so `backlot` must
be installed there. `stage start` uses the host's own `backlot` from its `PATH`, with `--session`
alone — this machine's executable path, `--remote` and `BACKLOT_STATE_DIR` mean nothing there. If
the host has no `backlot`, the start fails and names the target and the fix: install it there with
`curl -fsSL https://github.com/followcat/backlot/releases/latest/download/install.sh | BACKLOT_SKIP_CORE=1 sh`,
or run `backlot` on that host.

The stage has no heartbeat of its own — `backlot stage watch` is the external watchdog:

```sh
backlot stage watch --target bl-acme-shop-producer --session blrun1213a
```

It polls the core's terminal list and the studio ledgers every few seconds and sends a
`<watch>` line into `--target` on three edges: the `backlot-stage` terminal dying (and coming
back), a role terminal disappearing while its ledger still wants it live, and a studio's open
attention growing. Each condition reports once per edge. Without `--target` the lines go to the
watch's own stderr; `--studio` narrows the ledgers to one studio.

Add `--restart` to have the watch keep the stage alive as well — it starts a dead
`backlot-stage` again (five attempts in ten minutes, an exponential 2→4→8→16→30 second backoff
that a stable minute resets, then a pause announced once), reconnects the core after an `hc`
restart with a 2→5→15→30 second backoff, and reports `stage restarted` or
`stage restart failed: <reason>` into `--target`. The watch without `--restart` only observes.

To keep that running without a terminal, let systemd own it:

```sh
backlot stage service --write
```

The command writes `backlot-stage.service` under `~/.config/systemd/user/` (or
`backlot-stage-<session>.service` with `--session <s>`) and prints the
`systemctl --user daemon-reload && systemctl --user enable --now <unit>` line to run; it never
calls `systemctl` itself. `--print` (the default) shows the unit instead of writing it, and a
working role cannot run the command. The unit runs `stage watch --restart` with the stage's
`PATH` and `BACKLOT_STATE_DIR`; it has not been exercised under a real systemd user manager.

Run `backlot` to open the UI. The client is always in studio mode: the sidebar's agents section
is the **cast** of the current studio — one two-line entry per person, live ones first — and
`Ctrl+B y` — prefix, then `y` — opens the studio panel on the right as a column
of bordered blocks, one per studio. Pressing the chord again while it is focused closes it;
pressing it while the panel is open but unfocused focuses it again. A herdr binding on `y` still
wins while the panel does not own the keys. With no studio yet the opening page covers the pane
area (`Esc` peeks underneath; the cast's `others` row still lists what other clients created).
The former plain multiplexer is `backlot --plain`.

A new tab (`Ctrl+B c`) or a split opens a **studio shell**: your own terminal, started in the
worktree of the focused person (an actor's `work/<task>`, the director's `work/integration`) or
in the studio's repository. Shells are listed under a `shells` group in the cast after the
people, the role keys reach them after the people, and the stage never touches them — messaging,
nudging, sampling, counting, reaping, relabelling all skip a `backlot.shell` terminal. A finished
studio keeps its shells for as long as it stays in the document.

Four direct keys move around without the prefix: `Alt+Up`/`Alt+Down` step to the previous or
next studio, `Alt+Left`/`Alt+Right` to the previous or next person of the current studio's cast —
live people first, then the shells. Their defaults are `keys.previous_studio` and friends in
backlot's config.

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
its screen has held still — it can wait a while. When several items are owed at once they arrive
together, as one line naming a `messages/digest-<seq>.md` to read. A stage notice whose situation
has moved on is dropped instead of sent — the ledger keeps the drop (`message.dropped`) and
`backlot status` counts it under `messages:` — but a `tell` is a person's words and always goes.
In the ledger you are the role `user`, so person-facing commands take `--as user`.

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
`→` line under the studio. When any delivery reported usage, a `usage:` line under the limits
sums the studio's tokens and cost (nothing shows while every counter is zero).

## What happens next

In the normal path the steps run in this order:

| Step | Who | What |
|---|---|---|
| plan | producer + writer | The producer records requirement v1 (`backlot req revise`), asks for a plan (`backlot plan request`); the writer joins, they agree, the writer publishes it (`backlot plan publish`). |
| dispatch | producer | `backlot dispatch` hands the agreed plan to the director; the writer leaves. |
| cast | director | `backlot task new` splits the plan into tasks — `--depends-on t1` makes a task wait for another; `backlot cast` puts a candidate on each — an actor terminal starts in its own worktree. A cast on a task with an open dependency is refused, and an accepted dependency that is not merged yet refuses with `merge t1 into integration first`; `backlot status` shows `waits for t1` on the task's line. `backlot cast --task <id> --suggest` first shows every candidate's factual record for the task's kind across all studios — counts and honest notices only, never a pick; `--json` prints the same view structured. |
| deliver | actor | The actor works in `work/<task>` on branch `backlot/<studio>/<task>`, commits, and records the delivery (`backlot deliver` with a commit range resolved against that branch — empty ranges and off-branch tips refuse — a report file and the checks it reports). A runner that knows its own figures can also report `--tokens-in`, `--tokens-out`, `--cache-read-tokens` and `--cost-usd`. The stage then runs the cast's declared `[[checks]]` on the delivery itself, each recorded `task.checked` with its log. |
| review | director | The director reviews the delivery against the acceptance list and records a verdict (`backlot review`): `accepted`, `partially-accepted`, `changes-requested` or `blocked`. An accepting verdict waits for every declared check's stage record — only you may accept over pending ones — and names each failing check in the conclusion. |
| rework | actor | A `changes-requested` review goes back to the actor as a `[backlot]` message; it fixes and delivers again. The rework cap counts the rounds. |
| report | director → producer → you | `backlot report` at agreed nodes; the producer reports to you in its own terminal. |

The light path skips the ceremony: for a small job the producer can create and cast a task itself
and review the delivery — the same commands, recorded the same way.

### What the panel and the cast show

The sidebar's cast lists the current studio's people, live first, two lines each: mark, name and
task, the runner at the right edge; the terminal's title under it with a short model at the
right. An open attention replaces the whole second line, model included, with `! <attention>`;
a failed turn says `last turn failed` the same way. The focused person's pair of rows is highlighted with a `▌` marker. A
trailing `others` row expands to the agents that belong to no studio.

Expanded, the dock draws one bordered block per studio — the id in the frame, a header row
with the health glyph (`●` live, `◌` stage stopped, `×` offline or ledger error, `✓`
finished), `r<version> · <done>/<total>` and `!<waiting>` — then the owed-step line in role
names (`→ deliver t1 · actor-a`), then the people: mark, name with the actor's task and
priority flag, a `USER` badge on your own role, the `dir` tag on a director-owned prompt,
and the state word at the right. A user-owned prompt adds a `! <attention>  [y/d/p]` cue
line; a director-owned one gets no cue. Below the live people, task rows list the work no
actor carries — uncast, dependency-gated, paused, at cap or blocked, and accepted — and
six or more accepted tasks fold into one `✓ <n> accepted` line. `Space` opens a three-line
drawer under a person or task, or folds a studio block on its header. The marks:

| Mark | Meaning |
|---|---|
| spinning glyph | `working` — the role's screen shows its runner's working marker |
| `○` | `waiting` — live, not known to be working or blocked (an unmatched screen reads as waiting). A wanted role a `[limits]` cap holds back shows the same mark; the cast's second line says `waiting for a <runner> slot` |
| `×` | `blocked` — the role's screen shows a permission prompt |
| `!` | `lost` — the role's terminal vanished without a deliberate close |
| `✓` | `closed` — the stage released the role on purpose; it stays listed and can be reopened |

The title line counts the studios (`lot · 2`). `stage not running` means no live
`backlot-stage` terminal — the panel and the cast keep the last document. The collapsed
16-column strip shows each studio's health, a short `id·machine` label and progress or
attention, and one row per active or blocked role.

Panel keys while it is focused: `j`/`k` or the arrows move the selection, `Enter` focuses a
live person's terminal (a closed person reopens a resumable or fixed seat through the action
tracker; `o` does the same), `Space` opens a drawer or folds a studio, `v` opens the Task &
Delivery Sheet for the selected studio or task, `p` peeks, `y`/`d` answer a user-owned prompt,
`+`/`-` raise and lower a task's priority, `r`/`f` start the stage and finish the studio,
`c` switches collapsed and expanded, `q` closes, `?` opens help, `Esc` unfocuses the dock
beside the panes and steps back over an overlay — to the strip when it fits, else closed.
While the panel is focused it owns the keys — `Ctrl+B y` again closes it — except the four
`Alt+arrow` studio keys, which still switch studios and roles.

## Hollywood above the studios

Hollywood is the overseer: one agent above all studios on a core — see
[hollywood.md](hollywood.md) for its model and rules. In the TUI it is a bar floating over
everything: a collapsed strip at the top right, the film-strip band dropped from the top on
`Alt+Y`, and hidden. It floats — nothing resizes when it moves.

**The bar exists with or without an agent.** In studio mode it always draws: without a
`[hollywood]` section it shows the studios, the new-studio frame and one line naming
`backlot hollywood setup`, and the agent's parts — the title's role state, the headline, the
cards and the `tell hollywood` line — need the section. Under `backlot --plain` there is no bar
at all, and `Alt+Y` reaches the focused program.

**It starts expanded** on a client that opens with at least one studio, so the studios are the
first thing you see. A client that opens on the opening page keeps it collapsed, and the first
studio to arrive later leaves it collapsed. `Esc` collapses it, `x` hides it, and focusing a
studio from it collapses it again.

The expanded strip, top to bottom:

- the sprocket rows and the `H O L L Y W O O D` title, with the agent's square, its role state
  and runner / model / effort, and the right-hand count of studios with what waits on you;
- one frame per studio: the id in the top edge, `■ <state>` and a `!n` chip when something
  waits, the role on set, what it is doing (a broken ledger in red), and one `◆` per cast
  member — green working, peach with an attention, red failed or lost, dim idle — with
  `<n> cast · r<version>` on the right;
- the dashed **new-studio frame** last: `+  new studio`, repository and cast, and the `n` key;
- below them, with an agent: the newest headline, one row per open card with `apply`,
  `cancel` and `show`, and the `tell hollywood` line. Without one: the setup line.

Keys in the expanded bar: `←`/`→` (or `h`/`l`) move between the frames, the new-studio frame
included; `↓`/`↑` (or `j`/`k`) leave the frames for the cards and the input line and come back;
`Enter` opens the selected frame (a studio → its producer, the new-studio frame → the dialog,
the title → `bl-hollywood`); `n` opens the new-studio dialog from anywhere in the bar, which
collapses first so the overlay is never under it. `a` applies and `c` cancels the selected card,
`s` is Show me, `i` edits the `tell hollywood` line, `x` hides, `Esc` collapses. The frames
scroll sideways when there are more than fit (a `‹` or `›` at the band edge says so) and the
wheel does the same under the pointer; the card chips and the input line take the mouse, and a
click on the new-studio frame opens the dialog while other rows only select. On a narrow screen
the frames sit two per row — one under 40 columns — and the band
scrolls vertically.

Set the section up by hand or let the command write it:

```sh
backlot hollywood setup --runner claude --model claude-sonnet-5-5 --effort high
```

`setup` refuses a runner that is not supported or not installed before it touches the file, and
keeps every other line as it was. A running stage starts `bl-hollywood` on its next tick; a
running client picks the table up at its next start. Until a section exists, the expanded studio
panel's last row names this command too.

`Alt+Y` cycles hidden → strip → band → strip (rebind with `[hollywood] key = "prefix+h"`). Show
me on a `studio-new` card opens the draft's cast and brief read-only (`backlot card show <id>`
prints the same); on any other card it focuses the studio.

The bar only ever shows what the `backlot` document says and only ever acts through `backlot`
commands — `backlot card apply c3` from the bar is the same command you would type.

## Studios on several machines

`hc machine add` attaches another machine's core to this client — the sidebar already lists its
terminals. Each machine serves its own `backlot` document, and the client reads them all: the
panel, the cast and the Hollywood frames show every machine's studios together, this core's
first, and the same studio id on two machines is two studios.

While studios come from one machine nothing looks different. Once they span machines, a studio's
header and its Hollywood frame title carry `· <machine>` so `acme-shop · host-b` is never
confused with the local `acme-shop`. A machine that does not serve the document — no `backlot`
installed, a foreign schema, a core that does not answer — simply contributes nothing: its slot
stays blank, never an error.

Acting on a studio runs on the machine that owns it. A local studio's keys spawn `backlot` here
exactly as before; a remote studio's keys send `terminal.run` to that machine's core, where the
client locates `backlot` once (`command -v`, then the login shell, then `~/.local/bin`) and
reuses the path. Enter on a remote studio's frame or on one of its people focuses that terminal
on its machine, like a sidebar row does. A machine without `backlot` installed says so in a
notice instead of running anything.

Hollywood itself — the headline, the cards and the `tell hollywood` line — stays this core's;
only the frame list spans machines. The New Studio dialog's first step picks the machine:
`Local` or any attached machine — the create, the dry run and the `stage start` all run there
over `terminal.run`, and the generated cast travels on stdin (`studio new --cast -`).

Each machine row carries a small second line naming its `backlot` version — `hc <v>` for the
endpoint's core too when it fits. A running stage publishes its own version in the document,
so its machine needs no probe; a machine without a stage (or an older one) is asked once —
`command -v`, a login shell, `~/.local/bin` — and the answer is cached for six hours, on
reconnect and after an install. `backlot <v>` is what is there, `↑<latest>` when it is behind
the newest release, `no backlot` when there is none.

Right-click a machine row (or find it under `Ctrl+B ?`'s `machines` group) for its own menu —
only what is valid there: `Install backlot`, `Upgrade backlot to v<latest>`,
`Start`/`Restart the stage`, `New studio here`, and the endpoint ops. Install and upgrade open
a terminal on that machine running the official installer with `BACKLOT_SKIP_CORE=1` — hc is
never touched — and a `backlot` managed by mise, Homebrew, Nix, cargo or a dev build is refused
first with the channel's own upgrade command. An upgraded machine whose stage ran is restarted
afterwards: the ledger replays, so no studio state is lost. The same restart sits on the menu,
and a `stage not running` row in the dock starts the stage on Enter or click.

## Permission prompts

A role whose screen sits on a permission prompt for two seconds raises a `permission prompt`
attention — one per blocked episode — and the visible screen is saved under `attention/<seq>.txt`
in the state directory.

- An **actor's** prompt goes to the director, who may approve it when the command stays inside
  that actor's own work area.
- A prompt touching **real sessions, production, accounts or global settings**, or destructive
  outside the work area, is yours — and so is any prompt on a role that is not an actor (the
  producer, the director, the writer).
- A prompt reaching for **the role-identity variables or environment-clearing flags**
  — `BACKLOT_*`, `env -i`, `--ignore-environment` — is yours outright: the stage marks it
  `needs the user`, the director gets no notice and its `backlot approve` refuses.

Answer with `backlot approve`, naming the role's key — `producer`, `director`, `writer` or
`actor-<task>-c<n>` (the panel and `backlot status` show it):

```sh
backlot approve --person actor-t1-c1 --decision allow --studio acme-shop --as user
```

`--decision deny` answers no. The stage sends the runner's approval key only while the prompt your
attention saved is still the one on screen, never without your recorded decision. If the screen
moved to a different prompt before the key went out — an earlier prompt was answered in the
terminal itself and the next was already up — nothing is sent: the stale attention closes
`prompt changed before the decision was sent` and the prompt on show raises its own attention, so
you approve what is actually there. A prompt answered in the terminal itself closes
`prompt answered elsewhere` a couple of seconds later, so the next prompt raises fresh either way.
A denied role gets a notice telling it the command did not run.

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
| `stalled` | A live role's screen did not change for `stall_limit_s` while it counts as working — an actor whose task is `running` and whose work reads working or waiting (`blocked` never counts — a prompt has its own attention), or a fixed role sampled `working` — or, for a hook-wired Claude, its last signal reported working and went quiet for the limit. An idle one is normal and never stalls. | Look at the saved screen file — the detail says `screen unchanged` or `no hook signal for <n>s`. The role's reviewer is told; a producer's stall also tells a live director, and `backlot status` leads with a `producer alert:` line. A changed screen or a newer signal clears the attention. |
| `cannot resume` | A lost role could not be brought back: no recorded session id, or three losses inside ten minutes. | For an actor, its reviewer is told to abandon or recast the task. The writer's and director's losses notice the producer; the producer's is on you — a live director is told only so it does not wait, and `backlot status` shows a `producer alert:` line. There is no automatic way on for a fixed role; only you (or Hollywood) can intervene. |
| `session id not found` | A post-launch runner (`codex`, `devin`, `cursor`, `cmd`, `cline`) gave no session id within its discovery window — 60 seconds, 240 for `cmd`, whose session file lands late. | The role still runs and the stage keeps looking for up to ten minutes; a role past the default window but inside its runner's logs a warning only. If it never resolves, the role cannot be resumed after a restart — finish its work in this run. |
| `launch refused` | `terminal.run` refused the role's launch — a taken terminal name or an argv the core rejects. | The stage retries each tick; the detail names the refusal. Check the runner exists and the terminal name is free. |
| `not a git repository` | The studio's `--repo` path is not a git work tree — no director or actor starts. | Point the studio at a real work tree, or make the repo one; the stage retries each tick. |
| `work area failed` | A git step failed while preparing `work/` — the detail is git's first error line. | Fix what git complained about; the stage retries each tick. |
| `owed` | A role was nudged about a step it owes — the detail names the step — and `escalate_after_s` later the step is still owed and the role is still idle. | Do the step yourself or talk to the role with `backlot tell <role> --file <file>`; the producer is told for other roles. It clears once the nudged step is done — when none of the role's owed steps carries a delivered nudge — or when the role is no longer wanted; a newer step then earns its own nudge and its own `owed` attention. |
| `over budget` | A running or delivered task passed its `task_budget_s` — the detail names the task, the budget and the time used. | The director and the producer are both told; nothing was stopped or paused, so let it run, pause it, or recast. One raise per task ever — a recast does not re-raise — and it clears `done` when the task is terminal. |
| `runner not supported` | The cast's `runner` has no adapter — a typo, or a runner other than `claude`, `codex`, `devin`, `opencode`, `cursor`, `cmd`, `cline`. | The role never starts. `backlot correct` or a new studio is the way out — see below. |
| `permission profile not supported` | The cast's `permission_profile` is not `standard` or `worktree`. | Same: the role never starts; fix the cast for the next studio. |
| `model not supported` | A cline role's model names no provider the adapter knows — only `cline-pass/<model>` ids were measured, and there is no per-role config for a provider. | The role never starts; give the cast a `cline-pass/…` model. |
| `effort not supported` | A cline role's `effort` is not one `--thinking` takes (`none`, `low`, `medium`, `high`, `xhigh`). | Same: the role never starts; fix the cast's effort. |
| `config malformed` | `<state base>/config.toml` did not parse, or a `[runner_env.<runner>]` table held a non-string — the role gets no runner env and does not start. | Fix the file's TOML; the stage retries each tick. Only the error's line and column are reported, never the file's contents. |
| `cursor prep failed` | A cursor role's per-session setup failed — its directories, `cli-config.json` or the `mcp-disabled.json` list could not be written, or the global `mcp.json` did not parse. | The role does not start; the detail names what failed. Fix it and the start retries each tick. |
| `cursor dir was open` | A cursor role's `roles/<role>/cursor/` — the boundary that keeps the agent's `authInfo`-holding files unreachable — carried a group or other permission bit. The stage found it at launch prep or at session discovery and set it back to 700. | Informational only — the detail names the path suffix. Check what set the mode (a loose umask, an outside chmod); a repeat drift raises it again. |
| `inherited XDG_CONFIG_HOME` | The stage's own environment carries `XDG_CONFIG_HOME` — cursor's `auth.json` moves with it, so a cursor role would start logged out. | The role does not start. Unset `XDG_CONFIG_HOME` where the stage runs and it retries each tick. |
| `cmd prep failed` | A cmd role's per-session setup failed — its workdir `.commandcode/settings.json` could not be written, or the global `mcp.json` did not parse. | The role does not start; the detail names what failed. Fix it and the start retries each tick. |
| `cline prep failed` | A cline role's per-session setup failed — `<cwd>/.cline/hooks/PreToolUse.sh`, the MCP mask file or the worktree's `info/exclude` could not be written. Without the hook the role would run every tool unguarded, so nothing starts. | The role does not start; the detail names what failed. Fix it and the start retries each tick. |
| `rate limited` | Limit text was observed — in the `error` status's message or in the fresh tail of a non-working screen, including a reporting core reading `done`, `idle` or `waiting` — counted separately from `stalled` and rework. A `working` screen is never matched. | Usually nothing: the stage waits the parsed reset out and sends `continue`, up to three auto-retries. A successful resume resolves `resumed`; an episode that outlives the budget re-raises `needs the user` — answer it by hand. A **weekly or monthly** limit is different: it raises `needs the user` at once (no `continue` can shorten it), and the same banner never re-raises while it holds. |
| `connection error` | A non-working screen — or an `error` status's message — shows `Connection error`, as Devin does when it stops until someone writes to it. A rate limit on the same sample takes precedence. | Usually nothing: the stage sends `continue` after 15 seconds, up to three times, then re-raises `needs the user`. The banner disappearing clears it `resumed`. |
| `agent error` | The role's terminal reports `error` with no limit text in its message or the screen's fresh tail and no connection-error line — a plain agent error, not a limit. | The stage sends nothing: an actor's is the director's to retry, reword or recast; anyone else's is yours. It clears `status recovered` when the status leaves `error`, `rate limit text observed` if a limit banner shows, or `connection error observed` if the connection-error pass claimed it. |

A stale open attention — one whose condition no longer holds, such as a stage-restart leftover —
closes by hand:

```sh
backlot attention clear <seq> --studio acme-shop --as user
```

The seq is the attention's ledger seq from `backlot status`. Clearing one that is not open — or a
seq that was never an attention — is refused. The user and the director may clear; the record
names who did.

The ledger is append-only — a `correction` event changes the view, not the past. To repair a
recorded fact use `backlot correct --event <seq> --reason <r> --set <key=value> --as user`.

## After an hc restart

An hc restart ends every terminal, including the stage — the core never re-runs the original
commands. Start the stage again:

```sh
backlot stage start
```

Or let `backlot stage watch --restart` do it: a running watch reconnects to the returning core
and starts the stage again as soon as it notices it is down, under the same bounded budget
described above. `backlot stage service --write` installs that watch as a systemd `--user` unit
so a reboot or an `hc` restart needs no one at the keyboard.

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

Once the merge is done, `backlot studio prune` reaps what the studio left behind:

```sh
# Read-only plan for one finished studio.
backlot studio prune acme-shop --dry-run

# Remove clean worktrees and branches git considers merged.
backlot studio prune acme-shop

# Every finished studio at once.
backlot studio prune --all-finished

# Take a dirty worktree and an unmerged branch as well.
backlot studio prune acme-shop --force
```

A live studio is refused (exit 2) — prune never touches worktrees that may still be in use.
Without `--force`, a worktree with uncommitted changes is kept and reported, and a
`backlot/<studio>/*` branch is deleted only through `git branch -d`; a branch git will not
delete is kept with git's reason, so unmerged integration work is never lost. `--force` uses
`git worktree remove --force` and `git branch -D`, still only under that studio's `work/` and
`backlot/<studio>/` namespace. `--dry-run` changes nothing and prints exactly what would be
removed, pruned, deleted or kept. The ledger, `reports/`, `messages/` and `attention/` stay:
`backlot records` and `backlot status` keep the full history. To remove that history too,
delete the studio's directory under `studios/` yourself.

The ledger itself is versioned — every event line carries `v: 1`. Logs written before
versioning read fine unversioned, but you can stamp them explicitly:

```sh
# See what would change, writing nothing.
backlot studio migrate acme-shop --dry-run

# Rewrite unversioned lines to the current schema (keeps events.jsonl.v0.bak).
backlot studio migrate acme-shop

# Every studio, plus Hollywood's own log when it exists.
backlot studio migrate --all
```

A log already current is a no-op; a line carrying a `v` newer than the binary knows is refused
with an "upgrade backlot" error rather than a cryptic one.

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
  messages/                  role-to-role messages, stage notice bodies, digest-<seq>.md merges
  attention/                 screens saved when attention was raised, with <seq>.signal
                             when the raise came from a hook's permission request
  prompts/                   the rendered role prompt each fresh start got
  roles/<role>/claude-settings.json   a Claude session's own hook wiring (see below)
  roles/<role>/signals.jsonl          the one-line-per-event record those hooks append
  roles/<role>/cursor/                a cursor role's isolated config, data and sandbox trees
  work/integration           the director's worktree, branch backlot/acme-shop/integration
  work/<task>                each task's actor worktree, branch backlot/acme-shop/<task>
```

A Claude role starts with `--settings roles/<role>/claude-settings.json`: one extra settings
layer for that session only, holding `hooks` and nothing else — no permissions, no env. The
hooks report each turn start, tool call, permission prompt and turn end into
`signals.jsonl`, and the stage reads that file first for what the role is doing — your
screen is the fallback for other runners and for any session that sent nothing. The file
is rewritten and the signals file emptied on every start and resume, so a moved binary
heals on the next launch; nothing ever writes to your own `settings.json` or any other
settings file — hooks from every layer just merge, and yours keep running too.

A cursor role keeps its config, data and sandbox under `roles/<role>/cursor/`; its
`config/cli-config.json` holds the account's `authInfo` once the agent has run — the studio
state directory is private, so do not share or publish it.

`config.toml` holds per-runner environment — profiles, proxy settings — as `[runner_env.<runner>]`
tables. Values reach the role terminal's environment only; they are never written to the ledger,
the summary or logs. A malformed file raises `config malformed`. The new-studio wizard writes a
runner's `PATH` here when the cast's binary lives outside the inherited `PATH` (shown before it is
written, an existing `PATH` never touched); `studio new`'s installed check counts it the way a
launch would.

`backlot records` reads every studio's ledger and summarizes each actor identity's deliveries —
the factual record, with no score. `--kind <task_kind>` keeps only one task kind's rows (and
answers plainly when nothing delivered on it). An actor whose deliveries reported usage shows
its token and cost sums after the counts. The same records feed `backlot cast --task <id>
--suggest`, which lays them beside the studio's `[[candidates]]` for the task's kind — the
director still casts with `--actor` and `--reason` itself.

## The panel's config

`$XDG_CONFIG_HOME/backlot/config.toml` — `~/.config/backlot/config.toml` by default:

```toml
[panel]
key = "y"      # the chord tail after the prefix

[dock]
# width = 36   # optional fixed width, clamped 28-48 — unset follows the
               # frame's tier (38 at 160+, 36 at 100+, 28 under)

[hollywood]
runner = "claude"          # required; `backlot hollywood setup` writes this table
model = "claude-sonnet-5-5" # required
effort = "high"            # the cast file's effort rule applies
# key = "alt+y"            # optional — the layer's toggle: a direct key or prefix+<key>
# default_cast = "/home/user/cast.toml"  # optional
```

A missing file means defaults; a malformed one means defaults plus a notice carrying only the
line and column. The panel's open/closed state is per client and in memory only — nothing is
saved.

With no `[hollywood]` the bar is still there in its no-agent form (studios, the new-studio
frame and the setup line), on `Alt+Y`, with no timer; `backlot --plain` has no bar at all and
the screen is the herdr screen. A `key` value that cannot be bound falls back to `alt+y` with a
notice naming the file, never echoing the value.
