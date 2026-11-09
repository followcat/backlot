//! The stall decision (S6a): a live role whose visible screen has
//! not changed for the studio's `stall_limit_s` is stalled. An actor
//! only stalls on a `running` task — `working` and `waiting` both
//! count — and any other role only while `working`: an idle
//! producer, director or writer is normal. `blocked` never counts —
//! a permission prompt has its own attention. The decision reads the
//! same `Screen` stability track `sample_work` keeps for the send
//! gate, so detection needs no extra `terminal.read`.

use backlot_model::event::STALLED;
use backlot_model::state::{Attention, StudioState, TaskStatus, actor_task};
use backlot_model::summary::Work;

use crate::deliver::Screen;

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

/// The role's open `stalled` attention, when it has one.
pub fn open_stall<'s>(state: &'s StudioState, role: &str) -> Option<&'s Attention> {
    state
        .open_attention()
        .find(|a| a.role == role && a.reason == STALLED)
}

/// Who a `stalled` attention notices: an actor's task reviewer —
/// the producer on a producer cast, the director otherwise and on a
/// task already gone — the producer for the writer and the
/// director, and nobody for the producer: the panel already shows
/// its open attention.
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
        None
    } else {
        Some("producer".into())
    }
}

/// The `stalled` notice body: the role, the duration the raise
/// recorded, the saved screen and the ways on — a `tell` nudge for
/// any role, abandon-or-recast for an actor.
pub fn notice_body(attention: &Attention, screen_file: &str) -> String {
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
