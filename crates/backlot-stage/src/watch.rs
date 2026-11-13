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
use crate::workareas;

/// A live role may keep its session id hidden this long before the
/// stage asks for attention — the default window.
pub(crate) const SESSION_WAIT_MS: u64 = 60_000;

/// The discovery window is per-runner (S12d): cmd's `.jsonl` only
/// lands once the first turn's picker is answered — measured ~175s
/// — so its window is four minutes; every other runner keeps the
/// one-minute default. A session-less role past the default but
/// inside its own window is a soft warning in the log only; the
/// `session id not found` attention waits for its runner's window.
pub(crate) fn session_wait_ms(runner: &str) -> u64 {
    match runner {
        "cmd" => 240_000,
        _ => SESSION_WAIT_MS,
    }
}

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
/// A live role's screen is sampled for work at most this often.
pub(crate) const WORK_SAMPLE_EVERY_MS: u64 = 1_000;
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

/// A `session not found` attention from the role's current life —
/// raised at or after `since_ms`. An older one is the previous
/// terminal's business: it neither stops the looks nor holds a
/// pending `(runner, folder)` pair.
fn session_attention<'a>(
    state: &'a StudioState,
    role: &str,
    since_ms: u64,
) -> Option<&'a backlot_model::state::Attention> {
    state
        .attention
        .iter()
        .find(|a| a.role == role && a.reason == SESSION_NOT_FOUND && a.ts_ms >= since_ms)
}

