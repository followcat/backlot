//! The in-client installer's lifecycle: start, streamed output, the
//! `--version` check, cancel with the process-group grace, and retry.
//! Every run has a generation; an event from a cancelled or superseded
//! run is dropped, so a late line can never change what the card shows.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::update_flow::UpdateEvent;
use super::{App, BoxFut};
use crate::chrome::update_modal::Phase;
use crate::chrome::update_status::{SPINNER, Step};
use crate::update_run::{self, Plan};

/// Output lines kept for the card's tail.
const TAIL_LINES: usize = 16;
/// How long a cancelled group may take before SIGKILL (spec 5.2).
const GRACE: Duration = Duration::from_secs(2);
/// The spinner frame time (spec 5.3 mockup).
const FRAME: Duration = Duration::from_millis(180);

pub(crate) type SpawnFn = Arc<
    dyn Fn(&Plan, tokio::sync::mpsc::Sender<UpdateEvent>, u64) -> Result<i32, String> + Send + Sync,
>;
pub(crate) type SignalFn = Arc<dyn Fn(i32, i32) + Send + Sync>;
pub(crate) type AliveFn = Arc<dyn Fn(i32) -> bool + Send + Sync>;
pub(crate) type ProbeFn = Arc<dyn Fn(PathBuf) -> BoxFut<Option<String>> + Send + Sync>;

/// The installer's side effects, injectable so no test runs the real
/// installer or the network.
#[derive(Clone)]
pub(crate) struct Seams {
    pub spawn: SpawnFn,
    pub signal: SignalFn,
    pub alive: AliveFn,
    pub probe: ProbeFn,
}

pub(crate) fn default_seams() -> Seams {
    Seams {
        spawn: Arc::new(|plan, events, generation| {
            update_run::spawn_group(&plan.pipeline, events, generation).map_err(|e| e.to_string())
        }),
        signal: Arc::new(update_run::signal_group),
        alive: Arc::new(update_run::group_alive),
        probe: Arc::new(|bin| Box::pin(update_run::probe_version(bin))),
    }
}

/// One installer run from start to Done, Failed or cancel.
pub(crate) struct Install {
    pub generation: u64,
    pub plan: Plan,
    pub started: Instant,
    pub finished: Option<Instant>,
    pub step: Step,
    pub pgid: Option<i32>,
    pub tail: Vec<String>,
    pub reason: Option<String>,
    pub verified: Option<String>,
}

impl Install {
    /// Whole seconds since the run started, frozen once it finished.
    pub(crate) fn elapsed_secs(&self) -> u64 {
        self.finished
            .unwrap_or_else(Instant::now)
            .duration_since(self.started)
            .as_secs()
    }

    /// The spinner frame for the checklist row.
    pub(crate) fn spinner(&self) -> usize {
        (self.started.elapsed().as_millis() / FRAME.as_millis()) as usize % SPINNER.len()
    }

    fn running(&self) -> bool {
        matches!(self.step, Step::Running | Step::Verifying)
    }
}

impl App {
    /// `↵ update` or `↵ retry`: re-checks the install directory, then
    /// starts the pipeline in its own process group.
    pub(super) fn start_install(&mut self) {
        let Some(latest) = self.update_latest.clone() else {
            return;
        };
        let Some(dir) = self
            .update_modal
            .as_ref()
            .and_then(|modal| modal.dir.clone())
        else {
            return;
        };
        if !crate::update_install::dir_writable(&dir) {
            if let Some(modal) = self.update_modal.as_mut() {
                modal.writable = false;
            }
            self.sched.mark_dirty(Instant::now());
            return;
        }
        let plan = update_run::plan(&dir, &latest);
        self.install_generation += 1;
        let generation = self.install_generation;
        let spawned = (self.seams.spawn)(&plan, self.update_tx.clone(), generation);
        let (step, pgid, reason) = match spawned {
            Ok(pgid) => (Step::Running, Some(pgid), None),
            Err(error) => (
                Step::Failed,
                None,
                Some(format!(
                    "installer could not start: {}",
                    crate::sidebar::text::clean_ansi(&error)
                )),
            ),
        };
        self.install = Some(Install {
            generation,
            plan,
            started: Instant::now(),
            finished: (step == Step::Failed).then(Instant::now),
            step,
            pgid,
            tail: Vec::new(),
            reason,
            verified: None,
        });
        if let Some(modal) = self.update_modal.as_mut() {
            modal.phase = if step == Step::Failed {
                Phase::Failed
            } else {
                Phase::Updating
            };
            modal.scroll = 0;
        }
        self.sched.mark_dirty(Instant::now());
    }

