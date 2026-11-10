//! Stage-generated notices (S5): certain ledger events owe a role a
//! queued message with a generated body under `messages/`. Each
//! notice carries its source event's `seq` in `source_seq`, and is
//! owed only while no message has that seq — so a stage restart
//! repeats nothing.
//!
//! Owed today: a delivery to the task's reviewer — `producer` when
//! the producer cast it, the director otherwise — a
//! `changes_requested` review to the task's actor — except the
//! one that takes the task to `at_cap`, which is the producer's
//! call to make with the user — a blocked task
//! to its reviewer, a resume that returns the task to `running`
//! to its actor, a requirement revision above v1 to the
//! director naming the tasks now behind, an actor's
//! `permission prompt` attention to the director — a non-actor's
//! episode is the user's, and gets no notice — a `stalled`
//! attention to the role's reviewer — a `cannot resume`
//! attention routed like a stall — a `recovered` resume to
//! the role itself — an `over budget` attention to the
//! director and the producer both — and a `deny`
//! approval to the denied role, which the esc interruption left
//! waiting.

use std::collections::BTreeSet;
use std::path::Path;

use backlot_model::event::{ApprovalDecision, TaskStateChange, Verdict};
use backlot_model::state::{Message, StudioState, TaskStatus};

/// A notice the ledger owes: a generated body file and the queued
/// message that names it.
#[derive(Debug, Clone, PartialEq)]
pub struct Notice {
    /// The source event's seq — the dedup key (`source_seq`).
    pub source_seq: u64,
    /// The role the notice goes to.
    pub to: String,
    /// The body file's leaf label: `messages/<seq>-<label>.md`.
    pub label: &'static str,
    /// The generated markdown body.
    pub body: String,
}

/// Who a task's notices go to: `producer` when the producer cast
/// it — the light path has no director to tell — and the director
/// in every other case. `user` and the empty name have no
/// terminal, so they fall to the director too.
pub fn reviewer_for(task: &backlot_model::state::Task) -> String {
    task.reviewer().into()
}

/// The role key the task's current cast runs under — the same key
/// `desired` builds.
fn actor_key(task: &backlot_model::state::Task) -> String {
    format!("actor-{}-c{}", task.id, task.casts.len())
}

/// The label a stage-written message's file carries: the body lands
/// at `messages/<source_seq>-<label>.md`, so the leaf between the seq
/// and `.md` is the label `owed` chose. `None` when the file does not
/// match — which a generated notice never does.
pub(crate) fn label(m: &Message) -> Option<&str> {
    let seq = m.source_seq?;
    let rest = m.file.as_deref()?.strip_prefix("messages/")?;
    rest.strip_prefix(&format!("{seq}-"))?.strip_suffix(".md")
}

/// The `lost` notice body for a `cannot resume` attention: the
/// role, why it cannot come back, and for an actor the ways on —
/// the reviewer abandons the task or recasts it.
fn cannot_resume_body(a: &backlot_model::state::Attention) -> String {
    let ways = match backlot_model::state::actor_task(&a.role) {
        Some(id) => format!(
            "\nWays on:\n\n- `backlot task abandon --task {id} --reason <reason>` and a new task\n- recast it with `backlot cast --task {id} --actor <name> --reason <reason>`\n"
        ),
        None => String::new(),
    };
    format!(
        "# {} cannot resume\n\n`{}`'s terminal was lost and the stage could not resume it — {}.\n{ways}",
        a.role, a.role, a.detail,
    )
}

/// The changes-requested round `r` was for the task's latest cast —
/// the producer's cap budget counts from the latest cast, so a
/// count past `cap` marks the review that took the task `at_cap`.
fn cap_round(t: &backlot_model::state::Task, r: &backlot_model::state::ReviewRecord) -> u32 {
    let last_cast = t
        .casts
        .iter()
        .filter(|c| c.seq < r.seq)
        .map(|c| c.seq)
        .max()
        .unwrap_or(0);
    t.reviews
        .iter()
        .filter(|o| o.verdict == Verdict::ChangesRequested && o.seq > last_cast && o.seq <= r.seq)
        .count() as u32
}

