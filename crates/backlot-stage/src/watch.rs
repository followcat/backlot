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
pub(crate) const SESSION_WAIT_MS: u64 = 60_000;
/// A session-less role's id is looked for at most this often —
/// a rollout walk or a `devin list` every tick would block the loop.
pub(crate) const DISCOVERY_EVERY_MS: u64 = 5_000;
/// … and stops entirely after this long; the role's 60 second
/// attention has long said its piece by then.
pub(crate) const DISCOVERY_STOP_MS: u64 = 600_000;
/// The folder-trust dialog is answered only this early in a role.
pub(crate) const TRUST_WINDOW_MS: u64 = 60_000;
/// How long the dialog must be on screen before any key goes out:
/// measured on Claude Code 2.1.319 — a `down` sent as the dialog
/// first paints is lost, and an `enter` that early ends the runner.
pub(crate) const TRUST_SETTLE_MS: u64 = 2_000;
/// The least time between two keys, whatever they are.
pub(crate) const TRUST_KEY_GAP_MS: u64 = 1_500;
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

/// One role's trust exchange so far: when the dialog was first
/// seen and when the last key went out. Missing state means the
/// dialog is not up (or was never seen).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct TrustPace {
    pub seen_ms: u64,
    pub sent_ms: Option<u64>,
}

/// The pacing of the answer, pure so the injected clock tests it:
/// `key` is [`trust_key`]'s read of the current screen. No key on
/// screen resets the exchange — a dialog that comes back settles
/// again. The first key waits for [`TRUST_SETTLE_MS`] of settled
/// screen time; every later key waits [`TRUST_KEY_GAP_MS`] after
/// the previous one. Enter still needs the cursor on Yes —
/// `trust_key` decides that, this only decides when.
pub(crate) fn trust_step(
    prev: Option<TrustPace>,
    key: Option<&'static str>,
    now_ms: u64,
) -> (Option<TrustPace>, Option<&'static str>) {
    let Some(key) = key else {
        return (None, None);
    };
    let Some(pace) = prev else {
        return (
            Some(TrustPace {
                seen_ms: now_ms,
                sent_ms: None,
            }),
            None,
        );
    };
    let due = match pace.sent_ms {
        None => now_ms.saturating_sub(pace.seen_ms) >= TRUST_SETTLE_MS,
        Some(sent) => now_ms.saturating_sub(sent) >= TRUST_KEY_GAP_MS,
    };
    if due {
        (
            Some(TrustPace {
                seen_ms: pace.seen_ms,
                sent_ms: Some(now_ms),
            }),
            Some(key),
        )
    } else {
        (Some(pace), None)
    }
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

/// Whether this tick pays for a discovery look at all: never once
/// the role's attention exists or it is past the hard stop, and at
/// most once every [`DISCOVERY_EVERY_MS`] otherwise. `now_ms` is the
/// tick's clock, injected so the cadence is testable.
pub(crate) fn discovery_due(
    role: &str,
    state: &StudioState,
    age_ms: u64,
    last_look_ms: Option<u64>,
    now_ms: u64,
) -> bool {
    if age_ms > DISCOVERY_STOP_MS
        || state
            .attention
            .iter()
            .any(|a| a.role == role && a.reason == SESSION_NOT_FOUND)
    {
        return false;
    }
    last_look_ms.is_none_or(|t| now_ms.saturating_sub(t) >= DISCOVERY_EVERY_MS)
}

/// The ledger event a session-less live role's watch yields: a
/// `role.session` when the id was found, or one `attention.raised`
/// once the wait ran out — ever, so a resolved attention stays the
/// answer and the check is the log's whole history.
pub(crate) fn session_event(
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

            // The first-use folder-trust dialog: paced keys, in the
            // role's first minute only — the window bounds the
            // exchange.
            if can_show_trust(actor) && age_ms < TRUST_WINDOW_MS {
                self.answer_trust(studio, key, term, &repo, now_ms).await?;
            }

            if role.session_id.is_some() {
                continue;
            }
            // Discovery is throttled: at most one look per five
            // seconds per role, and none once the role's attention
            // exists or ten minutes passed.
            let pair = (studio.to_string(), key.clone());
            let last_look = self.discover_looked.borrow().get(&pair).copied();
            let found = if discovery_due(key, state, age_ms, last_look, now_ms) {
                self.discover_looked.borrow_mut().insert(pair, now_ms);
                // Claimed ids come from every studio, not just this
                // one, plus anything found earlier in this pass.
                let taken = taken_ids(&self.cached, &self.found_sessions.borrow());
                discover::find_session(
                    &actor.runner,
                    &term.argv,
                    &repo,
                    role.since_ms,
                    &taken,
                    &home,
                    &*self.exec,
                )
            } else {
                None
            };
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
    /// the role's own working directory, send the key [`trust_step`]
    /// paces — never before the dialog has settled, never inside
    /// the gap after the previous key.
    async fn answer_trust(
        &self,
        studio: &str,
        role: &str,
        term: &LiveTerminal,
        repo: &str,
        now_ms: u64,
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
        let pair = (studio.to_string(), role.to_string());
        let prev = self.trust_pace.borrow().get(&pair).copied();
        let (pace, key) = trust_step(prev, trust_key(&text), now_ms);
        match pace {
            Some(p) => self.trust_pace.borrow_mut().insert(pair, p),
            None => self.trust_pace.borrow_mut().remove(&pair),
        };
        let Some(key) = key else {
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