    /// `esc cancel`: SIGTERM to the whole group, SIGKILL after the
    /// grace if it lingers, and back to the notes card.
    pub(super) fn cancel_install(&mut self) {
        let Some(install) = self.install.take() else {
            return;
        };
        if let Some(pgid) = install.pgid {
            (self.seams.signal)(pgid, libc::SIGTERM);
            let (alive, signal) = (
                Arc::clone(&self.seams.alive),
                Arc::clone(&self.seams.signal),
            );
            tokio::spawn(async move {
                update_run::reap_after(pgid, GRACE, |p| alive(p), |p, s| signal(p, s)).await;
            });
        }
        if let Some(modal) = self.update_modal.as_mut() {
            modal.phase = Phase::Opened;
        }
        self.sched.mark_dirty(Instant::now());
    }

    /// Installer output, the exit status and the version check, for the
    /// live run only.
    pub(super) fn on_install_event(&mut self, event: UpdateEvent) {
        let latest = self.update_latest.clone().unwrap_or_default();
        let Some(install) = self.install.as_mut() else {
            return;
        };
        let outcome = match event {
            UpdateEvent::Output { generation, line } if generation == install.generation => {
                if install.tail.len() == TAIL_LINES {
                    install.tail.remove(0);
                }
                install.tail.push(crate::sidebar::text::clean_ansi(&line));
                None
            }
            UpdateEvent::Exited {
                generation,
                code: Some(0),
            } if generation == install.generation => {
                install.step = Step::Verifying;
                let bin = install.plan.bin.clone();
                let probe = Arc::clone(&self.seams.probe);
                let tx = self.update_tx.clone();
                tokio::spawn(async move {
                    let version = probe(bin).await;
                    let _ = tx
                        .send(UpdateEvent::Verified {
                            generation,
                            version,
                        })
                        .await;
                });
                None
            }
            UpdateEvent::Exited { generation, code } if generation == install.generation => {
                let reason = match code {
                    Some(code) => format!("installer exited with status {code}"),
                    None => "installer was stopped by a signal".to_owned(),
                };
                Some((Step::Failed, Some(reason), None))
            }
            UpdateEvent::Verified {
                generation,
                version,
            } if generation == install.generation => {
                let expected = format!("backlot {latest}");
                if version.as_deref() == Some(expected.as_str()) {
                    Some((Step::Done, None, Some(expected)))
                } else {
                    let got = version
                        .as_deref()
                        .and_then(|text| text.strip_prefix("backlot "))
                        .unwrap_or("nothing");
                    let reason = format!("binary check expected {latest}, got {got}");
                    Some((Step::Failed, Some(reason), None))
                }
            }
            _ => None,
        };
        if let Some((step, reason, verified)) = outcome {
            install.step = step;
            install.reason = reason.map(|r| crate::sidebar::text::clean_ansi(&r));
            install.verified = verified;
            install.finished = Some(Instant::now());
            let phase = if step == Step::Done {
                Phase::Done
            } else {
                Phase::Failed
            };
            if let Some(modal) = self.update_modal.as_mut() {
                modal.phase = phase;
            }
        }
        self.sched.mark_dirty(Instant::now());
    }

    /// The checklist and elapsed column tick while a run is live.
    pub(super) fn install_tick_deadline(&self) -> Option<Instant> {
        self.install
            .as_ref()
            .filter(|install| install.running())
            .map(|_| Instant::now() + FRAME)
    }

    /// The deadline arm: a live run redraws each frame.
    pub(super) fn install_tick(&mut self, now: Instant) -> bool {
        let live = self.install.as_ref().is_some_and(Install::running);
        if live {
            self.sched.mark_dirty(now);
        }
        live
    }

    /// `esc later` after a verified install: restart stays one key away.
    pub(super) fn keep_restart_ready(&mut self) {
        let done = self
            .install
            .as_ref()
            .is_some_and(|install| install.step == Step::Done);
        if done {
            self.restart_ready = self.update_latest.clone();
        }
    }

    /// `↵ restart` on a verified install. The installed version is marked
    /// seen first (its notes were read), then the run loop unwinds the way
    /// a detach does (leases released, panes shut down) and `cli` execs
    /// the installed binary with the original arguments.
    pub(super) fn request_restart(&mut self) {
        let done = self
            .install
            .as_ref()
            .is_some_and(|install| install.step == Step::Done);
        let Some(latest) = self.update_latest.clone().filter(|_| done) else {
            return;
        };
        self.restart = self
            .install
            .as_ref()
            .map(|install| install.plan.bin.clone());
        self.update_state.show_on_startup_version = Some(latest);
        self.persist_update_state();
        self.restart_ready = None;
        self.update_modal = None;
        self.quitting = true;
    }

    /// The installed binary to exec once the run loop has unwound.
    pub(crate) fn take_restart(&mut self) -> Option<std::path::PathBuf> {
        self.restart.take()
    }
}
