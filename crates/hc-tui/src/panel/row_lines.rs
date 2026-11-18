//! The row model's content lines: the three drawer lines a `Space`
//! row opens and the owed-step text a block's subheader shows. Kept
//! apart from `rows.rs`, which decides which rows exist; this file
//! decides what the lines under them say.

use backlot_model::summary::{RoleSummary, StudioSummary, TaskSummary};

use super::data::StudioRef;

/// The three drawer lines under a person row: task title and
/// requirement, the latest delivery/check/rework line, and the
/// action hints.
pub(crate) fn person_drawer(studio: StudioRef<'_>, role: &RoleSummary) -> Vec<String> {
    let task = role
        .task
        .as_deref()
        .and_then(|id| studio.studio.tasks.iter().find(|t| t.id == id));
    let mut lines = Vec::with_capacity(3);
    match task {
        Some(t) => {
            let mut l1 = format!("\"{}\" · r{}", t.title, t.requirement_version);
            if t.behind {
                l1 = format!(
                    "\"{}\" · v{}→v{}",
                    t.title, t.requirement_version, studio.studio.requirement_version
                );
            }
            lines.push(l1);
            let mut l2 = match (t.latest_delivery, &t.latest_checks) {
                (Some(n), checks) => {
                    let mut s = format!("d{n}");
                    if let Some(c) = checks {
                        s.push(' ');
                        s.push_str(c);
                    }
                    s
                }
                (None, _) => "no deliveries yet".into(),
            };
            if t.rework_cap > 0 {
                l2 = format!("{l2} · rework {}/{}", t.rework_rounds, t.rework_cap);
            }
            if let Some(v) = &t.latest_verdict {
                let word = match v {
                    backlot_model::event::Verdict::Accepted => "accepted",
                    backlot_model::event::Verdict::PartiallyAccepted => "partially accepted",
                    backlot_model::event::Verdict::ChangesRequested => "changes requested",
                    backlot_model::event::Verdict::Blocked => "blocked",
                };
                l2 = format!("{l2} · {word}");
            }
            lines.push(l2);
            lines.push("[v] sheet  [m] menu  [+/-]".into());
        }
        None => {
            // A fixed role (`producer`/`director`/`writer`): the
            // runner line names its engine, the second line what it
            // is doing or what waits on it (S2 §3).
            let agent = [&role.runner, &role.model, &role.effort]
                .into_iter()
                .flatten()
                .cloned()
                .collect::<Vec<_>>()
                .join("/");
            lines.push(if agent.is_empty() {
                role.name.clone()
            } else {
                agent
            });
            let l2 = role
                .attention_detail
                .as_deref()
                .or(role.attention.as_deref())
                .map(|a| format!("! {a}"))
                .or_else(|| role.doing.clone())
                .unwrap_or_else(|| role.state.clone());
            lines.push(l2);
            lines.push("[⏎] focus  [p] peek  [m] menu".into());
        }
    }
    lines
}

/// The three drawer lines under a task row: title and requirement
/// (with `v<pin>→v<cur>` when behind), dependency/delivery detail,
/// and the action hints.
pub(crate) fn task_drawer(studio: StudioRef<'_>, task: &TaskSummary) -> Vec<String> {
    let pin = if task.behind {
        format!(
            "v{}→v{}",
            task.requirement_version, studio.studio.requirement_version
        )
    } else {
        format!("r{}", task.requirement_version)
    };
    let l1 = format!("\"{}\" · {pin}", task.title);
    let mut l2 = if !task.waits_for.is_empty() {
        format!("waits {} · uncast", task.waits_for.join(", "))
    } else {
        let status = backlot_model::hollywood::status_str(task.status);
        match (task.latest_delivery, &task.latest_checks) {
            (Some(n), checks) => {
                let mut s = format!("d{n} · {status}");
                if let Some(c) = checks {
                    s = format!("d{n} {c} · {status}");
                }
                s
            }
            (None, _) => status.to_string(),
        }
    };
    if task.rework_cap > 0 && task.rework_rounds > 0 {
        l2 = format!("{l2} · rework {}/{}", task.rework_rounds, task.rework_cap);
    }
    let l3 = "[v] sheet  [+/-] priority".to_string();
    vec![l1, l2, l3]
}

/// The owed-step text for `next`: `"label — owner-key"` becomes
/// `label · owner-name` — the role's display name, never the raw
/// `actor-t1-c1` key (A11.3).
pub(crate) fn owed_text(studio: &StudioSummary, next: &str) -> String {
    let (label, owner) = next
        .rsplit_once(" — ")
        .map(|(l, o)| (l, Some(o)))
        .unwrap_or((next, None));
    let name = owner.and_then(|key| {
        studio
            .roles
            .iter()
            .find(|r| r.role == key)
            .map(|r| r.name.as_str())
            .or(if key == "user" { Some("user") } else { None })
    });
    match name {
        Some(name) => format!("{label} · {name}"),
        None => label.to_string(),
    }
}
