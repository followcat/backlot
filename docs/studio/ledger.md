# Studio ledger

Status: agreed for the first version.

The ledger is the durable record of a studio. It answers what was asked, who did what against which version of the
requirement, and what happened when it was reviewed.

## Storage

```
$XDG_STATE_HOME/backlot/<core id>/
  config.toml
  live.json           fresh per-role work samples and aggregated accounts summary
  accounts.json       persistent quota-stall state (`backlot-accounts/1`) across restarts
  studios/<studio>/
    events.jsonl      append-only log, the source of truth
    events.lock       flock for appends
    req/v<N>.md       requirement versions
    plan/             writer's plans and acceptance lists
    reports/          deliveries and node reports
    messages/         role-to-role and stage notice bodies, and digest-<seq>.md merges
    checks/           detached check worktrees and the <delivery-seq>-<name>.log each records
    outward/          logs captured from executed outward cards (`c<n>.log`, capped at 1 MiB)
    attention/        screens saved when an attention was raised
    prompts/          the rendered role prompt each fresh start was given
    work/integration  the director's worktree, backlot/<studio>/integration
    work/<task>       each task's actor worktree, backlot/<studio>/<task>
  hollywood/          the overseer's own ledger — beside studios/, never inside one (D13)
    events.jsonl      same envelope, own kinds and own sequence
    events.lock
    messages/         its copies of the `tell producer` files it sent studios (R26)
    reports/          bodies of its `hollywood.reported` headlines
    drafts/<studio>/  a studio-new card's draft — cast.toml and brief.md (D19)
    AGENTS.md         its role prompt        (H3)
    memory.md         kept across restarts   (H3)
```

`work/` trees stay after a task closes — a reopen resumes in the same folder. Removing them is not
part of this step.

- `backlot` commands append directly under the lock. One lock gives one total order, and no socket or daemon is needed
  to record an event.
- The stage reads new lines and acts on them.
- State is a pure reduction over the log, so restart recovery is a replay.
- Facts are never rewritten. A later finding or a correction is a new event.
- Actor records are computed across all studios' logs (D4). There is no second copy.

## Events

Every event has `v`, `seq`, `ts_ms`, `kind`, and `by` (the role and session). `v` is the ledger
schema version — `1` today. Lines written before versioning carry no `v` and read as `v: 1`;
a `v` newer than the binary's `LEDGER_VERSION` is refused with a corrupt `"ledger event version vN is newer than supported v1; upgrade backlot"` error, and malformed JSON, missing envelope fields (`seq`, `ts_ms`, `by`, `kind`), known kinds with invalid payloads, or `v: 0` return a line-numbered corrupt error (an unterminated trailing line without a newline is ignored on read as a torn write and truncated before any append).

