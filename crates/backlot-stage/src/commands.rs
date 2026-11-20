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

pub(crate) fn must_exist(path: &Path, flag: &str) -> Result<(), Fail> {
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
        append: None,
    }
}

/// `backlot studio new --cast` file: role identities and candidates.
#[derive(Debug, Deserialize)]
pub(crate) struct CastFile {
    #[serde(default)]
    pub(crate) mode: Option<StudioMode>,
    #[serde(default)]
    pub(crate) rework_cap: Option<u32>,
    #[serde(default)]
    pub(crate) stall_limit_s: Option<u32>,
    #[serde(default)]
    pub(crate) nudge_after_s: Option<u32>,
    #[serde(default)]
    pub(crate) escalate_after_s: Option<u32>,
    /// S9e: the per-task running budget in seconds; unset means none.
    #[serde(default)]
    pub(crate) task_budget_s: Option<u32>,
    /// S9e: `[limits]` — runner name → maximum live roles in this
    /// studio; a runner with no entry is unlimited.
    #[serde(default)]
    pub(crate) limits: std::collections::BTreeMap<String, u32>,
    pub(crate) producer: ActorIdentity,
    pub(crate) director: ActorIdentity,
    pub(crate) writer: ActorIdentity,
    #[serde(default)]
    pub(crate) candidates: Vec<Candidate>,
    #[serde(default)]
    pub(crate) check_env: Vec<String>,
    #[serde(default)]
    pub(crate) checks: Vec<crate::checks::CastCheck>,
}

/// Read and validate a `--cast` file — the same checks `studio new`
/// runs, shared by `--dry-run` and `hollywood propose studio-new`.
pub(crate) fn read_cast(path: &Path) -> Result<CastFile, Fail> {
    // `-` reads the cast from stdin — how the client's New Studio
    // dialog hands a generated cast to a remote `studio new` without
    // leaving a file on that machine.
    let text = if path == Path::new("-") {
        let mut buf = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin().lock(), &mut buf)
            .map_err(|e| Fail::usage(format!("--cast -: {e}")))?;
        buf
    } else {
        must_exist(path, "--cast")?;
        fs::read_to_string(path).map_err(|e| Fail::usage(format!("--cast: {e}")))?
    };
    let cast: CastFile = toml::from_str(&text)
        .map_err(|e| Fail::usage(format!("--cast {}: {e}", path.display())))?;
    // Zero stalls every waiting screen at once — the limit needs a
    // real number like every other cast value.
    if cast.stall_limit_s == Some(0) {
        return Err(Fail::usage("stall_limit_s must be at least 1"));
    }
    // A zero delay nudges or escalates on the same tick it measures.
    if cast.nudge_after_s == Some(0) {
        return Err(Fail::usage("nudge_after_s must be at least 1"));
    }
    if cast.escalate_after_s == Some(0) {
        return Err(Fail::usage("escalate_after_s must be at least 1"));
    }
    // A zero budget flags every cast the tick it lands.
    if cast.task_budget_s == Some(0) {
        return Err(Fail::usage("task_budget_s must be at least 1"));
    }
    // A zero cap seats nobody — a runner is either limited by a real
    // number or absent and unlimited.
    if let Some((runner, _)) = cast.limits.iter().find(|(_, cap)| **cap == 0) {
        return Err(Fail::usage(format!("limits.{runner} must be at least 1")));
    }
    // `cursor` and `devin` carry the thinking depth in the model id
    // — no effort flag exists. `cmd`'s `--effort` writes the global
    // config (measured), and `opencode`'s argv ignores the value,
    // so they take none either. A set effort on a runner that
    // carries none would lie in the ledger; every other runner that
    // consumes it still needs it declared.
    let roles = [
        ("producer", &cast.producer.runner, &cast.producer.effort),
        ("director", &cast.director.runner, &cast.director.effort),
        ("writer", &cast.writer.runner, &cast.writer.effort),
    ];
    for (who, runner, effort) in roles.into_iter().chain(
        cast.candidates
            .iter()
            .map(|c| (c.name.as_str(), &c.runner, &c.effort)),
    ) {
        let effort = effort.as_deref().unwrap_or_default();
        if runner.as_str() == "cursor" && !effort.is_empty() {
            return Err(Fail::usage(format!(
                "{who}: runner 'cursor' has no effort flag — \
                 the depth is part of the model id"
            )));
        }
        if runner.as_str() == "cmd" && !effort.is_empty() {
            return Err(Fail::usage(format!(
                "{who}: runner 'cmd' cannot take an effort — \
                 `--effort` writes the global config"
            )));
        }
        if !matches!(runner.as_str(), "cursor" | "devin" | "cmd" | "opencode") && effort.is_empty()
        {
            return Err(Fail::usage(format!(
                "{who}: runner '{runner}' needs an effort — 'cursor', 'devin', 'cmd' and \
                 'opencode' carry none"
            )));
        }
    }
    // The producer and the director are always wanted: they keep
    // their slots for the studio's whole run. A cap under their
    // count seats no fixed role, and a cap at their count while the
    // writer or a candidate shares the runner leaves everyone else
    // waiting for a slot forever — refuse both at `studio new`.
    for (runner, cap) in &cast.limits {
        let fixed = [&cast.producer, &cast.director]
            .into_iter()
            .filter(|a| a.runner == *runner)
            .count() as u32;
        if *cap < fixed {
            return Err(Fail::usage(format!(
                "limits.{runner} = {cap} leaves a fixed role unseated: \
                 the producer and the director stay live on {runner}"
            )));
        }
        let someone_waits =
            cast.writer.runner == *runner || cast.candidates.iter().any(|c| c.runner == *runner);
        if *cap == fixed && someone_waits {
            return Err(Fail::usage(format!(
                "limits.{runner} = {cap} leaves no slot for the writer or an actor: \
                 the producer and the director stay live on {runner}"
            )));
        }
    }
    Ok(cast)
}