/// How one `task.checked` reads in the delivery notice.
fn outcome(r: &backlot_model::state::CheckedRecord) -> String {
    if r.passed() {
        "pass".into()
    } else if r.timed_out {
        "timed out".into()
    } else {
        match r.exit_code {
            Some(code) => format!("fail (exit {code})"),
            // A signal or a run that never started — the log says
            // which.
            None => "fail (no exit)".into(),
        }
    }
}

/// Every notice the state owes — one per source event, deduped on
/// `source_seq` so the list is stable until the events land. A
/// purpose-bearing message — a `nudge`, a `plan`/`dispatch` call —
/// never counts: a nudge shares its `source_seq` with the step it
/// names, and neither suppresses nor is suppressed by the ordinary
/// notice on that seq.
pub fn owed(state: &StudioState, dir: &Path) -> Vec<Notice> {
    let sent: BTreeSet<u64> = state
        .messages
        .iter()
        .filter(|m| m.purpose.is_none())
        .filter_map(|m| m.source_seq)
        .collect();
    let cap = state.rework_cap();
    let declared = state
        .studio
        .as_ref()
        .map(|s| s.checks.as_slice())
        .unwrap_or(&[]);
    let mut out = Vec::new();
    for t in state.tasks.values() {
        for d in &t.deliveries {
            if sent.contains(&d.seq) {
                continue;
            }
            // The reviewer's notice waits for the stage's records:
            // while the task is still delivered on this delivery
            // every declared check must have one. A task that moved
            // on gets the notice with whatever landed — a check
            // that never ran is named as having no record.
            let still_delivered = t.status == TaskStatus::Delivered
                && t.deliveries.last().is_some_and(|last| last.n == d.n);
            if still_delivered
                && !declared.is_empty()
                && !t.checks_missing(d.n, declared).is_empty()
            {
                continue;
            }
            let stage_ran = if declared.is_empty() {
                "No checks were declared — the actor's reported results are all there is."
                    .to_string()
            } else {
                declared
                    .iter()
                    .map(|c| match t.checked_of(d.n).find(|r| r.name == c.name) {
                        Some(r) => format!(
                            "- {}: {} — {}",
                            c.name,
                            outcome(r),
                            dir.join(&r.output).display()
                        ),
                        None => format!("- {}: no record", c.name),
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            };
            let reported = if d.checks.is_empty() {
                "none".to_string()
            } else {
                d.checks
                    .iter()
                    .map(|c| format!("{}={}", c.name, if c.passed { "pass" } else { "fail" }))
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            let caster = t
                .cast
                .as_ref()
                .map(|c| c.candidate.as_str())
                .unwrap_or("the actor");
            out.push(Notice {
                source_seq: d.seq,
                to: reviewer_for(t),
                label: "delivered",
                body: format!(
                    "# {} delivery {}\n\nTask `{}` was delivered by {} as delivery {}.\n\n- commits: `{}`\n- report: `{}`\n- dispatched under requirement v{}\n\nStage ran:\n{}\n\nThe actor reported: {}\n\nReview it against the task's acceptance list and record a verdict with `backlot review`.\n",
                    t.id,
                    d.n,
                    t.id,
                    caster,
                    d.n,
                    d.commits,
                    dir.join(&d.report).display(),
                    d.requirement_version,
                    stage_ran,
                    reported,
                ),
            });
        }
        for r in &t.reviews {
            if sent.contains(&r.seq) || r.verdict != Verdict::ChangesRequested {
                continue;
            }
            if cap_round(t, r) > cap {
                // The review that took the task past the cap owes
                // the actor nothing — `deliver` needs `running` —
                // and the producer the call that is theirs to make
                // with the user.
                out.push(Notice {
                    source_seq: r.seq,
                    to: "producer".into(),
                    label: "at-cap",
                    body: format!(
                        "# {} at the rework cap\n\nThe review of delivery {} asked for changes — round {} past the rework cap of {}, so the task is `at_cap`. The producer decides this with the user.\n\nConclusion: {}\n\nWays on:\n\n- accept it with the gaps documented: `backlot review --task {} --delivery {} --verdict partially-accepted --conclusion <text>`\n- rescope it: `backlot task abandon --task {} --reason <reason>` and a new task\n- recast it: `backlot cast --task {} --actor <name> --reason <reason>`\n",
                        t.id,
                        r.delivery,
                        cap + 1,
                        cap,
                        r.conclusion,
                        t.id,
                        r.delivery,
                        t.id,
                        t.id,
                    ),
                });
                continue;
            }
            let findings = if r.findings.is_empty() {
                "none".to_string()
            } else {
                r.findings
                    .iter()
                    .map(|f| format!("- [{:?}] {}: {}", f.severity, f.id, f.summary))
                    .collect::<Vec<_>>()
                    .join("\n")
            };
            out.push(Notice {
                source_seq: r.seq,
                to: actor_key(t),
                label: "rework",
                body: format!(
                    "# rework for {} delivery {}\n\nThe review of delivery {} asked for changes (verdict: changes-requested).\n\nConclusion: {}\n\nFindings:\n{}\n\nFix what it names and deliver again with `backlot deliver`.\n",
                    t.id, r.delivery, r.delivery, r.conclusion, findings,
                ),
            });
        }
        for ch in &t.state_changes {
            if sent.contains(&ch.seq) {
                continue;
            }
            match ch.state {
                TaskStateChange::Blocked => {
                    out.push(Notice {
                        source_seq: ch.seq,
                        to: reviewer_for(t),
                        label: "blocked",
                        body: format!(
                            "# {} blocked\n\nTask `{}` is blocked{}\n",
                            t.id,
                            t.id,
                            if ch.reason.is_empty() {
                                ".".to_string()
                            } else {
                                format!(": {}", ch.reason)
                            },
                        ),
                    });
                }
                // A resume that returns the task to `running` — a
                // straight unblock, or a pause of running work —
                // owes the actor the word to carry on. Any other
                // restore is bookkeeping nobody acts on.
                TaskStateChange::Resumed
                    if ch.resumed_to == Some(TaskStatus::Running) && t.cast.is_some() =>
                {
                    out.push(Notice {
                        source_seq: ch.seq,
                        to: actor_key(t),
                        label: "resumed",
                        body: format!(
                            "# {} resumed\n\n`{}` resumed task `{}`{} — carry on from where it stopped.\n",
                            t.id,
                            ch.by,
                            t.id,
                            if ch.reason.is_empty() {
                                ".".to_string()
                            } else {
                                format!(": {}", ch.reason)
                            },
                        ),
                    });
                }
                _ => {}
            }
        }
    }
    for a in &state.attention {
        if sent.contains(&a.seq) {
            continue;
        }
        let file = dir
            .join(format!("attention/{}.txt", a.seq))
            .display()
            .to_string();
        match a.reason.as_str() {
            backlot_model::event::PERMISSION_PROMPT => {
                if let Some(body) = crate::permit::director_notice(a, &file) {
                    out.push(Notice {
                        source_seq: a.seq,
                        to: "director".into(),
                        label: "attention",
                        body,
                    });
                }
            }
            backlot_model::event::STALLED => {
                if let Some(to) = crate::stall::notice_to(state, a) {
                    out.push(Notice {
                        source_seq: a.seq,
                        to,
                        label: "stalled",
                        body: crate::stall::notice_body(a, &file),
                    });
                }
            }
            // A lost role that cannot come back gets one `lost`
            // notice, routed like a stall — an actor's to its task
            // reviewer, the writer's and director's to the producer,
            // the producer's to nobody.
            backlot_model::event::CANNOT_RESUME => {
                if let Some(to) = crate::stall::notice_to(state, a) {
                    out.push(Notice {
                        source_seq: a.seq,
                        to,
                        label: "lost",
                        body: cannot_resume_body(a),
                    });
                }
            }
            // An `owed` escalation that is not the producer's own
            // step owes the producer one notice (S9a): the step's
            // owner is the user's problem now, but the producer
            // should hear its role is idle.
            crate::reasons::OWED if a.role != "producer" => {
                out.push(Notice {
                    source_seq: a.seq,
                    to: "producer".into(),
                    label: "owed",
                    body: format!(
                        "# {} owes a step\n\n`{}` was nudged and stayed idle — an `owed` attention is now open for the user: {}.\n",
                        a.role, a.role, a.detail,
                    ),
                });
            }
            // An over-budget task tells the director and the producer
            // both (S9e): nothing was stopped or paused — the call to
            // pause, recast or let it run is theirs. Both notices ride
            // the one `source_seq`, queued the same pass.
            crate::reasons::OVER_BUDGET => {
                let task = backlot_model::state::actor_task(&a.role).unwrap_or("?");
                for to in ["director", "producer"] {
                    out.push(Notice {
                        source_seq: a.seq,
                        to: to.into(),
                        label: "over budget",
                        body: format!(
                            "# {task} over budget\n\n`{}` raised `over budget` on task `{task}`: {}.\n\nNothing was stopped or paused. Let it run, pause it with `backlot task pause --task {task} --reason <reason>`, or recast it — the attention clears `done` when the task is terminal.\n",
                            a.role, a.detail,
                        ),
                    });
                }
            }
            _ => {}
        }
    }
    // A `role.resumed` with reason `recovered` owes the role itself
    // one notice: its terminal was lost and the stage resumed its
    // session — anything running then has stopped, and a command
    // stopped at a permission prompt must be run again.
    for (key, r) in &state.roles {
        let Some(seq) = r.recovered_seq else {
            continue;
        };
        if sent.contains(&seq) {
            continue;
        }
        out.push(Notice {
            source_seq: seq,
            to: key.clone(),
            label: "recovered",
            body: format!(
                "# recovered\n\n`{key}`'s terminal was lost and the stage resumed its session. Anything that was running then has stopped: check `backlot status` and carry on. A command stopped at a permission prompt must be run again.\n"
            ),
        });
    }
    // A deny sends esc and the role lands on an interrupted screen
    // that waits for input — it is owed a notice, for any role.
    // `backlot block` is an actor's command: a fixed role can only
    // work around the denial or report back what could not be done.
    for a in &state.approvals {
        if sent.contains(&a.seq) || a.decision != ApprovalDecision::Deny {
            continue;
        }
        let way = if crate::permit::is_actor(&a.role) {
            "stop waiting on it with `backlot block --reason <reason>`"
        } else {
            "tell whoever asked what could not be done"
        };
        out.push(Notice {
            source_seq: a.seq,
            to: a.role.clone(),
            label: "denied",
            body: format!(
                "# permission denied\n\n`{}` denied your permission prompt — the command did not run.\n\nTry another way that needs no permission, or {way}.\n",
                a.by,
            ),
        });
    }
    for req in &state.requirements {
        if sent.contains(&req.seq) || req.version <= 1 {
            continue;
        }
        let behind: Vec<&str> = state
            .tasks
            .values()
            .filter(|t| t.is_behind(state.requirement_version))
            .map(|t| t.id.as_str())
            .collect();
        out.push(Notice {
            source_seq: req.seq,
            to: "director".into(),
            label: "req",
            body: format!(
                "# requirement v{}\n\nThe requirement was revised to v{}: `{}` — {}\n\n{}\n",
                req.version,
                req.version,
                dir.join(&req.file).display(),
                req.summary,
                if behind.is_empty() {
                    "No task is behind.".to_string()
                } else {
                    format!(
                        "Tasks now behind: {} (dispatched under an older version).",
                        behind.join(", ")
                    )
                },
            ),
        });
    }
    out
}
