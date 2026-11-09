//! Bringing a lost role back (S6b): resume into its recorded
//! session, or one `cannot resume` attention saying why not. The
//! guard caps a crash loop — losses inside the window count, and
//! the third one stops the resume and raises the attention instead.
//! The loss times live in `RoleState`, so a stage restart keeps
//! the count.

use backlot_model::state::{RoleLifecycle, RoleState};

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
