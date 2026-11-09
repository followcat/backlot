//! Human-readable views: `backlot status` and `backlot records`.

use std::fmt::Write as _;

use backlot_model::event::EventKind;
use backlot_model::state::{StudioState, Task, TaskStatus};
use backlot_model::{ActorIdentity, ActorRecord};

fn actor_short(a: &ActorIdentity) -> String {
    match &a.runner_version {
        Some(v) => format!(
            "{}/{}@{}/{}/{}",
            a.runner, a.model, v, a.effort, a.permission_profile
        ),
        None => format!(
            "{}/{}/{}/{}",
            a.runner, a.model, a.effort, a.permission_profile
        ),
    }
}

fn fmt_ms(ms: u64) -> String {
    let s = ms / 1000;
    if s >= 3600 {
        format!("{}h{}m", s / 3600, s % 3600 / 60)
    } else if s >= 60 {
        format!("{}m{}s", s / 60, s % 60)
    } else {
        format!("{s}s")
    }
}

fn task_line(t: &Task, current_req: u32, cap: u32) -> String {
    let mut s = format!(
        "  {:<4} {:<10} {:<18}",
        t.id,
        t.task_kind,
        format!("{:?}", t.status).to_lowercase()
    );
    if let Some(cast) = &t.cast {
        let _ = write!(s, " {:<12}", cast.candidate);
    } else {
        let _ = write!(s, " {:<12}", "-");
    }
    let _ = write!(s, " req v{}", t.requirement_version);
    if t.is_behind(current_req) {
        s.push_str(" BEHIND");
    }
    let _ = write!(
        s,
        "  deliveries {}  rework {}/{}",
        t.deliveries.len(),
        t.rework_rounds,
        cap
    );
    if let Some(from) = t.paused_from {
        let _ = write!(s, "  (was {})", fmt_status(from));
    }
    let _ = write!(s, "  \"{}\"", t.title);
    s
}

fn fmt_status(s: TaskStatus) -> String {
    format!("{s:?}").to_lowercase()
}

/// The status view for one studio.
pub fn status(name: &str, state: &StudioState) -> String {
    let mut out = String::new();
    let Some(studio) = &state.studio else {
        return format!("studio {name}: no events yet\n");
    };
    let _ = writeln!(
        out,
        "studio {name}: \"{}\" ({:?})",
        studio.title, studio.mode
    );
    let _ = writeln!(out, "  repo {}", studio.repo);
    if state.requirement_version > 0 {
        let cur = &state.requirements[state.requirements.len() - 1];
        let _ = writeln!(out, "  requirement v{} — \"{}\"", cur.version, cur.summary);
    } else {
        let _ = writeln!(out, "  requirement: none recorded");
    }
    let _ = writeln!(out, "  rework cap {}", studio.rework_cap);
    let _ = writeln!(out, "  roles:");
    for (role, cfg, live) in [
        (
            "producer",
            &studio.roles.producer,
            state.roles.get("producer"),
        ),
        (
            "director",
            &studio.roles.director,
            state.roles.get("director"),
        ),
        ("writer", &studio.roles.writer, state.roles.get("writer")),
    ] {
        let live = match live {
            Some(r) if r.live => "live",
            Some(r) if r.closed => "closed",
            Some(_) => "lost",
            None => "not started",
        };
        let _ = writeln!(out, "    {role:<9} {} ({live})", actor_short(cfg));
    }
    if !studio.candidates.is_empty() {
        let _ = writeln!(out, "  candidates:");
        for c in &studio.candidates {
            let _ = writeln!(out, "    {:<9} {}", c.name, actor_short(&c.actor()));
        }
    }
    if !state.plans.is_empty() {
        let _ = writeln!(out, "  plans:");
        for p in &state.plans {
            let _ = writeln!(
                out,
                "    {:?} r{} {}",
                p.plan_kind, p.requirement_version, p.file
            );
        }
    }
    if state.tasks.is_empty() {
        let _ = writeln!(out, "  tasks: none");
    } else {
        let _ = writeln!(out, "  tasks:");
        for t in state.tasks.values() {
            let _ = writeln!(
                out,
                "{}",
                task_line(t, state.requirement_version, studio.rework_cap)
            );
        }
    }
    let queued = state.messages.iter().filter(|m| !m.delivered).count();
    if !state.messages.is_empty() {
        let _ = writeln!(
            out,
            "  messages: {} queued, {} delivered",
            queued,
            state.messages.len() - queued
        );
    }
    if !state.reports.is_empty() {
        let _ = writeln!(out, "  reports:");
        for r in &state.reports {
            let _ = writeln!(out, "    {} → {} ({}) {}", r.from, r.to, r.node, r.file);
        }
    }
    let open: Vec<_> = state.open_attention().collect();
    if open.is_empty() {
        let _ = writeln!(out, "  attention: none");
    } else {
        let _ = writeln!(out, "  attention:");
        for a in open {
            let _ = writeln!(out, "    {}: {} — {}", a.role, a.reason, a.detail);
        }
    }
    if !state.corrections.is_empty() {
        let _ = writeln!(out, "  corrections: {}", state.corrections.len());
    }
    out
}