A valid JSON line with `v <= LEDGER_VERSION` and an unknown `kind` (for example a record added by a newer backlot release without bumping `v`) deserializes as `EventKind::Unknown { raw_kind }`, preserving its `seq`, `ts_ms`, and `by` and advancing `last_seq` / `next_seq` past it while ignoring its payload during state reduction. `backlot studio migrate` and any ledger rewrite preserve unknown-kind lines byte-for-byte. While a studio's ledger holds any unknown kind, CLI and stage read commands keep working, every write (`append_locked`) is refused with `"this ledger has records from a newer backlot (<kinds>); upgrade backlot"`, `backlot stage` pauses read-only on that studio (no reconciles, launches, watches, checks, relabels, or terminal sends; sibling studios keep running), and the studio's entry in `plugin.backlot.summary` sets `needs_upgrade: true` and `unknown_kinds` so the TUI Lot, Call Sheet, and studio frames show `"upgrade backlot"`. `backlot studio migrate [<studio> | --all] [--dry-run]` walks a studio's
log (and, with `--all`, Hollywood's) under the lock, rewrites any unversioned lines to the
current `v` through the model's step-by-step migration pipeline, keeps a `events.jsonl.v0.bak`
backup, and verifies the migrated log replays to the same state — a current log is a no-op that
prints `already at ledger v1`.

| Kind | Main fields |
|---|---|
| `studio.created` | title, repo, mode, candidates, role configurations, `check_env`, declared `checks`, optional `task_budget_s`, `[limits]` runner caps, `max_actors`, `design_gate` |
| `studio.cast` | the same cast fields `studio.created` holds — roles, candidates, rework cap, timings, `check_env`, declared `checks`, `task_budget_s`, `[limits]`, `max_actors`, `design_gate`, push knobs — plus the optional `card`. The newest `studio.cast` (or `studio.created`) is the settings the stage uses; earlier events keep what they recorded. The binding (`repo`, `members`, `no_git`, `mode`) and `push.remotes` are not cast and never change. A fixed role whose identity moves is also recorded as a `role.recast`, appended first. |
| `role.started` / `role.lost` / `role.resumed` / `role.closed` | role, terminal, actor identity, session ID, reason |
| `role.reopen` / `role.release` | role |
| `role.restart` | role, `fresh`, optional reason |
| `role.session` | role, session ID |
| `role.recast` | role, new actor identity, reason, optional `card` |
| `role.approval` | role, decision (`allow` or `deny`) |
| `studio.finished` | reason |
| `studio.reopened` | reason, optional `card` — `user` only; clears the finished marker so recasts, cast changes and new tasks work again |
| `req.version` | version, file, summary |
| `plan.published` | kind (plan, acceptance), file, requirement version |
| `task.created` | id, title, requirement version, acceptance, dependencies |
| `task.cast` | task, actor identity, reason, optional priority, by (the caster) — `--kind audit` requires `permission_profile == "readonly"` |
| `task.priority` | task, priority |
| `task.delivered` | task, delivery number, actor identity, resolved commit range (`sha..sha`, or empty `""` on `readonly`/`audit` tasks), report file, requirement version, actor-reported checks, optional `usage` |
| `task.reviewed` | task, delivery number, reviewer, verdict, written conclusion, findings, upstream changes (`upstream_changes` refused on `readonly`/`audit` tasks) |
| `task.checked` | task, delivery number, check name, exit code, `timed_out`, output log, duration — `stage` only (not scheduled for `readonly`/`audit` empty-commit deliveries) |
| `task.finding` | task, delivery number, findings, by |
| `task.state` | task, paused, resumed, abandoned, or taken over, with reason |
| `message.queued` / `message.delivered` | from, to, file, purpose, optional `source_seq`; `delivered` carries the target seq |
| `message.dropped` | target seq, reason — `stage` only |
| `report.sent` | from, to, node, file |
| `attention.raised` / `attention.escalated` / `attention.cleared` | role, reason, detail, `needs_user`; `attention.raised` carries optional `account` and `reset_at_ms` when `reason == "rate limited"`; `escalated` carries the target attention seq and replacement detail — `stage` only |
| `stage.answered` | role, dialog (`folder-trust`), key, working directory — `stage` only |
| `ask.answered` / `ask.acknowledged` | role, target ask seq — `stage` / `user` only |
| `role.progress` | role, optional task, one-line `text` (<= 200 chars) — any live studio role |
| `outward.requested` | role, optional task, `command`, `cwd`, `argv`, optional `why` — explicit `backlot ask --run` card request |
| `outward.completed` | `card`, `source_seq`, `outcome` (`Succeeded`/`Failed`/`TimedOut`/`Denied`/`Stale`), optional `exit_code`, `output_tail`, optional `log_file` — `stage` only |
| `design.opened` | `version`, `round`, `suggest_skip` (`bool`, omitted when `false`), optional `reason`, `doc_file` — `producer` or `user` |
| `design.entry` | `version`, `round`, `slot` (`proposal` / `buildability` / `summary` / `user_input` / `supplement`), `file`, optional `source` (`tell` / `hook` / `producer_note`), `refs` (`u64[]`, omitted when empty), `questions` (`DesignQuestion[]`: `{ id, prompt, recommendation, section_ref? }`, up to 3, omitted when empty) |
| `design.signed` | `version`, `round`, `mode` (`approved` / `skipped`), `doc_file`, optional `note` — `user` only |
| `correction` | target event, changed fields, reason |

`role.lost` means a role recorded as live vanished unexpectedly (its terminal is gone with no
`role.closed`). `role.closed` means the stage released the role on purpose — a stop or a recast
replacement — so a deliberate stop is never mistaken for a loss, and a closed role keeps its actor
identity and session ID: it stays resumable while its studio is live. A later `role.started` or
`role.resumed` clears the closed state.

`role.recast` swaps the identity a fixed role plays from then on. The reduced state uses the newest
identity for that role; the lifecycle record and every earlier event keep the identity they were
recorded with, so deliveries and reviews stay attributed to whoever made them. `card` is the `c<n>`
marker when a card carried the event out, and works like the marker on `task.state` — a stage
restart that finds it appends nothing twice. The stage carries a recast out by closing the old
terminal (`role.closed`, reason `recast`) and starting a fresh `role.started` under the new
identity; it never resumes the old session id, which belongs to the runner that left.

`role.reopen` marks a closed person `held` on its reduced state: the stage wants it back and starts
it with the session it closed with — the launch lands as `role.resumed` (reason `reopened`), not
`role.started`. `held` clears on `role.release` or `role.closed`; a released role stops being
desired, so the usual stop closes it. `role.reopen` is refused unless the role is closed, has a
session ID, is not already held and no other live role uses its terminal name; `role.release` is
refused unless held. Both are user- and director-facing events. `role.release` is `close`: it
releases a held person and nothing else — a fixed seat (producer, director, writer) refuses with
"the <role> is a fixed seat — use restart, or finish the studio", and an actor with no hold refuses
with "the <role> has nothing to close — it closes with its task".

`role.restart` is the in-place reload (L2): the stage stops the role's terminal deliberately
(`role.closed`, reason `restarted` — never counted as a loss) and launches it again under the same
identity and terminal name. The start lands as `role.resumed` (reason `restarted`) when it resumes a
recorded session, or `role.started` (reason `restarted`) for a `fresh` one; the reduced state clears
the request on that start and stamps `restarted_ms`, which the summary's role row and Hollywood read.
`--fresh` starts a new conversation — the old session id is not kept. Refused for a closed person
(reopen is that) and a finished studio. The same request lands in Hollywood's own ledger as
`role.restart` for the overseer.

Tasks carry a `priority` — `low`, `normal` (the default) or `high`. `task.cast` may set it, and
`task.priority` changes it on an existing task. It orders the panel's display only (R20): nothing in
reconcile or scheduling reads it.

Adapters that choose the session ID at launch record it in `role.started`. Ones the runner assigns
itself — found by a file scan or a CLI query after the terminal is up — land in `role.session`, which
the stage records once it finds the ID. A `role.session` for a role the log does not know is refused.

For a runner that assigns its own ID, the watch looks for it at most once every five seconds while
the role is live and session-less, until a hard stop ten minutes after the role's start — past the
60-second `session id not found` attention, so a slow first start still gets an ID. That age is
measured from the later of the role's `role.started` and the stage process's own start: a
`role.started` replays unchanged across a stage restart, so flooring the age at the process start
gives every still session-less live role a fresh ten-minute window after a (re)start. A restarted
stage therefore looks again at a role already past the stop, finds the ID, and records
`role.session` plus an `attention.cleared` for its open `session not found`.

Three `message.queued` purposes carry fixed meanings: `plan request` records `plan`
(to the writer), `dispatch` records `dispatch` (to the director), and `tell user --ask` records
`ask` as the explicit person-question mark. The writer is wanted from a
`plan` message until the next `dispatch`; other plain `tell`/`ask` messages carry no purpose, so a
plain `tell user` (or `tell user --note`) remains a report and never becomes a question merely
because it is newest. When a studio role (`producer`, `director`, `writer`, or `actor-<task>-c<n>`)
calls `backlot tell user` via the CLI with `?` or `？` outside inline code spans, fenced code blocks,
and URL query strings (`?key=`), the CLI requires either `--ask` (record a question) or `--note`
(explicitly send as plain information) and refuses with exit code 2 otherwise; `plugin.backlot.req`
(`tell_text`) and calls to other roles are unaffected. A plain
`tell` to the writer or director also keeps that role wanted — while it is undelivered, and after
delivery until the role writes to the ledger again — so a tell to a role closed as `no longer
desired` starts it again and delivers the message, the way a `plan` request does. A `tell` to an
actor whose task is finished or abandoned is refused; the note names the task and its state.

`message.queued` may carry `source_seq`, the seq of the ledger event it was generated from. The
stage's automatic notices — a delivery to whoever cast the task, a `changes_requested` review to
the actor, a blocked task or a requirement revision above v1 to the director, an actor's permission
prompt to the director, a `deny` approval to the denied role — are ordinary queued messages whose
body file lands under `messages/` first.
A notice is owed only while no message has its source event's `seq`, so a stage restart repeats
nothing. A `report.sent` to a role is deliverable the same way; `to: user` is never sent to a
terminal. `message.delivered` marks the target — a message or a report — only after `terminal.send`
reports sent. A crash between the send and the record can send the same line twice on restart;
that is harmless.

At send time every queued message carrying a `source_seq` is checked against the event it relays
(S9d): when the source no longer stands the stage records `message.dropped` with a short reason
and sends nothing — the table of what counts as stale is in `design.md`. Queued mail addressed to
a specific task actor (`actor-<task>-c<n>`) whose session has closed for good (finished, closed,
or exited with no resume and no live terminal for that actor id) likewise drops with reason
`actor_closed`; mail to fixed roles (`producer`, `director`, `writer`) and to paused, recovering,
reopened, restarting, or still-live actors is kept. A dropped message is
never pending again — not for the outbox, not for the idle rule that gates nudges — but it still
counts as handled in the dedup set, so the same notice is never re-queued; a dropped message is
never marked `message.delivered`, and remains visible with its reason in `status` and `exchange`.
Only `stage` may record the event, and only on a queued message
still undelivered — a send already in flight is exempt, since it cannot be un-sent. Fixed-role
messages without a `source_seq` (`tell`, `ask`, `dispatch`, the plan request) and reports are never
dropped.

When a role is owed two or more items at send time they go out as one digest: the stage writes
`messages/digest-<highest seq>.md` — one section per item — then sends the single line
`[backlot] stage → <role>: read <path> (<n> items)`. When the send answers, every item it covered
gets its own `message.delivered`. The same crash window applies: a crash between the send and
the records can send the digest twice.

`task.cast` records `by`, the role that cast. A producer's cast (the light path) routes the
delivery notice to the producer; a director's or unattributed cast routes it to the director.

`task.delivered`'s commit range is resolved before the append (S9b): `backlot deliver --commits A..B`
runs git in the task's `work/<task>` area, resolves both ends to full shas, refuses an empty range,
and refuses a tip that `git merge-base --is-ancestor` does not place on the task's
`backlot/<studio>/<task>` branch. The ledger holds `sha..sha`, never the symbolic input; the model
itself runs no git. On a `readonly` or `audit` task, `--commits` is refused: `backlot deliver
--report <file>` verifies every worktree is clean with unmoved `HEAD` (zero commits ahead of
integration), records an empty range (`""`), and skips stage `[[checks]]`. Audit tasks cannot be
depended upon, cannot carry `upstream_changes` on review, and `guard` refuses merging any audit
branch into `integration`.

`task.delivered` may carry `usage` — the runner's own reported figures, optional and absent by
default (P4a): `input_tokens`, `output_tokens`, `cache_read_tokens` and `cost_micros_usd`
(integer micro-dollars, so the JSON stays exact — `backlot deliver --cost-usd 0.08` parses to
`80000` with no float). They are facts the actor reports, not measurements backlot takes, and
they aggregate the same way — `StudioState` sums every delivery's usage, an actor's record sums
only its own, and the `backlot/1` summary carries the totals only while a counter is non-zero.
Like every added field they are serde-optional: old logs and old summaries read unchanged.

`task.checked` is the stage's own check record (S9b part 2): `studio.created` carries the cast's
`[[checks]]` — a name, the `run` command and `timeout_s` — and `check_env`, the stage-environment
names a check may see. Each declared check runs once per delivery on the stage, `sh -c` in a
detached worktree at the delivery's end sha with the environment stripped to a passthrough list
plus `check_env`; the timeout escalates TERM then KILL, and the capped log lands at
`checks/<delivery-seq>-<name>.log` before the event. An append that is not by `stage`, names an
undeclared check, targets a delivery that does not exist or repeats a `(delivery, check)` pair is
refused. A pass is exit 0 — a signal exit or a timeout is not. An accepting or partially accepting
`task.reviewed` is refused while a declared check has no record — the user alone may accept over
pending ones — and refused unless its conclusion names every failing or timed-out check; a
`correction` producing such a review is gated the same way. The reviewer's delivery notice waits
the same, then lists what the stage ran — pass, fail or timed out, with its log — apart from the
actor's own `--check` claims, and `backlot status` marks each declared check `running`, `pass` or
`fail` on the task line. A report file copied into the ledger gains a "Accepted with failing
stage checks" section while a task accepted over failures exists.

`role.approval` records a director's or user's decision on an open `permission prompt` attention —
refused without one, and refused for the director when the prompt is the user's (S9b): a flagged
attention, or a prompt on a role that is not an actor. An actor's unflagged `permission prompt`
opens `needs_user: false` (`stuck` — handled by the director) and escalates via `attention.escalated`
to `needs_user: true` with its wait time and reason either at once when the director is rate-limited,
stalled, or not live, or after `default_stall_limit_s` (10 minutes) if the director does not answer.
A permission prompt is flagged
`needs_user` when its saved screen, flattened of whitespace and box-drawing, reached for
`BACKLOT_*`, `env -i` or `--ignore-environment`; such an attention owes the director no notice and
shows as the user's. The stage sends the runner's approval key while the prompt still shows, then
`attention.cleared` lands; a prompt already gone clears with no key sent. The stage never answers a
prompt without a recorded decision, and a decision recorded before the attention does not answer a
later prompt. A `deny` also owes the denied role a notice naming who denied it — the esc
interruption alone leaves the role waiting for input.

`stage.answered` is the stage's own bookkeeping for a dialog it answered on a role's screen —
Claude Code's first-use folder-trust dialog today, with the key that accepted and the working
directory it was answered in. It is not an attention: it feeds no `!N` count, no digest, no notice
and nothing clears it; the role's drawer and `backlot status` show when it happened. A dialog that
shows up outside the role's work area is never answered — it raises a `needs_user` attention
(`folder trust refused`) instead, and the switch `[stage] answer_trust` in the user config opts
the whole pass out.

`ask.answered` is the stage's durable record that an open `tell user` at `target` was answered in
that role's terminal: the role started a new turn (`UserPromptSubmit` / `PreInvocation` on runners
with a prompt hook, or `waiting` → `working` on runners without one) while nothing had been sent
to its terminal and no restart, resume, recast, start or close happened since `target`. It clears
`RoleSummary.asks` (and the panel's waiting count) while leaving the question in `exchange`.
`ask.acknowledged` is the user's own receipt for the same target: it also clears the open question
without removing the exchange turn. Only the user may record it, and only while that explicit ask is open.

`role.refused` records a role's attempted outward command (`role`, optional `task`, `command` capped at
200 chars, `cwd`, and optional `decision` / `argv` when cardable or guarded) refused by `backlot guard`,
hooks, or the per-role `pre-push` hook. Harmless read-only `git`/`gh` probes (`git --version`,
`git --help`, layout-only `git rev-parse`, studio-wide read-only `git`, `gh pr`/`repo`/`issue`/`release`/`gist`
reads, `gh --help`) are allowed directly; read-only `git` probe refusals from config-override
rules (`alias.*`, `core.fsmonitor`, `core.hooksPath`, `submodule.recurse`) or `git path outside work area` on a
non-work `cwd` with no explicit outside-reach options are still refused by the guard but suppress
`role.refused` and `outward action`, incrementing `<studio>/suppressed-probes` (summarized as a
counter in `stage.log`). (`git hooksPath tampered` and `unknown git global option` are never suppressed
and still record `role.refused`.) For recorded refusals, the event also raises one `outward action` attention
(`needs_user: false`) once per open attention episode for that role and command. The refusal is
informational: it stays on the studio's attention list and the role's record, but it is `info`, so it
never counts toward the person's needs-you list, the lot pill or notifications. The role sees the
refusal line telling it to `backlot ask` when it really needs the command, and the attention clears
`role moved on` as soon as the role writes anything else to the ledger after it, or the stage delivers
it new work. When `decision` is `card`, the stage proposes an `Outward` card in
`hollywood/events.jsonl` (`by: stage`) and raises no attention at all.

