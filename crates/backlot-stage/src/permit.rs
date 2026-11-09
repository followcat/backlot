//! Permission prompts (D9): a live role sampled `blocked` on two
//! samples two seconds apart raises one `permission prompt`
//! attention per blocked episode, with the visible screen saved to
//! `attention/<seq>.txt`. An actor's episode owes the director a
//! notice pointing at that file; any other role's says it needs the
//! user. A recorded `role.approval` sends the runner's approval key
//! only while the prompt still shows — never without a recorded
//! decision — then clears the attention; a prompt already gone
//! clears with no key sent.

use backlot_model::event::{ApprovalDecision, PERMISSION_PROMPT};
use backlot_model::state::{ApprovalRecord, Attention, StudioState, actor_task};

/// Two samples this far apart make a blocked episode — the same
/// window the send gate's screen stability uses.
pub const EPISODE_MIN_MS: u64 = 2_000;

/// The key a recorded decision sends while the prompt still shows:
/// `enter` takes the highlighted allow option, `esc` dismisses. That
/// is measured on `fake` (the byte stream is asserted in tests) and
/// matches the permission pickers `claude` and `codex` show; any
/// other runner is unmeasured and answers nothing — the open
/// attention stays for the user. docs/studio/runners.md says which
/// are measured.
pub fn approval_keys(runner: &str, decision: ApprovalDecision) -> Option<&'static str> {
    match runner {
        "fake" | "claude" | "codex" => Some(match decision {
            ApprovalDecision::Allow => "enter",
            ApprovalDecision::Deny => "esc",
        }),
        _ => None,
    }
}

/// The role's open `permission prompt` attention, when it has one.
pub fn open_episode<'s>(state: &'s StudioState, role: &str) -> Option<&'s Attention> {
    state
        .open_attention()
        .find(|a| a.role == role && a.reason == PERMISSION_PROMPT)
}

/// The latest decision a role has recorded — the one the stage
/// acts on.
pub fn pending_approval<'s>(state: &'s StudioState, role: &str) -> Option<&'s ApprovalRecord> {
    state.approvals.iter().rev().find(|a| a.role == role)
}

/// Whether the role's current blocked episode already produced an
/// attention: one with a timestamp at or after the episode's start
/// covers it — open or cleared. The stage learns the episode start
/// from its own samples, so a restart starts a new episode.
pub fn episode_raised(state: &StudioState, role: &str, blocked_since_ms: u64) -> bool {
    state
        .attention
        .iter()
        .any(|a| a.role == role && a.reason == PERMISSION_PROMPT && a.ts_ms >= blocked_since_ms)
}

/// Whether the role key is an actor — only actors' episodes notify
/// the director; anyone else's needs the user.
pub fn is_actor(role: &str) -> bool {
    actor_task(role).is_some()
}

/// The director's notice for an actor's episode, pointing at the
/// saved screen — `None` for any other role, whose attention says
/// it needs the user.
pub fn director_notice(attn: &Attention, detail_file: &str) -> Option<String> {
    if !is_actor(&attn.role) {
        return None;
    }
    Some(format!(
        "# permission prompt on {}\n\n`{}` is waiting on a permission prompt; its screen was saved to `{detail_file}`.\n\nApprove only when the prompt stays inside that actor's own work area — a prompt touching real sessions, production, accounts or global settings, or destructive outside the work area, goes to the user.\n\n`backlot approve --person {} --decision allow` (or `deny`)\n",
        attn.role, attn.role, attn.role,
    ))
}
