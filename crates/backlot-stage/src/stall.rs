//! The unified stall rule (D4): a live, eligible role stalls when none of
//! the six liveness sources in [`crate::liveness::Evidence`] moved within
//! `stall_limit_s` (3x when the deepest descendant is waiting on a kernel inode
//! lock). An actor only stalls on a `running` task (`working` and `waiting`
//! both count); fixed roles stall only while `working`; `blocked` never stalls.

use backlot_model::event::{EventKind, STALLED};
use backlot_model::state::{Attention, StudioState, TaskStatus, actor_task};
use backlot_model::summary::Work;

use crate::act::record;
use crate::cli::Fail;
use crate::liveness::{Evidence, ObserveInput};
use crate::liveness_diff::role_work_roots;
use crate::render::age_short;
use crate::sample::{Raise, Watched};

/// Whether `(role, task, work)` is currently eligible to accumulate stall
/// silence (`blocked` never counts).
pub(crate) fn eligible(role: &str, task: Option<TaskStatus>, work: Work) -> bool {
    if work == Work::Blocked {
        return false;
    }
    match actor_task(role) {
        Some(_) => {
            task == Some(TaskStatus::Running) && matches!(work, Work::Working | Work::Waiting)
        }
        None => work == Work::Working,
    }
}

/// Whether `evidence` indicates `role` is stalled at `now_ms` under `limit_ms`.
pub(crate) fn stalled(
    role: &str,
    task: Option<TaskStatus>,
    work: Work,
    evidence: &Evidence,
    now_ms: u64,
    limit_ms: u64,
) -> bool {
    eligible(role, task, work) && evidence.is_stalled(now_ms, limit_ms)
}

/// The role's open `stalled` attention, when it has one.
pub fn open_stall<'s>(state: &'s StudioState, role: &str) -> Option<&'s Attention> {
    state
        .open_attention()
        .find(|a| a.role == role && a.reason == STALLED)
}

/// Who a `stalled` attention notices: an actor's task reviewer —
/// the producer on a producer cast, the director otherwise and on a
/// task already gone — the producer for the writer and the
/// director, and for the producer itself a live director, which gets
/// an informational word so it does not wait on a producer that cannot
/// answer. The producer's own problem is the user's (or Hollywood's)
/// to resolve; nobody else in the studio can restart it. A studio
/// whose director is not live leaves the panel and `backlot status` as
/// the only surfaces.
pub fn notice_to(state: &StudioState, attention: &Attention) -> Option<String> {
    if let Some(task) = actor_task(&attention.role) {
        return Some(
            state
                .tasks
                .get(task)
                .map(crate::notices::reviewer_for)
                .unwrap_or_else(|| "director".into()),
        );
    }
    if attention.role == "producer" {
        return director_live(state).then(|| "director".into());
    }
    Some("producer".into())
}

/// Whether the ledger's director is live — the one role a producer
/// problem can inform without pretending the studio can restart it.
fn director_live(state: &StudioState) -> bool {
    state.roles.get("director").is_some_and(|r| r.live)
}

/// The `stalled` notice body: the role, the duration the raise
/// recorded, the saved screen and the ways on — a `tell` nudge for
/// any role, abandon-or-recast for an actor. A producer's stall names
/// the one way on: it cannot be restarted from inside the studio.
pub fn notice_body(attention: &Attention, screen_file: &str) -> String {
    if attention.role == "producer" {
        return format!(
            "# producer stalled\n\n`producer` is not moving — {}. The screen is at `{screen_file}` — read it before acting.\n\nThe director cannot restart the producer: only the user (or Hollywood) can intervene. The studio waits meanwhile; no command here revives it.\n",
            attention.detail,
        );
    }
    let ways = match actor_task(&attention.role) {
        Some(id) => format!(
            "\n- `backlot task abandon --task {id} --reason <reason>` and a new task, or recast with `backlot cast --task {id} --actor <name> --reason <reason>`"
        ),
        None => String::new(),
    };
    format!(
        "# {} stalled\n\n`{}` is not moving — {}. The screen is at `{screen_file}` — read it before acting.\n\nWays on:\n\n- `backlot tell {} --file <file>` to nudge it{ways}\n",
        attention.role, attention.role, attention.detail, attention.role,
    )
}