`role.progress` records a role's voluntary one-line note (`role`, optional `task`, normalized `text`
with control characters stripped and length capped at 200 chars). Only roles running in a role
environment (`BACKLOT_ROLE` + `BACKLOT_STUDIO`) append it; `backlot progress` outside a role
environment exits with a usage error naming the variables it requires. The stage never requires it
and never raises nudges or attentions if unused; `RoleState.progress` keeps the latest `{ text,
ts_ms }` and `RoleSummary` exposes it alongside live descendant `running` (`{ cmd, since_ms }`,
published only while `work == working`); `doing` (`Option<String>`) stays unchanged for older clients.

`studio.finished` — the person's alone — is refused while any task is not terminal, naming the
open tasks; a role that runs `studio finish` is refused with `finishing the studio is the person's
call; say in your report that it is ready, and they finish it from the lot`, and nothing is
recorded. From then on `desired` holds only explicitly reopened people, so producer and director
close the usual way and keep their sessions. `role.reopen` still works after finish. A studio with
no requirement and no tasks is refused too (`nothing was built yet (no requirement, no tasks) — add
--force to finish anyway`) unless the command passed `--force` — the guard is the CLI's, so an
accidental finish of a just-made studio is caught before the event lands. The finish also answers
every open `tell user`: a role's `asks` projection reads a `studio.finished` after the question as
the answering event, so a finished studio shows no open question (L6).

