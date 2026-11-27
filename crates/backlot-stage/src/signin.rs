//! Startup sign-in tracking and effective structured-state projection.
//!
//! `agy` emits `status: { state: "blocked", kind: "auth" }` while its startup
//! `Signing in...` screen renders transiently before the first prompt. Wait
//! out [`DEFAULT_TIMEOUT_MS`] before raising `auth error` (`needs_user`), and
//! avoid misreading the person's non-empty composer draft behind `hc`'s
//! `waiting_for_person` hold flag as a blocked prompt.

use backlot_model::event::EventKind;
use backlot_model::summary::Work;

use crate::act;
use crate::cli::Fail;
use crate::daemon::Stage;
use crate::handoff::DEFAULT_TIMEOUT_MS;
use crate::handoff_term::AUTH_ERROR;
use crate::reconcile::LiveTerminal;
use crate::runners::{dialog, work};
use crate::sample::{Raise, Watched};

/// Project `hc`'s structured status and `waiting_for_person` flag together with
/// the visible screen so transient `agy` sign-in stays `Working` and an unsent
/// draft without a recognized dialog stays `Waiting`.
pub(crate) fn effective_structured(
    runner: &str,
    term: &LiveTerminal,
    screen: &str,
) -> Option<Work> {
    let raw = crate::watch::structured_state(term)?;
    if raw == Work::Blocked {
        if runner == "agy" && work::agy_signing_in(screen) {
            return Some(Work::Working);
        }
        if term.waiting_for_person
            && dialog::has_non_empty_draft(screen)
            && !work::recognized_permission(runner, screen)
            && !work::recognized_question(runner, screen)
        {
            return Some(Work::Waiting);
        }
    }
    Some(raw)
}

impl Stage {
    /// Track `agy`'s startup `Signing in...` screen per `(studio, role)`:
    /// stay silent while transient, raise `auth error` (`needs_user = true`)
    /// once it persists past [`DEFAULT_TIMEOUT_MS`], and clear it once the
    /// sign-in screen disappears.
    pub(crate) async fn signin(
        &self,
        studio: &str,
        w: &Watched<'_>,
        text: &str,
        now_ms: u64,
    ) -> Result<(), Fail> {
        let pair = (studio.to_string(), w.role.to_string());
        let signing_in = w.runner == "agy" && work::agy_signing_in(text);
        if signing_in {
            let since = *self.signin_since.borrow_mut().entry(pair).or_insert(now_ms);
            let elapsed_ms = now_ms.saturating_sub(since);
            if elapsed_ms >= DEFAULT_TIMEOUT_MS && !act::attention_open(w.state, w.role, AUTH_ERROR)
            {
                self.raise_attention(
                    Raise {
                        dir: w.dir,
                        role: w.role,
                        reason: AUTH_ERROR,
                        text,
                        needs_user: true,
                        bound: None,
                        account: None,
                        reset_at_ms: None,
                    },
                    move |_| {
                        format!(
                            "agy is still signing in after {}s; check authentication or run `agy` interactively",
                            elapsed_ms / 1000
                        )
                    },
                )
                .await?;
            }
            return Ok(());
        }
        self.signin_since.borrow_mut().remove(&pair);
        if let Some(attn) = w
            .state
            .open_attention()
            .find(|a| a.role == w.role && a.reason == AUTH_ERROR)
        {
            act::record(
                w.dir,
                EventKind::AttentionCleared {
                    target: attn.seq,
                    reason: Some("signed in".into()),
                },
            )?;
        }
        Ok(())
    }
}
