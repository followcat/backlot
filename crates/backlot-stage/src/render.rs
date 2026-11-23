//! Human-readable views: `backlot status` and `backlot records`.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use backlot_model::attention::AttentionClass;
use backlot_model::event::{CANNOT_RESUME, CheckDecl, EventKind, STALLED};
use backlot_model::state::{Attention, StudioState, Task, TaskStatus};
use backlot_model::summary::Work;
use backlot_model::{ActorIdentity, ActorRecord};

fn actor_short(a: &ActorIdentity) -> String {
    let mut s = format!("{}/{}", a.runner, a.model);
    if let Some(v) = &a.runner_version {
        let _ = write!(s, "@{v}");
    }
    // A runner carrying its depth in the model id records no effort
    // — the segment is absent, not an empty `//`.
    if let Some(e) = a.effort.as_deref().filter(|e| !e.is_empty()) {
        let _ = write!(s, "/{e}");
    }
    format!("{s}/{}", a.permission_profile)
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

/// `14:02` — a ledger millisecond stamp as the local wall clock's
/// hour and minute, the same shape the panel's clock keeps.
fn hh_mm(ms: u64) -> String {
    let timestamp: libc::time_t = (ms / 1000).min(i64::MAX as u64) as libc::time_t;
    let mut local: libc::tm = unsafe { std::mem::zeroed() };
    if unsafe { libc::localtime_r(&timestamp, &mut local) }.is_null() {
        return "?".into();
    }
    format!("{:02}:{:02}", local.tm_hour, local.tm_min)
}

/// `2h`, `3m`, `12s` — an age, compact enough for one line.
pub fn age_short(ms: u64) -> String {
    let s = ms / 1000;
    if s >= 3600 {
        format!("{}h", s / 3600)
    } else if s >= 60 {
        format!("{}m", s / 60)
    } else {
        format!("{s}s")
    }
}

/// The plain-words fact every reader of an attention gets: its class,
/// when it was raised, and — for a screen snapshot — when the screen
/// was taken with the stale warning. `backlot status` prints it and
/// Hollywood's notices quote it, so a stage diagnostic is never read
/// as the role's current state (P5, H1). `now_ms` ages the raise.
pub fn attention_fact(a: &Attention, now_ms: u64) -> String {
    let class = AttentionClass::of(&a.reason, a.needs_user).label();
    let mut fact = format!(
        "{class} · raised {} ago",
        age_short(now_ms.saturating_sub(a.ts_ms))
    );
    if a.detail.contains("screen saved to attention/") {
        fact.push_str(&format!(" · screen at {}, may be stale", hh_mm(a.ts_ms)));
    }
    fact
}

fn task_line(t: &Task, state: &StudioState, declared: &[CheckDecl], cap: u32) -> String {
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
    if t.is_behind(state.requirement_version) {
        s.push_str(" BEHIND");
    }
    let _ = write!(
        s,
        "  deliveries {}  rework {}/{}",
        t.deliveries.len(),
        t.rework_rounds,
        cap
    );
    // The stage's own results on the latest delivery: `pending` is
    // a record still owed — the CLI cannot see what the stage is
    // running — `-` one that never landed, `timeout` the timed-out.
    if !declared.is_empty()
        && let Some(d) = t.deliveries.last()
    {
        let live = t.status == TaskStatus::Delivered;
        let marks = declared
            .iter()
            .map(|c| {
                let mark = match t.checked_of(d.n).find(|r| r.name == c.name) {
                    Some(r) if r.passed() => "pass",
                    Some(r) if r.timed_out => "timeout",
                    Some(_) => "fail",
                    None if live => "pending",
                    None => "-",
                };
                format!("{}:{mark}", c.name)
            })
            .collect::<Vec<_>>()
            .join(" ");
        let _ = write!(s, "  checks {marks}");
    }
    if let Some(from) = t.paused_from {
        let _ = write!(s, "  (was {})", fmt_status(from));
    }
    let open = t.open_dependencies(state);
    if !open.is_empty() {
        let _ = write!(s, "  waits for {}", open.join(", "));
    }
    let _ = write!(s, "  \"{}\"", t.title);
    s
}

fn fmt_status(s: TaskStatus) -> String {
    format!("{s:?}").to_lowercase()
}

/// The status view for one studio. `now_ms` ages each owed step's
/// `next:` line. `live` carries the stage's last work sample per
/// `(studio, role)` — the file the stage writes beside the ledgers, so
/// a role terminal reads the person's current state, not just its
/// ledger record.
pub fn status(
    name: &str,
    state: &StudioState,
    failed: &BTreeMap<String, String>,
    now_ms: u64,
    live: &BTreeMap<(String, String), Work>,
) -> String {
    let mut out = String::new();
    let Some(studio) = &state.studio else {
        return format!("studio {name}: no events yet\n");
    };
    let work = |role: &str| live.get(&(name.to_string(), role.to_string())).copied();
    // R14: an experimental studio flags itself in the header; a
    // standard studio's line stays exactly what it was.
    let badge = match studio.mode {
        backlot_model::event::StudioMode::Experimental => " [experimental]",
        backlot_model::event::StudioMode::Standard => "",
    };
    let _ = writeln!(
        out,
        "studio {name}: \"{}\" ({:?}){badge}",
        studio.title, studio.mode
    );
    // The producer's own stall or unresumable terminal is the one
    // problem no role in the studio can fix; put it above everything
    // else so a glance at `backlot status` says why nothing moves.
    for a in state.open_attention() {
        if a.role == "producer" && matches!(a.reason.as_str(), STALLED | CANNOT_RESUME) {
            let _ = writeln!(
                out,
                "  producer alert: {} — {} (needs the user or Hollywood)",
                a.reason, a.detail
            );
        }
    }
    let _ = writeln!(out, "  repo {}", studio.repo);
    if state.requirement_version > 0 {
        let cur = &state.requirements[state.requirements.len() - 1];
        let _ = writeln!(out, "  requirement v{} — \"{}\"", cur.version, cur.summary);
    } else {
        let _ = writeln!(out, "  requirement: none recorded");
    }
    let _ = writeln!(out, "  rework cap {}", studio.rework_cap);
    let _ = writeln!(out, "  stall limit {}s", studio.stall_limit_s);
    // P4a: runner-reported usage — a factual line only while a
    // delivery recorded any of the counters.
    if !state.usage.is_empty() {
        let _ = writeln!(out, "  usage: {}", state.usage.compact());
    }
    let _ = writeln!(out, "  roles:");
    // S9f: a role whose last hook signal was a StopFailure carries
    // ` — last turn failed: <error>` on its own line until the next
    // signal — appended to the roles line it has, or on a line of
    // its own when the section lists none.
    let mut shown: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
    let suffix = |role: &str| -> String {
        failed
            .get(role)
            .map(|e| format!(" — last turn failed: {e}"))
            .unwrap_or_default()
    };
    // Runner limits (S9e): a held wanted role shows `waiting for a
    // <runner> slot` — a fixed role in place, actors after the fixed
    // three in wanted order.
    let held = backlot_model::limits::held(state);
    // A recast fixed role shows the identity it plays now.
    let producer = state
        .fixed_identity("producer")
        .unwrap_or(&studio.roles.producer);
    let director = state
        .fixed_identity("director")
        .unwrap_or(&studio.roles.director);
    let writer = state
        .fixed_identity("writer")
        .unwrap_or(&studio.roles.writer);
    for (role, cfg, live) in [
        ("producer", producer, state.roles.get("producer")),
        ("director", director, state.roles.get("director")),
        ("writer", writer, state.roles.get("writer")),
    ] {
        shown.insert(role);
        let life = match live {
            _ if held.contains_key(role) => {
                format!("waiting for a {} slot", held[role])
            }
            Some(r) if r.live => "live".into(),
            Some(r) if r.closed => "closed".into(),
            Some(_) => "lost".into(),
            None => "not started".into(),
        };
        let doing = match work(role) {
            Some(Work::Working) => " · working",
            Some(Work::Waiting) => " · waiting",
            Some(Work::Blocked) => " · blocked",
            None => "",
        };
        let _ = writeln!(
            out,
            "    {role:<9} {} ({life}){doing}{}",
            actor_short(cfg),
            suffix(role)
        );
    }
    let wanted = backlot_model::wanted::wanted_roles(state);
    for w in &wanted {
        let Some(runner) = held.get(&w.role) else {
            continue;
        };
        if matches!(w.role.as_str(), "producer" | "director" | "writer") {
            continue;
        }
        shown.insert(w.role.as_str());
        let _ = writeln!(
            out,
            "    {:<9} {} (waiting for a {runner} slot){}",
            w.role,
            actor_short(&w.actor),
            suffix(&w.role),
        );
    }
    // Actors and any other live role the fixed three do not cover:
    // the stage's work sample names what each is doing right now.
    for ((studio, role), work) in live {
        if studio != name || shown.contains(role.as_str()) {
            continue;
        }
        let life = match state.roles.get(role) {
            Some(r) if r.live => "live",
            Some(r) if r.closed => "closed",
            Some(_) => "lost",
            None => "not started",
        };
        let doing = match work {
            Work::Working => " · working",
            Work::Waiting => " · waiting",
            Work::Blocked => " · blocked",
        };
        let who = state
            .roles
            .get(role)
            .and_then(|r| r.actor.clone())
            .or_else(|| {
                wanted
                    .iter()
                    .find(|w| w.role == *role)
                    .map(|w| w.actor.clone())
            });
        match who {
            Some(a) => {
                let _ = writeln!(
                    out,
                    "    {role:<9} {} ({life}){doing}{}",
                    actor_short(&a),
                    suffix(role)
                );
            }
            None => {
                let _ = writeln!(out, "    {role:<9} ({life}){doing}");
            }
        }
    }
    for (role, error) in failed {
        if shown.contains(role.as_str()) {
            continue;
        }
        let r = state.roles.get(role);
        let live = match r {
            Some(r) if r.live => "live",
            Some(r) if r.closed => "closed",
            Some(_) => "lost",
            None => "not started",
        };
        let who = r.and_then(|r| r.actor.clone()).or_else(|| {
            wanted
                .iter()
                .find(|w| w.role == *role)
                .map(|w| w.actor.clone())
        });
        match who {
            Some(a) => {
                let _ = writeln!(
                    out,
                    "    {role:<9} {} ({live}) — last turn failed: {error}",
                    actor_short(&a)
                );
            }
            None => {
                let _ = writeln!(out, "    {role:<9} last turn failed: {error}");
            }
        }
    }
    if !studio.candidates.is_empty() {
        let _ = writeln!(out, "  candidates:");
        for c in &studio.candidates {
            let _ = writeln!(out, "    {:<9} {}", c.name, actor_short(&c.actor()));
        }
    }
    // `stage.answered`: the dialogs the stage answered itself —
    // bookkeeping beside the roles, never in the attention list
    // below: nothing waits on the user here.
    let answered: Vec<_> = state
        .roles
        .iter()
        .filter_map(|(role, r)| r.stage_answer_ms.map(|ms| (role, ms)))
        .collect();
    if !answered.is_empty() {
        let _ = writeln!(out, "  trust answered by stage:");
        for (role, ms) in answered {
            let _ = writeln!(out, "    {role} at {}", hh_mm(ms));
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
                task_line(t, state, &studio.checks, studio.rework_cap)
            );
        }
    }
    let pending = state
        .messages
        .iter()
        .filter(|m| !m.delivered && !m.dropped)
        .count();
    let dropped = state.messages.iter().filter(|m| m.dropped).count();
    if !state.messages.is_empty() {
        let _ = write!(
            out,
            "  messages: {} queued, {} delivered",
            pending,
            state.messages.len() - pending - dropped
        );
        if dropped > 0 {
            let _ = write!(out, ", {dropped} dropped");
        }
        let _ = writeln!(out);
    }
    if !state.reports.is_empty() {
        let _ = writeln!(out, "  reports:");
        for r in &state.reports {
            let _ = writeln!(out, "    {} → {} ({}) {}", r.from, r.to, r.node, r.file);
        }
    }
    for s in backlot_model::steps::next_steps(state) {
        let _ = writeln!(
            out,
            "  next: {} — {}, owed {}",
            s.label,
            s.owner,
            fmt_ms(now_ms.saturating_sub(s.since_ms))
        );
    }
    let open: Vec<_> = state.open_attention().collect();
    if open.is_empty() {
        let _ = writeln!(out, "  attention: none");
    } else {
        let _ = writeln!(out, "  attention:");
        for a in open {
            let _ = writeln!(
                out,
                "    {}: {} — {} ({})",
                a.role,
                a.reason,
                a.detail,
                attention_fact(a, now_ms)
            );
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
        // P4a: reported token/cost sums — factual, shown only when
        // a delivery recorded any.
        if !r.usage.is_empty() {
            let _ = write!(out, " · usage {}", r.usage.compact());
        }
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
            "    checks: stage ran {} pass, {} fail; actor reported {} pass, {} fail",
            r.stage_checks_passed,
            r.stage_checks_failed,
            r.reported_checks_passed,
            r.reported_checks_failed
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

/// The detail lines one `ActorRecord` contributes in
/// `cast --suggest`. `indent` leads every line. `records` keeps its
/// own wording — its output stays byte-identical.
fn record_lines(out: &mut String, indent: &str, r: &ActorRecord) {
    let _ = write!(
        out,
        "{indent}{}: {} task{}, {} deliveries",
        r.task_kind,
        r.tasks,
        if r.tasks == 1 { "" } else { "s" },
        r.deliveries
    );
    // P4a: reported token/cost sums — factual, shown only when a
    // delivery recorded any.
    if !r.usage.is_empty() {
        let _ = write!(out, " · usage {}", r.usage.compact());
    }
    if !r.enough_data {
        let _ = writeln!(
            out,
            " (not enough data: {}/{})",
            r.tasks,
            backlot_model::records::MIN_TASKS
        );
        return;
    }
    let _ = writeln!(out);
    let f = &r.first_delivery;
    let _ = writeln!(
        out,
        "{indent}  first delivery: {} accepted, {} partial, {} changes requested, {} blocked, {} unreviewed",
        f.accepted, f.partially_accepted, f.changes_requested, f.blocked, f.unreviewed
    );
    let _ = writeln!(
        out,
        "{indent}  defects: {} ({} actor-fault), {} closed, {} open, {} repeats, {} new-in-rework, {} later",
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
        "{indent}  checks: stage ran {} pass, {} fail; actor reported {} pass, {} fail",
        r.stage_checks_passed,
        r.stage_checks_failed,
        r.reported_checks_passed,
        r.reported_checks_failed
    );
    let _ = writeln!(
        out,
        "{indent}  upstream changes: {}, takeovers: {}",
        r.upstream_changes, r.takeovers
    );
    let fo = &r.final_outcome;
    let _ = writeln!(
        out,
        "{indent}  final: {} accepted, {} partial, {} blocked, {} abandoned, {} at cap, {} taken over, {} open",
        fo.accepted,
        fo.partially_accepted,
        fo.blocked,
        fo.abandoned,
        fo.at_cap,
        fo.taken_over,
        fo.open
    );
    let _ = writeln!(out, "{indent}  wall time: {}", fmt_ms(r.wall_time_ms));
}

/// `backlot cast --task <id> --suggest` (L3, D8): each candidate's
/// factual record for the task's kind. Declaration order, counts
/// and honest notices — the view never ranks or picks.
pub fn cast_suggest(
    task: &str,
    task_kind: &str,
    history: &[backlot_model::casting::CandidateHistory],
) -> String {
    let mut out =
        format!("task {task} kind '{task_kind}' — candidate history (factual, no ranking)\n");
    for h in history {
        let ident = h.candidate.actor();
        let effort = match &ident.effort {
            None => String::new(),
            Some(e) => format!("/{e}"),
        };
        let version_note = match h.version_matched {
            Some(backlot_model::casting::VersionMatch::AcrossVersions) => {
                " [matched across runner versions]"
            }
            _ => "",
        };
        let _ = writeln!(
            out,
            "  {} — {}/{}{} ({}){version_note}",
            h.candidate.name, ident.runner, ident.model, effort, ident.permission_profile
        );
        match &h.exact_kind {
            Some(r) => record_lines(&mut out, "    ", r),
            None => {
                let _ = writeln!(out, "    (no history for '{task_kind}')");
            }
        }
        if !h.other_kinds.is_empty() {
            let kinds: Vec<String> = h
                .other_kinds
                .iter()
                .map(|r| format!("{} ({} tasks)", r.task_kind, r.tasks))
                .collect();
            let _ = writeln!(out, "    other kinds: {}", kinds.join(", "));
        }
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
        EventKind::StudioDeleted { archive } => format!(" → {archive}"),
        EventKind::Correction { target, .. } => format!(" on event {target}"),
        _ => String::new(),
    };
    format!("event {}: {}{}", event.seq, event.kind.name(), subject)
}
