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
use crate::runners;

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

/// Every `(runner, working directory)` a new start must wait on: a
/// live role of a post-launch runner — in any studio — that has no
/// session id yet and has not hit its 60 second attention. Two such
/// roles in one folder could claim the same session, so the second
/// defers until the first has its id or is given up on.
pub(crate) fn discovery_blocked(
    states: &std::collections::BTreeMap<String, StudioState>,
) -> BTreeSet<(String, String)> {
    let mut blocked = BTreeSet::new();
    for s in states.values() {
        let Some(repo) = s.studio.as_ref().map(|c| c.repo.as_str()) else {
            continue;
        };
        for (key, r) in &s.roles {
            let pending = r.live
                && r.session_id.is_none()
                && r.actor
                    .as_ref()
                    .is_some_and(|a| runners::post_launch_session(&a.runner))
                && !s
                    .attention
                    .iter()
                    .any(|a| a.role == *key && a.reason == SESSION_NOT_FOUND);
            if pending {
                let runner = r
                    .actor
                    .as_ref()
                    .map(|a| a.runner.clone())
                    .unwrap_or_default();
                blocked.insert((runner, repo.to_string()));
            }
        }
    }
    blocked
}

/// Session ids already claimed, from every studio's roles — plus the
/// ids this pass already found and recorded, which the loaded states
/// cannot show yet. A `role.session` recorded this tick is never
/// handed out twice.
pub(crate) fn taken_ids(
    states: &std::collections::BTreeMap<String, StudioState>,
    found: &BTreeSet<String>,
) -> BTreeSet<String> {
    states
        .values()
        .flat_map(|s| s.roles.values())
        .filter_map(|r| r.session_id.clone())
        .chain(found.iter().cloned())
        .collect()
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
            // Claimed ids come from every studio, not just this one,
            // and include anything found earlier in this same pass.
            let taken = taken_ids(&self.cached, &self.found_sessions.borrow());
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
                    self.found_sessions.borrow_mut().insert(session_id.clone());
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
    use backlot_model::event::{ActorIdentity, RoleConfigs, StudioMode};
    use backlot_model::state::{Attention, RoleState, StudioConfig};

    fn actor(runner: &str) -> ActorIdentity {
        ActorIdentity {
            runner: runner.into(),
            runner_version: None,
            model: "m".into(),
            effort: "high".into(),
            permission_profile: "standard".into(),
        }
    }

    /// A minimal studio whose roles only need a repo and entries.
    fn studio(repo: &str) -> StudioState {
        StudioState {
            studio: Some(StudioConfig {
                title: "t".into(),
                repo: repo.into(),
                mode: StudioMode::Standard,
                roles: RoleConfigs {
                    producer: actor("codex"),
                    director: actor("codex"),
                    writer: actor("codex"),
                },
                candidates: Vec::new(),
                rework_cap: 2,
            }),
            ..StudioState::default()
        }
    }

    fn live(runner: &str, session: Option<&str>) -> RoleState {
        RoleState {
            live: true,
            actor: Some(actor(runner)),
            session_id: session.map(str::to_string),
            ..RoleState::default()
        }
    }

    fn raised(role: &str) -> Attention {
        Attention {
            seq: 1,
            role: role.into(),
            reason: SESSION_NOT_FOUND.into(),
            detail: String::new(),
            open: true,
        }
    }

    /// A live post-launch role with no id blocks its (runner,
    /// folder) pair — across studios — until it has the id or hit
    /// the attention; chosen-id runners and dead roles never block.
    #[test]
    fn discovery_blocked_while_an_id_is_outstanding() {
        let mut states = std::collections::BTreeMap::new();
        let mut one = studio("/repo");
        one.roles.insert("producer".into(), live("codex", None));
        states.insert("one".into(), one);
        let blocked = discovery_blocked(&states);
        assert!(
            blocked.contains(&("codex".into(), "/repo".into())),
            "{blocked:?}"
        );
        // A second studio's pending role blocks only its own pair.
        let mut two = studio("/other");
        two.roles.insert("director".into(), live("devin", None));
        states.insert("two".into(), two);
        let blocked = discovery_blocked(&states);
        assert_eq!(blocked.len(), 2, "{blocked:?}");
        assert!(blocked.contains(&("devin".into(), "/other".into())));
        // The id arriving — or the attention giving up — frees it.
        states
            .get_mut("one")
            .unwrap()
            .roles
            .get_mut("producer")
            .unwrap()
            .session_id = Some("s1".into());
        let blocked = discovery_blocked(&states);
        assert!(!blocked.contains(&("codex".into(), "/repo".into())));
        states
            .get_mut("one")
            .unwrap()
            .roles
            .get_mut("producer")
            .unwrap()
            .session_id = None;
        states
            .get_mut("one")
            .unwrap()
            .attention
            .push(raised("producer"));
        let blocked = discovery_blocked(&states);
        assert!(!blocked.contains(&("codex".into(), "/repo".into())));
        // A chosen-id runner or a dead role blocks nothing.
        let mut three = studio("/third");
        three.roles.insert("writer".into(), live("claude", None));
        let mut dead = live("codex", None);
        dead.live = false;
        three.roles.insert("actor-t1-c1".into(), dead);
        states.insert("three".into(), three);
        let blocked = discovery_blocked(&states);
        assert_eq!(blocked.len(), 1, "{blocked:?}");
    }

    /// Claimed ids pool across every studio and the ids this pass
    /// already recorded — a session found a second ago is never
    /// handed out again.
    #[test]
    fn taken_ids_pool_every_studio_and_the_pass() {
        let mut states = std::collections::BTreeMap::new();
        let mut one = studio("/a");
        one.roles
            .insert("producer".into(), live("codex", Some("s1")));
        states.insert("one".into(), one);
        let mut two = studio("/b");
        two.roles.insert("writer".into(), live("devin", Some("s2")));
        two.roles.insert("actor-t1-c1".into(), live("codex", None));
        states.insert("two".into(), two);
        let found = BTreeSet::from(["s3".to_string()]);
        let taken = taken_ids(&states, &found);
        assert_eq!(
            taken,
            BTreeSet::from(["s1".into(), "s2".into(), "s3".into()])
        );
    }

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
