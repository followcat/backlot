//! The tick's watch pass: after the actions, look at live roles for
//! the two things only the live view can show — the session id a
//! post-launch runner assigned (recorded as `role.session`, or once
//! after 60 seconds as `attention.raised`), and Claude Code's
//! first-use folder-trust dialog, answered once per role.

use std::collections::BTreeSet;
use std::path::Path;
use std::path::PathBuf;

use backlot_model::event::EventKind;
use backlot_model::state::StudioState;
use hc_proto::client::CallError;
use hc_proto::terminal::ReadParams;

use crate::act::record;
use crate::cli::Fail;
use crate::daemon::Stage;
use crate::discover;
use crate::reconcile::{self, LiveTerminal};

/// A live role may keep its session id hidden this long before the
/// stage asks for attention.
const SESSION_WAIT_MS: u64 = 60_000;
/// The folder-trust dialog is answered only this early in a role.
const TRUST_WINDOW_MS: u64 = 60_000;
/// The attention reason a missing session id raises.
pub(crate) const SESSION_NOT_FOUND: &str = "session id not found";

/// Claude Code's first-use folder-trust dialog on a terminal's
/// screen: the safety-check banner plus the trust option. Anything
/// else — permission prompts included — is not this dialog.
pub(crate) fn trust_dialog(text: &str) -> bool {
    text.contains("Quick safety check") && text.contains("Yes, I trust this folder")
}

/// Whether the role might show the trust dialog at all: claude, or
/// the fake runner's trust mode that exercises the same path.
fn can_show_trust(actor: &backlot_model::ActorIdentity) -> bool {
    actor.runner == "claude" || (actor.runner == "fake" && actor.model == "trust")
}

/// The ledger event a session-less live role's watch yields: a
/// `role.session` when the id was found, or one `attention.raised`
/// once the wait ran out — ever, so a resolved attention stays the
/// answer and the check is the log's whole history.
fn session_event(
    role: &str,
    runner: &str,
    found: Option<String>,
    state: &StudioState,
    age_ms: u64,
) -> Option<EventKind> {
    if let Some(session_id) = found {
        return Some(EventKind::RoleSession {
            role: role.into(),
            session_id,
        });
    }
    if age_ms < SESSION_WAIT_MS
        || state
            .attention
            .iter()
            .any(|a| a.role == role && a.reason == SESSION_NOT_FOUND)
    {
        return None;
    }
    Some(EventKind::AttentionRaised {
        role: role.into(),
        reason: SESSION_NOT_FOUND.into(),
        detail: format!("runner '{runner}' gave no session id within 60s of the role starting"),
    })
}

impl Stage {
    /// Watch one studio's live roles after its actions ran.
    pub(crate) async fn watch(
        &self,
        studio: &str,
        dir: &Path,
        state: &StudioState,
        live: &[LiveTerminal],
        now_ms: u64,
    ) -> Result<(), Fail> {
        let home = self
            .env_vars
            .get("HOME")
            .map(PathBuf::from)
            .unwrap_or_default();
        let Some(repo) = state.studio.as_ref().map(|s| s.repo.clone()) else {
            return Ok(());
        };
        for (key, role) in &state.roles {
            if !role.live {
                continue;
            }
            let (Some(actor), Some(name)) = (&role.actor, reconcile::terminal_name(studio, key))
            else {
                continue;
            };
            let Some(term) = live.iter().find(|t| t.name == name) else {
                continue;
            };
            let age_ms = now_ms.saturating_sub(role.since_ms);

            // The first-use folder-trust dialog: answered once, in
            // the role's first minute only.
            if can_show_trust(actor)
                && age_ms < TRUST_WINDOW_MS
                && !self.trust_answered.borrow().contains(&key_of(studio, key))
            {
                self.answer_trust(studio, key, term, &repo).await?;
            }

            if role.session_id.is_some() {
                continue;
            }
            let taken: BTreeSet<String> = state
                .roles
                .iter()
                .filter(|(k, _)| *k != key)
                .filter_map(|(_, r)| r.session_id.clone())
                .collect();
            let found = discover::find_session(
                &actor.runner,
                &term.argv,
                &repo,
                role.since_ms,
                &taken,
                &home,
                &*self.exec,
            );
            if let Some(kind) = session_event(key, &actor.runner, found, state, age_ms) {
                if let EventKind::RoleSession { session_id, .. } = &kind {
                    eprintln!("stage: {key} session is {session_id}");
                }
                record(dir, kind)?;
            }
        }
        Ok(())
    }

