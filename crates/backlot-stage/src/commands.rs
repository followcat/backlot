//! Build the `Pending` for each command: the event plus the file copies
//! destined for the studio directory. Content a command is given is
//! copied under `req/`, `plan/`, `reports/` or `messages/` — the ledger
//! never points into a worktree.

use std::fs;
use std::path::{Path, PathBuf};

use backlot_model::event::{default_rework_cap, *};
use backlot_model::state::StudioState;
use serde::Deserialize;

use crate::args::Cmd;
use crate::cli::{Caller, Fail};
use crate::store::{Pending, PendingFile, sanitize};

/// The closure `store::append` calls inside the lock.
pub type PendingBuilder = Box<dyn FnOnce(&StudioState, u64) -> Result<Pending, Fail>>;

fn pending(event: EventKind, by: backlot_model::EventBy, files: Vec<PendingFile>) -> Pending {
    Pending {
        event: NewEvent::new(by, event),
        files,
    }
}

fn must_exist(path: &Path, flag: &str) -> Result<(), Fail> {
    if path.is_file() {
        Ok(())
    } else {
        Err(Fail::usage(format!(
            "{flag}: file not found: {}",
            path.display()
        )))
    }
}

/// `priority`, `reopen` and `close` are person-facing commands: the
/// user or the director records them; a working role cannot.
fn person_gate(caller: &Caller) -> Result<(), Fail> {
    match caller.role.as_str() {
        "user" | "director" => Ok(()),
        other => Err(Fail::refused(format!(
            "role '{other}' cannot do this; the user or the director can"
        ))),
    }
}

/// `seq`-prefixed copy name under a content dir.
fn copy_to(subdir: &str, seq: u64, src: &Path, label: Option<&str>) -> PendingFile {
    let base = src
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".into());
    let leaf = match label {
        Some(l) => format!("{seq}-{l}-{}", sanitize(&base)),
        None => format!("{seq}-{}", sanitize(&base)),
    };
    PendingFile {
        src: src.to_path_buf(),
        dest: format!("{subdir}/{leaf}"),
    }
}

/// `backlot studio new --cast` file: role identities and candidates.
#[derive(Debug, Deserialize)]
struct CastFile {
    #[serde(default)]
    mode: Option<StudioMode>,
    #[serde(default)]
    rework_cap: Option<u32>,
    producer: ActorIdentity,
    director: ActorIdentity,
    writer: ActorIdentity,
    #[serde(default)]
    candidates: Vec<Candidate>,
}

fn read_cast(path: &Path) -> Result<CastFile, Fail> {
    must_exist(path, "--cast")?;
    let text = fs::read_to_string(path).map_err(|e| Fail::usage(format!("--cast: {e}")))?;
    toml::from_str(&text).map_err(|e| Fail::usage(format!("--cast {}: {e}", path.display())))
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path, flag: &str) -> Result<T, Fail> {
    must_exist(path, flag)?;
    let text = fs::read_to_string(path).map_err(|e| Fail::usage(format!("{flag}: {e}")))?;
    serde_json::from_str(&text).map_err(|e| Fail::usage(format!("{flag} {}: {e}", path.display())))
}

/// Report goes director → producer → user (design.md flow).
fn report_to(role: &str) -> Result<String, Fail> {
    match role {
        "director" => Ok("producer".into()),
        "producer" => Ok("user".into()),
        other => Err(Fail::usage(format!(
            "role '{other}' has no report target; reports flow director → producer → user"
        ))),
    }
}

/// A role-to-role message with its body copied into `messages/`.
/// `purpose` tags the two command-sent messages the stage acts on:
/// `plan` (writer wanted) and `dispatch` (writer released).
fn message_cmd(
    by: EventBy,
    file: PathBuf,
    to: String,
    purpose: Option<&'static str>,
) -> Result<PendingBuilder, Fail> {
    must_exist(&file, "--file")?;
    Ok(Box::new(move |_, seq| {
        let pf = copy_to("messages", seq, &file, None);
        Ok(pending(
            EventKind::MessageQueued {
                to,
                file: Some(pf.dest.clone()),
                purpose: purpose.map(String::from),
                source_seq: None,
            },
            by,
            vec![pf],
        ))
    }))
}

