# Running a studio

A studio is one requirement worked on by a small cast of agents: a producer you talk to, a
director who assigns the work, a writer who writes the plan and the acceptance list, and actors
who do the tasks. Every role is an ordinary `hc` terminal running an agent CLI. A ledger records
what actually happened — talking never moves a task, a `backlot` command does — and a panel
inside `backlot` shows who is doing what.

This guide walks one studio end to end. [limits.md](limits.md) says plainly what has and has not
been verified; read it before trusting a studio with real work.

## Reading backlot's own docs

These files ship inside the binary. `backlot guide` lists the topics — one per file in
`docs/studio/` — and `backlot guide <topic>` prints one whole; `--grep <word>` narrows either to
the matching lines, over every topic when no topic is named:

```sh
backlot guide
backlot guide guide
backlot guide runners --grep permission
```

The topics are `design`, `guide`, `hollywood`, `ledger`, `limits`, `panel`, `plan`, `progress`,
`requests`, `requirements`, `runners`, `server` and `testing`. A role is told to read them from
here, never to search the disk: `docs/studio/` is not in its work area, and a search only stops at
a permission prompt. Nothing is read from disk at run time — the text is compiled in — and the
command needs no studio, ledger or core.

## What you need

- **`hc` installed and a core running.** `backlot` (the TUI) starts a local core itself when
  nothing answers on the session socket; `hc server start` does it without the UI. The studio
  commands (`backlot studio`, `backlot stage`, `backlot status`) do not start a core — they fail
  with "cannot reach the hc core" until one is up.