`design.opened`, `design.entry`, and `design.signed` record the opening design discussion per
requirement version when `design_gate` is `required` (`required` by default for new studios;
existing studios without `design_gate` on `studio.created` stay `off`, and `design_gate = "off"`
disables it per studio). `design.opened` (`producer` or `user`) starts Round 1 (`round: 1`,
`suggest_skip: false`), posts a skip suggestion (`round: 1`, `suggest_skip: true`, optional
non-empty `reason`), replaces an open `suggest_skip` with Round 1, or opens Round `N+1` once
`user_input` follows the previous round's summary (`round` matches the prior round `+ 1`,
`doc_file` names `design/v<version>.md`). `design.entry` writes one immutable section file (`file`
under `design/v<version>/`) for one slot in order: `proposal` (`writer`, from `waiting_proposal`),
`buildability` (`director`, from `waiting_buildability`), `summary` (`producer`, from
`waiting_summary`, with `0..=3` `questions` whose `id`, `prompt`, and `recommendation` are
non-empty and optional `section_ref` is non-empty when set), `user_input` (`user`, `producer`, or
`stage`, with `source`: `tell` / `hook` / `producer_note`, in `waiting_user`), and `supplement`
(`writer` or `director` during `waiting_user` with non-empty `refs` pointing to existing `messages`
or `design.entry` `seq`s). `design.signed` (`user` only — refused from role terminals and `roles.rs`)
closes the active requirement version as `approved` (from `waiting_user`, freezing
`design/v<version>-final.md`) or `skipped` (from `suggest_skip` or `waiting_user`, creating no
`-final.md`). A new `req.version` resets any unsigned active `design` to `idle` and preserves
signed versions in `design_history`.

