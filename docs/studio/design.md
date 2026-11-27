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

The cast's `[limits]` table caps how many live roles one runner may carry — `claude = 3`, `devin = 1` — an account's
rate limit is shared by every session on it. Every live role on the runner counts, fixed roles and actors alike, and a
role being started counts too; a missing runner is unlimited. A wanted start that would pass the cap waits — held
starts go in the order producer, director, writer, then actors by task priority and task seq — retried every tick
until a slot frees, with nothing recorded for the wait. `backlot status` and the panel show
`waiting for a <runner> slot` on the held role. The cap is per studio: two studios on one account each count only
their own roles. `studio new` refuses a cap that can never sit its fixed roles — under the producer-and-director
count on the runner, or at their count while the writer or a candidate shares it — because no seat could ever free
for the rest. A held role accrues no task budget and owes nothing until its first `role.started`, which then becomes
the floor for its obligations' age.

On top of the per-runner caps, the cast's `max_actors` (default 4) caps how many actors play at once. Only actors
count — the producer, the director and the writer never fill an actor slot — and an actor being started counts too.
A candidate is a *kind* of actor, not a person: one candidate may be cast to several tasks at once, each cast a
separate actor with its own terminal, worktree and branch. A wanted actor past the cap waits with
`waiting for an actor slot` (distinct from a `[limits]` wait) and starts the tick an actor closes; `backlot status`
shows `actors <live>/<cap>` so the director casts up to the cap and no further. A `max_actors` below 1 is refused.
Both caps travel in the cast and are what `studio cast set`, `Edit cast…` and Hollywood `cast` cards change.

The cast file only **initializes** a studio; it locks nothing afterwards. A `studio.cast` event carries the whole new
cast — the three roles, the candidates, the rework cap, the timings, `check_env`, the declared checks,
`task_budget_s`, `[limits]`, `max_actors` and the push knobs — and the newest `studio.cast` (or `studio.created` when there is none)
is what the stage reads from then on. The binding (`repo`, `members`, `no_git`, `mode`) and the push remotes recorded
at creation are facts, not cast, and never travel in the event. A fixed role whose identity the change moves is
recorded as a `role.recast` for that role, appended just before the `studio.cast`: that is what makes the stage close a
live terminal and start a fresh conversation, the same path `backlot recast` takes. Everything else lands in place —
a removed candidate only stops future picks, a new cap applies to the next start, a new check runs on the next
delivery, and the new timings apply from the next tick. The runnable checks (a supported, installed runner, the effort
and cline-model rules, a positive limit, caps that still seat the fixed roles) are the cast file's own, minus the ones
that only creation needs; a studio's binding is not re-checked.

It uses `hc-proto` directly, because `doc.get` and `doc.set` have no CLI verb.

## Role terminals

Each role is a terminal started through `terminal.run`:

- **Name:** `bl-<studio>-<role>`, within hc's limit of 1-32 characters of `a-z 0-9 - _`.
- **Labels:** `backlot.studio`, `backlot.role`, `backlot.task`, plus `herdr.space`, `herdr.tab` and `herdr.seen=0` so
  the terminal appears in the left sidebar like any other.
- **Environment:** `BACKLOT_STUDIO`, `BACKLOT_ROLE`, `BACKLOT_TASK`, plus the runner's environment profile.
  `BACKLOT_ROLE` is the role's ledger key — `actor-<task>-c<n>` for an actor, the fixed name otherwise
  — so a `backlot` command run inside the terminal needs no `--as`: it is the role itself. The bare
  `actor` is no caller: the ledger refuses it for every kind, while replay still reads old logs whose
  events it recorded.
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
| Any | `backlot status`, `backlot progress`, `backlot msg show <seq>`, `backlot tell <role> [--ask \| --note]`, `backlot guide` |

`backlot guide [<topic>] [--grep <word>]` prints backlot's own docs, compiled into the binary from
`docs/studio/` (one topic per file) — no studio, ledger or core. Every role prompt says its docs come
from there and never from a disk search, and the launch pre-allows it like any other `backlot`
command. The shared read-only command set — `git status`, `git log`, `git show`, `git diff`,
`git branch --list`, `git branch --show-current`, `ls`, `rg`, `wc`, `head`, `backlot msg show` — is pre-allowed for every
role on every runner that carries an allow list (`runners.md`), so a role's forensic reads never ask a
person; one plain command per shell call, since a chained or `git -C` form does not match.

The table is enforced, not just followed (S9b): the ledger refuses an event from a role outside its whitelist — an
actor records only its own task's `task.delivered`, its own `blocked` and purpose-free messages, and `task.delivered`
from anyone who is not the task's current actor is the user's alone. `stage start` and `stage run` refuse actor and
writer callers too.

**A delivery is only what `backlot deliver` records**, with a commit range and a report. An idle status, a finished turn
or a report file appearing are not deliveries. Status can read idle in the middle of a turn, and a report file can appear
before the agent has finished. The range is real: the CLI resolves both ends in the task's work area and refuses an
empty range or a tip that is not on the task's `backlot/<studio>/<task>` branch, so the ledger holds the resolved
`sha..sha` (S9b).

**The checks on a delivery are the stage's, not the actor's** (S9b part 2). The cast declares
`[[checks]]` — a name, a `run` command and `timeout_s` — and may pass stage environment names
through `check_env`; dangerous names (`BACKLOT_*`, `HC_*`, proxies, credentials) are refused at
`studio new`. Each declared check runs once per delivery on the stage itself: `sh -c` in a
detached worktree at the delivery's end sha, under an environment stripped to a passthrough list
plus `check_env`, one run per studio polled across ticks, the timeout escalated TERM then KILL
five seconds later, the log capped and written under `checks/` before the result is recorded
`task.checked` by `stage`. A stage stopped mid-check owes it again — the ledger either holds its
record or it reruns. What the actor reports with `deliver --check` stays its own claim, kept
apart from the stage's records in the reviewer's notice (which waits for them), `status` and
`records`. An accepting or partially accepting review is refused while a declared check has no
record — the user alone may accept over pending ones — and unless its conclusion names every
failing check; a `correction` producing such a review is gated the same way.

If an actor goes quiet without delivering, the stage reminds it once and then raises attention to the director.

## Messages

