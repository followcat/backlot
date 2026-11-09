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
        let work = runners::work_state(w.runner, &text);
        self.work.borrow_mut().insert(pair.clone(), work);
        // The send gate's screen-stability track — same screen, same
        // `same_since`; a new screen resets the window.
        let prev = self.screens.borrow().get(&pair).cloned();
        self.screens.borrow_mut().insert(
            pair.clone(),
            crate::deliver::Screen::note(prev.as_ref(), text.clone(), now_ms),
        );
        self.permission(studio, w, work, &text, now_ms).await?;
        self.stall(studio, w, work).await
    }

    /// The S6 stall pass on the sample just stored: a screen held
    /// unchanged for the studio's `stall_limit_s` raises `stalled`
    /// once — one open attention per role and reason — with the
    /// screen saved under `attention/`; a screen that changed since
    /// the raise clears the attention. A stage restart loses the
    /// in-memory track, so the first sample after it reads as a
    /// fresh screen: an open stall clears and can re-raise one
    /// limit later — acceptable until S6b's recovery work.
    async fn stall(&self, studio: &str, w: &Watched<'_>, work: Work) -> Result<(), Fail> {
        let (dir, state, role) = (w.dir, w.state, w.role);
        let pair = (studio.to_string(), role.to_string());
        let Some(screen) = self.screens.borrow().get(&pair).cloned() else {
            return Ok(());
        };
        if let Some(attn) = stall::open_stall(state, role) {
            // The screen moved since the raise: the episode is over.
            if screen.same_since_ms > attn.ts_ms {
                record(
                    dir,
                    EventKind::AttentionCleared {
                        target: attn.seq,
                        reason: Some("screen changed".into()),
                    },
                )?;
            }
            return Ok(());
        }
        let task = actor_task(role)
            .and_then(|id| state.tasks.get(id))
            .map(|t| t.status);
        let limit_ms = u64::from(state.stall_limit_s()) * 1000;
        if stall::stalled(role, task, work, &screen, limit_ms) {
            let held_s = screen.at_ms.saturating_sub(screen.same_since_ms) / 1000;
            self.raise_attention(dir, role, STALLED, &screen.text, move |seq| {
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
    /// `role.approval` sends the runner's key while the prompt still
    /// shows and clears — a gone prompt clears with no key sent.
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
                if due
                    && !permit::episode_raised(state, role, since)
                    && !crate::act::attention_open(state, role, PERMISSION_PROMPT)
                {
                    self.raise_permission(dir, role, text).await?;
                }
            }
            None => {
                self.blocked_since.borrow_mut().remove(&pair);
            }
        }
        let Some(attn) = permit::open_episode(state, role) else {
            return Ok(());
        };
        let Some(appr) = permit::pending_approval(state, role, attn.ts_ms) else {
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
    /// `attention/<seq>.txt` via the append's own file copy. An
    /// actor's detail points the director at the file; any other
    /// role's says it needs the user.
    async fn raise_permission(&self, dir: &Path, role: &str, text: &str) -> Result<(), Fail> {
        let actor = permit::is_actor(role);
        self.raise_attention(dir, role, PERMISSION_PROMPT, text, move |seq| {
            if actor {
                format!("screen saved to attention/{seq}.txt")
            } else {
                format!("needs the user; screen saved to attention/{seq}.txt")
            }
        })
        .await?;
        eprintln!("stage: {role} at a permission prompt");
        Ok(())
    }

    /// `attention.raised` with the screen saved to
    /// `attention/<seq>.txt` via the append's own file copy — the
    /// seq is only known inside the lock, so `detail` builds from it.
    async fn raise_attention(
        &self,
        dir: &Path,
        role: &str,
        reason: &'static str,
        text: &str,
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
                    },
                ),
                files: vec![crate::store::PendingFile {
                    src: pending_scratch,
                    dest: format!("attention/{seq}.txt"),
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
