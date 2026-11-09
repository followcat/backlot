//! Stage-generated notices (S5): certain ledger events owe a role a
//! queued message with a generated body under `messages/`. Each
//! notice carries its source event's `seq` in `source_seq`, and is
//! owed only while no message has that seq — so a stage restart
//! repeats nothing.
//!
//! Owed today: a delivery to the director (or, on the light path,
//! whoever cast the task), a `changes_requested` review to the
//! task's actor, a blocked task to the director, a requirement
//! revision above v1 to the director naming the tasks now behind,
//! and an actor's `permission prompt` attention to the director —
//! a non-actor's episode is the user's, and gets no notice.

use std::collections::BTreeSet;
use std::path::Path;

use backlot_model::event::{TaskStateChange, Verdict};
use backlot_model::state::StudioState;

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

/// Who a task's notices go to: whoever cast it (`cast.by` — the
/// light path's producer) or the director when the cast names
/// nobody.
fn reviewer_for(task: &backlot_model::state::Task) -> String {
    task.cast
        .as_ref()
        .map(|c| c.by.as_str())
        .filter(|by| !by.is_empty())
        .unwrap_or("director")
        .to_string()
}

/// The role key the task's current cast runs under — the same key
/// `desired` builds.
fn actor_key(task: &backlot_model::state::Task) -> String {
    format!("actor-{}-c{}", task.id, task.casts.len())
}

/// Every notice the state owes — one per source event, deduped on
/// `source_seq` so the list is stable until the events land.
pub fn owed(state: &StudioState, dir: &Path) -> Vec<Notice> {
    let sent: BTreeSet<u64> = state.messages.iter().filter_map(|m| m.source_seq).collect();
    let mut out = Vec::new();
    for t in state.tasks.values() {
        for d in &t.deliveries {
            if sent.contains(&d.seq) {
                continue;
            }
            let checks = if d.checks.is_empty() {
                "none recorded".to_string()
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
                    "# {} delivery {}\n\nTask `{}` was delivered by {} as delivery {}.\n\n- commits: `{}`\n- checks: {}\n- report: `{}`\n- dispatched under requirement v{}\n\nReview it against the task's acceptance list and record a verdict with `backlot review`.\n",
                    t.id,
                    d.n,
                    t.id,
                    caster,
                    d.n,
                    d.commits,
                    checks,
                    dir.join(&d.report).display(),
                    d.requirement_version,
                ),
            });
        }
        for r in &t.reviews {
            if sent.contains(&r.seq) || r.verdict != Verdict::ChangesRequested {
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
            if sent.contains(&ch.seq) || ch.state != TaskStateChange::Blocked {
                continue;
            }
            out.push(Notice {
                source_seq: ch.seq,
                to: "director".into(),
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
    }
    for a in &state.attention {
        if sent.contains(&a.seq) || a.reason != backlot_model::event::PERMISSION_PROMPT {
            continue;
        }
        let file = dir
            .join(format!("attention/{}.txt", a.seq))
            .display()
            .to_string();
        if let Some(body) = crate::permit::director_notice(a, &file) {
            out.push(Notice {
                source_seq: a.seq,
                to: "director".into(),
                label: "attention",
                body,
            });
        }
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
