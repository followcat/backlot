//! Bringing a lost role back (S6b): resume into its recorded
//! session, or one `cannot resume` attention saying why not. The
//! guard caps a crash loop — losses inside the window count, and
//! the third one stops the resume and raises the attention instead.
//! The loss times live in `RoleState`, so a stage restart keeps
//! the count.

use std::collections::BTreeSet;

use backlot_model::event::{CANNOT_RESUME, RECOVERED};
use backlot_model::state::{RoleLifecycle, RoleState, StudioState};

use super::{Action, Resume, Start};

/// Losses inside this window count against the resume guard.
pub const LOSS_WINDOW_MS: u64 = 10 * 60 * 1_000;

/// This many losses inside the window still allow a resume; the
/// next one raises `cannot resume` instead.
pub const LOSS_MAX: usize = 3;

/// What a wanted lost role gets this pass.
#[derive(Debug, Clone, PartialEq)]
pub enum Lost {
    /// `Action::Start` resuming this session, reason `recovered`.
    Resume(String),
    /// `attention.raised` with `cannot resume` and this detail.
    Attention(String),
}

/// Resume into the recorded session inside the guard; otherwise the
/// `cannot resume` detail — `no session id to resume`, or
/// `lost <n> times in 10 minutes`.
pub fn lost_step(role: &RoleState, now_ms: u64) -> Lost {
    let recent = role
        .losses
        .iter()
        .filter(|ts| now_ms.saturating_sub(**ts) <= LOSS_WINDOW_MS)
        .count();
    match &role.session_id {
        Some(session) if recent < LOSS_MAX => Lost::Resume(session.clone()),
        Some(_) => Lost::Attention(format!("lost {recent} times in 10 minutes")),
        None => Lost::Attention("no session id to resume".into()),
    }
}

/// `role.lost` plus a `role lost` clear for every open attention of
/// the role — the prompt or stall it named is gone with the
/// terminal, in the same pass. `cleared` keeps a seq from clearing
/// twice in one pass.
pub fn lost_records(
    state: &StudioState,
    key: &str,
    records: &mut Vec<Action>,
    cleared: &mut BTreeSet<u64>,
) {
    records.push(Action::Lost {
        role: key.into(),
        reason: "terminal gone".into(),
    });
    for a in state.open_attention().filter(|a| a.role == key) {
        if cleared.insert(a.seq) {
            records.push(Action::ClearAttention {
                target: a.seq,
                reason: "role lost".into(),
            });
        }
    }
}

/// The wanted lost role's moves for this pass. A live terminal
/// under the role's name records `role.resumed` with that address —
/// no `terminal.run`, and the `recovered` notice still goes out —
/// checked before the guard. Without one the guard decides: a
/// resume `Start` into the recorded session, or a `cannot resume`
/// attention saying why not.
pub fn lost_actions(
    key: &str,
    role: &RoleState,
    want: &Start,
    term: Option<&String>,
    now_ms: u64,
    records: &mut Vec<Action>,
    starts: &mut Vec<Action>,
) {
    if let (Some(session), Some(address)) = (&role.session_id, term) {
        records.push(Action::Resumed {
            role: key.into(),
            address: address.clone(),
            session: session.clone(),
        });
        return;
    }
    match lost_step(role, now_ms) {
        Lost::Resume(session) => {
            let mut want = want.clone();
            want.resume = Some(Resume {
                session,
                reason: RECOVERED,
            });
            starts.push(Action::Start(want));
        }
        Lost::Attention(detail) => {
            records.push(Action::Attention {
                role: key.into(),
                reason: CANNOT_RESUME.into(),
                detail,
            });
        }
    }
}

/// The clear an open `cannot resume` is owed this pass — once the
/// role is live again or no longer wanted. `cleared` keeps a seq
/// from clearing twice in one pass.
pub fn cannot_resume_clear(
    key: &str,
    role: &RoleState,
    state: &StudioState,
    wanted: bool,
    records: &mut Vec<Action>,
    cleared: &mut BTreeSet<u64>,
) {
    let Some(reason) = cannot_resume_done(role, wanted) else {
        return;
    };
    let Some(a) = state
        .open_attention()
        .find(|a| a.role == key && a.reason == CANNOT_RESUME)
    else {
        return;
    };
    if cleared.insert(a.seq) {
        records.push(Action::ClearAttention {
            target: a.seq,
            reason: reason.into(),
        });
    }
}

/// Whether the role's open `cannot resume` attention is done — the
/// role is live again or no longer wanted — and the clear's reason.
pub fn cannot_resume_done(role: &RoleState, wanted: bool) -> Option<&'static str> {
    if role.lifecycle() == RoleLifecycle::Live {
        Some("role live")
    } else if !wanted {
        Some("no longer wanted")
    } else {
        None
    }
}