## Hollywood's ledger

The overseer keeps its own log at `hollywood/events.jsonl` beside `studios/` — the same envelope
(`seq`, `ts_ms`, `by`, `kind`), the same lock-and-fsync append, its own sequence, and a strict
append-time validation of its own. It is not a studio: `studios/` lists never contain it, the name
`hollywood` is reserved for it, and a studio's role rules see it only as a writer of a plain `tell`
message to the producer (R26).

| Kind | Main fields |
|---|---|
| `role.state` | lifecycle motion (`started`/`lost`/`resumed`/`closed`), terminal, actor, session, reason — `stage` only; the studios' four role events folded into one (H3) |
| `role.restart` | `fresh`, optional reason — `user` only; the stage closes (`restarted`) then relaunches `bl-hollywood`, resuming its session unless `fresh` |
| `role.progress` | role, one-line `text` (<= 200 chars) — `hollywood` only |
| `card.proposed` | the exact `action` (D18, including stage-proposed `Outward` cards), a one-line `why`, and the `base` (`GitHead` for outward) — `hollywood` or `stage` |
| `card.applied` | card id — `user` only |
| `card.cancelled` | card id, reason — `user` only |
| `card.withdrawn` | card id, reason — `hollywood` only, on its own open card |
| `card.superseded` | card id, `by_card`, reason — `hollywood` or `stage`, appended with the newer proposal so a batch apply cannot run both |
| `card.stale` | card id, reason — `user` at apply time on an open card, `stage` on an applied one (H3) |
| `card.done` / `card.failed` | card id, optional detail / reason — `stage` only (H3) |
| `hollywood.reported` | headline (one nonempty line), optional body file — `hollywood` only |
| `message.sent` | to, optional studio, text or file, optional `source_seq` — `user`→`hollywood`, `hollywood`→`user`, `stage`→`hollywood` (notices), or `hollywood`→a studio's `producer` |
| `message.delivered` | target seq — `stage` only, after a successful `terminal.send` (H3) |
| `message.dropped` | target seq, reason — `stage` only, on a queued notice whose source moved on (H3) |