/// `backlot records`: actor records across all studios.
pub fn records(recs: &[ActorRecord]) -> String {
    if recs.is_empty() {
        return "no deliveries recorded in any studio\n".into();
    }
    let mut out = String::new();
    let mut last_actor: Option<&ActorIdentity> = None;
    for r in recs {
        if last_actor != Some(&r.actor) {
            let _ = writeln!(out, "{}", actor_short(&r.actor));
            last_actor = Some(&r.actor);
        }
        let _ = write!(
            out,
            "  {}: {} task{}, {} deliveries",
            r.task_kind,
            r.tasks,
            if r.tasks == 1 { "" } else { "s" },
            r.deliveries
        );
        if !r.enough_data {
            let _ = writeln!(out, " — not enough data");
            continue;
        }
        let _ = writeln!(out);
        let f = &r.first_delivery;
        let _ = writeln!(
            out,
            "    first delivery: {} accepted, {} partial, {} changes requested, {} blocked, {} unreviewed",
            f.accepted, f.partially_accepted, f.changes_requested, f.blocked, f.unreviewed
        );
        let _ = writeln!(
            out,
            "    defects: {} ({} actor-fault), {} closed, {} open, {} repeats, {} new-in-rework, {} later",
            r.defects,
            r.actor_fault_defects,
            r.defects_closed,
            r.defects_open,
            r.defect_repeats,
            r.new_in_rework,
            r.later_defects
        );
        let _ = writeln!(
            out,
            "    upstream changes: {}, takeovers: {}",
            r.upstream_changes, r.takeovers
        );
        let fo = &r.final_outcome;
        let _ = writeln!(
            out,
            "    final: {} accepted, {} partial, {} blocked, {} abandoned, {} at cap, {} taken over, {} open",
            fo.accepted,
            fo.partially_accepted,
            fo.blocked,
            fo.abandoned,
            fo.at_cap,
            fo.taken_over,
            fo.open
        );
        let _ = writeln!(out, "    wall time: {}", fmt_ms(r.wall_time_ms));
    }
    out
}

/// The one-line confirmation printed after an append.
pub fn confirm(event: &backlot_model::Event) -> String {
    let subject = match &event.kind {
        EventKind::StudioCreated { title, .. } => format!(" '{title}'"),
        EventKind::ReqVersion { version, .. } => format!(" v{version}"),
        EventKind::PlanPublished { plan_kind, .. } => format!(" {plan_kind:?}"),
        EventKind::TaskCreated { task, title, .. } => format!(" {task} '{title}'"),
        EventKind::TaskCast {
            task, candidate, ..
        } => format!(" {task} {candidate}"),
        EventKind::TaskPriority { task, priority } => format!(" {task} {priority}"),
        EventKind::RoleReopen { role } | EventKind::RoleRelease { role } => {
            format!(" {role}")
        }
        EventKind::TaskDelivered { task, delivery, .. } => format!(" {task} d{delivery}"),
        EventKind::TaskReviewed {
            task,
            delivery,
            verdict,
            ..
        } => format!(" {task} d{delivery} {verdict:?}"),
        EventKind::TaskFinding { task, delivery, .. } => format!(" {task} d{delivery}"),
        EventKind::TaskState { task, state, .. } => format!(" {task} {state:?}"),
        EventKind::MessageQueued { to, .. } => format!(" → {to}"),
        EventKind::ReportSent { node, to, .. } => format!(" {node} → {to}"),
        EventKind::Correction { target, .. } => format!(" on event {target}"),
        _ => String::new(),
    };
    format!("event {}: {}{}", event.seq, event.kind.name(), subject)
}
