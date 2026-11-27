//! Routing an actor's unanswered permission prompt (field notes
//! 1126 §5): when the director is rate-limited, stalled, or not live,
//! the prompt escalates to `needs_user` at once instead of waiting;
//! when the director is live and unblocked, an actor prompt that goes
//! unanswered past the threshold (`default_stall_limit_s`, 10 minutes)
//! escalates to `needs_user` with the wait duration in its detail.

use backlot_model::event::{EventKind, PERMISSION_PROMPT, STALLED};
use backlot_model::state::StudioState;

use crate::act::record;
use crate::cli::Fail;
use crate::daemon::Stage;

/// Unanswered prompt threshold before a live, unblocked director prompt
/// escalates to the user — reuses `default_stall_limit_s` (10 minutes).
pub fn threshold_ms() -> u64 {
    u64::from(backlot_model::event::default_stall_limit_s()) * 1000
}

/// Why the director cannot answer, most specific first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Blocker {
    /// A `rate limited` attention is open on the director; the reset
    /// time, when the stage knows it.
    RateLimited(Option<u64>),
    /// A `stalled` attention is open on the director.
    Stalled,
    /// The director is closed, lost or never started.
    Gone,
}

/// Which blocker the director shows, read from its open attention
/// reasons and its lifecycle — or `None` when the director can
/// answer. A rate limit outranks a stall, which outranks a closed
/// role: the reset is the most useful thing to tell the person.
pub fn blocker(
    rate_due_ms: Option<u64>,
    rate_limited: bool,
    stalled: bool,
    live: bool,
) -> Option<Blocker> {
    if rate_limited {
        return Some(Blocker::RateLimited(rate_due_ms));
    }
    if stalled {
        return Some(Blocker::Stalled);
    }
    if !live {
        return Some(Blocker::Gone);
    }
    None
}

/// Whether an actor's open prompt has waited past the threshold.
pub fn due(attn_ts_ms: u64, now_ms: u64) -> bool {
    now_ms.saturating_sub(attn_ts_ms) >= threshold_ms()
}

/// The detail when a live director leaves an actor's prompt unanswered
/// past the threshold.
pub fn unanswered_detail(waited_ms: u64) -> String {
    let waited = backlot_model::event::age_short(waited_ms);
    format!("director did not answer within {waited}; waiting {waited}; ")
}

fn prompt_prefix(detail: &str) -> String {
    let head = detail
        .split("screen saved to attention/")
        .next()
        .map(str::trim)
        .unwrap_or("");
    if head.is_empty() {
        String::new()
    } else if head.ends_with(';') {
        format!("{head} ")
    } else {
        format!("{head}; ")
    }
}

/// The detail the re-raised attention carries: the reason, the reset
/// when it is a rate limit, and how long the prompt has waited.
pub fn detail(blocker: Blocker, waited_ms: u64, now_ms: u64) -> String {
    let waited = backlot_model::event::age_short(waited_ms);
    match blocker {
        Blocker::RateLimited(Some(reset_ms)) => format!(
            "the director is rate-limited until {}; waiting {waited}; ",
            crate::ratelimit::format_reset_local(reset_ms.max(now_ms), now_ms)
        ),
        Blocker::RateLimited(None) => {
            format!("the director is rate-limited; waiting {waited}; ")
        }
        Blocker::Stalled => format!("the director is stalled; waiting {waited}; "),
        Blocker::Gone => format!("the director is closed; waiting {waited}; "),
    }
}

/// The director's blocker from the ledger and the stage's rate pace.
fn director_blocker(state: &StudioState, rate_due_ms: Option<u64>) -> Option<Blocker> {
    let open = |reason: &str| {
        state
            .open_attention()
            .any(|a| a.role == "director" && a.reason == reason)
    };
    let live = state.roles.get("director").is_some_and(|r| r.live);
    blocker(
        rate_due_ms,
        open(crate::ratelimit::RATE_LIMITED),
        open(STALLED),
        live,
    )
}

impl Stage {
    /// Escalate any actor's open, director-owned permission prompt to
    /// the person (`needs_user = true`) at once when the director is
    /// rate-limited, stalled, or not live, or once unanswered past the
    /// threshold when the director is live. Returns whether the ledger
    /// grew, so the caller reloads before building notices on it.
    pub(crate) async fn route_prompts(
        &self,
        studio: &str,
        dir: &std::path::Path,
        state: &StudioState,
        now_ms: u64,
    ) -> Result<bool, Fail> {
        let due_ms = self
            .rate_pace
            .borrow()
            .get(&(studio.to_string(), "director".to_string()))
            .map(|p| p.reset_at_ms.unwrap_or(p.due_ms));
        let blocker = director_blocker(state, due_ms);
        let mut grew = false;
        for attn in state.open_attention() {
            if attn.reason != PERMISSION_PROMPT
                || attn.needs_user
                || !crate::permit::is_actor(&attn.role)
            {
                continue;
            }
            if blocker.is_none() && !due(attn.ts_ms, now_ms) {
                continue;
            }
            let waited_ms = now_ms.saturating_sub(attn.ts_ms);
            let why = match blocker {
                Some(b) => detail(b, waited_ms, now_ms),
                None => unanswered_detail(waited_ms),
            };
            let prefix = prompt_prefix(&attn.detail);
            record(
                dir,
                EventKind::AttentionEscalated {
                    target: attn.seq,
                    detail: format!("{why}{prefix}screen saved to attention/{}.txt", attn.seq),
                },
            )?;
            grew = true;
            crate::stagelog::stage_log!(
                "{}'s permission prompt escalated to the user: {}",
                attn.role,
                why.trim_end_matches("; ")
            );
        }
        Ok(grew)
    }
}