A card's **base** is the fact it was proposed against — `studio_absent` for `studio-new`,
`person_held`/`person_closed` for `close`/`reopen`, `task_priority { was }` for `priority`,
`task_status { was }` for `pause`/`resume`/`abandon`, `cast_seq { was }` for `cast`, and
`studio_finished` for `reopen-studio`. The reducer holds cards by seq; staleness is a
pure read of the card against the studio's state, checked at apply time (a moved base records
`card.stale` instead of `card.applied`) and again when the stage executes an applied card.
`card.done`/`card.failed`/`card.stale`-by-stage are refused on a card that is not `applied`; an
applied card records `applied_seq`, a closed one `closed_seq`.

`message.sent` may carry `source_seq`, like the studios' `message.queued` — the seq of the event a
notice relays: in the named studio's ledger, or Hollywood's own for card outcomes and the
recovered-terminal word. `source_seq` dedups a restart and names what to check for staleness;
a `failed` notice has no event to point at and carries the stage's error text in `text` instead.
`message.delivered` marks a sent message, `message.dropped` a queued one whose source no longer
stands — delivered or dropped, a notice is never owed again. Stage wakeup notices are emitted for
`needs you` attentions, `stuck` attentions open past `default_stall_limit_s` (10 minutes — with
waited duration and director reason on overdue actor prompts), and kinds with their own labels
(`finished`, `failed`, `card`, `prompt`, `recovered`).