/// Validate a `--cast` file for the client's New Studio dialog —
/// the same checks `studio new` runs, with the failure's text.
pub fn validate_cast_file(path: &Path) -> Result<(), String> {
    read_cast(path)
        .map(|_| ())
        .map_err(|e| e.message().to_string())
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

/// The path a materialized source names — `Text` and `Stdin` only
/// exist between `parse` and `cli::run`'s materialization, never at
/// build time (G2).
fn source_file(source: &crate::args::TextSource) -> Result<PathBuf, Fail> {
    match source {
        crate::args::TextSource::File(path) => Ok(path.clone()),
        _ => Err(Fail::refused("internal: unresolved message source")),
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
        Cmd::Status { .. }
        | Cmd::Records { .. }
        | Cmd::CastSuggest { .. }
        | Cmd::StageStart
        | Cmd::StageStop
        | Cmd::StageRun
        | Cmd::StageWatch { .. }
        | Cmd::StageService { .. }
        | Cmd::Runners { .. }
        | Cmd::Repos { .. }
        | Cmd::StudioPrune { .. }
        | Cmd::StudioMigrate { .. }
        | Cmd::TellHollywood { .. } => {
            unreachable!("commands without a ledger event do not append")
        }
        Cmd::Hollywood(..) | Cmd::Card(..) => {
            unreachable!("Hollywood's own ledger is appended in hwood")
        }
        Cmd::StudioNew {
            title,
            repo,
            cast_file,
            dry_run,
            interactive,
            mode,
            init,
            no_git,
        } => {
            debug_assert!(!dry_run, "dry-run is answered in cli::run");
            debug_assert!(!interactive, "--interactive is answered by the wizard");
            // `hollywood` names the overseer's ledger beside
            // `studios/` — a studio can never take it (D13).
            if caller.studio == backlot_model::hollywood::HOLLYWOOD {
                return Err(Fail::refused(
                    "the name 'hollywood' is reserved for the overseer",
                ));
            }
            let cast = read_cast(&cast_file.expect("non-interactive parses require --cast"))?;
            let (check_env, checks) = crate::checks::declarations(&cast.check_env, &cast.checks)?;
            // The shape check runs again at launch on this machine
            // (D4) — the ledger records what is, not what the pick
            // saw. Member repositories are the event's `members`;
            // `--init`'s `git init` and every empty repository's
            // initial commit run before the record lands. A path
            // that does not exist yet binds nothing today, as
            // before: the stage's `not a git repository` attention
            // names it at the first role start.
            let repo = repo.expect("non-interactive parses require --repo");
            let binding = match std::path::Path::new(&repo).is_dir() {
                true => crate::paths::check(&repo, init, no_git)?,
                false => crate::paths::Binding::default(),
            };
            crate::paths::initialize(&repo, &binding.members, binding.no_git, init)?;
            let members = binding.members.clone();
            Ok(Box::new(move |_, _| {
                Ok(pending(
                    EventKind::StudioCreated {
                        title: title.expect("non-interactive parses require --title"),
                        repo,
                        members,
                        no_git: binding.no_git,
                        // `--mode` on the command line wins over the
                        // cast file's `mode`; both default to standard.
                        mode: mode.or(cast.mode).unwrap_or_default(),
                        roles: Box::new(RoleConfigs {
                            producer: cast.producer,
                            director: cast.director,
                            writer: cast.writer,
                        }),
                        candidates: cast.candidates,
                        rework_cap: cast.rework_cap.unwrap_or_else(default_rework_cap),
                        stall_limit_s: cast.stall_limit_s.unwrap_or_else(default_stall_limit_s),
                        nudge_after_s: cast.nudge_after_s.unwrap_or_else(default_nudge_after_s),
                        escalate_after_s: cast
                            .escalate_after_s
                            .unwrap_or_else(default_escalate_after_s),
                        check_env,
                        checks,
                        task_budget_s: cast.task_budget_s,
                        limits: cast.limits,
                    },
                    by,
                    Vec::new(),
                ))
            }))
        }
        Cmd::ReqRevise { source, summary } => {
            let file = source_file(&source)?;
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
                        append: None,
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
        Cmd::Tell { role, source } => message_cmd(by, source_file(&source)?, role, None),
        Cmd::Report { node, file, .. } => {
            must_exist(&file, "--file")?;
            let to = report_to(&caller.role)?;
            Ok(Box::new(move |state, seq| {
                let mut pf = copy_to("reports", seq, &file, Some(&sanitize(&node)));
                pf.append = crate::checks::failing_section(state);
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
        // `approve` answers an open `permission prompt` attention on
        // the person — the model refuses when none is open (D9: no
        // prompt answered without a recorded decision).
        Cmd::Approve { person, decision } => {
            person_gate(caller)?;
            Ok(Box::new(move |_, _| {
                Ok(pending(
                    EventKind::RoleApproval {
                        role: person,
                        decision,
                    },
                    by,
                    Vec::new(),
                ))
            }))
        }
        // `attention clear` resolves a still-open attention by hand:
        // stage-restart leftovers and prompts the passes could not
        // prove gone (S12d). Clearing one that is not open — or a
        // seq that was never an attention — is refused.
        Cmd::AttentionClear { target } => {
            person_gate(caller)?;
            let who = caller.role.clone();
            Ok(Box::new(move |state, _| {
                if !state.open_attention().any(|a| a.seq == target) {
                    return Err(Fail::refused(format!("no open attention {target}")));
                }
                Ok(pending(
                    EventKind::AttentionCleared {
                        target,
                        reason: Some(format!("cleared by {who}")),
                    },
                    by,
                    Vec::new(),
                ))
            }))
        }
        // `studio finish` ends the studio; the model refuses while
        // any task is not terminal, naming them.
        Cmd::StudioFinish { reason } => {
            match caller.role.as_str() {
                "user" | "producer" => {}
                other => {
                    return Err(Fail::refused(format!(
                        "role '{other}' cannot do this; the user or the producer can"
                    )));
                }
            }
            Ok(Box::new(move |_, _| {
                Ok(pending(
                    EventKind::StudioFinished { reason },
                    by,
                    Vec::new(),
                ))
            }))
        }
        Cmd::Deliver {
            commits,
            report,
            checks,
            usage,
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
                let mut pf = copy_to(
                    "reports",
                    seq,
                    &report,
                    Some(&format!("{task}-d{}", t.deliveries.len() + 1)),
                );
                pf.append = crate::checks::failing_section(state);
                Ok(pending(
                    EventKind::TaskDelivered {
                        task: task.clone(),
                        delivery: t.deliveries.len() as u32 + 1,
                        actor: cast.actor,
                        commits,
                        report: pf.dest.clone(),
                        requirement_version: t.requirement_version,
                        checks,
                        usage,
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
        // `report show` is read-only — `hwood_read` serves it and
        // the append path never sees it (R24).
        Cmd::ReportShow { .. } => Err(Fail::refused("report show appends nothing")),
    }
}
