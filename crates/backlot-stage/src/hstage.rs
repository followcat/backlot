//! Hollywood's terminal lifecycle decisions (H3, D13-D14): one
//! `bl-hollywood` per core while `[hollywood]` is configured —
//! started, adopted after a stage restart, resumed after a loss the
//! way a producer is, and closed on a runner change or a config
//! removal. Every move lands in Hollywood's own ledger as
//! `role.state`; the layer owes no steps, is never nudged, and the
//! stage never answers a prompt in its terminal (D17a). The pure
//! planning half lives here; the Stage-side driver is `hlife.rs`,
//! the watch pass `hwatch.rs`.

use std::path::Path;

use backlot_model::EventBy;
use backlot_model::event::ActorIdentity;
use backlot_model::hollywood::{HOLLYWOOD, HWRole, HollywoodKind, NewHollywoodEvent, RoleMotion};

use crate::cli::Fail;
use crate::config::HollywoodCfg;
use crate::store::{self, HPending};

/// Three `lost` records inside ten minutes stop the resume loop —
/// the producer's crash guard, against Hollywood's own ledger.
const LOST_LIMIT: usize = 3;
const LOST_WINDOW_MS: u64 = 10 * 60 * 1000;

/// The `(studio, role)` key Hollywood's work samples, screens and
/// sends share — the layer is its own pair.
pub(crate) fn hkey() -> (String, String) {
    (HOLLYWOOD.into(), HOLLYWOOD.into())
}

/// `role.state` appended by the stage — the lifecycle record.
fn hrecord(hdir: &Path, kind: HollywoodKind) -> Result<u64, Fail> {
    let event = store::append_hollywood(hdir, move |_, _| {
        Ok::<HPending, std::convert::Infallible>(HPending {
            event: NewHollywoodEvent::new(EventBy::role("stage"), kind),
            files: Vec::new(),
        })
    })
    .map_err(|e| match e {
        store::StoreError::Io(e) => Fail::refused(format!("hollywood append: {e}")),
        store::StoreError::Corrupt(c) => Fail::refused(format!("{c}")),
        store::StoreError::Refused(r) => Fail::refused(format!("{r}")),
        store::StoreError::Build(_) => unreachable!("the build closure cannot fail"),
    })?;
    Ok(event.seq)
}

/// One lifecycle record.
pub(crate) fn record_role(
    hdir: &Path,
    state: RoleMotion,
    terminal: Option<String>,
    actor: Option<ActorIdentity>,
    session_id: Option<String>,
    reason: Option<&str>,
) -> Result<u64, Fail> {
    hrecord(
        hdir,
        HollywoodKind::RoleState {
            state,
            terminal,
            actor,
            session_id,
            reason: reason.map(String::from),
        },
    )
}

/// The actor identity `[hollywood]` names — `standard` is the one
/// permission profile the overseer gets (D17a).
pub(crate) fn identity(cfg: &HollywoodCfg) -> ActorIdentity {
    ActorIdentity {
        runner: cfg.runner.clone(),
        runner_version: None,
        model: cfg.model.clone(),
        effort: cfg.effort.clone(),
        permission_profile: "standard".into(),
    }
}

/// What one reconcile pass decides — every move but `Drop` records.
#[derive(Debug, PartialEq)]
pub(crate) enum HMove {
    /// Fresh `terminal.run`, then `role.state started`.
    Start,
    /// A live terminal nothing recorded — `started` with the adopt
    /// reason, no run.
    Adopt,
    /// A live terminal on a `lost` record — `resumed recovered`.
    Rejoin,
    /// A live record, no terminal — `lost`; the resume runs next
    /// tick, like a producer's.
    Lost,
    /// A lost record, no terminal — `terminal.run` on the recorded
    /// session, then `resumed recovered`; a refused run is another
    /// `lost`.
    ResumeRun,
    /// Three losses in ten minutes — `closed "cannot resume"`, and
    /// the layer stays down.
    GiveUp,
    /// `closed` with the reason; the terminal goes with it.
    Close(&'static str),
    /// A recorded `closed` that is not the resume guard's — the
    /// config asked for it back, so start over.
    Revive,
    /// A live `bl-hollywood` nothing wants — remove it, no record.
    Drop,
}

/// The crash guard: `LOST_LIMIT` losses inside the window stops it.
fn lost_loop(role: &HWRole, now_ms: u64) -> bool {
    role.losses
        .iter()
        .filter(|t| now_ms.saturating_sub(**t) < LOST_WINDOW_MS)
        .count()
        >= LOST_LIMIT
}

/// The reconcile decision — pure: config, the role record, whether
/// `bl-hollywood` is live and when this stage process was born go in,
/// moves come out. `stage_started_ms` is what lets a `cannot resume`
/// give-up come back: a close older than this process means the
/// person restarted the stage — the one way back (R1-4).
pub(crate) fn hplan(
    cfg: Option<&HollywoodCfg>,
    role: &HWRole,
    term: bool,
    now_ms: u64,
    stage_started_ms: u64,
) -> Vec<HMove> {
    match cfg {
        None => {
            if role.live {
                vec![HMove::Close("not configured")]
            } else if term {
                vec![HMove::Drop]
            } else {
                vec![]
            }
        }
        Some(cfg) => {
            if role.live && term {
                if role.actor.as_ref().is_some_and(|a| a.runner != cfg.runner) {
                    vec![HMove::Close("runner changed"), HMove::Start]
                } else {
                    vec![]
                }
            } else if role.live {
                vec![HMove::Lost]
            } else if role.closed {
                if term {
                    vec![HMove::Drop]
                } else if role.closed_reason.as_deref() == Some("cannot resume") {
                    // The give-up stands while this stage process
                    // lives; a close older than this process means the
                    // person restarted the stage — start a new
                    // conversation, the loss count already reset.
                    if role.since_ms < stage_started_ms {
                        vec![HMove::Revive]
                    } else {
                        vec![]
                    }
                } else {
                    vec![HMove::Revive]
                }
            } else if term {
                if role.recorded() {
                    vec![HMove::Rejoin]
                } else {
                    vec![HMove::Adopt]
                }
            } else if !role.recorded() {
                vec![HMove::Start]
            } else if lost_loop(role, now_ms) {
                vec![HMove::GiveUp]
            } else {
                vec![HMove::ResumeRun]
            }
        }
    }
}

/// What a `terminal.run` attempt ended as — `Defer` is the shared
/// real-launch slot's word, `Refused` the core's.
pub(crate) enum HRun {
    Up {
        address: String,
        session: Option<String>,
    },
    Defer,
    Refused(String),
}
