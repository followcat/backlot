//! The `permission prompt` episode: a `blocked` screen held for
//! two seconds raises the attention with the screen saved under
//! `attention/`; a recorded `role.approval` sends the runner's key
//! only while the prompt the decision was made on is still the one
//! on show.

use std::path::Path;

use backlot_model::event::{EventKind, PERMISSION_PROMPT};
use backlot_model::summary::Work;
use hc_proto::client::CallError;

use crate::act::record;
use crate::cli::Fail;
use crate::daemon::Stage;
use crate::permit;
use crate::runners;
use crate::sample::{Raise, Watched};
use crate::signals;

impl Stage {
    /// The D9 pass on one fresh sample: a `blocked` screen held for
    /// [`permit::EPISODE_MIN_MS`] raises `permission prompt` once per
    /// episode with the screen saved under `attention/`; a recorded
    /// `role.approval` sends the runner's key only while the prompt
    /// the decision was made on is still the one pending — for a
    /// signal-bound attention, that the last signal is still the
    /// bound `PermissionRequest`; otherwise that the saved screen's
    /// prompt block is still the one on show. A prompt that changed
    /// under the decision clears `prompt changed` and raises its own
    /// attention, and a gone prompt clears with no key sent.
    pub(crate) async fn permission(
        &self,
        studio: &str,
        w: &Watched<'_>,
        work: Work,
        text: &str,
        request: Option<&signals::Signal>,
        now_ms: u64,
    ) -> Result<(), Fail> {
        let (dir, state, role) = (w.dir, w.state, w.role);
        let pair = (studio.to_string(), role.to_string());
        // The area a cursor file picker's target is checked
        // against — the actor's own worktree (S12f).
        let area = crate::workareas::role_dir(
            dir,
            state
                .studio
                .as_ref()
                .map(|s| s.repo.as_str())
                .unwrap_or_default(),
            role,
        );
        // The trust dialog is the trust pass's concern, not a
        // permission episode — it resets the blocked clock.
        let ep = permit::in_episode(work, text);
        let (since, due) =
            permit::episode_due(ep, self.blocked_since.borrow().get(&pair).copied(), now_ms);
        match since {
            Some(since) => {
                self.blocked_since.borrow_mut().insert(pair.clone(), since);
                self.unblocked_since.borrow_mut().remove(&pair);
                if due
                    && !permit::episode_raised(state, role, since)
                    && !crate::act::attention_open(state, role, PERMISSION_PROMPT)
                {
                    self.raise_permission(dir, role, w.runner, text, request, &area)
                        .await?;
                }
            }
            None => {
                self.blocked_since.borrow_mut().remove(&pair);
                self.unblocked_since
                    .borrow_mut()
                    .entry(pair.clone())
                    .or_insert(now_ms);
            }
        }
        let Some(attn) = permit::open_episode(state, role) else {
            return Ok(());
        };
        let Some(appr) = permit::pending_approval(state, role, attn.ts_ms) else {
            // No decision recorded: a role out of its blocked episode
            // for the whole window answered the prompt somewhere else
            // — the stale attention clears so the next prompt raises
            // its own instead of inheriting this one's flag.
            if !ep {
                let gone = self
                    .unblocked_since
                    .borrow()
                    .get(&pair)
                    .copied()
                    .unwrap_or(now_ms);
                if now_ms.saturating_sub(gone) >= permit::EPISODE_MIN_MS {
                    record(
                        dir,
                        EventKind::AttentionCleared {
                            target: attn.seq,
                            reason: Some("prompt answered elsewhere".into()),
                        },
                    )?;
                }
            }
            return Ok(());
        };
        if !ep {
            // The prompt went away on its own — nothing to send.
            record(
                dir,
                EventKind::AttentionCleared {
                    target: attn.seq,
                    reason: Some("prompt gone before the decision was sent".into()),
                },
            )?;
            return Ok(());
        }
        let Some(key) = permit::approval_keys(w.runner, appr.decision) else {
            // Unmeasured runner: the open attention stays for the user.
            return Ok(());
        };
        // The key answers the prompt the attention saved — the
        // pending prompt must be that same one. An attention raised
        // from a `PermissionRequest` signal carries the request in
        // `<seq>.signal` (S9f): the compare is that the pending
        // request is still that request — the same `ts_ms` and
        // `tool_input` — `Notification` chimes skipped — *and* that
        // the screen still reads Blocked. Claude Code emits no hook
        // between the answer and the tool's end, so a prompt
        // answered at the role's terminal leaves its request the
        // pending one and the screen is the only proof it went:
        // `prompt gone` then, nothing sent and nothing raised.
        // Anything else keeps the S9c screen compare — the saved
        // screen's prompt block must still be the one on show. A
        // prompt that changed under the recorded decision (an
        // earlier prompt answered in the terminal, the next one up)
        // is a different prompt: nothing goes out, the stale
        // attention clears, and this screen raises its own
        // attention — `needs_user` computed on it, the director
        // notified as for any new prompt. A screen with no
        // comparable block refuses the same way.
        enum Fresh {
            Send,
            Changed,
            Gone,
        }
        let screen_blocked = runners::work_state(w.runner, text) == Work::Blocked;
        let fresh = if let Ok(bound) =
            std::fs::read_to_string(dir.join("attention").join(format!("{}.signal", attn.seq)))
        {
            let was: Option<signals::Signal> = serde_json::from_str(&bound).ok();
            match (was, request) {
                (Some(w), Some(n)) if w.ts_ms == n.ts_ms && w.tool_input == n.tool_input => {
                    if screen_blocked {
                        Fresh::Send
                    } else {
                        Fresh::Gone
                    }
                }
                (Some(_), Some(_)) => Fresh::Changed,
                _ => {
                    if screen_blocked {
                        Fresh::Changed
                    } else {
                        Fresh::Gone
                    }
                }
            }
        } else {
            let saved =
                std::fs::read_to_string(dir.join("attention").join(format!("{}.txt", attn.seq)))
                    .unwrap_or_default();
            match (
                permit::prompt_block(w.runner, &saved),
                permit::prompt_block(w.runner, text),
            ) {
                (Some(was), Some(now)) if was == now => Fresh::Send,
                // Neither screen carries an identifiable block —
                // the episode came from structured state and the
                // prompt's kind is unread: no key can be bound, so
                // nothing is sent (S12d).
                (None, None) => Fresh::Gone,
                _ => Fresh::Changed,
            }
        };
        match fresh {
            Fresh::Send => {}
            Fresh::Changed => {
                record(
                    dir,
                    EventKind::AttentionCleared {
                        target: attn.seq,
                        reason: Some("prompt changed before the decision was sent".into()),
                    },
                )?;
                self.raise_permission(dir, role, w.runner, text, request, &area)
                    .await?;
                return Ok(());
            }
            Fresh::Gone => {
                record(
                    dir,
                    EventKind::AttentionCleared {
                        target: attn.seq,
                        reason: Some("prompt gone before the decision was sent".into()),
                    },
                )?;
                return Ok(());
            }
        }
        let sent = self
            .client
            .call(
                "terminal.keys",
                Some(serde_json::json!({
                    "terminal": w.term.address,
                    "keys": [key],
                })),
            )
            .await;
        match sent {
            Ok(_) => {
                eprintln!("stage: sent {key} for {}'s permission prompt", role);
                record(
                    dir,
                    EventKind::AttentionCleared {
                        target: attn.seq,
                        reason: Some(format!("sent {key}")),
                    },
                )?;
                // The episode ends with the answer: whatever the
                // screen shows next — the next prompt already up,
                // or this one still unanswered — starts its own
                // count instead of riding on the cleared episode's.
                self.blocked_since.borrow_mut().remove(&pair);
            }
            // A terminal that raced away keeps the attention; the
            // role's loss is reconciled elsewhere.
            Err(CallError::Rpc(_)) => {}
            Err(e) => return Err(Fail::call(e)),
        }
        Ok(())
    }