- **A filesystem path.** Usually a git repository — one with zero commits is fine, the studio
  writes an empty initial commit at launch. A directory whose direct children are git
  repositories binds as a **workspace** — distinct repositories: linked worktrees of one
  repository count once. Work areas hold one worktree per member and commits
  land in the member they were made in — no synthetic parent repository is ever created. A
  plain directory binds too: `studio new --init` runs `git init` plus an empty initial commit
  in it (the interactive flows offer that by default), or `studio new --no-git` binds it
  degraded — every role shares the path with no worktrees, no integration branch and no merge
  checks.
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
max_actors = 4          # optional, default 4 — actors live at once
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
| `escalate_after_s` | Seconds after that nudge was delivered, or after the owner's last ledger write if that is later, during which the step may stay owed and the owner idle before an `owed` attention raises. Optional; default 600. Must be at least 1. |
| `task_budget_s` | Running-time budget per task in seconds, counted from the task's first actor `role.started` to its close, leaving out paused stretches — a cast a `[limits]` cap still holds has used nothing. A running or delivered task past it raises one `over budget` attention — the director and the producer are told; nothing is killed or paused. Optional; unset means no budget. Must be at least 1. |
| `[limits]` | Live-role cap per runner in this studio — `claude = 3` starts no fourth live role on `claude` until a slot frees. Fixed roles and actors count alike, a role being started counts, and a runner with no entry is unlimited. Held starts go producer, director, writer, then actors by task priority and seq, and the waiting role shows `waiting for a <runner> slot`. Per studio: two studios on one account each count only their own roles. A `0` is refused — a runner is limited by a real number or absent. The cap must seat its fixed roles first — the producer and the director stay live for the studio's whole run — so `studio new` refuses a cap under their count on the runner, or at their count while the writer or a candidate shares it: nobody else could ever sit. |
| `max_actors` | The studio-wide actor cap: how many actors play at once. Optional; default 4. Only actors count — the producer, the director and the writer never fill an actor slot — and an actor being started counts too. A wanted actor past the cap waits (`waiting for an actor slot`, separate from a `[limits]` runner wait) and starts the tick one actor closes. A candidate is a *kind* of actor: one candidate can be cast to several tasks at once, so several actors share its identity. Must be at least 1 — a studio with no actor slot can never cast. `backlot status` shows `actors <live>/<cap>`. |
| `runner` | The agent CLI that fills the role: `claude`, `codex`, `devin`, `opencode`, `cursor`, `cmd`, `cline` or `agy`. (`fake` is the test runner shipped for the suite — never put it in a real cast file.) |
| `model` | The model name, passed to the runner's own flag. A `cline` model must be a `cline-pass/<model>` id: the provider is taken from it, and any other id is refused. |
| `effort` | Optional. Thinking depth, passed to the runner's own flag where it has one (Claude `--effort` — `low`, `medium`, `high`, `xhigh` or `max`; Codex `model_reasoning_effort` — the same plus `ultra`; agy `--effort` — `low` through `max`; cline `--thinking` — `none`, `low`, `medium`, `high` or `xhigh`). `cursor` and `devin` carry the depth in the model id, `cmd`'s flag writes global config and `opencode`'s launch carries none, so all four omit it — a set `effort` on `cursor` or `cmd` is refused, on `devin` or `opencode` ignored. |
| `permission_profile` | `standard` or `worktree`. On Claude these map to `--permission-mode manual` and `acceptEdits`; the other runners accept the same two values. `worktree` is the right choice for actors working inside their own worktree. |
| `runner_version` | Optional. Recorded on the role's identity as the configuration that did the work; the stage fills it from the launch program's own `--version` (cursor's is `cursor-agent`, not the `cursor` IDE launcher) when you leave it out. |
| `mode` | Optional. `standard` is the default. `experimental` (R14) marks the studio an experimental production — `backlot studio new --mode` overrides it, the wizard and the New Studio dialog offer both, each role's prompt gains research-level guidance (hypotheses, competing variants, honest negatives), a task may be created with no recorded requirement for direct-to-task exploration, and `status` plus the dock/sheet flag it `[experimental]`/`exp`. |
| `check_env` | Optional list of stage environment names a check may see, valued from the stage's own environment. Names that could carry identity or credentials — `BACKLOT_*`, `HC_*`, proxies — are refused at `studio new`. |
| `[[checks]]` | One table per check the stage runs itself on every delivery: `name`, the `run` command (`sh -c` in a detached worktree at the delivery's tip, under a stripped environment), and `timeout_s` — past it the run is TERM'd, then KILL'd five seconds later. Results land as `task.checked`; an accepting review waits for them and names each failure in its conclusion. A single-repository feature — refused on a workspace or no-git studio at `studio new`. |
| `[push]` | Optional outward push policy (`policy = "recorded" | "any" | "none"`, `protected_branches`, `allow_remotes`, `allow_tags`, `allow_gh_pr`; overrides also via `--push-*` flags or `[push]` in user config). Under the default `recorded` mode, non-protected branch pushes to remotes recorded at `studio new` pass directly, while protected branches, tags, other remotes, and `gh pr create` propose a user card; force pushes and `core.*` overrides stay refused. |

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
be picked), **2 ID & Title** (the repository's name, validated live, plus an optional
**What should this studio build?** brief — multi-line, `alt+⏎` adds a line, `Ctrl+E` edits it in
`$EDITOR`; left empty the producer asks first, as before), **3 Cast** (the runners
the target machine's `backlot runners --json` found, cycled per role; the table opens on the
cast the machine's last successful `studio new` used when one was saved, `Ctrl+E` edits the
generated cast.toml in `$EDITOR`), **4 Check & Start** (`studio new --dry-run` on the target
machine — its summary card shows the brief's first line under `First ask` — the card disclosing
that the stage answers the agent's folder-trust prompt for the
picked repo itself — the `[stage] answer_trust` switch in the user config turns that off —
then `Enter` creates — over hc's `terminal.run` when the machine is remote, the brief written
there as an `--ask-file` temp — and
optionally starts the stage before the new producer's terminal takes focus). `Esc` steps back
and then closes; `Ctrl+C` cancels from any step. When a studio exists, the page is gone and the
studio is current.

A create that succeeds ends the dialog: the studio replaces its wizard and the focus moves to the
new producer. That holds when the `stage start` after it fails too — the studio exists, so the
failure is a notice and the dialog does not come back. Only a refused `studio new` keeps the
dialog on step 4 with its error and the draft intact, and a dry run never closes anything.

The repository list comes from `backlot repos --json` on the target machine — a breadcrumb, a
live filter, `→` to descend and `←` to ascend. Git repositories lead, workspaces over member
repositories follow, and plain `will git init` directories come last, so a `$HOME` listing opens
on real work; dot-directories (`.cache`, `.config`, …) stay hidden until the filter starts with
`.`. Every listed path can be picked: a git repository
shows its branch, one without commits says `0 commits · will create initial commit`, a
directory holding member repositories says `workspace · N repos · <names>` and a plain
directory says `directory · will git init`. A path another studio uses warns, and a remote
path is never checked against the local disk. Step 4's launch plan restates the shape —
`workspace over N repos: a, b`, `will run: git init && git commit --allow-empty -m "chore:
initial commit" in <path>`, or `will create initial commit in <path>` — and only there does a
plain-directory pick offer the `[ ] no git (reduced isolation)` toggle (`g`).

In a terminal, `backlot studio new --interactive` asks the same questions. Its repository
question is a full-screen picker when stdin and stdout are terminals (the plain
`repository:` line otherwise, so scripted input still works):

```
 New studio · repository
 path › ~/code/▏                               Tab completes

   ~/code
 ▶ ├   acme-shop  ● main · 2 h ago
   ├   backlot    ● main · 5 min ago
   ├ ▸ clients    ○ workspace · 2 repos
   └ ▸ scratch    ○ directory · will git init

 ● every directory binds a path — Enter picks it
 ↑↓ move   → open   ← close/up   Tab complete   Enter pick
```

- While the input is untouched, a "repositories in use" group leads the tree: up to eight
  repositories the core's live terminals have as their working directory — usually the one you
  wanted anyway. It is simply absent when the core cannot be reached.
- Typing filters live: the tree's root follows the deepest existing directory of what you typed
  and the rest of the last component filters its children, repositories first. `Tab` completes
  the longest common prefix and descends when one match is left.
- `↑`/`↓` (and `PgUp`/`PgDn`, `Home`/`End`) move, `→` expands a directory or enters it, `←`
  collapses or climbs to the parent, `Enter` picks the highlighted row — every readable
  directory binds a path. `Esc` returns to the input; a second `Esc` cancels. `Ctrl+C` cancels
  at once — nothing is written.
- `●` is a repository with a commit — its branch and last-commit age sit at the right. `○`
  marks the other bindable shapes: `0 commits · will create initial commit` on an empty
  repository, `workspace · N repos` on a directory holding member repositories, `directory ·
  will git init` on a plain one. Hidden directories stay hidden unless the filter starts with
  `.`; `node_modules`, `target` and `.git` are always skipped. Typing a path without a
  trailing `/` and pressing `Enter` picks it; the trailing slash keeps `Enter` on navigation.

The draft's runner block comes from a detector that looks past the inherited `PATH`: the login
shell's `PATH`, then well-known install dirs (`~/.local/bin`, every `~/.nvm/versions/node/*/bin`
newest first, Volta, bun, cargo, npm-global, deno, mise and asdf shims, Homebrew and
`/usr/local/bin`). Each runner is listed with its version and where it was found; a binary only
the fallback reached is marked `needs PATH`, and the dry run or the create writes
`[runner_env.<runner>] PATH = "<binary dir>:<inherited PATH>"` in the state root's `config.toml`
— the lines are shown before they are written, a missing file is born `0600`, and a PATH you set
yourself is never rewritten (a warning instead when it still cannot reach the binary).

`backlot runners` prints the same table whenever a role says "not found" — runner, status,
version, path, needs PATH — read-only, with no core needed. `backlot runners --json` prints the
detection as JSON, with the models and efforts backlot knows per runner — it is what the New
Studio dialog's cast table cycles. `backlot repos --json [--from <dir>] [--all]` lists a directory's
entries the same way — each classified `git_ready`, `git_empty` or `dir`, a `dir` carrying its
direct-child repository names as `members`, each entry naming the studio that uses it — and is
how the dialog browses a remote machine's repositories. Git repositories and workspaces lead the
list; dot-directories stay hidden unless `--all` asks for them (the dialog passes it and hides
them behind a `.`-prefixed filter).

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

Start with the requirement instead of asking for it — the first `tell` the stage delivers is the
brief:

```sh
backlot studio new --studio acme-shop --title "Shop" --repo /home/user/acme-shop --cast /home/user/cast.toml --ask-file /home/user/brief.md
```

`backlot studio new` records `studio.created` in the studio's ledger and prints it back.
`--studio` names the studio — `[a-z0-9_-]{1,16}` — and every later command takes the same
`--studio` flag or reads `BACKLOT_STUDIO`. The name resolves across sessions: when it is not
under the current session's state directory the command scans the sibling core directories, so
`backlot studio finish --studio aiflows` in a plain shell finds `aiflows` wherever it was made —
exactly one match wins, none keeps `no studio`, and several matches refuse with the core ids so
`--session` can pick one. `BACKLOT_STATE_DIR`, `--session` and `--remote` pin the directory and
skip the scan. `--repo` must be an absolute path — it names the
bound path, whatever its shape: a repository, a workspace directory holding member
repositories, or a plain directory. A plain directory needs the choice spelled out: `--init`
runs `git init` plus an empty initial commit in it at launch, `--no-git` binds it degraded —
shared path, no worktrees, no integration branch — and `--init`/`--no-git` are refused on
repositories and workspaces. `--title` is a display name. With `--interactive` instead,
whatever the flags leave out is asked for on the terminal with the same defaults as the native
dialog — its picker binds any directory and defaults a plain one to `git init`. `--cast -`
reads the cast file from stdin — how the client's dialog hands a generated cast to a remote
`studio new` without leaving a file on that machine. `--mode standard|experimental` overrides
the cast file's `mode` — an invalid value is a usage error. `--ask <text>` (or `--ask-file <f>`)
queues the person's first message to the new producer: right after `studio.created` it is
recorded as a `message.queued` — the same event `backlot tell producer --as user` writes, with the
user as its sender — so the stage delivers it to the producer's terminal as its first
`[backlot] user → producer: read <file>` line. An empty ask is refused and nothing is created;
`--dry-run` restates its first line and records nothing. This is how a studio can start with a
requirement instead of the producer's opening "What would you like us to build?".

`backlot stage start` starts `backlot-stage`, a terminal on the same core that runs the
scheduling loop. It is idempotent — a second `stage start` says it is already running. The stage
then reads the ledger and starts the producer and the director as `bl-acme-shop-producer` and
`bl-acme-shop-director`; the writer joins when a plan is wanted, and an actor's terminal is named
for its task (`bl-acme-shop-t1`). Role terminals are ordinary `hc` terminals: each studio is its
own space in the client with its people as tabs, and `hc ls` lists them all. The stage narrates
what it does as `stage: …` lines on its own stderr — a terminal starts, a card lands, a summary
publishes. A run of identical lines collapses into one with a `×N` count (`stage: summary
written ×42`), so the timeline stays readable instead of flooding.

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
For running a single machine-wide `backlot server` instance (including `adopt` from per-core state
and `server service`), see [server.md](server.md).

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

Four direct keys move around without the prefix: `Alt+Left`/`Alt+Right` step to the previous or
next studio, `Alt+Up`/`Alt+Down` to the previous or next person of the current studio's cast —
live people first, then the shells. Their defaults are `keys.previous_studio` and friends in
backlot's config.

Every studio command also accepts `--session <s>` and `--remote <t>` to reach another core, the
same flags the TUI takes.

## Talking to the producer

Focus the producer's terminal — its sidebar row or pane, or Enter on the studio's row in the
panel — and type, exactly like talking to the agent yourself. Tell it what you want built, in as
much detail as you would give a person.

A studio created with `--ask` (the wizard's build field) starts with that message already queued
as the producer's first `tell` — read it in the producer's terminal as `[backlot] user → producer:
read <file>`; the producer records it as the requirement instead of asking. A studio created
without one opens on the producer's own question, "What would you like us to build?".

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

For each working role, `backlot status` adds `(live) · working <dur> · "<progress>" <age> ago · <running> <dur>` (omitting missing parts); `backlot status --json` includes `progress` (`{ text, ts_ms }`) and live descendant `running` (`{ cmd, since_ms }`) per working role while leaving `doing` unchanged for older clients. Agents running inside a role terminal (`BACKLOT_ROLE` + `BACKLOT_STUDIO`, or `BACKLOT_ROLE=hollywood` onto `hollywood/events.jsonl`) can update their quoted `<progress>` note voluntarily at step boundaries — never required, never audited — via `backlot progress "running the parity suite"` (or `--file /home/user/note.md`, `--text "running the parity suite"`, or `--stdin`); calling `backlot progress "note"` outside a role environment fails with exit code 2 naming `BACKLOT_ROLE` / `BACKLOT_STUDIO`.

`backlot studio list` answers the question before that one — what studios exist at all. It reads
every core's state directory and prints one row per studio with its core (`core.json`'s name when
the core carries one, else the short core id, and `local` for an unbound state directory), ledger
state (`live`, `finished`, or `failed` when the log will not load), title and bound path, newest
activity first; `--json` prints the same rows structured. A pinned `BACKLOT_STATE_DIR`,
`--session` or `--remote` lists that one directory only, so a role terminal never sees other
cores. `--remote <host>` (`backlot --remote host-b studio list`) reads the studios on that
machine; `--help` spells both reads out. `--archived` reads the `archive/` directory instead — the
studios `studio delete` retired — with each entry's name, stamp, repo and last requirement title;
`studio restore <archived-name> [--name <new-name>]` brings one back as a finished studio.

```sh
backlot studio list
backlot studio list --json
backlot studio list --archived
backlot studio restore acme-shop-20260101T120000Z --name acme-shop-2
backlot --remote host-b studio list
```

The client shows the archive too: when `studio list --archived --json` on a machine is
non-empty, the left tree's finished group gains an `Archived (n) …` row. Enter opens the list —
each entry's name, stamp, repo and last requirement — and choosing one offers `Restore as
<name>`, an editable name defaulting to the archived studio id. Enter runs the restore with
`--dry-run` first and shows its plan; a second Enter copies the ledger back for real.

The `next:` lines are what the ledger still owes — one per outstanding step, naming the step, the
role that owes it and how long it has been owed (`next: dispatch — producer, owed 4m`). `user` as
the owner means the step is yours (for example `finish` once every task is terminal and reported).
While a step sits with an idle owner the stage nudges it once after `nudge_after_s` and raises an
`owed` attention `escalate_after_s` after the nudge was delivered — so a `next:` line that survives
for minutes is a real hold-up, not a refresh artifact. An actor's `deliver` on a running task is the
one step that gets a second, firmer nudge: when the actor answers the first nudge with only a
`backlot progress` line and goes back to its prompt, the stage nudges again after another
`nudge_after_s`, and a bare progress write does not delay the `owed` attention (any other ledger
write, a commit or a work-area change still does). The panel shows the first owed step as the
`→` line under the studio. When any delivery reported usage, a `usage:` line under the limits
sums the studio's tokens and cost (nothing shows while every counter is zero).

## What happens next

In the normal path the steps run in this order:

| Step | Who | What |
|---|---|---|
| plan | producer + writer | The producer records requirement v1 (`backlot req revise`), asks for a plan (`backlot plan request`); the writer joins, they agree, the writer publishes it (`backlot plan publish`). |
| dispatch | producer | `backlot dispatch` hands the agreed plan to the director; the writer leaves. |
| cast | director | `backlot task new` splits the plan into tasks — `--depends-on t1` makes a task wait for another; `backlot cast` puts a candidate on each — an actor terminal starts in its own worktree. A cast on a task with an open dependency is refused, and an accepted dependency that is not merged yet refuses with `merge t1 into integration first`; `backlot status` shows `waits for t1` on the task's line. `backlot cast --task <id> --suggest` first shows every candidate's factual record for the task's kind across all studios — counts and honest notices only, never a pick; `--json` prints the same view structured. |
| deliver | actor | The actor works in `work/<task>` on branch `backlot/<studio>/<task>`, commits, and records the delivery (`backlot deliver` with a commit range resolved against that branch — empty ranges and off-branch tips refuse — a report file and the checks it reports). On a workspace the actor works inside `work/<task>/<member>`; `deliver --commits` takes comma-separated ranges — a bare `a..b` resolves to the one member whose task branch holds the tip, `member:a..b` pins it, and the ledger stores `member:sha..sha`. | A runner that knows its own figures can also report `--tokens-in`, `--tokens-out`, `--cache-read-tokens` and `--cost-usd`. The stage then runs the cast's declared `[[checks]]` on the delivery itself, each recorded `task.checked` with its log. |
| review | director | The director reviews the delivery against the acceptance list and records a verdict (`backlot review`): `accepted`, `partially-accepted`, `changes-requested` or `blocked`. An accepting verdict waits for every declared check's stage record — only you may accept over pending ones — and names each failing check in the conclusion. |
| rework | actor | A `changes-requested` review goes back to the actor as a `[backlot]` message; it fixes and delivers again. The rework cap counts the rounds. |
| report | director → producer → you | `backlot report` at agreed nodes; the producer reports to you in its own terminal. |

The light path skips the ceremony: for a small job the producer can create and cast a task itself
and review the delivery — the same commands, recorded the same way.

### What the panel and the cast show

The sidebar's cast lists the current studio's people, live first, a first row each: mark,
name and task, the runner at the right edge. The rest wraps over as many rows as it needs
(up to four per person) — the full state text and, on a row of its own, the full model with
its effort. An open attention wraps its whole state text (`needs you · …`, `asks you · …`)
and takes the model's row away; a failed turn says `last turn failed` the same way. When the
studio's people do not all fit, only the focused person keeps the full height and the others
fall back to two rows. The focused person's rows are highlighted with a `▌` marker. A
trailing `others` row expands to the agents that belong to no studio.

Expanded, the dock draws one bordered block per studio — the id in the frame, a header row
with the health glyph (`●` live, `◌` stage stopped, `×` offline or ledger error, `✓`
finished), `r<version> · <done>/<total>` and `!<waiting>` — then the owed-step line in role
names (`→ deliver t1 · actor-a`), then the people split into the producer, director, writer and
actor groups by a thin `├──┤` rule (an empty group draws none): role badge, the actor's
task and priority flag, a `USER` badge on your own role, the `dir` tag on a director-owned
prompt, and the state word at the right. A user-owned prompt adds a `! <attention>  [y/d/p]` cue
line; a director-owned one gets no cue. When a working role has neither a cue nor a failure line,
a dim detail line underneath shows `working <dur> · "<progress>" <age> ago · <running> <dur>` (shortening
`<progress>` inside quotes with `…`, then dropping `<running>`, on narrower panels). Below the live people, task rows list the work no
actor carries — uncast, dependency-gated, paused, at cap or blocked, and accepted — and
six or more accepted tasks fold into one `✓ <n> accepted` line. `Space` opens a three-line
drawer under a person or task, or folds a studio block on its header. Roles read as one-cell
badges — `Ⓟ producer Ⓓ director Ⓦ writer Ⓐ actor` — coloured from the theme; a terminal set to
draw East-Asian-ambiguous characters wide will shift the cells after them. The marks:

| Mark | Meaning |
|---|---|
| spinning glyph | `working` — the role's screen shows its runner's working marker |
| `○` | `waiting` — live, not known to be working or blocked (an unmatched screen reads as waiting); a role that has sat at its prompt longer than 60 s reads `idle <n>m` instead. A wanted role a `[limits]` cap holds back shows the same mark; the cast's second line says `waiting for a <runner> slot` (or `waiting for an actor slot` past `max_actors`) |
| `×` | `blocked` — the role's screen shows a permission prompt |
| `!` | `lost` — the role's terminal vanished without a deliberate close |
| `✓` | `closed` — the stage released the role on purpose; it stays listed and can be reopened |

The title line counts the studios (`lot · 2`). `stage stopped` means no live
`backlot-stage` terminal — the panel and the cast keep the last document. The collapsed
16-column strip shows each studio's health, a short `id·machine` label and progress or
attention, and one row per active or blocked role.

Panel keys while it is focused: `j`/`k` or the arrows move the selection, `Enter` focuses a
live person's terminal (a closed person reopens a resumable or fixed seat through the action
tracker; `o` does the same), `Space` opens a drawer or folds a studio, `v` opens the Task &
Delivery Sheet for the selected studio or task, `p` peeks, `y`/`d` answer a user-owned prompt,
`+`/`-` raise and lower a task's priority, `s`/`f` start the stage and finish the studio,
`c` switches collapsed and expanded, `q` closes, `?` opens help, `Esc` unfocuses the dock
beside the panes and closes it outright over an overlay.
While the panel is focused it owns the keys — `Ctrl+B y` again closes it — except the four
`Alt+arrow` studio keys, which still switch studios and roles.

## Hollywood above the studios

Hollywood is the overseer: one agent above all studios on a core — see
[hollywood.md](hollywood.md) for its model and rules. In the TUI it is a bar floating over
everything: a collapsed strip at the top right, the film-strip band dropped from the top on
`Alt+Y`, and hidden. It floats — nothing resizes when it moves.

**The bar exists with or without an agent.** In studio mode it always draws: without a
`[hollywood]` section it shows the studios, the new-studio frame and the `⏎ hollywood setup`
line — Enter or a click opens the in-TUI setup dialog — and the agent's parts — the title's
role state, the headline, the cards and the `tell hollywood` line — need the section. Under
`backlot --plain` there is no bar at all, and `Alt+Y` reaches the focused program.

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
  `cancel` and `show`, the recent conversation (the newest turn and the one before it, older
  lines folded into `… <n> earlier · ↵ read`) and the `tell hollywood` line. A card Hollywood
  proposes draws under the person's line as its own reply; the newest Hollywood line wraps to a
  few rows so its end is never cut off. Without one: the `⏎ hollywood setup` line — the setup
  dialog's button.

Keys in the expanded bar: `←`/`→` (or `h`/`l`) move between the frames, the new-studio frame
included; `↓`/`↑` (or `j`/`k`) leave the frames for the cards and the input line and come back;
`Enter` opens the selected frame (a studio → its producer, the new-studio frame → the dialog,
the title → `bl-hollywood`); `n` opens the new-studio dialog from anywhere in the bar, which
collapses first so the overlay is never under it. `a` applies and `x` cancels the selected card,
`s` is Show me, `i` edits the `tell hollywood` line, `c` reopens the setup dialog prefilled,
`x` hides, `Esc` collapses. The frames
scroll sideways when there are more than fit (a `‹` or `›` at the band edge says so) and the
wheel does the same under the pointer; the card chips and the input line take the mouse, and a
click on the new-studio frame opens the dialog while other rows only select. On a narrow screen
the frames sit two per row — one under 40 columns — and the band
scrolls vertically.

Set the section up in the TUI — the setup line's `⏎ hollywood setup`, the dock's last row, or
the settings overseer row open the dialog; `c` on a configured bar reopens it prefilled — or
let the command write it:

```sh
backlot hollywood setup --runner claude --model claude-sonnet-5-5 --effort high
```

`setup` refuses a runner that is not supported or not installed — a binary found only past the
inherited `PATH` earns its `[runner_env]` write here first — before it touches the file, and
keeps every other line as it was. A running stage starts `bl-hollywood` on its next tick; a
running client picks the table up at its next start. Until a section exists, the expanded studio
panel's last row opens the same setup dialog.

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
confused with the local `acme-shop`.

The **left machines tree** lists every machine's studios even when its document cannot answer.
Drawing the tree is only half of it: `Ctrl+B t` moves the keyboard into it (opening the sidebar
if it is collapsed), `↑`/`↓` move the highlight, `Enter` runs the row's click action — a studio's
space, `stage down` start, the finished fold, a finished studio's menu — and `m` opens the row's
menu; `Esc` returns the keys to the terminal. The same rows are in `Ctrl+B ?`'s `machines` group.
A machine whose stage is down — or that published no document — is asked for
`backlot studio list --json` (locally, or over `terminal.run` for a remote), and the answer is
cached and refreshed when the stage state changes or every 30 s at most. Its studios draw dim
under the machine row, tagged `stage down`; Enter or a click on one starts that machine's stage.
Each machine also carries a folded `finished (n)` row under its live studios — Enter or Space
unfolds it (the machine menu's `Unfold finished studios (n)` does the same from the keyboard),
and a finished row opens the existing finished-studio menu (reopen producer, sheet, requirement,
records, delete).
A machine that cannot be reached at all shows one `unreachable` line, never an empty tree. Rows
never drop out of a short sidebar: the section scrolls to the machine row of the studio the
selection landed on, so `Alt+Right` into a studio deeper down brings its machine into view instead
of hiding it. A machine that does not serve the document — no `backlot` installed, a foreign
schema, a core that does not answer — simply contributes nothing beyond that line: its slot stays
blank, never an error.

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
`Start the stage` when it is down, `Restart the stage`, `Stop the stage…` and `Stage log` while
it runs, `Unfold finished studios (n)`, `New studio here`, and the endpoint ops. The launcher's
`machines` group carries the same rows per machine — `Stage log · <machine>`,
`Unfold finished studios (n) · <machine>` (or `Fold …`), and `Machine menu · <machine>`, which
opens this very menu — so the tree's surfaces are all reachable from the keyboard and filter by
machine name (`/host-b`). `Stage log` opens the stage's own terminal as
the observe-only peek: its output is readable, and no key reaches the stage process from there.
Starting the stage from any entry keeps the focus where it was; from the opening page the client
lands on a studio's view, or on the lot when there is none, and never on the stage tab. When the
stage's terminal does hold the focus its frame is read-only, with `stage log — read only` on the
bar, so typed keys and a stray `Ctrl+C` cannot reach it. The stage serves its whole machine, so its stop sits
here rather than in any one studio's menu: every studio on that machine stops advancing until
the stage starts again, while running roles and the ledger keep going. Install and upgrade open
a terminal on that machine running the official installer with `BACKLOT_SKIP_CORE=1` — hc is
never touched — and a `backlot` managed by mise, Homebrew, Nix, cargo or a dev build is refused
first with the channel's own upgrade command. An upgraded machine whose stage ran is restarted
afterwards: the ledger replays, so no studio state is lost. The same restart sits on the menu,
and a `stage stopped` row in the dock starts the stage on `s`, Enter or click.

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
- An actor's prompt the **director cannot answer** — it is rate-limited, stalled or gone — is
  routed to you after a one-minute grace (see below).

Read-only commands do not prompt at all: `git status`, `git log`, `git show`, `git diff`,
`git branch --list`, `git branch --show-current`, `ls`, `rg`, `wc` and `head` are pre-allowed for
every role on every runner that has an allow list (`runners.md` says which). One plain command per
shell call: a `cd x && git log`, a pipe or `git -C` does not match the rule and prompts.

**A prompt the director cannot answer reaches you with the reason.** An actor's prompt is the
director's to answer, but a director that is rate-limited, stalled or closed cannot. Past a
one-minute grace the stage clears the director-owned attention `routed to the user`, raises a fresh
`needs the user` one, and its detail says why and how long — *the director is rate-limited until
12:10; waiting 14m* — with the same screen saved under `attention/<seq>.txt`. From then the prompt
is yours: the panel and Hollywood's `prompt` notice show it, and the director's `backlot approve`
refuses like any user-owned prompt.

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

## Restarting a stuck role

`close` only releases a held actor; `reopen` needs a closed seat; `recast` changes the identity.
To reload a producer, director, writer or actor **in place** — same identity — use `restart`:

```sh
backlot restart --person director --reason "stuck at a prompt" --studio acme-shop --as user
backlot restart --person producer --fresh --studio acme-shop --as user
```

The stage carries it out on its next tick: the role's terminal is stopped deliberately
(`role.closed`, reason `restarted` — never counted as a loss) and launched again under the same
identity and the same `bl-<studio>-<role>` terminal name. It **resumes the recorded session** when
the runner supports resume by id and a session id was recorded; `--fresh` starts a new session with
the role's prompt instead. The ledger records the role as `restarted`, which the panel and
Hollywood read.

`restart` is the user's or the director's. It refuses a closed person (`closed — use reopen`) and a
finished studio (`the studio is finished — reopen or restore it first`). The three fixed seats —
producer, director, writer — are always live while the studio runs, so `close` on one refuses:
`the director is a fixed seat — use restart, or finish the studio`. `close` only releases a held
person; on anything else it says `the actor-t1-c1 has nothing to close — it closes with its task;
use restart to reload it`.

`restart --person hollywood` reloads the overseer itself, from its own ledger and the same
`[hollywood]` setup; a fresh restart starts a new conversation.

The client's role menu offers the same command as **`Restart <name>`** for every live or stuck
role; Hollywood's own menu carries **`Restart Hollywood`**. The confirm has two choices —
`Resume its session` (the default when the role's runner keeps a resumable session) and
`Fresh start` — and runs `backlot restart … --as user` on the studio's machine. A fixed seat
(producer, director, writer) no longer offers `Close`, which `close` refuses on it; actors and
held seats keep it.

## Changing who plays a role

`backlot recast` swaps the agent that plays a **fixed role** — producer, director or writer — or
an **actor**, by its key `actor-<task>-c<n>` (the key `backlot status` shows for the task's
current actor):

```sh
backlot recast --person director --runner cmd --model deepseek-v4.1-flash --reason "quota errors" --studio acme-shop
backlot recast --person actor-t1-c1 --runner codex --model gpt-5.3-codex --effort medium --reason "the runner keeps timing out" --studio acme-shop
```

The new identity takes the runner, model, effort, permission profile and, at launch, the runner
version. `--permission-profile standard|worktree` overrides the profile the role plays now; without
it the current one is kept. The command is the user's: a role terminal is refused, exactly like
`card apply`. It runs the same identity and seat rules `studio new --dry-run` does — a supported,
installed runner, the cast file's effort and cline-model rules, and, for a fixed role, the
studio's `[limits]` still seating the fixed roles — and refuses a `finished` studio and an
identity that changes nothing.

An **actor's** key keeps its terminal name and its work area (the task's own worktree on the task
branch); only the agent moves. Its first prompt takes the task over and names what to read first:
the task and its acceptance list, `backlot status`, `backlot records` and the commits already on
the branch. The task and everything recorded against it — deliveries, rework rounds, the budget
clock — stay as they were; the one-raise-over-budget and rework-cap rules keep counting per task,
across the recast. Only the task's newest actor key is recastable: a `c<n>` the task has already
left is refused with `is not the task's current actor`. To cast a *different* candidate onto a
task, use `backlot cast` instead (that mints the next `c<n>`).

The stage carries it out on its next tick: the old terminal is closed deliberately (`role.closed`,
reason `recast` — never counted as a loss) and a fresh terminal starts under the same
`bl-<studio>-<role>` name. **The conversation is new** — the old runner's session is never resumed
— and the new role's first prompt says it is taking the role over and must read `backlot status`
and `backlot records` before acting. **The record is not:** the ledger keeps every event, earlier
deliveries and reviews stay attributed to the identity that made them, and messages already queued
for the role are delivered to the new terminal under the usual guarded-send rules. If the role has
not started yet (for instance the writer before the director asks for a plan), no terminal starts
right away; the panel lists it as a dim `standby` row with its effective recast or cast identity,
and `backlot recast` (as well as the closed card outcome) notes `writer starts when the director
asks for a plan` (or `<role> starts with the studio` for producer and director). A recast while
the old role has an open permission prompt still closes it — the answer waits, the replacement does
not.

Hollywood may propose the same change as a card (see [hollywood.md](hollywood.md)):

```sh
backlot hollywood propose recast --studio acme-shop --person director --runner cmd --model deepseek-v4.1-flash --why "quota errors" --as hollywood
```

The card shows `recast director → cmd/deepseek-v4.1-flash`; applying it re-checks the role's
identity, and the stage records one `role.recast` with the card's marker.

The client offers the same command as **`Recast…`** on every live person's role menu — a fixed
seat or an actor. The picker asks for the runner, then the model, then the effort, taking the
runner and model lists from `backlot runners --json` on the studio's machine. The effort step
follows the runner's caps: a runner that requires an effort lists its levels with one
preselected, an optional one allows an empty choice, and a runner with no effort flag has no
effort step at all. A summary line shows the old identity beside the new one before Enter runs
`backlot recast … --as user`; a refusal comes back verbatim. Recasting an actor keeps its task,
terminal and worktree (see [Changing who plays a role](#changing-who-plays-a-role)).

## Changing a studio after it starts

The cast file you give `backlot studio new` only **initializes** the studio. The roles, the
`[[candidates]]`, the `[limits]`, the `max_actors`, the checks and the timings can all change
afterwards — the binding (`repo`, `members`, `no_git`, `mode`) and the push remotes recorded at
creation do not.

`backlot studio cast show` prints the cast the studio **plays now** as a cast file: the fixed
roles as their newest recast has them, everything else as last set. Pipe it to a file, edit it, and
`backlot studio cast set` records it:

Print the effective cast, write it to a file, edit it — drop a candidate, add a check, move
the director's model — then check and record it:

```sh
backlot studio cast show --studio acme-shop
backlot studio cast set --file /home/user/acme-shop.cast.toml --dry-run --studio acme-shop
backlot studio cast set --file /home/user/acme-shop.cast.toml --studio acme-shop
```

`--dry-run` prints a readable diff — the roles whose identity moved, the candidates added, removed
or changed, the limits, the `max_actors`, the checks and the timings — and records nothing; `--json` prints the same
structured. `set` records a `studio.cast` event carrying the whole new settings, and the earlier
events keep what they recorded.

`set` runs the same **runnable** checks `studio new --dry-run` does, and only those: every runner
has an adapter and launches on this machine, the effort and cline-model rules hold, every limit is
a positive number, and `[limits]` can still seat the fixed roles. A file that says what the studio
already plays is refused with `nothing changes`. It is the user's command — a role terminal is
refused — and a finished studio must be reopened or restored first, like every other change.

A fixed role whose identity the file moves is carried out as a `role.recast` for that role, in the
same apply: the stage closes the old terminal (reason `recast`) and starts a fresh conversation, as
[a recast](#changing-who-plays-a-role) does. The other changes land in place:

- **Live actors keep running.** Removing a candidate only stops future `backlot cast` picks; an
  actor already cast from it is untouched.
- **New limits apply to the next start.** A role that is already live is never stopped by a new
  cap; it takes effect as slots free and new roles are wanted.
- **New checks run on the next delivery.** A delivery made before the check existed is not
  rechecked; the next one is.
- **New timings** — the rework cap, the stall and nudge limits, the task budget — apply from the
  next tick.

The client offers the same change as **`Edit cast…`** on the studio menu. It opens the effective
cast in your `$VISUAL`/`$EDITOR` in a floating terminal; save and close it and the client checks the
file with `backlot studio cast set --dry-run`, shows the diff in a confirm, and records it on
Enter. A refusal (a runner the machine has not got, a cap that leaves a seat empty) shows in the
dialog with its reason; a file that changes nothing says "nothing changes". On a server client the
file is edited on your machine and sent as content — the hub never resolves your path.

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
| `outward action` | An outward-action guard layer stopped a role from running a blocked outward command, or queued a cardable push/PR action. | Informational (`needs_user: false`): the role was told to `backlot ask` if it still needs the command, so nothing waits on you and it never counts as "needs you". For a carded push or `gh pr create` (or a request sent with `backlot ask --run [--why <r>] -- <cmd>`) apply (`a`/`y`), cancel (`x`/`d`), or inspect (`s`/Enter `card show`) the open card in the Call Sheet or Studio Dock (works with or without `[hollywood]`). A hard-refused command cannot run via a card — run it outside backlot yourself if wanted. Either way an open `outward action` attention clears (`role moved on`) as soon as the role writes anything else to the ledger after it, or the stage delivers it new work (`outward` completion notices report push/PR results back to the role). |
| `question` | A role's screen has sat on a non-permission question or unrecognised dialog picker for two seconds (`agy` `kind: "question"`, a recognised question picker, or D2's generic picker rule). | An actor's notices its task reviewer; `director` and `writer` notice `producer`; `producer` and Hollywood wait for you (`needs you`). The saved screen is under `attention/<seq>.txt`; it clears `dialog closed` after two seconds out of the dialog. |
| `stalled` | A live role's screen did not change for `stall_limit_s` while it counts as working — an actor whose task is `running` and whose work reads working or waiting (`blocked` never counts — a prompt has its own attention), or a fixed role sampled `working` — or, for a hook-wired Claude, its last signal reported working and went quiet for the limit. An idle one is normal and never stalls. | Look at the saved screen file — the detail says `screen unchanged` or `no hook signal for <n>s`. The role's reviewer is told; a producer's stall also tells a live director, and `backlot status` leads with a `producer alert:` line. A changed screen or a newer signal clears the attention. |
| `cannot resume` | A lost role could not be brought back: no recorded session id, or three losses inside ten minutes. | For an actor, its reviewer is told to abandon or recast the task. The writer's and director's losses notice the producer; the producer's is on you — a live director is told only so it does not wait, and `backlot status` shows a `producer alert:` line. There is no automatic way on for a fixed role; only you (or Hollywood) can intervene. |
| `session id not found` | A post-launch runner (`codex`, `devin`, `cursor`, `cmd`, `cline`) gave no session id within its discovery window — 60 seconds, 240 for `cmd`, whose session file lands late. | The role still runs and the stage keeps looking for up to ten minutes; a role past the default window but inside its runner's logs a warning only. If it never resolves, the role cannot be resumed after a restart — finish its work in this run. |
| `folder trust refused` | Claude Code's first-use folder-trust dialog showed up in a role's first minute — but outside the role's own work area, and the stage answers only inside the bound repo or its worktrees. | The dialog waits for you in the role's terminal. Answer it by hand if the folder is right; if it is not, close the role and check the bound path. Nothing clears the attention automatically — `backlot attention clear <seq>` once you have handled the dialog. A dialog inside the work area never raises this: the stage answers it and records a `stage.answered` event in the ledger, bookkeeping that names the key it pressed and never feeds the attention list. |
| `launch refused` | `terminal.run` refused the role's launch — a taken terminal name or an argv the core rejects. | The stage retries each tick; the detail names the refusal. Check the runner exists and the terminal name is free. |
| `not a git repository` | The studio's `--repo` path is not a git work tree — no director or actor starts. | Point the studio at a real work tree, or make the repo one; the stage retries each tick. |
| `work area failed` | A git step failed while preparing `work/` — the detail is git's first error line. | Fix what git complained about; the stage retries each tick. |
| `owed` | A role was nudged about a step it owes — the detail names the step — and `escalate_after_s` later the step is still owed and the role is still idle. | Do the step yourself or talk to the role with `backlot tell <role> --file <file>`; the producer is told for other roles. It clears once the nudged step is done — when none of the role's owed steps carries a delivered nudge — or when the role is no longer wanted; a newer step then earns its own nudge and its own `owed` attention. It also clears (`role active again`) once the role writes to the ledger after the escalation, for example while you talk it through; if it then sits idle a whole `escalate_after_s` with the step still owed, the attention raises again. |
| `over budget` | A running or delivered task passed its `task_budget_s` — the detail names the task, the budget and the time used. | The director and the producer are both told; nothing was stopped or paused, so let it run, pause it, or recast. One raise per task ever — a recast does not re-raise — and it clears `done` when the task is terminal. |
| `runner not supported` | The cast's `runner` has no adapter — a typo, or a runner other than `claude`, `codex`, `devin`, `opencode`, `cursor`, `cmd`, `cline`. | The role never starts. `backlot correct` or a new studio is the way out — see below. |
| `permission profile not supported` | The cast's `permission_profile` is not `standard` or `worktree`. | Same: the role never starts; fix the cast for the next studio. |
| `model not supported` | A cline role's model names no provider the adapter knows — only `cline-pass/<model>` ids were measured, and there is no per-role config for a provider. | The role never starts; give the cast a `cline-pass/…` model. |
| `effort not supported` | A cline role's `effort` is not one `--thinking` takes (`none`, `low`, `medium`, `high`, `xhigh`). | Same: the role never starts; fix the cast's effort. |
| `config malformed` | `<state base>/config.toml` or the user config's `[runner_env]` did not parse, a `[runner_env.<runner>]` table held a non-string, or an env file `[env_file]` names could not be read or parsed — the role gets no runner env and does not start. | Fix the file's TOML or the env file; the stage retries each tick. Only the error's line and column or the file's path are reported, never the contents. |
| `cursor prep failed` | A cursor role's per-session setup failed — its directories, `cli-config.json` or the `mcp-disabled.json` list could not be written, or the global `mcp.json` did not parse. | The role does not start; the detail names what failed. Fix it and the start retries each tick. |
| `cursor dir was open` | A cursor role's `roles/<role>/cursor/` — the boundary that keeps the agent's `authInfo`-holding files unreachable — carried a group or other permission bit. The stage found it at launch prep or at session discovery and set it back to 700. | Informational only — the detail names the path suffix. Check what set the mode (a loose umask, an outside chmod); a repeat drift raises it again. |
| `inherited XDG_CONFIG_HOME` | The stage's own environment carries `XDG_CONFIG_HOME` — cursor's `auth.json` moves with it, so a cursor role would start logged out. | The role does not start. Unset `XDG_CONFIG_HOME` where the stage runs and it retries each tick. |
| `cmd prep failed` | A cmd role's per-session setup failed — its workdir `.commandcode/settings.json` could not be written, or the global `mcp.json` did not parse. | The role does not start; the detail names what failed. Fix it and the start retries each tick. |
| `cline prep failed` | A cline role's per-session setup failed — `<cwd>/.cline/hooks/PreToolUse.sh`, the MCP mask file or the worktree's `info/exclude` could not be written. Without the hook the role would run every tool unguarded, so nothing starts. | The role does not start; the detail names what failed. Fix it and the start retries each tick. |
| `agy prep failed` | An agy role's per-session setup failed — its workdir `.agy-settings.json` or `.agents/hooks.json` could not be written. | The role does not start; the detail names what failed. Fix it and the start retries each tick. |
| `devin prep failed` | A devin role's per-session setup failed — its workdir `.devin/config.local.json` or `info/exclude` could not be written. | The role does not start; the detail names what failed. Fix it and the start retries each tick. |
| `hooks unavailable` | The repo already tracks the runner's hook config file (`.agents/hooks.json`, `.commandcode/settings.json`, or `.devin/config.local.json`), so the stage left it untouched instead of dirtying git. | Informational only — the role still runs on its remaining evidence (terminal state, session logs, and shims/fallback deny rules). |
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

When everything is accepted, the producer reports that the studio is ready to finish and what is still yours
to do. Finishing is yours alone — the producer never runs it, even when you say yes in chat — and the studio
menu in the lot does it for you:

```sh
backlot studio finish --reason "all tasks accepted" --studio acme-shop --as user
```

`studio finish` refuses while any task is open and names them. A studio with no requirement and no
tasks refuses too — `nothing was built yet (no requirement, no tasks) — add --force to finish
anyway` — so a just-made studio is not finished by accident; `--force` finishes it. The client's
`Finish studio…` says `Nothing was built yet (no requirement, no tasks). Finish anyway?` first and
passes `--force`; a studio that has a requirement or a task keeps the plain confirm. From then on
the stage stops scheduling the studio; producer and director close like any released role, and
only a person you explicitly reopen comes back. Reopen — `backlot reopen --person <role key> --as
user`, or Enter twice on a closed row — brings the person back into its recorded runner session; a
person whose session id was never found has nothing to resume into, and the panel says so.

Finishing also answers every open question: a role's unanswered `tell user` — the producer's
`What would you like us to build?` — no longer shows as the studio's last message or as `asks you`
on its row. The finish is the answering event, so a finished studio shows no open question.

Finishing also clears the studio's windows. A closed role's program is killed, and if the core
kept its finished terminal record — the leftover `new`, `cast · <id>` and `producer` panes reading
"Terminal unavailable" — the stage removes it on the next tick. Only terminals carrying that
studio's own `backlot.studio` label go: another studio's, a hand-named one and your own shells
stay. The studio itself stays listed as finished, and reopening still works — the stage starts a
fresh terminal under the same name.

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

A finished studio's menu offers the same as **`Free work areas (prune)…`**: it fetches the
`--dry-run` plan, shows it verbatim in a confirm, and Enter then runs `studio prune <studio>`.
The ledger, `reports/` and `messages/` stay.

To retire a studio completely (finish if needed, close its role and shell terminals, prune its
worktrees and merged branches, and archive its ledger under `archive/<studio>-<stamp>/`):

```sh
backlot studio delete acme-shop --dry-run
backlot studio delete acme-shop
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

### Reopening a finished studio, or starting a new one

A finished studio goes quiet: the stage stops scheduling it, and recasting a role, changing the
cast, adding a task and answering a permission prompt are all refused (`the studio is finished —
reopen or restore it first`). Reopening is yours alone — no role runs it, and Hollywood can only
ask for it with a card:

```sh
backlot studio reopen --studio acme-shop --as user
```

The reopening is recorded (`studio.reopened`), and every change a finished studio refused works
again. Reopen when the studio still owns the work — another task on the same requirement, a
different actor for the same role, or a fix to the same integration branch.

Start a **new** studio instead when the requirement is different, when the old studio's cast,
limits or repository no longer fit, or when you would rather keep the old ledger closed as a
record. A new studio shares nothing with the old one — a new ledger, new work areas, new
`backlot/<name>/*` branches — so the finished ledger stays as an audit trail. To revisit an
archived studio, `studio restore` brings it back finished and it reopens the same way.

### Finish, reopen, prune, delete, restore

| Command | What it does | The ledger |
|---|---|---|
| `studio finish [--force]` | Ends the studio; producer and director close, only a reopened person comes back. Refuses while a task is open, and on an empty studio unless `--force`. | Kept where it is; `studio.finished` appended. |
| `studio reopen <s>` | Brings a finished studio back; recast, `cast set`, new tasks and permission answers work again. Only `--as user`. | `studio.reopened` appended. |
| `studio prune <s> \| --all-finished` | Reaps a finished studio's `work/` worktrees and `backlot/<studio>/*` branches git considers merged. | Kept; nothing appended. |
| `studio delete <s>` | Finishes if needed, closes the studio's terminals, prunes safely, records `studio.deleted`, and moves the ledger under `archive/<s>-<stamp>`. | Archived to `archive/`, gone from `studios/`. |
| `studio list --archived` | Lists what is under `archive/`: name, stamp, repo and last requirement title. | Read-only. |
| `studio restore <archived-name> [--name <new>]` | Copies an archived ledger back under `studios/` as a finished studio; `--name` renames it. Refuses a name already taken; `--dry-run` shows the plan. | The copy is live again; the archive copy stays. |
| `studio cast show [--json]` | Prints the cast the studio plays now as a cast file (the fixed roles as their newest recast has them); `--json` is the same settings structured. | Read-only. |
| `studio cast set --file <f> [--dry-run]` | Validates a cast file with `studio new --dry-run`'s runnable rules and, unless `--dry-run`, records it. A fixed role whose identity moved is recast in the same apply. | `studio.cast` (plus a `role.recast` per moved role) appended; the earlier events keep what they recorded. |

`studio restore` recreates no work areas: the studio comes back `finished` and `reopen` works from
there. `prune` stays what it was — it never touches the ledger.

Which repositories prune works follows the binding (D4, D10). A workspace-bound studio is
pruned member by member, over the member list the ledger recorded at `studio new` — never a
scan of the parent directory, so a repository added later is left alone. A member's lines name
it (`svc-api: backlot/acme-shop/integration`); a member that is no longer a work tree is kept
with that reason and the other members still prune, so one broken member never strands the
rest. A no-git studio has no worktrees or studio branches: it reports `nothing to prune` and
touches no git. Only a single-repository studio bound to a path that is no work tree is
refused.

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

## Using backlot from another machine

When one hub runs `backlot-server` in its `backlot` `hc` session (`host-a`), another machine
(`host-b`) can open the studio TUI and run person commands against that server with no local
stage:

1. **`--server <host>[/<session>]`** (session defaults to `backlot`).
2. **`BACKLOT_SERVER`** environment variable (`host-a` or `host-a/backlot`).
3. **`[client]` in `~/.config/backlot/config.toml`** (`server = "host-a"`, optional
   `session = "backlot"`).

Explicit `--remote` or `--session` stays on that session as before (combining `--server` with
either is a usage error), `--plain` without `--server` stays local, and leaving all three unset
keeps today's local behaviour unchanged.

```sh
# CLI commands route over ssh host-a hc connect --session backlot:
backlot tell producer --text "check status" --server host-a --studio acme-shop --as user
backlot tell hollywood "any studio waiting?" --server host-a
backlot priority --task t1 --level high --server host-a --studio acme-shop --as user
```

- **TUI reads (`backlot --server host-a`):** connects the TUI over `ssh <host> hc connect
  --session <session>`, reads `backlot` and `backlot-cat` from the server's core, follows live
  `doc` updates, and attaches role terminals there. Your own everyday local terminals stay on your
  local core (`backlot --plain` or an unconfigured shell).
- **Actions via the server queue (`plugin.backlot.req` -> `plugin.backlot.res`):** both the TUI's
  panel/Call Sheet actions and the remote CLI's person verbs (`tell`, `tell hollywood`,
  `card apply`, `card cancel`, `card show`, `attention clear`, `recast`, `restart`, `close`, `reopen`,
  `priority`, `pause|resume|abandon`, `hollywood setup`) append a timestamped request with CAS
  (up to 3 retries) and wait up to 5 s for its matching ack. Queue overflow or CAS contention prints
  `server busy`; requests past the server's 60 s window fail with `request expired`; missing replies
  time out with `timed out waiting for ack of '<req_id>' (no reply from server)`.
  Role terminals cannot use `hc run|exec|send|keys|connect`, interpreter code (`python -c`, `node -e`, a
  heredoc) that writes `plugin.backlot.req` or `doc.set` or reads the core socket, or detach tricks such as
  `systemd-run` and `tmux new -d`, because roles run as the person's own user and this is a seatbelt against an
  overeager agent, not isolation.
- **Studio operations over the same queue:** `studio new`, `finish`, `prune`, `delete`, `restore`, and the New
  Studio dialog's `repos` / `runners` probes are requests too. `--cast`, `--cast -`, `--ask` and `--ask-file`
  are read on the client and travel as content; the hub parses the cast, canonicalizes `--repo` against its own
  filesystem and runs the same code the local CLI runs. `studio delete` and `studio prune` (and `studio restore
  --name`, because it renames) are reviewed: a `--dry-run` returns `{ report, confirm_token }`, where the token is
  `sha256` of the report's canonical JSON. The CLI's single invocation sends the dry run, then the commit with that
  token; a commit without a token refuses with
  `confirm this from a dry run first`, and one whose fresh dry run differs refuses with
  `the studio changed since you reviewed it; run it again`. `studio new --interactive` runs where its terminal is
  and is not a server verb, and `--remote` on `studio new` still refuses: placement is Phase 2.
- **On the hub itself:** when the resolved server host matches the local machine (`host-a`), CLI
  verbs keep writing the local ledger directly (pointing at the resolved session) so they keep
  working even while the server restarts; the TUI connects to the local server session directly.
- **Stopped server:** when `backlot-cat` exists on the server core while no `backlot-server` or
  `backlot-stage` terminal is live, the TUI displays one read-only line with the command to start
  it (`systemctl --user start backlot-server.service` or `backlot server start --session backlot`)
  and never starts a server process itself.

## Where the files live

The state root is `$XDG_STATE_HOME/backlot/<core id>/` — `~/.local/state/backlot/<core id>/` by
default — one per core, overridable with `BACKLOT_STATE_DIR`. A studio named by `--studio` or
`BACKLOT_STUDIO` is looked up under the current core's directory first, then across the sibling
`<core id>` directories — the CLI finds the studio for you, and pins (`BACKLOT_STATE_DIR`,
`--session`, `--remote`) keep the lookup on one core:

```
config.toml                  optional [runner_env.<runner>] env for role terminals
agent.env                  optional dotenv file merged under it, every runner
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

A workspace studio holds one worktree per member instead — `work/integration/<member>` and
`work/<task>/<member>` on that member's own `backlot/<studio>/…` branch, the role's terminal
in the container directory so the members sit side by side. A no-git studio has no `work/`
tree at all — every role shares the bound path.


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
the summary or logs. A malformed file raises `config malformed`. `studio new` — dry run and real,
cast file or wizard — writes a runner's `PATH` here when the cast's binary lives outside the
inherited `PATH` (shown before it is written, an existing `PATH` never touched), and the same
resolver answers the installed check the way a launch would: a runner the picker offers is a
runner that can seat.

The stage also reads `[runner_env.<runner>]` from the **user config**
(`~/.config/backlot/config.toml`) as a lower layer — that is where `hollywood setup` writes a
runner's `PATH`, and the `[hollywood]` table there applies to every session on the machine, so its
`PATH` must too. The per-core table wins on any key both set; the user config's `[env_file]` is
never read. Every role reads the pair through the same lookup, Hollywood included, so one
`hollywood setup` covers the overseer and every studio role on the machine.

Larger env sets — a company proxy's whole bundle, say — live in dotenv files rather than inline.
An `agent.env` beside the `config.toml` applies to every runner the moment it exists, and
`[env_file]` tables name more: `default` for every runner, a runner's own key for that one only.
Paths are absolute or `~/`-relative, a missing file is skipped, and each layer sits under the one
after it — `agent.env`, then `default`, then the runner's file, then the user config's
`[runner_env]` strings, then the per-core `[runner_env]` strings, which always win.

```toml
[env_file]
default = "~/.config/secrets/http-proxy.env"   # KEY=VALUE lines, `export` and `#` comments ok
claude = "~/.config/backlot/claude.env"        # this runner only
```

A file that exists but does not parse or read is `config malformed` — the message names the path
and the line's position only, never the value beside it.

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

[client]
# server = "host-a"        # hub host (or "host-a/backlot"); overridden by BACKLOT_SERVER / --server
# session = "backlot"      # optional default session when `server` omits `/<session>`
```

A missing file means defaults; a malformed one means defaults plus a notice carrying only the
line and column. The panel's open/closed state is per client and in memory only — nothing is
saved.

With no `[hollywood]` the bar is still there in its no-agent form (studios, the new-studio
frame and the setup line), on `Alt+Y`, with no timer; `backlot --plain` has no bar at all and
the screen is the herdr screen. A `key` value that cannot be bound falls back to `alt+y` with a
notice naming the file, never echoing the value.