impl crate::daemon::Stage {
    /// Update `role`'s D4 liveness evidence on the sample just stored, clear an
    /// open `stalled` attention if any evidence moved, or raise one once no
    /// source moved for `stall_limit_s` (3x on kernel inode lock waits).
    pub(crate) async fn stall(
        &self,
        studio: &str,
        w: &Watched<'_>,
        work: Work,
        now_ms: u64,
    ) -> Result<(), Fail> {
        let (dir, state, role) = (w.dir, w.state, w.role);
        let pair = (studio.to_string(), role.to_string());
        let Some(screen) = self.screens.borrow().get(&pair).cloned() else {
            return Ok(());
        };
        let signal = self.last_signal.borrow().get(&pair).cloned();
        let task = actor_task(role)
            .and_then(|id| state.tasks.get(id))
            .map(|t| t.status);
        let is_eligible = eligible(role, task, work);
        let local_machine = self.is_local && !w.term.address.contains('/');
        let work_roots = if local_machine {
            role_work_roots(dir, state, role)
        } else {
            Vec::new()
        };
        let terminal_working = w.term.status.as_ref().is_some_and(|s| s.state == "working");
        let input = ObserveInput {
            now_ms,
            runner: w.runner,
            eligible: is_eligible,
            raw_screen: &screen.text,
            raw_same_since_ms: screen.same_since_ms,
            signal: signal.as_ref(),
            terminal_working,
            activity_revision: w.term.activity_revision,
            local_machine,
            pid: w.term.pid,
            work_roots: &work_roots,
            ledger: state.last_by_role.get(role),
        };

        let limit_ms = u64::from(state.stall_limit_s()) * 1000;
        let open = open_stall(state, role);
        let (cleared_action, raise_snap) = {
            let mut map = self.liveness.borrow_mut();
            let mut git = self.git_runner();
            let ev = map
                .entry(pair.clone())
                .and_modify(|e| {
                    e.observe(&input, &*self.proc_read, &*self.work_area_read, &mut **git);
                })
                .or_insert_with(|| {
                    Evidence::new(&input, &*self.proc_read, &*self.work_area_read, &mut **git)
                });

            if let Some(attn) = open {
                (
                    ev.clear_reason(attn.ts_ms, now_ms).map(|r| (attn.seq, r)),
                    None,
                )
            } else {
                ev.clear_raised();
                let raise = stalled(role, task, work, ev, now_ms, limit_ms).then(|| ev.clone());
                (None, raise)
            }
        };

        if let Some((target, reason)) = cleared_action {
            record(
                dir,
                EventKind::AttentionCleared {
                    target,
                    reason: Some(reason.into()),
                },
            )?;
            return Ok(());
        }
        if open.is_some() {
            return Ok(());
        }

        if let Some(ev_snap) = raise_snap {
            self.raise_attention(
                Raise {
                    dir,
                    role,
                    reason: STALLED,
                    text: &screen.text,
                    needs_user: false,
                    bound: None,
                },
                move |seq| stall_detail(&ev_snap, now_ms, seq),
            )
            .await?;
            if let Some(ev) = self.liveness.borrow_mut().get_mut(&pair) {
                ev.mark_raised(now_ms);
            }
            crate::stagelog::stage_log!("{role} stalled");
        }
        Ok(())
    }
}

/// Format the `detail` field of `AttentionRaised` (`STALLED`).
pub(crate) fn stall_detail(ev: &Evidence, now_ms: u64, seq: u64) -> String {
    let silent_s = now_ms.saturating_sub(ev.silence_start_ms()) / 1000;
    if let Some((argv, since_ms)) = ev.lock_wait() {
        let dur = age_short(now_ms.saturating_sub(since_ms));
        return format!("waiting on: {argv} for {dur}; screen saved to attention/{seq}.txt");
    }
    if ev.animates_only() {
        return format!(
            "no sign of life for {silent_s}s; screen animates only (spinner); screen saved to attention/{seq}.txt"
        );
    }
    match ev.last_moved_stamp() {
        Some(stamp) => {
            let age = age_short(now_ms.saturating_sub(stamp.at_ms));
            format!(
                "no sign of life for {silent_s}s; last: {} ({}) {age} ago; screen saved to attention/{seq}.txt",
                stamp.desc, stamp.tag
            )
        }
        None => format!("no sign of life for {silent_s}s; screen saved to attention/{seq}.txt"),
    }
}
