# Studio ledger

Status: agreed for the first version.

The ledger is the durable record of a studio. It answers what was asked, who did what against which version of the
requirement, and what happened when it was reviewed.

## Storage

```
$XDG_STATE_HOME/backlot/<core id>/
  config.toml
  studios/<studio>/
    events.jsonl      append-only log, the source of truth
    events.lock       flock for appends
    req/v<N>.md       requirement versions
    plan/             writer's plans and acceptance lists
    reports/          deliveries and node reports
    messages/         role-to-role and stage notice bodies
    attention/        screens saved when an attention was raised
    prompts/          the rendered role prompt each fresh start was given
    work/integration  the director's worktree, backlot/<studio>/integration
    work/<task>       each task's actor worktree, backlot/<studio>/<task>
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

Every event has `seq`, `ts_ms`, `kind`, and `by` (the role and session).

| Kind | Main fields |
|---|---|
| `studio.created` | title, repo, mode, candidates, role configurations |
| `role.started` / `role.lost` / `role.resumed` / `role.closed` | role, terminal, actor identity, session ID, reason |
| `role.reopen` / `role.release` | role |
| `role.session` | role, session ID |
| `role.approval` | role, decision (`allow` or `deny`) |
| `studio.finished` | reason |
| `req.version` | version, file, summary |
| `plan.published` | kind (plan, acceptance), file, requirement version |
| `task.created` | id, title, requirement version, acceptance, dependencies |
| `task.cast` | task, actor identity, reason, optional priority, by (the caster) |
| `task.priority` | task, priority |
| `task.delivered` | task, delivery number, actor identity, commit range, report file, requirement version, checks run |
| `task.reviewed` | task, delivery number, reviewer, verdict, written conclusion, findings, upstream changes |
| `task.finding` | task, delivery number, findings, by |
| `task.state` | task, paused, resumed, abandoned, or taken over, with reason |
| `message.queued` / `message.delivered` | from, to, file, purpose, optional `source_seq` |
| `report.sent` | from, to, node, file |
| `attention.raised` / `attention.cleared` | role, reason, detail |
| `correction` | target event, changed fields, reason |

`role.lost` means a role recorded as live vanished unexpectedly (its terminal is gone with no
`role.closed`). `role.closed` means the stage released the role on purpose — a stop or a recast
replacement — so a deliberate stop is never mistaken for a loss, and a closed role keeps its actor
identity and session ID: it stays resumable while its studio is live. A later `role.started` or
`role.resumed` clears the closed state.

`role.reopen` marks a closed person `held` on its reduced state: the stage wants it back and starts
it with the session it closed with — the launch lands as `role.resumed` (reason `reopened`), not
`role.started`. `held` clears on `role.release` or `role.closed`; a released role stops being
desired, so the usual stop closes it. `role.reopen` is refused unless the role is closed, has a
session ID, is not already held and no other live role uses its terminal name; `role.release` is
refused unless held. Both are user- and director-facing events.

Tasks carry a `priority` — `low`, `normal` (the default) or `high`. `task.cast` may set it, and
`task.priority` changes it on an existing task. It orders the panel's display only (R20): nothing in
reconcile or scheduling reads it.

Adapters that choose the session ID at launch record it in `role.started`. Ones the runner assigns
itself — found by a file scan or a CLI query after the terminal is up — land in `role.session`, which
the stage records once it finds the ID. A `role.session` for a role the log does not know is refused.

Two `message.queued` events carry a `purpose` the stage acts on: `plan request` records `plan`
(to the writer), `dispatch` records `dispatch` (to the director). The writer is wanted from a
`plan` message until the next `dispatch`; plain `tell`/`ask` messages carry no purpose.

`message.queued` may carry `source_seq`, the seq of the ledger event it was generated from. The
stage's automatic notices — a delivery to whoever cast the task, a `changes_requested` review to
the actor, a blocked task or a requirement revision above v1 to the director, an actor's permission
prompt to the director — are ordinary queued messages whose body file lands under `messages/` first.
A notice is owed only while no message has its source event's `seq`, so a stage restart repeats
nothing. A `report.sent` to a role is deliverable the same way; `to: user` is never sent to a
terminal. `message.delivered` marks the target — a message or a report — only after `terminal.send`
reports sent. A crash between the send and the record can send the same line twice on restart;
that is harmless.

`task.cast` records `by`, the role that cast. A producer's cast (the light path) routes the
delivery notice to the producer; a director's or unattributed cast routes it to the director.

`role.approval` records a director's or user's decision on an open `permission prompt` attention —
refused without one. The stage sends the runner's approval key while the prompt still shows, then
`attention.cleared` lands; a prompt already gone clears with no key sent. The stage never answers a
prompt without a recorded decision, and a decision recorded before the attention does not answer a
later prompt.

`studio.finished` — producer or user only — is refused while any task is not terminal, naming the
open tasks. From then on `desired` holds only explicitly reopened people, so producer and director
close the usual way and keep their sessions. `role.reopen` still works after finish.

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
