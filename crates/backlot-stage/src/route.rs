//! Routing an actor's open permission prompt to the person when the
//! director cannot answer it (field notes 1126 §5): a prompt that
//! waits on a director who is rate-limited, stalled or gone must not
//! wait for hours. Past a short grace the actor's open `permission
//! prompt` — the one whose notice only the director could act on — is
//! cleared and re-raised `needs_user`, with the reason and the wait
//! in its detail (`the director is rate-limited until 12:10; waiting
//! 14m`). The person is then the prompt's owner: the panel and
//! Hollywood's `prompt` notice show it, and the director's `approve`
//! is refused like any other user-owned prompt.

use backlot_model::event::{EventKind, PERMISSION_PROMPT, STALLED};
use backlot_model::state::StudioState;

use crate::act::record;
use crate::cli::Fail;
use crate::daemon::Stage;

/// How long an actor's prompt may wait on an unavailable director
/// before it is the person's (measured from the attention's raise —
/// a prompt already older than this routes at once).
pub const ROUTE_GRACE_MS: u64 = 60_000;

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

/// Whether an actor's open prompt has waited past the grace.
pub fn due(attn_ts_ms: u64, now_ms: u64) -> bool {
    now_ms.saturating_sub(attn_ts_ms) >= ROUTE_GRACE_MS
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
    /// Re-route any actor's open, director-owned permission prompt to
    /// the person once the director cannot answer it. Returns whether
    /// the ledger grew, so the caller reloads before building notices
    /// on it. An already-user-owned prompt is never touched.
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
        let Some(blocker) = director_blocker(state, due_ms) else {
            return Ok(false);
        };
        let mut grew = false;
        for attn in state.open_attention() {
            if attn.reason != PERMISSION_PROMPT
                || attn.needs_user
                || !crate::permit::is_actor(&attn.role)
                || !due(attn.ts_ms, now_ms)
            {
                continue;
            }
            // The same screen the first attention saved, so the
            // person reads exactly what the director would have.
            let saved = dir.join("attention").join(format!("{}.txt", attn.seq));
            let text = self
                .driver
                .read_file(&saved)
                .unwrap_or_else(|_| attn.detail.clone());
            record(
                dir,
                EventKind::AttentionCleared {
                    target: attn.seq,
                    reason: Some(format!(
                        "routed to the user — {}",
                        detail(blocker, now_ms.saturating_sub(attn.ts_ms), now_ms).trim_end()
                    )),
                },
            )?;
            let why = detail(blocker, now_ms.saturating_sub(attn.ts_ms), now_ms);
            self.raise_attention_now(dir, &attn.role, &text, &why)
                .await?;
            grew = true;
            crate::stagelog::stage_log!(
                "{}'s permission prompt routed to the user: {}",
                attn.role,
                why.trim_end_matches("; ")
            );
        }
        Ok(grew)
    }

    /// The `needs_user` re-raise of a routed prompt: the same screen,
    /// the reason prefix in the detail, and `attention/<seq>.txt`
    /// saved by the append.
    async fn raise_attention_now(
        &self,
        dir: &std::path::Path,
        role: &str,
        text: &str,
        why: &str,
    ) -> Result<(), Fail> {
        use crate::sample::Raise;
        let why = why.to_string();
        self.raise_attention(
            Raise {
                dir,
                role,
                reason: PERMISSION_PROMPT,
                text,
                needs_user: true,
                bound: None,
            },
            move |seq| format!("{why}screen saved to attention/{seq}.txt"),
        )
        .await
    }
}
