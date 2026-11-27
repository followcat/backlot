//! What an attention reason means to the person seeing it: something
//! they must act on, something the stage is still retrying, or the
//! stage's own bookkeeping. One function, shared by the stage, the
//! client and Hollywood, so no surface invents its own reading of a
//! reason.

use serde::{Deserialize, Serialize};

/// How one open attention reads to the person.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttentionClass {
    /// The person must act: a user-owned prompt or failure that neither
    /// the stage nor another role can resolve, a budget overrun, an
    /// owed step, or anything the stage does not recognise.
    NeedsYou,
    /// Something the stage or another role is handling — a
    /// director-handled actor prompt, question, or agent error, a stall
    /// with nudges left, a rate limit, or a connection error that has
    /// not escalated yet.
    Stuck,
    /// The stage's own bookkeeping. The person cannot act on it; it
    /// never changes a state and never counts as flagged.
    Info,
}

impl AttentionClass {
    /// The plain word every surface prints for this class — the panel's
    /// table, `backlot status` and Hollywood's facts all read it here,
    /// so no surface invents its own wording.
    pub fn label(self) -> &'static str {
        match self {
            AttentionClass::NeedsYou => "needs you",
            AttentionClass::Stuck => "stuck",
            AttentionClass::Info => "info",
        }
    }

    /// Sort `reason` into its class. `needs_user` is the ledger
    /// attention's own escalation/ownership flag — when set (e.g. a
    /// non-actor's prompt, a sensitive command, or an escalated
    /// director/stage episode), the class is always `NeedsYou`. Reasons
    /// handled by the director or stage (`permission prompt`,
    /// `question`, `agent error`, `stalled`, `rate limited`,
    /// `connection error`) are `Stuck` while `needs_user` is false.
    /// An unknown reason is `NeedsYou`, so nothing important is hidden.
    ///
    /// An `outward action` refusal is the role's own record of what the
    /// guard stopped: the role was told to `backlot ask` if it still
    /// needs the command, so nothing waits on the person and it is
    /// `Info`. The flag still escalates a raise marked `needs_user`.
    pub fn of(reason: &str, needs_user: bool) -> AttentionClass {
        if needs_user {
            return AttentionClass::NeedsYou;
        }
        if reason == crate::event::OUTWARD_ACTION {
            return AttentionClass::Info;
        }
        match reason {
            "folder-trust"
            | "folder trust refused"
            | "cannot resume"
            | "launch refused"
            | "runner not supported"
            | "permission profile not supported"
            | "model not supported"
            | "effort not supported"
            | "config malformed"
            | "not a git repository"
            | "work area failed"
            | "over budget"
            | "owed"
            | "auth error" => AttentionClass::NeedsYou,
            "permission prompt" | "agent error" | "stalled" | "rate limited"
            | "connection error" | "question" => AttentionClass::Stuck,
            "session id not found"
            | "inherited XDG_CONFIG_HOME"
            | "cursor dir was open"
            | "hooks unavailable" => AttentionClass::Info,
            r if r.ends_with(" prep failed") => AttentionClass::NeedsYou,
            _ => AttentionClass::NeedsYou,
        }
    }
}

/// Interactive prompt/dialog attention reasons that may only be answered in
/// the agent's own conversation or terminal, never cleared over `plugin.backlot.req`.
pub fn is_prompt_reason(reason: &str) -> bool {
    matches!(
        reason,
        crate::event::PERMISSION_PROMPT
            | crate::event::QUESTION
            | crate::event::FOLDER_TRUST
            | "folder trust refused"
    ) || reason.ends_with(" prompt")
        || reason.contains("dialog")
}

#[cfg(test)]
#[path = "attention_tests.rs"]
mod tests;
