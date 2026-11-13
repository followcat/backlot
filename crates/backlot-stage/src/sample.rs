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
use crate::watch::{structured_state, work_due};

/// What a live role's samples need: its ledger view, its identity
/// and its terminal — bundled so the pass stays readable.
pub(crate) struct Watched<'a> {
    pub dir: &'a Path,
    pub state: &'a StudioState,
    pub role: &'a str,
    pub runner: &'a str,
    pub term: &'a LiveTerminal,
}

/// What one `attention.raised` carries: the role and reason, the
/// screen it saves, the S9b flag, and a `PermissionRequest`
/// signal's JSON binding it (S9f) when the raise came from one.
pub(crate) struct Raise<'a> {
    pub dir: &'a Path,
    pub role: &'a str,
    pub reason: &'static str,
    pub text: &'a str,
    pub needs_user: bool,
    pub bound: Option<String>,
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
        // Signals first (S9f): the last state-mapping line the
        // role's own session sent is its state; the screen answers
        // when the session sent nothing — every other runner, and
        // any session with no signal yet. The pending request is
        // the same tail's first non-Notification line — a
        // `permission_prompt` notification is the same prompt's
        // six-second chime, never a new prompt — minus the
        // requests the ledger says this stage already answered:
        // their prompt is gone but no hook line comes until the
        // tool's own, so they read Working meanwhile.
        let session = w
            .state
            .roles
            .get(w.role)
            .and_then(|r| r.session_id.as_deref());
        let (signal, request, signal_state) = session
            .map(|sid| {
                let tail = signals::tail(&signals::signals_path(w.dir, w.role));
                let answered = signals::answered_requests(w.dir, w.state, w.role);
                (
                    signals::last_work(&tail, sid),
                    signals::pending_request(&tail, sid, &answered),
                    signals::state(&tail, sid, &answered),
                )
            })
            .unwrap_or((None, None, None));
        match &signal {
            Some(s) => self
                .last_signal
                .borrow_mut()
                .insert(pair.clone(), s.clone()),
            None => self.last_signal.borrow_mut().remove(&pair),
        };
        // Structured state sits between the signal and the screen
        // (S12d): a core that reports — natively or through screen
        // inference — is believed ahead of the stage's own read;
        // a `blocked` from any source still wins the map.
        let work = signals::combine(
            signal_state,
            structured_state(w.term),
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
        self.permission(studio, w, work, &text, request.as_ref(), now_ms)
            .await?;
        self.stall(studio, w, work, now_ms).await?;
        self.rate_limit(studio, w, &text, now_ms).await
    }

    /// The S12d rate-limit pass on one fresh sample: a limited
    /// role gets one stage-owned attention while the stage waits
    /// the parsed reset out and sends `continue`; three retries
    /// and the episode escalates `needs_user`. A `continue` that
    /// worked — the limit lifted — resolves `resumed`; a cleared
    /// or escalated episode never re-raises while the limit
    /// holds. Counted by its own reason, never as stall or rework.
    async fn rate_limit(
        &self,
        studio: &str,
        w: &Watched<'_>,
        text: &str,
        now_ms: u64,
    ) -> Result<(), Fail> {
        use crate::ratelimit as rl;
        let (dir, state, role) = (w.dir, w.state, w.role);
        let pair = (studio.to_string(), role.to_string());
        let limited = rl::limited(w.term, text);
        let open = state
            .open_attention()
            .find(|a| a.role == role && a.reason == rl::RATE_LIMITED);
        match (limited, open) {
            (None, Some(attn)) => {
                record(
                    dir,
                    EventKind::AttentionCleared {
                        target: attn.seq,
                        reason: Some("resumed".into()),
                    },
                )?;
                self.rate_pace.borrow_mut().remove(&pair);
            }
            (None, None) => {
                self.rate_pace.borrow_mut().remove(&pair);
            }
            (Some(wait), None) => {
                if self.rate_pace.borrow().contains_key(&pair) {
                    return Ok(());
                }
                self.raise_attention(
                    Raise {
                        dir,
                        role,
                        reason: rl::RATE_LIMITED,
                        text,
                        needs_user: false,
                        bound: None,
                    },
                    move |seq| {
                        format!("rate limited; auto-retry after the reset; screen saved to attention/{seq}.txt")
                    },
                )
                .await?;
                self.rate_pace.borrow_mut().insert(
                    pair,
                    rl::RatePace {
                        retries: 0,
                        due_ms: now_ms + wait + rl::RETRY_MARGIN_MS,
                    },
                );
                eprintln!("stage: {role} rate limited");
            }
            (Some(wait), Some(attn)) => {
                if attn.needs_user {
                    return Ok(());
                }
                let pace = self
                    .rate_pace
                    .borrow()
                    .get(&pair)
                    .copied()
                    .unwrap_or(rl::RatePace {
                        retries: 0,
                        due_ms: now_ms,
                    });
                match rl::step(pace, pace.retries < rl::MAX_RETRIES, now_ms) {
                    rl::Step::Wait => {}
                    rl::Step::Send => {
                        let sent = self
                            .client
                            .call(
                                "terminal.send",
                                Some(serde_json::json!({
                                    "terminal": w.term.address,
                                    "text": rl::CONTINUE,
                                })),
                            )
                            .await;
                        match sent {
                            Ok(_) => {
                                eprintln!(
                                    "stage: sent '{:?}' to {role} after its rate-limit reset",
                                    rl::CONTINUE
                                );
                                self.rate_pace.borrow_mut().insert(
                                    pair,
                                    rl::RatePace {
                                        retries: pace.retries + 1,
                                        due_ms: now_ms + wait + rl::RETRY_MARGIN_MS,
                                    },
                                );
                            }
                            Err(CallError::Rpc(_)) => {}
                            Err(e) => return Err(Fail::call(e)),
                        }
                    }
                    rl::Step::Escalate => {
                        record(
                            dir,
                            EventKind::AttentionCleared {
                                target: attn.seq,
                                reason: Some(format!(
                                    "auto-retries exhausted after {}",
                                    rl::MAX_RETRIES
                                )),
                            },
                        )?;
                        self.raise_attention(
                            Raise {
                                dir,
                                role,
                                reason: rl::RATE_LIMITED,
                                text,
                                needs_user: true,
                                bound: None,
                            },
                            move |seq| {
                                format!(
                                    "rate limited after {} auto-retries; needs the user — screen saved to attention/{seq}.txt",
                                    rl::MAX_RETRIES
                                )
                            },
                        )
                        .await?;
                        eprintln!(
                            "stage: {role} rate limit escalated after {} auto-retries",
                            rl::MAX_RETRIES
                        );
                    }
                }
            }
        }
        Ok(())
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
        // The structured stall's clock (S12d): while the terminal
        // reports `working`, its activity revision must keep
        // moving — `rev_since` is when the current revision first
        // showed. A terminal not reporting `working` drops its
        // entry and never stalls this way.
        let working = w.term.status.as_ref().is_some_and(|s| s.state == "working");
        let rev_since = match w.term.activity_revision.filter(|_| working) {
            None => {
                self.revision_since.borrow_mut().remove(&pair);
                None
            }
            Some(rev) => {
                let mut map = self.revision_since.borrow_mut();
                let e = map.entry(pair.clone()).or_insert((rev, now_ms));
                if e.0 != rev {
                    *e = (rev, now_ms);
                }
                Some(e.1)
            }
        };
        let Some(screen) = self.screens.borrow().get(&pair).cloned() else {
            return Ok(());
        };
        let signal = self.last_signal.borrow().get(&pair).cloned();
        if let Some(attn) = stall::open_stall(state, role) {
            // A newer signal — the turn demonstrably alive — an
            // activity revision that moved since the raise, or the
            // screen having moved ends the episode.
            let cleared = if signal.as_ref().is_some_and(|s| s.ts_ms > attn.ts_ms) {
                Some("new hook signal")
            } else if rev_since.is_some_and(|s| s > attn.ts_ms) {
                Some("activity resumed")
            } else if screen.same_since_ms > attn.ts_ms {
                Some("screen changed")
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
            self.raise_attention(
                Raise {
                    dir,
                    role,
                    reason: STALLED,
                    text: &screen.text,
                    needs_user: false,
                    bound: None,
                },
                move |seq| {
                    format!("no hook signal for {silent_s}s; screen saved to attention/{seq}.txt")
                },
            )
            .await?;
            eprintln!("stage: {role} stalled");
        } else if rev_since.is_some_and(|s| now_ms.saturating_sub(s) >= limit_ms) {
            // The terminal still says `working` but its activity
            // revision has not moved for the limit — the
            // structured twin of the frozen-screen stall.
            let held_s = now_ms.saturating_sub(rev_since.unwrap_or(now_ms)) / 1000;
            self.raise_attention(
                Raise {
                    dir,
                    role,
                    reason: STALLED,
                    text: &screen.text,
                    needs_user: false,
                    bound: None,
                },
                move |seq| {
                    format!(
                        "status 'working' but activity revision unchanged for {held_s}s; screen saved to attention/{seq}.txt"
                    )
                },
            )
            .await?;
            eprintln!("stage: {role} stalled");
        } else if stall::stalled(role, task, work, &screen, limit_ms) {
            let held_s = screen.at_ms.saturating_sub(screen.same_since_ms) / 1000;
            self.raise_attention(
                Raise {
                    dir,
                    role,
                    reason: STALLED,
                    text: &screen.text,
                    needs_user: false,
                    bound: None,
                },
                move |seq| {
                    format!("screen unchanged for {held_s}s; screen saved to attention/{seq}.txt")
                },
            )
            .await?;
            eprintln!("stage: {role} stalled");
        }
        Ok(())
    }

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
    async fn permission(
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
                    self.raise_permission(dir, role, w.runner, text, request)
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
                self.raise_permission(dir, role, w.runner, text, request)
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
    /// one, else the prompt's own block. An actor's ordinary prompt
    /// gets the director's detail; anything else says it needs the
    /// user.
    async fn raise_permission(
        &self,
        dir: &Path,
        role: &str,
        runner: &str,
        text: &str,
        request: Option<&signals::Signal>,
    ) -> Result<(), Fail> {
        let command = request.and_then(|s| s.tool_target());
        // A prompt kind the markers cannot identify — a blocked
        // raised from structured state alone — is the user's: no
        // recorded decision can be bound to a prompt the stage
        // cannot read (S12d).
        let user_owned = permit::needs_user(role, runner, text, command)
            || permit::prompt_block(runner, text).is_none();
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

    /// `attention.raised` with the screen saved to
    /// `attention/<seq>.txt` via the append's own file copy — the
    /// seq is only known inside the lock, so `detail` builds from
    /// it. `raise.bound` is a PermissionRequest signal's JSON,
    /// saved to `attention/<seq>.signal` for the approve-time
    /// compare (S9f); `raise.needs_user` marks a `permission
    /// prompt` the director may not answer (S9b).
    async fn raise_attention(
        &self,
        raise: Raise<'_>,
        detail: impl Fn(u64) -> String + 'static,
    ) -> Result<(), Fail> {
        let (dir, role) = (raise.dir, raise.role);
        let scratch = dir.join("attention").join(format!(".new-{role}"));
        std::fs::create_dir_all(dir.join("attention"))
            .map_err(|e| Fail::refused(format!("attention file: {e}")))?;
        std::fs::write(&scratch, raise.text)
            .map_err(|e| Fail::refused(format!("attention file: {e}")))?;
        let bound_scratch = raise
            .bound
            .map(|b| {
                let path = dir.join("attention").join(format!(".new-{role}.signal"));
                std::fs::write(&path, b).map(|_| path)
            })
            .transpose()
            .map_err(|e| Fail::refused(format!("attention file: {e}")))?;
        let role_key = role.to_string();
        let pending_scratch = scratch.clone();
        let pending_bound = bound_scratch.clone();
        let needs_user = raise.needs_user;
        let reason = raise.reason;
        let result = store::append(dir, move |_, seq| {
            let mut files = vec![crate::store::PendingFile {
                src: pending_scratch,
                dest: format!("attention/{seq}.txt"),
                append: None,
            }];
            if let Some(src) = pending_bound.clone() {
                files.push(crate::store::PendingFile {
                    src,
                    dest: format!("attention/{seq}.signal"),
                    append: None,
                });
            }
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
                files,
            })
        });
        let _ = std::fs::remove_file(&scratch);
        if let Some(p) = &bound_scratch {
            let _ = std::fs::remove_file(p);
        }
        result.map_err(|e| match e {
            StoreError::Io(e) => Fail::refused(format!("append: {e}")),
            StoreError::Corrupt(c) => Fail::refused(format!("{c}")),
            StoreError::Refused(r) => Fail::refused(format!("{r}")),
            StoreError::Build(_) => unreachable!("the build closure cannot fail"),
        })?;
        Ok(())
    }
}