/// Every `(runner, working directory)` a new start must wait on: a
/// live role of a post-launch runner — in any studio — that has no
/// session id yet and has not hit its 60 second attention *in this
/// life*. Two such roles in one folder could claim the same session,
/// so the second defers until the first has its id or is given up
/// on. The folder is the role's work area, so `dir_of` maps a studio
/// name to its state directory.
pub(crate) fn discovery_blocked(
    states: &std::collections::BTreeMap<String, StudioState>,
    dir_of: &dyn Fn(&str) -> PathBuf,
) -> BTreeSet<(String, String)> {
    let mut blocked = BTreeSet::new();
    for (name, s) in states {
        let Some(repo) = s.studio.as_ref().map(|c| c.repo.as_str()) else {
            continue;
        };
        let dir = dir_of(name);
        for (key, r) in &s.roles {
            let pending = r.live
                && r.session_id.is_none()
                && r.actor
                    .as_ref()
                    .is_some_and(|a| runners::post_launch_session(&a.runner))
                && session_attention(s, key, r.since_ms).is_none();
            if pending {
                let runner = r
                    .actor
                    .as_ref()
                    .map(|a| a.runner.clone())
                    .unwrap_or_default();
                blocked.insert((
                    runner,
                    workareas::role_dir(&dir, repo, key)
                        .to_string_lossy()
                        .into_owned(),
                ));
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

/// Whether this tick pays for a discovery look at all: at most once
/// every [`DISCOVERY_EVERY_MS`], and never past the hard stop. The
/// 60 second attention never pauses the looks — a slow first start
/// still gets found until then. `now_ms` is the tick's clock,
/// injected so the cadence is testable.
pub(crate) fn discovery_due(age_ms: u64, last_look_ms: Option<u64>, now_ms: u64) -> bool {
    if age_ms > DISCOVERY_STOP_MS {
        return false;
    }
    last_look_ms.is_none_or(|t| now_ms.saturating_sub(t) >= DISCOVERY_EVERY_MS)
}

/// Whether this tick reads the role's screen for a work sample:
/// never before a second has passed since the last read.
pub(crate) fn work_due(last_ms: Option<u64>, now_ms: u64) -> bool {
    last_ms.is_none_or(|t| now_ms.saturating_sub(t) >= WORK_SAMPLE_EVERY_MS)
}

/// The terminal's structured word for the work map (S12d):
/// `waiting_for_person` is the core's own blocked flag and catches
/// prompt kinds no screen marker knows; a `status.state` of the
/// usual words maps straight. `error` is the rate-limit pass's
/// input — the work map reads the screen for it — and a terminal
/// reporting nothing (`None`) leaves the whole answer to the
/// screen, so an upstream core without the fork's inference still
/// works.
pub(crate) fn structured_state(term: &LiveTerminal) -> Option<backlot_model::summary::Work> {
    use backlot_model::summary::Work;
    if term.waiting_for_person {
        return Some(Work::Blocked);
    }
    match term.status.as_ref().map(|s| s.state.as_str()) {
        Some("working") => Some(Work::Working),
        Some("blocked") => Some(Work::Blocked),
        Some("idle") | Some("done") | Some("waiting") => Some(Work::Waiting),
        _ => None,
    }
}

/// The ledger events a session-less live role's watch yields: a
/// `role.session` — plus an `attention.cleared` for any still-open
/// `session not found` of the role — when the id was found, or one
/// `attention.raised` once the wait ran out in this life, so a
/// relaunched role can raise its own while a resolved one stays the
/// answer.
pub(crate) fn session_event(
    role: &str,
    runner: &str,
    found: Option<String>,
    state: &StudioState,
    since_ms: u64,
    age_ms: u64,
) -> Vec<EventKind> {
    if let Some(session_id) = found {
        let mut events = vec![EventKind::RoleSession {
            role: role.into(),
            session_id,
        }];
        events.extend(
            state
                .open_attention()
                .filter(|a| a.role == role && a.reason == SESSION_NOT_FOUND)
                .map(|a| EventKind::AttentionCleared {
                    target: a.seq,
                    reason: Some("session found".into()),
                }),
        );
        return events;
    }
    let wait_ms = session_wait_ms(runner);
    if age_ms < wait_ms || session_attention(state, role, since_ms).is_some() {
        return Vec::new();
    }
    vec![EventKind::AttentionRaised {
        role: role.into(),
        reason: SESSION_NOT_FOUND.into(),
        detail: format!(
            "runner '{runner}' gave no session id within {}s of the role starting",
            wait_ms / 1000
        ),
        needs_user: false,
    }]
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
            // The folder the role launched in: its work area.
            let cwd = workareas::role_dir(dir, &repo, key)
                .to_string_lossy()
                .into_owned();

            // The first-use folder-trust dialog: paced keys, in the
            // role's first minute only — the window bounds the
            // exchange.
            if can_show_trust(actor) && age_ms < TRUST_WINDOW_MS {
                self.answer_trust(studio, key, term, &cwd, now_ms).await?;
            }

            // The work sample: one screen read a second per role,
            // classified into the `work` map for the summary — and
            // the D9 pass on the same text.
            let watched = crate::sample::Watched {
                dir,
                state,
                role: key,
                runner: &actor.runner,
                term,
            };
            self.sample_work(studio, &watched, now_ms).await?;

            if role.session_id.is_some() {
                continue;
            }
            // Discovery is throttled: at most one look per five
            // seconds per role, for as long as the role is young
            // enough — its 60 second attention never pauses the
            // looks.
            let pair = (studio.to_string(), key.clone());
            let last_look = self.discover_looked.borrow().get(&pair).copied();
            let found = if discovery_due(age_ms, last_look, now_ms) {
                self.discover_looked.borrow_mut().insert(pair, now_ms);
                // A cursor role's root keeps the agent-written
                // files under it unreachable: heal a drifted-open
                // mode here too — one informational attention per
                // drift, the path suffix only.
                if actor.runner == "cursor"
                    && let Ok(true) =
                        runners::cursor::ensure_private(&dir.join("roles").join(key).join("cursor"))
                    && !crate::act::attention_open(state, key, crate::reasons::CURSOR_DIR_OPEN)
                {
                    record(dir, runners::cursor::dir_open_event(key))?;
                }
                // Claimed ids come from every studio, not just this
                // one, plus anything found earlier in this pass.
                let taken = taken_ids(&self.cached, &self.found_sessions.borrow());
                let look = discover::Look {
                    home: &home,
                    role_dir: &dir.join("roles").join(key),
                };
                discover::find_session(
                    &actor.runner,
                    &term.argv,
                    &cwd,
                    role.since_ms,
                    &taken,
                    &look,
                    &*self.exec,
                )
            } else {
                None
            };
            // Past the default window but inside the runner's own:
            // an anomaly worth one log line, not yet an attention.
            if found.is_none()
                && age_ms >= SESSION_WAIT_MS
                && age_ms < session_wait_ms(&actor.runner)
                && self
                    .session_warned
                    .borrow_mut()
                    .insert((studio.to_string(), key.clone()))
            {
                eprintln!(
                    "stage: {key} still has no session id at {}s; '{}' waits {}s",
                    age_ms / 1000,
                    actor.runner,
                    session_wait_ms(&actor.runner) / 1000
                );
            }
            for kind in session_event(key, &actor.runner, found, state, role.since_ms, age_ms) {
                if let EventKind::RoleSession { session_id, .. } = &kind {
                    self.found_sessions.borrow_mut().insert(session_id.clone());
                    eprintln!("stage: {key} session is {session_id}");
                }
                record(dir, kind)?;
            }
        }
        // Samples belong to live roles — a role that stopped being
        // live drops its entry (other studios' entries stay).
        let keep = |s: &str, r: &str| s != studio || state.roles.get(r).is_some_and(|e| e.live);
        self.work.borrow_mut().retain(|(s, r), _| keep(s, r));
        self.work_looked.borrow_mut().retain(|(s, r), _| keep(s, r));
        self.screens.borrow_mut().retain(|(s, r), _| keep(s, r));
        self.blocked_since
            .borrow_mut()
            .retain(|(s, r), _| keep(s, r));
        self.session_warned.borrow_mut().retain(|(s, r)| keep(s, r));
        self.revision_since
            .borrow_mut()
            .retain(|(s, r), _| keep(s, r));
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
            Err(e) => return Err(Fail::call(e)),
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
            Err(e) => return Err(Fail::call(e)),
        }
        Ok(())
    }
}
