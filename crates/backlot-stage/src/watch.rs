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

/// The one key the trust dialog wants next, read off the screen.
/// The picker cursor `❯` on the `No, exit` line needs a `down` to
/// reach the trust option; on the `Yes, I trust this folder` line an
/// `enter` accepts. No cursor on an option line — or a screen that is
/// not the dialog — sends nothing. Enter goes out only with the
/// cursor on Yes.
pub(crate) fn trust_key(text: &str) -> Option<&'static str> {
    if !trust_dialog(text) {
        return None;
    }
    for line in text.lines() {
        if !line.contains('❯') {
            continue;
        }
        if line.contains("No, exit") {
            return Some("down");
        }
        if line.contains("Yes, I trust this folder") {
            return Some("enter");
        }
    }
    None
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

            // The first-use folder-trust dialog: one key per tick,
            // in the role's first minute only — the window bounds
            // the exchange.
            if can_show_trust(actor) && age_ms < TRUST_WINDOW_MS {
                self.answer_trust(key, term, &repo).await?;
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

    /// Read the role's screen and, when the trust dialog shows in
    /// the role's own working directory, send the one key the
    /// cursor position calls for. One key per tick: the dialog
    /// needs the redraw between them, and [`trust_key`] never sends
    /// Enter while the cursor is off the trust option.
    async fn answer_trust(&self, role: &str, term: &LiveTerminal, repo: &str) -> Result<(), Fail> {
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
        let Some(key) = trust_key(&text) else {
            return Ok(());
        };
        let sent = self
            .client
            .call(
                "terminal.keys",
                Some(serde_json::json!({
                    "terminal": term.address,
                    "keys": [key],
                })),
            )
            .await;
        match sent {
            Ok(_) => eprintln!("stage: answered folder trust for {role} ({key})"),
            Err(CallError::Rpc(_)) => {}
            Err(e) => return Err(Fail::refused(format!("core call failed: {e}"))),
        }
        Ok(())
    }
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

    /// The key follows the `❯`: on `No, exit` it is `down`, on
    /// `Yes, I trust this folder` it is `enter`; an option without
    /// the cursor, a cursor elsewhere or another prompt sends
    /// nothing — Enter only ever goes out on Yes.
    #[test]
    fn trust_key_follows_the_cursor() {
        let on_no = "Quick safety check\nDo you trust the files in this folder?\n❯ No, exit\n  Yes, I trust this folder\n";
        assert_eq!(trust_key(on_no), Some("down"));
        let on_yes = "Quick safety check\nDo you trust the files in this folder?\n  No, exit\n❯ Yes, I trust this folder\n";
        assert_eq!(trust_key(on_yes), Some("enter"));
        // Options drawn with the cursor elsewhere, or not at all.
        let cursor_elsewhere =
            "Quick safety check\n❯ Continue\n  No, exit\n  Yes, I trust this folder\n";
        assert_eq!(trust_key(cursor_elsewhere), None);
        let no_cursor = "Quick safety check\n  No, exit\n  Yes, I trust this folder\n";
        assert_eq!(trust_key(no_cursor), None);
        // Not the dialog at all.
        assert_eq!(trust_key("❯ allow edit? [y/n]\n"), None);
        assert_eq!(trust_key(""), None);
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