    /// `attention.raised` for a blocked episode: the screen lands at
    /// `attention/<seq>.txt` via the append's own file copy. When the
    /// tail's pending request is a `PermissionRequest` (S9f), the
    /// detail names the request's tool and trimmed command or path
    /// instead of only pointing at the screen, and the request itself
    /// rides with the raise as `attention/<seq>.signal` — the
    /// approve-time freshness compare runs on it. A prompt reaching
    /// for `BACKLOT_*` or an environment-clearing flag is flagged
    /// `needs_user` — the director never sees the notice and may not
    /// answer it; the flag reads the signal's command when there is
    /// one, else the prompt's own block. So is a cursor file picker
    /// whose target does not resolve strictly inside the role's
    /// work area — or whose ask the stage cannot read (S12f) — and
    /// a prompt no recorded decision can bind to: no
    /// `PermissionRequest` signal, no identifiable block; a
    /// signal-bound raise needs no block, the decision compares on
    /// the request. An actor's ordinary prompt gets the director's
    /// detail; anything else says it needs the user.
    pub(crate) async fn raise_permission(
        &self,
        dir: &Path,
        role: &str,
        runner: &str,
        text: &str,
        request: Option<&signals::Signal>,
        area: &Path,
    ) -> Result<(), Fail> {
        let command = request.and_then(|s| s.tool_target());
        // A prompt kind nothing can bind a recorded decision to —
        // a blocked raised from structured state alone, no
        // `PermissionRequest` signal and no identifiable block —
        // is the user's (S12d). A signal-bound raise needs no
        // block: the decision compares on the request.
        let user_owned = permit::needs_user(role, runner, text, command)
            || permit::cursor_needs_user(runner, text, area)
            || (request.is_none() && permit::prompt_block(runner, text).is_none());
        // The bound request: `backlot approve` refuses a key when
        // the last signal is no longer this one.
        let bound = request.and_then(|s| serde_json::to_string(s).ok());
        let tool = match (request.and_then(|s| s.tool_name.as_deref()), command) {
            (Some(name), Some(target)) => format!("{name}: {target}; "),
            (Some(name), None) => format!("{name}; "),
            _ => String::new(),
        };
        self.raise_attention(
            Raise {
                dir,
                role,
                reason: PERMISSION_PROMPT,
                text,
                needs_user: user_owned,
                bound,
            },
            move |seq| {
                if user_owned {
                    format!("{tool}needs the user; screen saved to attention/{seq}.txt")
                } else {
                    format!("{tool}screen saved to attention/{seq}.txt")
                }
            },
        )
        .await?;
        eprintln!("stage: {role} at a permission prompt");
        Ok(())
    }
}