Hollywood's `message.sent` to a producer is its own copy of a `tell producer` — the studio's
`message.queued` remains the delivery record; the file lands under Hollywood's `messages/` too.

## Task states

```
created -> cast -> running -> delivered -> accepted
                     ^            |
                     |            +-> changes requested -> running (next delivery)
                     +--- paused  +-> partially accepted
                                  +-> blocked
```

A task can also be abandoned from any state. Reaching the rework cap sends it to the producer.

## Actor identity

An actor is the complete configuration that did the work, not a model name:

`runner + runner version + model + thinking depth + permission profile`

Each delivery is recorded against the identity that actually made it. Recasting mid-task does not move earlier deliveries
to the new actor.

## Evaluation rules (R12, R16)

These follow directly from the requirement:

- Review is evaluation. There is no separate scoring agent. Every delivery produces a new observation.
- Several deliveries of one task remain one task. A final pass does not erase earlier failures, and it is not counted as
  a separate success.
- Each delivery is bound to the requirement version it was dispatched under, so a changed requirement is not recorded as
  an actor's failure.
- An actor's own debugging before delivery is normal work. Only what reaches the upstream counts as rework.
- Message counts, changed-line counts and tool-call counts are not quality measures.

## Review records (D8)

Each review records a verdict, the findings, and one written conclusion from the director. The first version has no
numeric score or grade.

**Review findings.** Each finding has a stable defect ID, a summary, a severity and a cause. A defect that survives a
rework keeps its ID, which is what shows repeated rework. One defect is one record, even when several roles comment on it.
A defect found outside the review loop — during integration or real use, even after the task closed — is recorded as
`task.finding` on the delivery it belongs to; it changes no task state.

| Cause | Meaning |
|---|---|
| `delivery_defect` | An explicit requirement was not met |
| `unresolved_rework` | The same defect is still there after clear feedback |
| `regression_from_fix` | A fix introduced a new problem |
| `requirement_change` | The user added or changed a requirement |
| `upstream_error` | The plan or instructions were wrong |
| `environment` | The environment or tooling failed |
| `research_negative` | The experiment was done correctly and did not support the hypothesis |
| `unattributed` | Not known yet. The default until there is evidence |

**Upstream changes.** When the director fixes something itself, record what changed and why, and link the defect only if
the delivery really had one. This does not imply the actor could not have fixed it.

**Record view.** Per actor identity and task kind: first-delivery result, whether rework closed defects, repeats of the
same defect, upstream changes or takeovers, final acceptance, later defects, and wall time. Fewer than five tasks shows
"not enough data".

## Metrics and digest

`backlot metrics` aggregates quality, checks, findings, continuity, attention classes, rate-limit clusters,
approvals, guard refusals, Hollywood notices, and timing percentiles directly from studio ledgers and
`hollywood/events.jsonl`. `backlot digest` builds the period report and `design v<N> §<id>` coverage matrix
from the ledger plus task acceptance files. Both skip `unknown` events cleanly and see [metrics.md](metrics.md) for
exact metric definitions and the JSON schema.
