//! The per-role screen sample: classify the visible screen into the
//! `work` map for the summary and the send gate's stability track,
//! then hand the same text to the attention-episode passes — the
//! `permission prompt` episode in `permission.rs`, the `stalled`
//! and `rate limited` episodes in `limits.rs`.

use std::path::Path;

use backlot_model::event::EventKind;
use backlot_model::state::StudioState;
use hc_proto::client::CallError;
use hc_proto::terminal::ReadParams;

use crate::cli::Fail;
use crate::daemon::Stage;
use crate::reconcile::LiveTerminal;
use crate::runners;
use crate::signals;
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
        let screen_work = runners::work_state(w.runner, &text);
        let work = signals::combine(signal_state, structured_state(w.term), screen_work);
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
        self.rate_limit(studio, w, &text, screen_work, now_ms).await
    }

    /// `attention.raised` with the screen saved to
    /// `attention/<seq>.txt` via the append's own file copy — the
    /// seq is only known inside the lock, so `detail` builds from
    /// it. `raise.bound` is a PermissionRequest signal's JSON,
    /// saved to `attention/<seq>.signal` for the approve-time
    /// compare (S9f); `raise.needs_user` marks a `permission
    /// prompt` the director may not answer (S9b).
    pub(crate) async fn raise_attention(
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