- **User to producer:** the user types in the producer's terminal. Selecting a studio focuses it.
- **Role to role:** `backlot tell` records the message and writes its body to a file. When a studio role
  (`producer`, `director`, `writer`, or `actor-<task>-c<n>`) sends `backlot tell user` containing `?` or `？`
  outside inline code spans, fenced code blocks, and URL query strings (`?key=`), it must pass `--ask`
  (question awaiting the person) or `--note` (explicit plain information), or the CLI refuses (exit code 2).
  A `tell` to the writer or the
  director while it is closed because it is not wanted wakes it again — the same way a `plan` request does, unless the
  studio is finished: a delivered-pending tell keeps the role in the wanted set until the role answers it (its next
  ledger write, such as a step-boundary `progress`, or a `tell` back). An actor of a finished or abandoned task is never
  woken: `backlot tell` to it is refused at append time saying so. The stage sends the target one
  line — `[backlot] <from> → <to>: read <file>` — with `terminal.send`, at most one send per role at a time. A send
  goes only while the target is live, its work sample is `waiting`, its screen has been unchanged for two samples at
  least two seconds apart, it has been up at least five seconds, and no send to it finished in the last five seconds.
  A refused send stays queued and is retried; a crash between send and record can repeat a line, which is harmless.
  `terminal.send` can hold for minutes, so it is spawned off the stage loop and collected on a later tick.
  `report.sent` to a role delivers the same way. The stage's own notices (see ledger.md `source_seq`) queue as
  ordinary messages with a generated body file under `messages/`.
- **Only what is still true is sent (S9d).** Before a send, every queued message carrying a `source_seq` — a stage
  notice or a nudge — is checked against the event it relays. An addressee check runs first: a notice to a role the
  ledger no longer wants live — a released writer, an actor whose task closed, every role once `studio.finished`
  lands — is dead mail whatever its label and drops `no longer wanted` (`user` is always wanted, and a person held
  for reopening keeps its mail). Then the label table:

  | label / purpose | stale when | reason |
  |---|---|---|
  | `attention`, `stalled`, `lost`, `owed` | that attention is cleared | `attention cleared` |
  | `delivered` | that delivery has a review, a newer delivery exists, or the task is terminal | `delivery reviewed` / `superseded` / `task closed` |
  | `rework` | a newer delivery exists or the task is no longer running | `superseded` / `no longer running` |
  | `at-cap` | the task is no longer at cap | `no longer at cap` |
  | `blocked` | the task is no longer blocked | `no longer blocked` |
  | `resumed` | the task is terminal | `task closed` |
  | `nudge` | the step keyed by `source_seq` is no longer in `next_steps` | `step done` |
  | `recovered`, `denied`, `req` | never | — |

  A dropped message counts as handled in the notices' dedup set — it is never re-queued — while staying out of the
  outbox and the nudge pass's idle count, and it is never marked `message.delivered`. Messages without a
  `source_seq` (`tell`, `ask`, `dispatch`, the plan request) and reports are never dropped — a person wrote them and
  the words stand. A send already in flight is exempt: it cannot be un-sent, so a notice that went stale while the
  wire held still lands.
- **Several pending items go as one digest.** When the gate opens on two or more items for a role the stage writes
  `messages/digest-<top seq>.md` and sends `[backlot] stage → <role>: read <path> (<n> items)`; on the answer every
  item gets its `message.delivered`. The file gives each item a `## <label> (seq <n>)` section — a stage-written
  notice's body inlined, a role-written file or a report named by path, a file-less message shown as the line it
  would have sent alone. Items sit in this order (`digest::rank` is the one place it lives), ties by seq:

  1. `blocked`
  2. `at-cap`
  3. `delivered`
  4. `rework`
  5. `attention`
  6. `denied`
  7. `stalled`
  8. `lost`
  9. `owed`
  10. role-written messages and reports, in seq order
  11. `req`
  12. `nudge`
  13. `resumed`
  14. `recovered`

  One pending item keeps the single-line send above.
- **Producer to user:** in the producer's terminal, with a one-line summary in the panel.

## Flow

Normal path:

