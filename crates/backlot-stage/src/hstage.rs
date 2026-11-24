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

/// The session id a resume may hand the runner: only one recorded
/// under the runner being launched. A `closed` or a session-less
/// `started` clears the id, but a ledger written before that fix can
/// still hold another runner's session — never pass it on.
pub(crate) fn resume_session<'a>(role: &'a HWRole, actor: &ActorIdentity) -> Option<&'a str> {
    let recorded = role.actor.as_ref()?;
    role.session_id
        .as_deref()
        .filter(|_| recorded.runner == actor.runner)
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

/// First-prompt value for `LaunchCtx.prompt`: `None` on resume, a
/// `<hdir>/prompts/hollywood.first.txt` path for `devin` (`--prompt-file`),
/// and the inline sentence for every other runner.
pub(crate) fn hfirst_prompt(hdir: &Path, runner: &str, resume: Option<&str>) -> Option<String> {
    if resume.is_some() {
        return None;
    }
    let line = crate::prompts::first_line(&hdir.join("AGENTS.md"));
    match runner {
        "devin" => Some(
            hdir.join("prompts")
                .join(format!("{HOLLYWOOD}.first.txt"))
                .to_string_lossy()
                .into_owned(),
        ),
        _ => Some(line),
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
    /// A `role.restart` the user recorded: close the live terminal on
    /// purpose (`restarted`) and, next tick, start it again —
    /// resuming the recorded session unless the request was `fresh`.
    Restart,
    /// The close a restart recorded, with its session kept: resume it
    /// and record `restarted`.
    ReviveResume,
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
                if role.restart.is_some() {
                    // The user's restart: close on purpose with the
                    // `restarted` reason; the start lands next tick.
                    vec![HMove::Restart]
                } else if role.actor.as_ref().is_some_and(|a| a.runner != cfg.runner) {
                    vec![HMove::Close("runner changed"), HMove::Start]
                } else {
                    vec![]
                }
            } else if role.live {
                vec![HMove::Lost]
            } else if role.closed {
                if term {
                    vec![HMove::Drop]
                } else if let Some(req) = &role.restart {
                    // The close a restart recorded: start it again —
                    // resumed when it kept a session, fresh with the
                    // role's prompt when the request asked for one or
                    // no session was kept.
                    if !req.fresh && role.session_id.is_some() {
                        vec![HMove::ReviveResume]
                    } else {
                        vec![HMove::Start]
                    }
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
            } else if role.restart.is_some() {
                // A lost role a restart names comes back at once,
                // around the loss guard.
                vec![HMove::ResumeRun]
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

pub(crate) fn refresh_agents_md(hdir: &Path) -> Result<(), Fail> {
    let agents = hdir.join("AGENTS.md");
    let want = crate::hwcli::agents_md();
    let unchanged = std::fs::read_to_string(&agents)
        .map(|cur| cur == want)
        .unwrap_or(false);
    if !unchanged {
        std::fs::create_dir_all(hdir).map_err(|e| Fail::refused(format!("hollywood dir: {e}")))?;
        std::fs::write(&agents, want).map_err(|e| Fail::refused(format!("AGENTS.md: {e}")))?;
    }
    Ok(())
}

/// The folder's durable files: `AGENTS.md` (rewritten every start, the
/// generated CLI reference in place), `prompts/hollywood.first.txt` when
/// `devin` starts fresh, and `memory.md` (created once and never
/// overwritten).
pub(crate) fn hprepare_launch(hdir: &Path, runner: &str, resume: Option<&str>) -> Result<(), Fail> {
    for sub in [
        "messages",
        "reports",
        "attention",
        "drafts",
        "roles",
        "prompts",
    ] {
        std::fs::create_dir_all(hdir.join(sub))
            .map_err(|e| Fail::refused(format!("hollywood dir: {e}")))?;
    }
    let agents = hdir.join("AGENTS.md");
    std::fs::write(&agents, crate::hwcli::agents_md())
        .map_err(|e| Fail::refused(format!("AGENTS.md: {e}")))?;
    if resume.is_none() && runner == "devin" {
        let line = crate::prompts::first_line(&agents);
        crate::prompts::first_file(hdir, HOLLYWOOD, &line)
            .map_err(|e| Fail::refused(format!("prompt: {e}")))?;
    }
    let memory = hdir.join("memory.md");
    if !memory.exists() {
        std::fs::write(&memory, "").map_err(|e| Fail::refused(format!("memory.md: {e}")))?;
    }
    Ok(())
}
