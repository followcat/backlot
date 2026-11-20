//! The stall decision (S6a): a live role whose visible screen has
//! not changed for the studio's `stall_limit_s` is stalled. An actor
//! only stalls on a `running` task — `working` and `waiting` both
//! count — and any other role only while `working`: an idle
//! producer, director or writer is normal. `blocked` never counts —
//! a permission prompt has its own attention. The decision reads the
//! same `Screen` stability track `sample_work` keeps for the send
//! gate, so detection needs no extra `terminal.read`.

use backlot_model::event::{EventKind, STALLED};
use backlot_model::state::{Attention, StudioState, TaskStatus, actor_task};
use backlot_model::summary::Work;

use crate::act::record;
use crate::cli::Fail;
use crate::deliver::Screen;
use crate::sample::{Raise, Watched};

/// Whether this sample means `role` stalled. `task` is the actor's
/// own task status — `None` for a role with no task — and `screen`
/// the send gate's stability track for the role.
pub fn stalled(
    role: &str,
    task: Option<TaskStatus>,
    work: Work,
    screen: &Screen,
    limit_ms: u64,
) -> bool {
    if work == Work::Blocked || screen.at_ms.saturating_sub(screen.same_since_ms) < limit_ms {
        return false;
    }
    match actor_task(role) {
        Some(_) => {
            task == Some(TaskStatus::Running) && matches!(work, Work::Working | Work::Waiting)
        }
        None => work == Work::Working,
    }
}

/// The hook-signal half of the stall rule (S9f): a role whose last
/// signal reports Working and is older than the stall limit is as
/// stalled as a frozen screen — the screen may keep spinning while
/// the turn itself is dead. `blocked` never counts, and the
/// actor / fixed-role scoping matches the screen rule's.
pub fn signal_stalled(
    role: &str,
    task: Option<TaskStatus>,
    work: Work,
    signal: &crate::signals::Signal,
    limit_ms: u64,
    now_ms: u64,
) -> bool {
    if work == Work::Blocked
        || signal.work() != Some(Work::Working)
        || now_ms.saturating_sub(signal.ts_ms) < limit_ms
    {
        return false;
    }
    match actor_task(role) {
        Some(_) => task == Some(TaskStatus::Running),
        None => true,
    }
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
    /// The S6 stall pass on the sample just stored: a screen held
    /// unchanged for the studio's `stall_limit_s` raises `stalled`
    /// once — one open attention per role and reason — with the
    /// screen saved under `attention/`; a screen that changed since
    /// the raise clears the attention. A stage restart loses the
    /// in-memory track, so the first sample after it reads as a
    /// fresh screen: an open stall clears and can re-raise one
    /// limit later — acceptable until S6b's recovery work.
    ///
    /// S9f adds the signal half: a role whose last hook signal
    /// reports Working and is older than the limit is as stalled as
    /// a frozen screen — the spinner may keep moving while the turn
    /// itself is dead. Either source can clear an open stall: a new
    /// screen, or a newer signal.
    pub(crate) async fn stall(
        &self,
        studio: &str,
        w: &Watched<'_>,
        work: Work,
        now_ms: u64,
    ) -> Result<(), Fail> {
        let (dir, state, role) = (w.dir, w.state, w.role);
        let pair = (studio.to_string(), role.to_string());
        // The structured stall's clock (S12d): while the terminal
        // reports `working`, its activity revision must keep
        // moving — `rev_since` is when the current revision first
        // showed. A terminal not reporting `working` drops its
        // entry and never stalls this way.
        let working = w.term.status.as_ref().is_some_and(|s| s.state == "working");
        let rev_since = match w.term.activity_revision.filter(|_| working) {
            None => {
                self.revision_since.borrow_mut().remove(&pair);
                None
            }
            Some(rev) => {
                let mut map = self.revision_since.borrow_mut();
                let e = map.entry(pair.clone()).or_insert((rev, now_ms));
                if e.0 != rev {
                    *e = (rev, now_ms);
                }
                Some(e.1)
            }
        };
        let Some(screen) = self.screens.borrow().get(&pair).cloned() else {
            return Ok(());
        };
        let signal = self.last_signal.borrow().get(&pair).cloned();
        if let Some(attn) = open_stall(state, role) {
            // A newer signal — the turn demonstrably alive — an
            // activity revision that moved since the raise, or the
            // screen having moved ends the episode.
            let cleared = if signal.as_ref().is_some_and(|s| s.ts_ms > attn.ts_ms) {
                Some("new hook signal")
            } else if rev_since.is_some_and(|s| s > attn.ts_ms) {
                Some("activity resumed")
            } else if screen.same_since_ms > attn.ts_ms {
                Some("screen changed")
            } else {
                None
            };
            if let Some(reason) = cleared {
                record(
                    dir,
                    EventKind::AttentionCleared {
                        target: attn.seq,
                        reason: Some(reason.into()),
                    },
                )?;
            }
            return Ok(());
        }
        let task = actor_task(role)
            .and_then(|id| state.tasks.get(id))
            .map(|t| t.status);
        let limit_ms = u64::from(state.stall_limit_s()) * 1000;
        if let Some(sig) = signal.filter(|s| signal_stalled(role, task, work, s, limit_ms, now_ms))
        {
            let silent_s = now_ms.saturating_sub(sig.ts_ms) / 1000;
            self.raise_attention(
                Raise {
                    dir,
                    role,
                    reason: STALLED,
                    text: &screen.text,
                    needs_user: false,
                    bound: None,
                },
                move |seq| {
                    format!("no hook signal for {silent_s}s; screen saved to attention/{seq}.txt")
                },
            )
            .await?;
            crate::stagelog::stage_log!("{role} stalled");
        } else if rev_since.is_some_and(|s| now_ms.saturating_sub(s) >= limit_ms) {
            // The terminal still says `working` but its activity
            // revision has not moved for the limit — the
            // structured twin of the frozen-screen stall.
            let held_s = now_ms.saturating_sub(rev_since.unwrap_or(now_ms)) / 1000;
            self.raise_attention(
                Raise {
                    dir,
                    role,
                    reason: STALLED,
                    text: &screen.text,
                    needs_user: false,
                    bound: None,
                },
                move |seq| {
                    format!(
                        "status 'working' but activity revision unchanged for {held_s}s; screen saved to attention/{seq}.txt"
                    )
                },
            )
            .await?;
            crate::stagelog::stage_log!("{role} stalled");
        } else if stalled(role, task, work, &screen, limit_ms) {
            let held_s = screen.at_ms.saturating_sub(screen.same_since_ms) / 1000;
            self.raise_attention(
                Raise {
                    dir,
                    role,
                    reason: STALLED,
                    text: &screen.text,
                    needs_user: false,
                    bound: None,
                },
                move |seq| {
                    format!("screen unchanged for {held_s}s; screen saved to attention/{seq}.txt")
                },
            )
            .await?;
            crate::stagelog::stage_log!("{role} stalled");
        }
        Ok(())
    }
}