    /// Read the role's screen and, when it is the trust dialog in the
    /// role's own working directory, send Down then Enter — once: the
    /// answered set is marked only after the keys land.
    async fn answer_trust(
        &self,
        studio: &str,
        role: &str,
        term: &LiveTerminal,
        repo: &str,
    ) -> Result<(), Fail> {
        if term.cwd != repo {
            return Ok(());
        }
        let text = match self
            .client
            .terminal_read(ReadParams {
                terminal: term.address.clone(),
                lines: 0,
                caller: None,
            })
            .await
        {
            Ok(r) => r.text,
            // A refused read (the terminal raced away) is nothing.
            Err(CallError::Rpc(_)) => return Ok(()),
            Err(e) => return Err(Fail::refused(format!("core call failed: {e}"))),
        };
        if !trust_dialog(&text) {
            return Ok(());
        }
        let sent = self
            .client
            .call(
                "terminal.keys",
                Some(serde_json::json!({
                    "terminal": term.address,
                    "keys": ["down", "enter"],
                })),
            )
            .await;
        match sent {
            Ok(_) => {
                self.trust_answered
                    .borrow_mut()
                    .insert(key_of(studio, role));
                eprintln!("stage: answered folder trust for {role}");
            }
            Err(CallError::Rpc(_)) => {}
            Err(e) => return Err(Fail::refused(format!("core call failed: {e}"))),
        }
        Ok(())
    }
}

/// The answered set's key: studio-scoped so a role name in two
/// studios answers twice, as it should.
fn key_of(studio: &str, role: &str) -> (String, String) {
    (studio.into(), role.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use backlot_model::state::Attention;

    /// The dialog is its two marker lines together — either alone,
    /// and any other prompt, is left alone.
    #[test]
    fn trust_dialog_needs_both_markers() {
        let screen = "Quick safety check\nDo you trust the files in this folder?\n  Yes, I trust this folder\n  No, exit\n";
        assert!(trust_dialog(screen));
        assert!(!trust_dialog("Yes, I trust this folder\n"));
        assert!(!trust_dialog("Quick safety check\n"));
        assert!(!trust_dialog("Allow edit to src/main.rs? [y/n]\n"));
        assert!(!trust_dialog(""));
    }

    /// A found id records `role.session`; under the wait nothing
    /// happens; past it, one attention — and never a second, even
    /// after the first resolved.
    #[test]
    fn session_event_timing() {
        let state = StudioState::default();
        let found = session_event("producer", "codex", Some("s1".into()), &state, 5_000);
        assert_eq!(
            found,
            Some(EventKind::RoleSession {
                role: "producer".into(),
                session_id: "s1".into(),
            })
        );
        // Inside the wait a miss stays quiet.
        assert_eq!(
            session_event("producer", "codex", None, &state, SESSION_WAIT_MS - 1),
            None
        );
        // Past it the attention raises, once.
        let raised = session_event("producer", "codex", None, &state, SESSION_WAIT_MS);
        assert!(
            matches!(&raised, Some(EventKind::AttentionRaised { reason, .. }) if reason == SESSION_NOT_FOUND),
            "{raised:?}"
        );
        // A prior attention for the same role and reason — open or
        // resolved — means never again; a different role's does not
        // count.
        let mut state = StudioState::default();
        for (role, open) in [("producer", true), ("other", false)] {
            state.attention.push(Attention {
                seq: 1,
                role: role.into(),
                reason: SESSION_NOT_FOUND.into(),
                detail: String::new(),
                open,
            });
        }
        assert_eq!(
            session_event("producer", "codex", None, &state, SESSION_WAIT_MS + 1),
            None
        );
        state.attention[0].open = false;
        assert_eq!(
            session_event("producer", "codex", None, &state, SESSION_WAIT_MS + 1),
            None,
            "resolved stays answered"
        );
        assert!(
            session_event("other", "codex", None, &state, SESSION_WAIT_MS + 1).is_none(),
            "a different role's resolved attention does not fire again"
        );
        assert!(session_event("director", "codex", None, &state, SESSION_WAIT_MS + 1).is_some());
    }
}
