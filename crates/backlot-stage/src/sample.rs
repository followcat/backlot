//! The per-role screen sample: classify the visible screen into the
//! `work` map for the summary and the send gate's stability track,
//! then run the D9 and S6 passes on the same text — a `blocked`
//! screen held for an episode raises `permission prompt`, a
//! recorded `role.approval` answers it only while the prompt still
//! shows, and a screen unchanged for the stall limit raises
//! `stalled`.

use std::path::Path;

use backlot_model::event::{EventKind, PERMISSION_PROMPT, STALLED};
use backlot_model::state::{StudioState, actor_task};
use backlot_model::summary::Work;
use hc_proto::client::CallError;
use hc_proto::terminal::ReadParams;

use crate::act::record;
use crate::cli::Fail;
use crate::daemon::Stage;
use crate::permit;
use crate::reconcile::LiveTerminal;
use crate::runners;
use crate::signals;
use crate::stall;
use crate::store::{self, StoreError};
use crate::watch::work_due;

/// What a live role's samples need: its ledger view, its identity
/// and its terminal — bundled so the pass stays readable.
pub(crate) struct Watched<'a> {
    pub dir: &'a Path,
    pub state: &'a StudioState,
    pub role: &'a str,
    pub runner: &'a str,
    pub term: &'a LiveTerminal,
}

impl Stage {
    /// Read one live role's visible screen into the `work` map, at
    /// most once a second per role. A refused read — the terminal
    /// raced away — keeps the last value. The same text feeds the
    /// D9 episode and approval pass.
    pub(crate) async fn sample_work(
        &self,
        studio: &str,
        w: &Watched<'_>,
        now_ms: u64,
    ) -> Result<(), Fail> {
        let pair = (studio.to_string(), w.role.to_string());
        if !work_due(self.work_looked.borrow().get(&pair).copied(), now_ms) {
            return Ok(());
        }
        self.work_looked.borrow_mut().insert(pair.clone(), now_ms);
        let text = match self
            .client
            .terminal_read(ReadParams {
                terminal: w.term.address.clone(),
                lines: 0,
                caller: None,
            })
            .await
        {
            Ok(r) => r.text,
            Err(CallError::Rpc(_)) => return Ok(()),
            Err(e) => return Err(Fail::call(e)),
        };
        // Signals first (S9f): the last line the role's own session
        // sent is its state; the screen answers when the session
        // sent nothing — every other runner, and any session with
        // no signal yet.
        let session = w
            .state
            .roles
            .get(w.role)
            .and_then(|r| r.session_id.as_deref());
        let signal = session.and_then(|sid| {
            signals::last(&signals::tail(&signals::signals_path(w.dir, w.role)), sid)
        });
        match &signal {
            Some(s) => self
                .last_signal
                .borrow_mut()
                .insert(pair.clone(), s.clone()),
            None => self.last_signal.borrow_mut().remove(&pair),
        };
        let work = signals::combine(
            signal.as_ref().and_then(|s| s.work()),
            runners::work_state(w.runner, &text),
        );
        self.work.borrow_mut().insert(pair.clone(), work);
        // The send gate's screen-stability track — same screen, same
        // `same_since`; a new screen resets the window.
        let prev = self.screens.borrow().get(&pair).cloned();
        self.screens.borrow_mut().insert(
            pair.clone(),
            crate::deliver::Screen::note(prev.as_ref(), text.clone(), now_ms),
        );
        self.permission(studio, w, work, &text, now_ms).await?;
        self.stall(studio, w, work, now_ms).await
    }

    /// The S6 stall pass on the sample just stored: a screen held
    /// unchanged for the studio's `stall_limit_s` raises `stalled`
    /// once — one open attention per role and reason — with the
    /// screen saved under `attention/`; a screen that changed since
    /// the raise clears the attention. A stage restart loses the
    /// in-memory track, so the first sample after it reads as a
    /// fresh screen: an open stall clears and can re-raise one
    /// limit later — acceptable until S6b's recovery work.
    ///
    /// S9f adds the signal half: a role whose last hook signal
    /// reports Working and is older than the limit is as stalled as
    /// a frozen screen — the spinner may keep moving while the turn
    /// itself is dead. Either source can clear an open stall: a new
    /// screen, or a newer signal.
    async fn stall(
        &self,
        studio: &str,
        w: &Watched<'_>,
        work: Work,
        now_ms: u64,
    ) -> Result<(), Fail> {
        let (dir, state, role) = (w.dir, w.state, w.role);
        let pair = (studio.to_string(), role.to_string());
        let Some(screen) = self.screens.borrow().get(&pair).cloned() else {
            return Ok(());
        };
        let signal = self.last_signal.borrow().get(&pair).cloned();
        if let Some(attn) = stall::open_stall(state, role) {
            // The screen moved or a newer signal landed since the
            // raise: the episode is over.
            let cleared = if screen.same_since_ms > attn.ts_ms {
                Some("screen changed")
            } else if signal.as_ref().is_some_and(|s| s.ts_ms > attn.ts_ms) {
                Some("new hook signal")
            } else {
                None
            };
            if let Some(reason) = cleared {
                record(
                    dir,
                    EventKind::AttentionCleared {
                        target: attn.seq,
                        reason: Some(reason.into()),
                    },
                )?;
            }
            return Ok(());
        }
        let task = actor_task(role)
            .and_then(|id| state.tasks.get(id))
            .map(|t| t.status);
        let limit_ms = u64::from(state.stall_limit_s()) * 1000;
        if let Some(sig) =
            signal.filter(|s| stall::signal_stalled(role, task, work, s, limit_ms, now_ms))
        {
            let silent_s = now_ms.saturating_sub(sig.ts_ms) / 1000;
            self.raise_attention(dir, role, STALLED, &screen.text, false, move |seq| {
                format!("no hook signal for {silent_s}s; screen saved to attention/{seq}.txt")
            })
            .await?;
            eprintln!("stage: {role} stalled");
        } else if stall::stalled(role, task, work, &screen, limit_ms) {
            let held_s = screen.at_ms.saturating_sub(screen.same_since_ms) / 1000;
            self.raise_attention(dir, role, STALLED, &screen.text, false, move |seq| {
                format!("screen unchanged for {held_s}s; screen saved to attention/{seq}.txt")
            })
            .await?;
            eprintln!("stage: {role} stalled");
        }
        Ok(())
    }