1. The user and producer settle the goal and constraints. The producer records requirement version 1.
2. When `design_gate = "required"` (default on new studios), an opening design discussion runs across Round 1+ (`writer` proposal, `director` buildability review, `producer` convergence summary and questions, and user `design.signed` — or a user-confirmed `--suggest-skip`) before any `plan`, `dispatch`, or `task` is allowed (see [Opening design discussion](#opening-design-discussion)).
3. Once the requirement version's design is signed (or when `design_gate = "off"`), the producer starts the writer. They agree on a formal plan and acceptance list (citing `design v<N> §R<r>.*` when signed off normally), and the writer publishes it.
4. The producer dispatches to the director.
5. The director splits the work into tasks that can each be delivered on their own, and casts actors from the candidates.
6. Each actor works in its own area and delivers.
7. The director reviews against the plan and acceptance list, requests rework or accepts, and integrates.
8. At nodes, the director reports to the producer, and the producer reports to the user.

Light path: the producer dispatches one task directly to one actor, or does the job itself — `task new`, `cast` and
`review` are the producer's and director's to run, so `backlot … --as producer` runs the whole path. A producer's cast
is recorded in `task.cast`'s `by` and routes the delivery notice to the producer. It is recorded the same way, so the
two paths can be compared on real work.

When everything is accepted the user decides and finishes the studio themselves; the producer reports
it ready and what is still theirs to do, and never runs the finish. `backlot studio finish` records
`studio.finished` and the studio stops scheduling — only a person explicitly reopened is desired again.

## Opening design discussion

When `design_gate` is `"required"` (the default for newly created studios; set `design_gate = "off"` in `cast.toml` or via `backlot studio cast set` to disable per studio, while existing studios whose `studio.created` lacks `design_gate` stay `"off"`), every new requirement version passes through a single user-gated design lifecycle before planning, dispatch, or task creation is allowed:

- **State machine (`DesignPhase`):**
  1. `idle`: a `req.version` was recorded, no round is open yet.
  2. `suggest_skip`: `design.opened` with `suggest_skip = true` (optional `reason`, allowed only for tiny fixes or an already-finalized external plan). The person either accepts skipping (`design.signed` with `mode = "skipped"`) or rejects skipping (`backlot design open` / remote `design-start-round`), moving to Round 1 (`waiting_proposal`).
  3. `waiting_proposal`: Round `r` opened; the `writer` posts `design.entry` (`slot = "proposal"`).
  4. `waiting_buildability`: the `director` posts `design.entry` (`slot = "buildability"`).
  5. `waiting_summary`: the `producer` posts `design.entry` (`slot = "summary"`) with 0–3 structured `DesignQuestion`s (`id`, `prompt`, `recommendation`, optional `section_ref`).
  6. `waiting_user`: the studio waits on the person. The person may sign off (`design.signed`), reply via `backlot tell producer` / producer terminal (appending `design.entry` `slot = "user_input"` with `source = "tell" | "hook" | "producer_note"`), or address `writer` / `director` for an in-round `design.entry` (`slot = "supplement"`, requiring `--refs <seq>` pointing at existing message or design entry `seq`s). Once `user_input` follows the round's `summary`, `producer` or `user` may advance with `backlot design next` (`design.opened`, `round = r + 1`, `waiting_proposal`).
  7. `signed`: `design.signed` (`user` only, refused in `roles` and `proc_guard`/`BACKLOT_*` when called by any agent role) closes the active requirement version with `mode = "approved"` or `"skipped"`.

- **Hard gate & reset:** While `design_gate == required` and `design_phase != signed`, `validate` refuses `plan.published`, `dispatch`, `task.created`, and `task.cast` (`Refusal::DesignNotSigned`). Recording a new `req.version` resets any unsigned active `DesignState` back to `idle` while preserving signed versions in `design_history`.
- **Steps, wanted roles, and nudges:** `steps::next_steps` emits an owed step for each active phase (`open design`, `design proposal`, `design buildability`, `design summary`, `sign off or open next round`, `reject skip`, plus post-sign `derive plan from design` and `dispatch`). `wanted_roles` keeps `writer` alive through active rounds and post-sign plan derivation; `nudge` times role-owned slots and never nudges user-owned `suggest_skip` or `waiting_user`.
- **Immutable section files, assembly, and freeze (`design_doc.rs`):** Non-empty entry bodies stage as `PendingFile` and write to immutable files under `design/v<N>/r<round>-<seq>-<slug>.md` (`proposal`, `buildability`, `summary`, `user_input`, `supplement`). Under `events.lock`, commits atomically rewrite `design/v<N>.md` (temp file + `sync_data` + rename) with `# Design discussion · req v<N>[: <summary>]`, `## R<r>`, deterministic section headings (`### §R<r>.W`, `### §R<r>.D`, `### §R<r>.P`, `### §R<r>.U`, `### §R<r>.W+` / `### §R<r>.D+`, plus `#### Questions`), and trailing `## Sign-off (#<seq>)` when signed. `approved` sign-off idempotently freezes `design/v<N>-final.md` (matching `design/v<N>.md` at sign-off; a mismatched existing `-final.md` refuses); `skipped` creates no `-final.md`.
- **Producer terminal capture:** Unless `[design] hook_capture = false` is set in `<state_dir>/config.toml` or `~/.config/backlot/config.toml`, `backlot hook` captures non-empty, non-`[backlot]` producer terminal prompts strictly while `design_gate == required` and `phase == waiting_user` on supported runners (`claude`, `devin` `UserPromptSubmit`, `agy` `PreInvocation`) into `<studio>/design/.hook-capture.jsonl`. Stage terminal writes (`deliver`, key sends, lifecycle actions) taint and suppress spurious capture; when capture is disabled or unsupported, untainted producer turns create `<studio>/design/.pending-user-note.json`, refusing `backlot design next` until `backlot design note --from user` (or `tell producer`) lands.

## Stop conditions

Agents reviewing agents can loop without end, so the stage enforces limits:

- Rework rounds per task. Default 2. At the cap the task goes to the producer to accept with documented gaps, rescope or
  recast.
- A running time budget per task, optional — `task_budget_s` in the cast file. The clock runs from the task's first
  actor `role.started` to its close or to now, leaving out `paused`→`resumed` stretches — a cast a runner cap still
  holds accrues nothing. A running or delivered task past it raises
  one `over budget` attention on the task's current actor key — once ever, a recast does not re-raise — the director
  and the producer each get the notice, nothing is killed or paused, and the attention clears `done` when the task
  turns terminal.
- A stall limit on a role's unchanged screen — `stall_limit_s` in the cast file, default 600 s — detailed under
  "Watching agents".

## Watching agents

Status alone cannot say what each person is doing. It can read `done` on an agent that has been stuck for hours, and on
official hc, agents that do not report status have none at all. The stage therefore combines:

| Source | Used for |
|---|---|
| Explicit `backlot` commands | Task state and role ledger events (`last_by_role`). This is the authority. |
| hc status and turn events | Working or stopped, when reported (`activity.revision` for `revision_stalls` runners) |
| `terminal.read` samples | Work classification, D2/permission prompts, send gate (raw screen), and normalised screen liveness |
| Local OS / git probes (local machine only) | Descendant processes (`term.pid` via `/proc` or `ps`), `cutime`, `wchan` lock waits, and git `HEAD`/`index`/`status` |
| Per-runner screen patterns and D2 generic dialog fallback | Attention reasons: `permission prompt`, `question` |

Instead of separate screen, signal, and revision clocks, the stage keeps one per-role liveness `Evidence` track combining six sources:

1. **Hook signal** (`Stage.last_signal`): any new state-mapping hook event (`ts_ms`).
2. **Activity revision**: `activity.revision` movement while terminal status is `working`, for runners whose revision advances mid-turn (`backlot_model::caps::revision_stalls`).
3. **Normalised screen**: `terminal.read` after stripping per-runner spinner animations (braille glyphs, Claude/cmd spinner symbols, elapsed timers, `Thought for …` headers, token counters, and status clocks). The delivery gate keeps using the raw screen.
4. **Process tree** (local machine only, sampled at most every 10 s): walks descendants from `term.pid` via `/proc/<pid>/task/*/children` (with `/proc/*/stat` `ppid` fallback on Linux; `ps` CPU-only on macOS). Any new child PID, descendant CPU or `rchar+wchar` growth, or waited child CPU (`cutime+cstime` on root) counts, and the walk records the deepest active descendant command (`argv` joined, capped at 60 chars, with start time `since_ms`) so working roles publish `RoleSummary.running`. The agent root's own CPU and I/O counters (`depth == 0`) count strictly above `8 jiffies/s` CPU or `16 KB/s` I/O — floors measured on `agy` only in R7a (`2-4 jiffies/s` and `1-7 KB/s` idle noise) — and **only when `last_signal` is absent for the current session** (no hook signals wired or no signal yet); when hook signals are active (`last_signal` is `Some`), they govern the root's own turn so TUI render redraw CPU/I/O never overrides a stale hook, while descendants and `cutime+cstime` still count.
5. **Work area** (local machine only): `HEAD` (and resolved branch ref target), `.git/index` mtime, and a hash of `git status --porcelain` sampled at most every 60 s — never walking `target/`, `node_modules/`, or `.git/objects/`.
6. **Role ledger event**: any new ledger event whose `by.role` is this role.

A role stalls only when it is eligible — an actor on a `running` task in `working` or `waiting`, or producer/director/writer while `working` (`blocked` never counts) — and **none** of the six sources moved within `stall_limit_s` (default 600 s), or `3 × stall_limit_s` when the deepest descendant's `wchan` contains `lock_inode_wait`.
One `stalled` attention raises per episode with the screen saved under `attention/<seq>.txt` and a `detail` line naming the silent window plus the last evidence (`no sign of life for <n>s; last: <desc> (<source>) <age> ago`), `screen animates only (spinner)` when only stripped animations moved, or `waiting on: <argv> for <dur>` for inode lock waits. Any new evidence clears the episode (`AttentionCleared` naming the moving source). The notice goes to the actor's task reviewer and to the producer for the other fixed roles — the producer's own stall is panel-only unless a live director can read it.

A permission prompt follows the approval policy. Rate-limit and connection-error screen patterns classify into their bounded retry episodes (`rate limited`, `connection error`); unrecognised stalled states reach the reviewer once no evidence moves.

## Hook signals (S9f)

Screen patterns are the fallback, not the source. A Claude role launches with
`--settings roles/<role>/claude-settings.json` — one session-only settings layer holding `hooks` and a `permissions.deny`
entry for `AskUserQuestion` (no env) — wiring eight events (`UserPromptSubmit`, `PreToolUse`, `PostToolUse`, `PostToolUseFailure`, `PermissionRequest`,
`Notification`, `Stop`, `StopFailure`) to `backlot hook --file roles/<role>/signals.jsonl`. The command is the running
binary's own absolute path, shell-quoted, and both files are rewritten on every start and resume — the settings
fresh, the signals truncated — so a moved binary or directory heals on the next launch. The sink appends one compact
line per event — `ts_ms`, the event, `session_id`, the notification or error type, and for a `PermissionRequest` the
tool name with its command or path — each string capped and the line under 4 KiB — silent and exit-0 on every path,
so a hook can never get in the runner's way.

The stage reads the file's last 16 KB and takes the newest line the role's *own* session sent — the session id the
ledger recorded at `role.started`, so another session's lines never move the role. The three work sources combine in a
fixed order:

1. `Blocked` wins from any source (signal, structured status, or screen) — a prompt can reach the screen before its signal lands, and a stale signal never hides an interactive prompt.
2. When the screen shows the runner's captured interrupt marker after the last working signal (for Claude Code, `Interrupted · What should Claude do instead?` with no active working marker below it — emitted without a `Stop` hook when the user presses Esc), the turn ends (`Waiting`).
3. Otherwise the signal's state (`UserPromptSubmit` and tool events → `Working`, `PermissionRequest`/`permission_prompt` → `Blocked`, `Stop`/`StopFailure`/`idle_prompt` → `Waiting`), except that on runners whose core structured status reliably reports `idle`/`done`/`waiting` (`StatusSource::Native` or `StatusSource::ForkInference` in `caps.rs`: `claude`, `codex`, `devin`, `opencode`, `cursor`, `cmd`, `agy`), a `Working` signal older than `SIGNAL_STALE_MS` (5 s) yields to `Waiting` when both the core and the screen read `Waiting`. Inside the 5 s grace window, a fresh `UserPromptSubmit` stays `Working` before the screen and core catch up.
4. `Stop` / `StopFailure`, or a newer working signal (starting a new turn even if the previous turn's interrupt line stays visible in scrollback), takes effect immediately as today.
5. Every hook-enabled runner normalizes into this same JSON line schema via `backlot hook --runner <name> --file <role's signals.jsonl>`:
   - **`agy`:** `<cwd>/.agents/hooks.json` splices top-level `"backlot"` across `SessionStart`, `PreInvocation`, `PreToolUse`, `PostToolUse`, `PostInvocation`, and `Stop` (`conversationId` -> `session_id`, `PreInvocation` -> `UserPromptSubmit`, `PostInvocation` -> `Stop`).
   - **`cmd`:** `<cwd>/.commandcode/settings.json` splices `"hooks"` beside `"permissions"` across `SessionStart`, `PreToolUse`, `PostToolUse`, and `Stop`.
   - **`devin`:** `<cwd>/.devin/config.local.json` splices `"hooks"` per work area (without touching `~/.config/devin/config.json`) across `SessionStart`, `UserPromptSubmit`, `PreToolUse`, `PostToolUse`, `PermissionRequest`, `Stop`, and `SessionEnd`.
   - When `.agents/hooks.json`, `.commandcode/settings.json`, or `.devin/config.local.json` is new and untracked, launch/resume prep adds it to `<git>/info/exclude` (following linked-worktree `commondir`). When the repository already tracks the hook file, prep leaves it untouched, logs `hooks unavailable`, raises one informational `hooks unavailable` attention, and leaves the role on its remaining evidence.
6. A session that sent nothing — and runners without hooks — answers from structured status and the screen.

Two more uses of the same tail: hook signals feed the unified D4 liveness track alongside the other five sources (any newer state-mapping signal refreshes evidence and clears an open stall `new hook signal`), while a `StopFailure` shows `last turn failed: <error>` on `backlot status` and the panel until the
role's next signal; nothing is raised — the owed-step machinery already nudges an idle role.

A `PermissionRequest` also binds the permission attention it raises: the detail names the tool and the trimmed
command or path (`Bash: cargo test`), the request's own JSON lands at `attention/<seq>.signal`, and `needs_user`
reads the request's command — the screen never shows it. Before a recorded decision's key is sent, the last signal
must still be that request — the same `ts_ms` and `tool_input` — otherwise it is a different prompt: nothing goes
out, the stale attention clears `prompt changed before the decision was sent`, and the pending request raises its
own bound attention. The screen-block compare stays for any attention not raised from a signal, and the keys still
go to the terminal: a hook never answers a prompt.

## Owed steps (S9a)

A stall says a screen stopped moving; it says nothing about whose turn it is. `backlot_model::steps::next_steps` reads
the reduced ledger the way the notices do and returns every obligation still outstanding — publish the plan pair,
dispatch, split tasks, cast, deliver, review, unblock, the node report, the producer's report to the user, finish —
each keyed on the seq of the event that created it, with an owner (a role key or `user`) and the time it became owed.
Nothing is owed once the studio is finished.

The stage's nudge pass runs inside the deliver pass. An owner counts as **idle** when it is ledger-live, its terminal
is up, its latest work sample reads `waiting`, nothing undelivered is queued for it and no send is in flight; the stage
keeps an in-memory idle-since per role (a restart starts it over). Once the later of the step's age and the idle
stretch passes `nudge_after_s` (default 180 s), exactly one `message.queued` with `purpose: nudge` and the step's key
as `source_seq` joins the outbox — the body names what is owed and the command that discharges it. Still owed and
still idle `escalate_after_s` (default 600 s) after that nudge was delivered, the step raises one `owed` attention on
the owner; a non-producer owner also owes the producer one `owed` notice. The attention is tied to the step it
escalated: it clears as soon as none of the role's owed steps carries a delivered nudge — so a newer, unnudged step
does not hold it up — or when the role is no longer wanted. The newer step then runs its own nudge and its own `owed`
attention. A step the user owes is shown, never pushed. A nudge shares `source_seq` with ordinary
notices without either suppressing the other: the dedup set counts only purpose-free messages, and — among the
purpose-free kinds — only a plain `tell` to the writer or director keeps it wanted (see Messages).

**An actor's delivery step gets a directive nudge and a second one** (an actor otherwise reads the nudge, writes one
`backlot progress` line and ends the turn, so the plain rule waits a whole escalation window). When the owed step is
an actor's delivery on a running task, the first nudge body names the task, says keep working until
`backlot deliver`, says not to end the turn with a progress line, and sends a block to the task's reviewer with
`backlot tell <reviewer> --file <file>`. If the actor is idle again after that nudge was delivered and, since the
delivery, the only ledger writes are `role.progress` (no deliver, no commit, the work area unchanged per the liveness
probe), a second, firmer nudge joins the outbox once the idle stretch passes `nudge_after_s` again. Every other step
keeps one nudge ever. For this step only, the `owed` clock and the `ROLE_ACTIVE` clear ignore a bare `role.progress`:
any other ledger write, a commit, or a work-area change still counts as activity.

`backlot status` prints one `next:` line per owed step — label, owner and age. The summary document carries the first
as an optional capped `next` field (`"label — owner"`, deliberately free of clock text so `same_as` stays stable), and
the panel draws it as one line under the studio like the `error` line.

## State directory

`$XDG_STATE_HOME/backlot/<core id>/`, overridable with `BACKLOT_STATE_DIR`. Keying by core ID keeps scratch sessions
apart from real ones. See `ledger.md` for the contents.

A `--studio`/`BACKLOT_STUDIO` name resolves across the per-core directories: not found under the resolved base, a
command scans the sibling `<core id>` directories — one match is used, none keeps `no studio`, several refuse with
the core ids (`--session` disambiguates). `BACKLOT_STATE_DIR`, `--session` and `--remote` pin the base and skip the
scan, so role terminals and directed commands never drift.

`backlot studio list [--json]` is the discovery half: it reads every core directory (or the pinned one) and prints
each studio's core (`core.json`'s name, else the short core id, else `local`), ledger state (`live`/`finished`/`failed`),
title and bound path, newest activity first. `--remote <host> studio list` reads one machine's studios; the command's
`--help` explains the local default and that form.

## Summary document

The stage writes one core document named `backlot`, holding all studios. One fixed name is needed because the watcher's
document list is set when it starts, while studios come and go.

- The stage is the only writer, and it writes with the CAS version.
- It writes only on change, at most once every 2 seconds. Core rewrites all documents in one file, so writes stay rare.
- It stays small: target under 64 KiB, against the 1 MiB limit. Field lengths are capped and old finished studios drop
  off. While `work == working`, live roles carry optional `progress` (`{ text, ts_ms }` from `role.progress`) and optional `running` (`{ cmd, since_ms }`, the deepest active descendant command capped at 60 chars); `doing` (`Option<String>`) stays unchanged for older clients. Closed people stay listed while their studio is live (R19), with no `doing` or `running` field. The full history is in the
  ledger.
- No credentials, environment values or prompt text.

Whether the stage is running comes from the terminal inventory (`backlot-stage` live or not), not from a heartbeat, so a
liveness signal costs no document writes.

An optional `hollywood` object rides in the same document: the overseer's role state, runner and model, its newest
headline and its open cards. It appears only while `[hollywood]` is configured. Its `exchange` lines carry an optional
`kind` — `tell` for a `message.sent` to Hollywood, `report` for a `hollywood.reported` headline, and (additive on
`backlot/1`) `card` for a `card.proposed` Hollywood wrote itself — so the band can draw a proposal as a reply and the
Reader can open it as a card. Independently of `[hollywood]`, each
`StudioSummary` projects up to eight open outward push/PR cards (`outward_cards` with command, rule, why, remote URL,
and commit summary) so both the Call Sheet and Studio Dock show them in agent and non-agent modes alike; when
`design_gate = "required"` and the current requirement version's opening design is beyond `idle`, it also carries an
optional `design` (`DesignSummary`: `version`, `round`, `phase`, `waiting_user`, `suggest_skip`, `skip_reason`,
`doc_file`, capped `summary`, up to 3 capped `questions`, `waiting_since_ms`), slimming to header fields (`summary:
null`, `questions: []`) once `signed` and all tasks for that version have been cast; and when a studio's ledger
contains unknown event kinds at `v <= LEDGER_VERSION`, the stage pauses read-only for that studio and marks its
summary entry with `needs_upgrade: true` and `unknown_kinds` (omitted on normal studios) so clients display
`"upgrade backlot"`.

## Hollywood

One overseer floats above all studios on a core — `docs/studio/hollywood.md` owns the detail; the stage's part is this:
with `[hollywood]` in the user config it runs one terminal `bl-hollywood` (labelled `backlot.role=hollywood`, never
`backlot.studio`) in `<state dir>/hollywood/`, started, adopted and resumed by the same rules a producer is, with its
lifecycle in its own ledger. The stage delivers to `bl-hollywood` through a **two-lane inbox**: pending `user → hollywood` tells
take the Person lane first and send inline (` · `-joined up to 2 KiB, else `messages/user-<top>.md`
with a 120-character preview) together with a deferred-notice footer (`(+N stage notices waiting: …)`).
Actionable stage notices — closed/`info` attentions and `card.applied` filtered out, `≤ 1` pending per
`(studio, role, label)` and `≤ 1` `recovered`, ranked `stalled > failed > card > prompt > attention > finished > recovered`
— wait in the Notice lane until the Person lane is empty and the newest notice is ≥ 5 s old
(`NOTICE_DEFER_MS`), going inline if one notice fits within 400 B and as a digest otherwise; obsolete,
superseded, and post-finish notices record `message.dropped`. Passive outcomes (`card.done` and
`accepted` reviews) never wake Hollywood alone and ride once as `(context: …)` on the next send. Passing
`--answers <seq>[,<seq>]` to `hollywood propose` or `hollywood report` appends `message.seen { target, by_seq }`
so already-handled tells skip inbox delivery. On adopt or stage boot, `AGENTS.md` is refreshed whenever
its bytes differ from the compiled prompt and CLI reference.

Hollywood changes nothing directly: it proposes **cards**, the user applies or cancels, and the stage carries an
applied card out exactly once — re-checking the card's base, recording the studio event `by: user` through the same
validation the user's own command would hit, and recording `card.done`, `card.failed` or `card.stale` back in
Hollywood's ledger. A `studio-new` card re-runs the `studio new --dry-run` checks, writes `studio.created` and queues
the brief to the new producer. An `outward` card (proposed by the stage from a carded `role.refused` or explicit
`backlot ask --run`) re-checks work-area containment, clean local git config, `GuardVerdict::Card` classification, and
`GitHead` / remote branch staleness, then spawns asynchronously in a stripped person environment (`BACKLOT_*` and
`GIT_CONFIG_*` removed, guard shims filtered off `PATH`) with output saved to `outward/c<n>.log`, a 120 s timeout, and
an `"outward"` completion notice back to the requesting role. The config gates the agent terminal, never the card pass.

The bar in the TUI (U7, D15, D16) is studio mode's own surface, not the agent's: it draws with or without a
`[hollywood]` table, since the studios, the new-studio frame and the setup line need no agent — the line opens the
in-TUI setup dialog, whose apply is the same `backlot hollywood setup`. The
agent's parts — the role state in the title, the headline, the cards and the `tell hollywood` line — appear only with
the table, and the stage side is unchanged: no table, no `bl-hollywood`. Under `--plain` the bar does not exist at
all, which is what keeps the parity harness's screen herdr's.

## Recovery

An hc server restart ends every terminal, including the stage; the person starts it again with
`backlot stage start` — hc never re-runs the original command, and `--keep` does not help.

When the stage starts, it replays each ledger and lists terminals by label. A role that should be live and is still
there is adopted. One that is gone is recorded `role.lost`, and the same pass clears the role's open attention — the
prompt or stall it named is gone with the terminal. A lost role that is still wanted is resumed with its runner's
resume flag into the recorded session ID as `role.resumed` with reason `recovered`, then told once, through the usual
notice path, that its terminal was lost, the session was resumed, anything running then has stopped and a
permission-prompted command must be run again.

A role closed on purpose — a stop, a recast, `studio.finished` releasing everyone — gets the same treatment in the
same pass, with reason `role closed`, and an attention left open on an already-closed entry clears on the next pass:
the prompt it named lived on that terminal and there is nothing left to answer, so it cannot dangle — a user-owned
one included.

**Finishing also clears the screen.** Stopping a role kills its program and removes its terminal, but the core can
refuse the removal while the kill is still landing, and the record outlives it as `finished` — a dead
`new`/`cast · <id>`/`producer` pane with "Terminal unavailable" in every client, closed by hand one at a time. So a
finished studio's `finished` records get their own `terminal.rm` from the stage. The stage owns role terminal
lifecycle already, so it is the layer that can see which records linger — `studio finish` records the decision and
stops there; it could not know what the core later refused. Ownership is the `backlot.studio`
label the launch set, never the name: another studio's record, a hand-named `bl-…` and the user's own shell are
untouched, and a record already gone is a clean `not_found`. Nothing is recorded — a terminal record is not history,
the ledger is. The studio stays in the document as `finished` and a person reopened from it resumes the recorded
session into whatever terminal that needs, so cleanup costs the reopen path nothing. Finished records are listed only
while some studio has finished, and a core that cannot list them just leaves the panes as they were. A removal the
core refuses is retried next tick and is not the studio's `error`.

A crash loop is bounded: the reducer keeps every `role.lost` time, so the guard survives the restart, and a third loss
inside ten minutes — or a lost role with no recorded session — raises a `cannot resume` attention instead of starting,
routed to the role's reviewer like a stalled one. A live terminal already carrying the role's name is re-joined by
recording `role.resumed` alone — no `terminal.run` — checked before the guard. Session discovery for a post-launch
runner keeps looking past the 60-second `session id not found` attention until the discovery stop, so a slow first
start still gets an id.

A studio whose ledger cannot be loaded is skipped and retried every tick — a repair at the same length is picked up —
while one whose act, watch or deliver pass fails is logged once and tried again on the next tick; either way the
summary shows the ledger error, otherwise the last pass error, as the studio's `error` line, and the other studios
carry on. Only losing the core ends the stage: a dead wire — `Closed` or `Protocol` on any call — or a lost event
stream. A timeout, a full queue or an unoffered method is one studio's refused pass, and a failed terminal-list
refresh keeps the last inventory for the next pass.

The panel shows a one-line "stage stopped" while keeping the last document on screen. `s`, Enter
or a click on the line runs `backlot stage start` on that machine — the stage runs as its own
process either way.

## Forward compatibility (#185)

Adding new ledger event kinds (such as `design.*`) uses a two-step rollout so older `backlot` processes never
corrupt a newer ledger or misallocate `seq`:

1. **Step 1 — tolerate unknown kinds read-only:** `v <= LEDGER_VERSION` lines with unrecognised `kind` strings
   deserialise as `EventKind::Unknown { raw_kind }` (`ledger.md`), keeping `seq`, `ts_ms`, and `by`, advancing
   `next_seq`, ignoring payloads during state reduction, and preserving raw JSON lines byte-for-byte during `studio
   migrate` or ledger rewrites. Read commands keep working; `store::append_locked` refuses writes with `"this ledger
   has records from a newer backlot (<kinds>); upgrade backlot"`; `backlot stage` pauses read-only for that studio
   while continuing sibling studios; and `StudioSummary` sets `needs_upgrade: true` + `unknown_kinds`. Lines with
   `v > LEDGER_VERSION` remain a corrupt-ledger error.
2. **Step 2 — enable new writers after Step 1 lands everywhere:** deploy Step 1 to every machine reading the shared
   state directories before rolling out binaries that write `design.*` events and default new studios to `design_gate =
   "required"`. Any remaining older host safely pauses read-only with an upgrade notice instead of colliding on `seq` or
   spurious wakeups.

## Studio panel

- **Geometry:** a right column, tiered by window width (38 at 160+, 36 at 100+, 28 under). The panel sits beside the
  panes only while the sidebar plus the column still leave at least 44 columns for the panes. Otherwise the expanded
  column resolves to the collapsed strip: beside the panes from 60 columns, a 16-column overlay at the pane surface's
  right edge under that — the dock never takes the whole surface.
- **States (R17, D6, D12):** closed, collapsed or expanded; the default is expanded. Closed draws nothing and is the
  state the parity harness checks (`--plain`). Collapsed is a narrow column of about 16 columns listing only the working
  roles, each with its status mark and the number of people working, such as `director` and `actors 2`. Expanded shows
  the people, and each role group can collapse on its own. The strip fallback above applies to whichever width the
  panel has.
- **Content (R19):** studios, each with its roles and the people in each role: runner and model, state, a working detail row (`working <dur> · "<progress>" <age> ago · <running> <dur>`), and what needs attention.
- **Work states (R17):** working, waiting for input, blocked, lost and closed each have their own colour and mark, from
  the palette in `chrome/status.rs`. Working is animated (spinning or a slow blink). The animation runs only while
  something is working, so an idle panel causes no redraws. The stage derives the state from the hook signals first
  (see "Hook signals"), per-runner screen patterns as the fallback, and publishes it in the summary, separately from
  whether the role is live, lost or closed. A role whose last turn failed draws `! last turn failed: <error>` beside
  it.
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

A task created with `--depends-on` may not be cast while a named dependency is open: the ledger refuses `task.cast`
until every dependency is `accepted` or `partially-accepted`, and the command also checks that each dependency's
accepted delivery end sha is already an ancestor of `backlot/<studio>/integration` — a task's worktree is cut from
the integration tip, so an unmerged dependency would leave it working without the code it needs. The refusal names
the open dependencies, or `merge t1 into integration first` for an accepted-but-unmerged one. While a task waits,
`next_steps` owes nobody its cast and `backlot status` shows `waits for t1` on the task's line.

A studio repo that is not a git work tree raises `not a git repository` and starts no actor or director — unless the
studio was bound with no git, in which case every role's cwd is the bound path itself. Every step is
idempotent: an existing branch or worktree is reused, a failure raises `work area failed` once with git's first stderr
line and is retried on a later tick, and a recast or a reopen reuses the task's tree — work continues on top. Worktrees
stay after a task closes; removing them is a later step.

### The bound path's shapes (D4)

The bound path is classified from the filesystem — the path itself first, then its direct children, never deeper — and
the classification is checked again at launch on the machine the studio runs on:

| Shape | What it is | Launch does |
|---|---|---|
| repository | the path is a git repository | nothing — or one empty initial commit when the repository has none |
| workspace | not a repository; one or more direct children are distinct repositories | records the member repositories; work areas hold one worktree per member |
| plain directory | no repository in it | `git init` plus `git commit --allow-empty -m "chore: initial commit"`, or binds with no git when the user says so |

Members are distinct repositories — `rev-parse --git-common-dir` is a member's identity, so linked
worktrees of one repository count once (the main checkout names the member) and per-member worktrees
cannot collide on the single checkout a branch allows.

In a workspace studio the work-area layout is per member: `<studio dir>/work/integration/<member>` on the member's
`backlot/<studio>/integration` (branched from the member's HEAD) and `<studio dir>/work/<task>/<member>` on the
member's `backlot/<studio>/<task>` (cut from that member's integration tip). The role's terminal cwd is the
`work/<task>` or `work/integration` directory itself, so every member sits side by side and a commit lands in the
member checkout it was made in. `deliver --commits` takes one or more comma-separated ranges: a bare `a..b` resolves
to the one member whose `backlot/<studio>/<task>` branch holds the tip, `member:a..b` pins a member explicitly, and
the ledger stores every range as `member:sha..sha`. A range that resolves in no member, or in several, refuses and
says why. The `cast` dependency check runs per member against that member's `backlot/<studio>/integration`. Declared
`[[checks]]` are refused on a workspace or no-git studio in this version — the director still runs the project's
checks in each member's integration worktree.

A no-git studio shares the bound path between all roles: no worktree isolation, no integration branch, `deliver
--commits` recorded verbatim, and the `depends-on` merge check skipped.

The picker's row label is the only affordance — no dialog follows a pick. A repository with commits shows its branch;
a repository with none shows `0 commits · will create initial commit`; a workspace shows `workspace · N repos` (distinct repositories — linked worktrees count once); a
plain directory shows `directory · will git init`. Step 4 restates the launch plan — `workspace over 3 repos: a, b,
c`, or `will run: git init && git commit --allow-empty -m "chore: initial commit" in <path>` (with the machine name
when remote) — and only there does a plain directory offer the explicit `no git (reduced isolation)` toggle.
`studio new` takes `--init` / `--no-git` for the non-interactive form; a plain directory without either is refused
with both options named, and the flags are refused on repositories and workspaces.

## Permission prompts (D9)

Each role starts with a permission profile from `runners.md`. A live role sampled `blocked` on two samples two seconds
apart raises one `permission prompt` attention per blocked episode, with the visible screen saved to
`attention/<seq>.txt`. An actor's episode notices the director, who may approve it when it stays inside the actor's own
work area; `backlot approve --person <role> --decision allow|deny` records `role.approval` and the stage sends the
runner's approval key — never without a recorded decision — then clears the attention. A prompt that touches real
sessions, production, accounts or global settings, or is destructive outside the work area, goes to the user and shows
in the panel; so does a prompt on any role that is not an actor.

The key lands only on the prompt it was made for: before it is sent, the screen on show is compared with the saved
`attention/<seq>.txt` through a per-runner *prompt block* — the whitespace-normalised region that names the prompt
(`runners.md` lists each). A prompt that changed under the recorded decision — an earlier prompt answered in the
terminal itself, the next one already up — is a different prompt: nothing is sent, the stale attention clears `prompt
changed before the decision was sent`, and the screen on show raises its own attention with `needs_user` computed on
it, noticed like any new prompt. A screen with no comparable block refuses the same way; a runner with no approval
keys is untouched. A prompt answered in the terminal itself — no decision recorded and the role out of its blocked
episode for a whole episode window — clears `prompt answered elsewhere`, so the next prompt always raises fresh.

Three prompts are the user's outright (S9b, S12f), and the director's `role.approval` on each is refused: a prompt on
a role that is not an actor — the director answers only an actor's; a prompt whose own block reaches for the
role-identity variables or the environment-clearing env flags; and a cursor file picker whose target the stage cannot
vouch for — `runners.md` lists the shapes — because the target must resolve strictly inside the role's own work area
and name no protected state, and anything outside, escaping, relative, `~`-rooted, truncated or unreadable is the
user's, as is a picker question the stage cannot name. The env-flag check reads the block — the whole screen when the
runner maps none — with whitespace and box-drawing characters removed, so a wrapped prompt box cannot hide a mid-word
`BACKLOT_`, `env -i` or `--ignore-environment`, and an earlier prompt's line still scrolling above cannot flag the new
prompt. It flags the attention `needs_user`: no notice to the director, `backlot status` and the panel mark it as the
user's.

**A prompt the director cannot answer is routed to the person.** An actor's prompt is the director's to answer, but a
director that is rate-limited, stalled or closed cannot. The deliver pass (`route.rs`) reads the director's open
`rate limited` / `stalled` attention and its lifecycle — a rate limit outranks a stall, which outranks a closed or
never-started role — and once the actor's open, director-owned prompt is past a one-minute grace (`ROUTE_GRACE_MS`),
it clears that attention `routed to the user` and re-raises it `needs_user` with the same saved screen. The detail
names the reason and the wait — `the director is rate-limited until 12:10; waiting 14m; screen saved to attention/<n>.txt`
— so the panel and Hollywood's `prompt` notice read as one line. From then it is the user's: the director's
`backlot approve` is refused like any `needs_user` prompt, and an already-flagged or non-actor prompt is never touched.
Only the re-raise's own reason text distinguishes a routed prompt in the ledger; the flag is what routes it. The state
is reloaded before the notices are built, so the new seq lands in the same pass.

## Guard (D3)

A role works against local state and controlled feature-branch pushes under its studio's `[push]` policy (`recorded` default allows non-protected branch pushes to recorded studio or member remotes; `any` and `none` widen or close auto-pushes). Benign local read-only probes bypass outward-action refusals: non-repo `git` metadata (`--version`, `-v`, `version`, `--help`, `-h`, `help`), layout-only `git rev-parse` flags (`--show-toplevel`, `--git-dir`, `--git-common-dir`, `--is-inside-work-tree`, `--is-bare-repository`, `--show-prefix`, etc. with no external `-C`/`--git-dir`/`--work-tree`/`GIT_DIR`/`GIT_WORK_TREE` escapes), read-only `git` queries inside the studio's own `work/*`, bound repository root, or workspace members (`status`, `log`, `show`, `diff`, read-only `branch`/`remote`/`config`, `ls-files`, etc., with `GIT_OPTIONAL_LOCKS=0` outside the role's own worktree so status checks never take the person's index lock), and read-only `gh` commands (`--help`/`-h`, `pr view|list|status|diff|checks`, `repo view|list`, `issue view|list|status`, `release view|list`, `gist view|list`, `workflow list|view`, GET `api`) are allowed without recording `role.refused`. Protected branches, tags (when permitted by policy), unrecorded remotes, and `gh pr create` classify as `Card`: the guard stops the role cleanly and records `role.refused` (`decision: "card"`, full `argv`) so `stage` proposes an `Outward` card (or the role submits one explicitly via `backlot ask --run [--why <r>] -- <cmd>`). Hard-refused calls (force/mirror pushes, `git hooksPath tampered`, unknown git global options, config/transport overrides, `gh` writes, `hc` control (`update`, `machine`, `server stop`, `run`, `exec`, `send`, `keys`, `connect`), interpreter code (`python -c`, `node -e`, heredocs) that writes `plugin.backlot.req` or `doc.set` or reads the core socket, detach tricks that leave the role's process tree (`systemd-run`, `tmux new -d`/`new-session -d`), `scp`, remote `rsync`, `ssh <host> <cmd>`, `npm`/`cargo publish`, `backlot server run|start|stop|adopt|service`, shell alias or env mutations, and explicit outside-reach `git` flags) record `role.refused` and stop outright, raising one informational `outward action` attention (`needs_user: false`, class `info`) per open episode. Evidence-backed noisy read-only `git` probe refusals — config-override rules for `alias.*`, `core.fsmonitor`, `core.hooksPath`, or `submodule.recurse`, plus `git path outside work area` for plain non-work `cwd` read-only `git` commands with no explicit outside-reach flags — are still refused by the guard, but suppress `role.refused` and `outward action` and instead increment `<studio>/suppressed-probes` so `stage` logs a concise counter in `stage.log`. It clears `role moved on` once the role writes anything else to the ledger after it, or the stage delivers it new work. `role_env`/`hrole_env` stamp `BACKLOT_TERMINAL`, and user-privileged CLI verbs (`card`, `--as user`, `approve --as user`, `perms revoke`, and `backlot server …`) verify the calling process tree against `BACKLOT_TERMINAL` so a subshell cannot escape by unsetting `BACKLOT_*`. The guard sits at three runner-independent layers written before every start and resume alongside per-session hooks:

1. **Command shims (`roles/<role>/bin`).** `role_env` and `hrole_env` place `<role>/bin` first on `PATH`, ahead of `exe_dir`. Small shell wrappers invoke `backlot guard <name> -- "$@"`. Writes are refused (`gh` writes on `pr`/`release`/`repo`/`issue`/`gist`/`secret`, `workflow run|enable|disable`, and `api` with non-`GET` method or `-f`/`-F`; `hc update`, `hc machine`, `hc server stop`, `hc run|exec|send|keys|connect`; `socat`/`nc`/`ncat` naming a server token; `systemd-run` and detached `tmux new`/`new-session`; `scp`; `rsync` with a `host:` spec; `ssh <host> <command>`; `npm publish`; `cargo publish`); allowed calls resolve and `exec` the real binary past `<role>/bin` on `PATH` (skipping shim markers).
2. **`git push` hook (`roles/<role>/git-hooks`).** Role env appends `core.hooksPath=<role>/git-hooks` via `GIT_CONFIG_COUNT` / `GIT_CONFIG_KEY_n` / `GIT_CONFIG_VALUE_n` without clobbering any existing `GIT_CONFIG_*` entries. `pre-push` calls `backlot guard git-push -- "$@"`, while client commit/merge/rebase hooks pass through to the repository's own hooks path (`core.hooksPath` or `--git-common-dir`/hooks) when one exists. The user's own `git push` outside the role environment is untouched.
3. **Refusal record and attention.** A refused call prints `refused by backlot: <command> is the person's to run; ask with backlot ask`, exits 126, appends `role.refused` (`role`, optional `task`, `command` capped at 200 chars, `cwd`) in studio ledgers, and raises one `outward action` attention (`needs_user: false`, class `info`) once per open attention episode (except suppressed read-only git probe refusals, which increment `suppressed-probes` for `stage.log`). Hollywood gets the same shims and hooks.
4. **Hook & runner prefix defence (`agy`).** `agy`'s `PreToolUse` hook runs BEFORE `.agy-settings.json` `permissions.deny`. `backlot hook --runner agy` inspects `run_command` `CommandLine` through `guard_rules`, records `role.refused` (or increments `suppressed-probes` on suppressed read-only git probe refusals) and one `outward action` attention per episode via `guard::record_verdict`, and returns `{"decision":"deny","reason":"refused by backlot: <command> is the person's to run; ask with backlot ask"}` within a 1.5 s internal fail-open budget (per-hook timeout 5 s). Allowed calls, internal errors, and timeouts return `{"decision":"allow"}` because empty output or `{}` denies `agy` tool calls. `runners::agy` also splices `permissions.deny` (`command(<prefix>)`, no trailing ` *`) for `gh api`, write-specific `gh repo`/`gh release`/`gh issue` subcommands, `hc`, `systemd-run`, `tmux`, `scp`, `rsync`, `ssh`, and `publish` into session `.agy-settings.json` beside `toolPermission: "always-proceed"` as defence in depth when hooks are skipped or not yet loaded, leaving read-only `gh` (`view`/`list`/`status`), `gh pr`, and `git` to the hook and PATH shims.

## Question dialogs and generic fallback (D2, R7b)

Role prompts forbid asking questions on screen (`backlot ask` and `backlot tell` reach the right listener), and Claude
session settings deny `AskUserQuestion`. When an interactive dialog still appears on a role's screen — either via a
recognised non-permission picker (`agy`'s `Question N/M:` plan picker), structured `status.state: "blocked"` with
`kind: "question"`, or `dialog::generic_dialog`'s 12-line fallback rule (an option cursor `❯`/`>`/`→`/`›` + space + text
paired with a contiguous sibling option or dialog footer, excluding idle composer boxes, code fences, and `esc to cancel`
alone) — `work` reads `Blocked`, keeping the send gate (`gate_open`) closed. After `EPISODE_MIN_MS` (2 s), the stage
raises one `question` attention per episode with `attention/<seq>.txt` saved (permission prompts remain `permission
prompt`). Actor `question` attentions notice the task's reviewer (`notices::reviewer_for`), `director` and `writer`
notice `"producer"`, and `producer` and Hollywood surface with `needs_user` (`can_answer_prompt: false`, no approval keys
sent); exiting the dialog for two seconds clears the attention as `dialog closed`.

## Defaults that can still change

**Requirement changes.** Each task is pinned to the requirement version it was dispatched under. A new version marks
unfinished tasks as behind, and the panel shows it. The director decides per task to continue, pause or redo, and the
producer tells the user what is affected. Nothing is killed automatically.

**Usage.** Show only what a runner reports. Record wall time and rework rounds for every task, and show the rest as
unknown rather than estimating.

**Experimental production.** Designed after the first version. The studio record already carries a `mode` field.