/// Parse and pre-check the command; the returned closure runs under
/// the append lock with the replayed state and the seq it will get.
pub fn build(caller: &Caller, cmd: Cmd) -> Result<PendingBuilder, Fail> {
    let by = caller.by();
    match cmd {
        Cmd::Help(_) => unreachable!("help is answered in cli::run"),
        Cmd::Status { .. } | Cmd::Records { .. } | Cmd::StageStart | Cmd::StageRun => {
            unreachable!("commands without a ledger event do not append")
        }
        Cmd::StudioNew {
            title,
            repo,
            cast_file,
        } => {
            let cast = read_cast(&cast_file)?;
            Ok(Box::new(move |_, _| {
                Ok(pending(
                    EventKind::StudioCreated {
                        title,
                        repo,
                        mode: cast.mode.unwrap_or_default(),
                        roles: Box::new(RoleConfigs {
                            producer: cast.producer,
                            director: cast.director,
                            writer: cast.writer,
                        }),
                        candidates: cast.candidates,
                        rework_cap: cast.rework_cap.unwrap_or_else(default_rework_cap),
                    },
                    by,
                    Vec::new(),
                ))
            }))
        }
        Cmd::ReqRevise { file, summary } => {
            must_exist(&file, "--file")?;
            Ok(Box::new(move |state, _| {
                let version = state.requirement_version + 1;
                Ok(pending(
                    EventKind::ReqVersion {
                        version,
                        file: format!("req/v{version}.md"),
                        summary,
                    },
                    by,
                    vec![PendingFile {
                        src: file,
                        dest: format!("req/v{version}.md"),
                    }],
                ))
            }))
        }
        Cmd::PlanRequest => Ok(Box::new(move |_, _| {
            Ok(pending(
                EventKind::MessageQueued {
                    to: "writer".into(),
                    file: None,
                    purpose: Some("plan".into()),
                    source_seq: None,
                },
                by,
                Vec::new(),
            ))
        })),
        Cmd::PlanPublish { plan_kind, file } => {
            must_exist(&file, "--file")?;
            let kind = match plan_kind {
                PlanKind::Plan => "plan",
                PlanKind::Acceptance => "acceptance",
            };
            Ok(Box::new(move |state, seq| {
                let pf = copy_to("plan", seq, &file, Some(kind));
                Ok(pending(
                    EventKind::PlanPublished {
                        plan_kind,
                        file: pf.dest.clone(),
                        requirement_version: state.requirement_version,
                    },
                    by,
                    vec![pf],
                ))
            }))
        }
        Cmd::Dispatch { file } => message_cmd(by, file, "director".into(), Some("dispatch")),
        Cmd::Ask { file } => message_cmd(by, file, "producer".into(), None),
        Cmd::Tell { role, file } => message_cmd(by, file, role, None),
        Cmd::Report { node, file } => {
            must_exist(&file, "--file")?;
            let to = report_to(&caller.role)?;
            Ok(Box::new(move |_, seq| {
                let pf = copy_to("reports", seq, &file, Some(&sanitize(&node)));
                Ok(pending(
                    EventKind::ReportSent {
                        to,
                        node,
                        file: pf.dest.clone(),
                    },
                    by,
                    vec![pf],
                ))
            }))
        }
        Cmd::TaskNew {
            title,
            task_kind,
            acceptance_file,
            depends_on,
        } => {
            must_exist(&acceptance_file, "--acceptance-file")?;
            Ok(Box::new(move |state, seq| {
                let id = format!("t{}", state.tasks.len() + 1);
                let pf = copy_to(
                    "plan",
                    seq,
                    &acceptance_file,
                    Some(&format!("{id}-acceptance")),
                );
                Ok(pending(
                    EventKind::TaskCreated {
                        task: id,
                        title,
                        task_kind,
                        requirement_version: state.requirement_version,
                        acceptance: pf.dest.clone(),
                        dependencies: depends_on,
                    },
                    by,
                    vec![pf],
                ))
            }))
        }
        Cmd::Cast {
            actor,
            reason,
            priority,
        } => {
            let task = caller.task.clone().unwrap_or_default();
            Ok(Box::new(move |state, _| {
                let cand = state
                    .studio
                    .as_ref()
                    .and_then(|s| s.candidates.iter().find(|c| c.name == actor))
                    .ok_or_else(|| Fail::refused(format!("'{actor}' is not a studio candidate")))?;
                Ok(pending(
                    EventKind::TaskCast {
                        task,
                        candidate: cand.name.clone(),
                        actor: cand.actor(),
                        reason,
                        priority,
                    },
                    by,
                    Vec::new(),
                ))
            }))
        }
        Cmd::Priority { level } => {
            person_gate(caller)?;
            let task = caller.task.clone().unwrap_or_default();
            Ok(Box::new(move |_, _| {
                Ok(pending(
                    EventKind::TaskPriority {
                        task,
                        priority: level,
                    },
                    by,
                    Vec::new(),
                ))
            }))
        }
        Cmd::Reopen { person } => {
            person_gate(caller)?;
            Ok(Box::new(move |_, _| {
                Ok(pending(
                    EventKind::RoleReopen { role: person },
                    by,
                    Vec::new(),
                ))
            }))
        }
        // `close` releases the hold: a live held person stops being
        // desired and the stage's Stop path records `role.closed`.
        Cmd::Close { person } => {
            person_gate(caller)?;
            Ok(Box::new(move |_, _| {
                Ok(pending(
                    EventKind::RoleRelease { role: person },
                    by,
                    Vec::new(),
                ))
            }))
        }
        Cmd::Deliver {
            commits,
            report,
            checks,
        } => {
            must_exist(&report, "--report")?;
            let task = caller.task.clone().unwrap_or_default();
            Ok(Box::new(move |state, seq| {
                let t = state
                    .tasks
                    .get(&task)
                    .ok_or_else(|| Fail::refused(format!("no task '{task}'")))?;
                let cast = t
                    .cast
                    .clone()
                    .ok_or_else(|| Fail::refused(format!("task '{task}' has no cast")))?;
                let pf = copy_to(
                    "reports",
                    seq,
                    &report,
                    Some(&format!("{task}-d{}", t.deliveries.len() + 1)),
                );
                Ok(pending(
                    EventKind::TaskDelivered {
                        task: task.clone(),
                        delivery: t.deliveries.len() as u32 + 1,
                        actor: cast.actor,
                        commits,
                        report: pf.dest.clone(),
                        requirement_version: t.requirement_version,
                        checks,
                    },
                    by,
                    vec![pf],
                ))
            }))
        }
        Cmd::Block { reason } => {
            let task = caller.task.clone().unwrap_or_default();
            Ok(Box::new(move |_, _| {
                Ok(pending(
                    EventKind::TaskState {
                        task,
                        state: TaskStateChange::Blocked,
                        reason,
                    },
                    by,
                    Vec::new(),
                ))
            }))
        }
        Cmd::TaskState { change, reason } => {
            let task = caller.task.clone().unwrap_or_default();
            Ok(Box::new(move |_, _| {
                Ok(pending(
                    EventKind::TaskState {
                        task,
                        state: change,
                        reason,
                    },
                    by,
                    Vec::new(),
                ))
            }))
        }
        Cmd::Review {
            delivery,
            verdict,
            conclusion,
            findings,
            upstream_changes,
        } => {
            let findings_data: Vec<Finding> = match &findings {
                Some(f) => read_json(f, "--findings")?,
                None => Vec::new(),
            };
            let upstream_data: Vec<UpstreamChange> = match &upstream_changes {
                Some(f) => read_json(f, "--upstream-changes")?,
                None => Vec::new(),
            };
            let task = caller.task.clone().unwrap_or_default();
            let reviewer = caller.role.clone();
            Ok(Box::new(move |_, seq| {
                let mut files = Vec::new();
                if let Some(f) = findings {
                    files.push(copy_to("reports", seq, &f, Some("findings")));
                }
                if let Some(f) = upstream_changes {
                    files.push(copy_to("reports", seq, &f, Some("upstream")));
                }
                Ok(pending(
                    EventKind::TaskReviewed {
                        task,
                        delivery,
                        reviewer,
                        verdict,
                        conclusion,
                        findings: findings_data,
                        upstream_changes: upstream_data,
                    },
                    by,
                    files,
                ))
            }))
        }
        Cmd::Finding { delivery, findings } => {
            let findings_data: Vec<Finding> = read_json(&findings, "--findings")?;
            let task = caller.task.clone().unwrap_or_default();
            Ok(Box::new(move |_, seq| {
                let pf = copy_to("reports", seq, &findings, Some("finding"));
                Ok(pending(
                    EventKind::TaskFinding {
                        task,
                        delivery,
                        findings: findings_data,
                    },
                    by,
                    vec![pf],
                ))
            }))
        }
        Cmd::Correct { event, reason, set } => Ok(Box::new(move |_, _| {
            Ok(pending(
                EventKind::Correction {
                    target: event,
                    reason,
                    set,
                },
                by,
                Vec::new(),
            ))
        })),
    }
}