    /// The D9 pass on one fresh sample: a `blocked` screen held for
    /// [`permit::EPISODE_MIN_MS`] raises `permission prompt` once per
    /// episode with the screen saved under `attention/`; a recorded
    /// `role.approval` sends the runner's key only while the saved
    /// screen's prompt block is still the one on show — a prompt
    /// that changed under the decision clears `prompt changed` and
    /// raises its own attention, and a gone prompt clears with no
    /// key sent.
    async fn permission(
        &self,
        studio: &str,
        w: &Watched<'_>,
        work: Work,
        text: &str,
        now_ms: u64,
    ) -> Result<(), Fail> {
        let (dir, state, role) = (w.dir, w.state, w.role);
        let pair = (studio.to_string(), role.to_string());
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
                    self.raise_permission(dir, role, w.runner, text).await?;
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
        // screen on show must be that same prompt. A prompt that
        // changed under the recorded decision (an earlier prompt
        // answered in the terminal, the next one up) is a different
        // prompt: nothing goes out, the stale attention clears, and
        // this screen raises its own attention — `needs_user`
        // computed on it, the director notified as for any new
        // prompt. A screen with no comparable block refuses the
        // same way.
        let saved =
            std::fs::read_to_string(dir.join("attention").join(format!("{}.txt", attn.seq)))
                .unwrap_or_default();
        let same_prompt = matches!(
            (
                permit::prompt_block(w.runner, &saved),
                permit::prompt_block(w.runner, text),
            ),
            (Some(was), Some(now)) if was == now
        );
        if !same_prompt {
            record(
                dir,
                EventKind::AttentionCleared {
                    target: attn.seq,
                    reason: Some("prompt changed before the decision was sent".into()),
                },
            )?;
            self.raise_permission(dir, role, w.runner, text).await?;
            return Ok(());
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
    /// `attention/<seq>.txt` via the append's own file copy. A
    /// prompt reaching for `BACKLOT_*` or an environment-clearing
    /// flag is flagged `needs_user` — the director never sees the
    /// notice and may not answer it; the flag reads the prompt's own
    /// block, so an earlier prompt's line still scrolling above does
    /// not flag this one. An actor's ordinary prompt gets the
    /// director's detail; anything else says it needs the user.
    async fn raise_permission(
        &self,
        dir: &Path,
        role: &str,
        runner: &str,
        text: &str,
    ) -> Result<(), Fail> {
        let user_owned = permit::needs_user(role, runner, text);
        self.raise_attention(dir, role, PERMISSION_PROMPT, text, user_owned, move |seq| {
            if user_owned {
                format!("needs the user; screen saved to attention/{seq}.txt")
            } else {
                format!("screen saved to attention/{seq}.txt")
            }
        })
        .await?;
        eprintln!("stage: {role} at a permission prompt");
        Ok(())
    }

    /// `attention.raised` with the screen saved to
    /// `attention/<seq>.txt` via the append's own file copy — the
    /// seq is only known inside the lock, so `detail` builds from
    /// it. `needs_user` marks a `permission prompt` the director may
    /// not answer (S9b); stalls pass `false`.
    async fn raise_attention(
        &self,
        dir: &Path,
        role: &str,
        reason: &'static str,
        text: &str,
        needs_user: bool,
        detail: impl Fn(u64) -> String + 'static,
    ) -> Result<(), Fail> {
        let scratch = dir.join("attention").join(format!(".new-{role}"));
        std::fs::create_dir_all(dir.join("attention"))
            .map_err(|e| Fail::refused(format!("attention file: {e}")))?;
        std::fs::write(&scratch, text)
            .map_err(|e| Fail::refused(format!("attention file: {e}")))?;
        let role_key = role.to_string();
        let pending_scratch = scratch.clone();
        let result = store::append(dir, move |_, seq| {
            Ok::<_, std::convert::Infallible>(crate::store::Pending {
                event: backlot_model::NewEvent::new(
                    backlot_model::EventBy::role("stage"),
                    EventKind::AttentionRaised {
                        role: role_key.clone(),
                        reason: reason.into(),
                        detail: detail(seq),
                        needs_user,
                    },
                ),
                files: vec![crate::store::PendingFile {
                    src: pending_scratch,
                    dest: format!("attention/{seq}.txt"),
                    append: None,
                }],
            })
        });
        let _ = std::fs::remove_file(&scratch);
        result.map_err(|e| match e {
            StoreError::Io(e) => Fail::refused(format!("append: {e}")),
            StoreError::Corrupt(c) => Fail::refused(format!("{c}")),
            StoreError::Refused(r) => Fail::refused(format!("{r}")),
            StoreError::Build(_) => unreachable!("the build closure cannot fail"),
        })?;
        Ok(())
    }
}
