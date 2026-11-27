//! The command dispatch `cli::run` drives (split out under the file
//! size guideline): parse → identity → the no-ledger arms → the
//! reads → G2's `--text`/`--stdin` materialization → `exec_event`.

use std::path::PathBuf;

use crate::args::{Cmd, parse};
use crate::cli::{Env, Fail, base, check_identity, check_stage_caller, exec_event};

/// `run` with stdin already read — the tests' seam: `None` reads the
/// real stream, and only when a `--stdin` command asks for it (G2).
pub(crate) fn run_stdin(args: &[String], env: &Env, stdin: Option<&str>) -> Result<String, Fail> {
    let local_host = crate::server_cfg::system_hostname();
    let user = crate::server_cfg::env_user(&env.vars);
    run_stdin_with_host(args, env, stdin, local_host.as_deref(), user.as_deref())
}

pub(crate) fn run_stdin_with_host(
    args: &[String],
    env: &Env,
    stdin: Option<&str>,
    local_hostname: Option<&str>,
    current_user: Option<&str>,
) -> Result<String, Fail> {
    let (mut common, mut cmd) = parse(args)?;
    if let Cmd::Help(u) = cmd {
        return Ok(format!("{u}\n{}", crate::args::COMMON));
    }
    check_identity(&common, env)?;
    if matches!(cmd, Cmd::Design(crate::args::DesignCmd::Sign { .. })) {
        crate::proc_guard::check_cli_gate(crate::proc_guard::CliVerb::DesignSign, &common, env)?;
    }
    if matches!(
        cmd,
        Cmd::RepoNote(
            crate::args::RepoNoteCmd::Show { .. } | crate::args::RepoNoteCmd::Remove { .. }
        )
    ) {
        crate::proc_guard::check_cli_gate(crate::proc_guard::CliVerb::RepoNoteAdmin, &common, env)?;
        return crate::repo_note::admin(
            &common,
            match cmd {
                Cmd::RepoNote(rcmd) => rcmd,
                _ => unreachable!(),
            },
            env,
        );
    }
    if let Some(res) =
        crate::remote_doc_cli::route(&mut common, &cmd, env, stdin, local_hostname, current_user)
    {
        return res;
    }
    if matches!(cmd, Cmd::StudioNew { .. }) && common.remote.is_some() {
        return Err(Fail::refused(
            "remote studio placement is Phase 2; create studios on the server host",
        ));
    }
    if let Cmd::StudioNew {
        interactive: true,
        title,
        repo,
        cast_file,
        dry_run,
        mode,
        ..
    } = &cmd
    {
        // The wizard resolves the answers itself, then runs the
        // ordinary `studio new` write — `exec_event` never sees
        // `interactive`.
        return crate::wizard::run(
            common,
            crate::wizard::WizardArgs {
                title: title.clone(),
                repo: repo.clone(),
                cast_file: cast_file.clone(),
                dry_run: *dry_run,
                mode: *mode,
            },
            env,
        );
    }
    if let Cmd::Approve {
        ref person,
        decision,
    } = cmd
    {
        crate::proc_guard::check_cli_gate(
            crate::proc_guard::CliVerb::Approve { person },
            &common,
            env,
        )?;
        if person == backlot_model::hollywood::HOLLYWOOD {
            return crate::hwood::approve(&common, env, decision);
        }
    }
    // `restart --person hollywood` restarts the overseer's own
    // terminal: its own ledger, carried out by the stage's `hplan`.
    if let Cmd::Restart {
        ref person,
        fresh,
        ref reason,
    } = cmd
        && person == backlot_model::hollywood::HOLLYWOOD
    {
        return crate::hwood::restart(&common, env, fresh, reason);
    }
    match cmd {
        // The stage connects itself; no ledger command needs it here.
        Cmd::StageStart => {
            check_stage_caller(&common, env)?;
            return crate::stage::start(&common, env);
        }
        Cmd::StageRun => {
            check_stage_caller(&common, env)?;
            return crate::stage::run(&common, env);
        }
        Cmd::StageStop => {
            check_stage_caller(&common, env)?;
            return crate::stage::stop(&common, env);
        }
        Cmd::StageWatch {
            ref target,
            restart,
        } => {
            check_stage_caller(&common, env)?;
            return crate::watchdog::watch(&common, env, target.clone(), restart);
        }
        Cmd::StageService { write } => {
            check_stage_caller(&common, env)?;
            return crate::service::run(&common, env, write);
        }
        Cmd::Server(scmd) => {
            crate::proc_guard::check_cli_gate(
                crate::proc_guard::CliVerb::ServerControl,
                &common,
                env,
            )?;
            check_stage_caller(&common, env)?;
            return match scmd {
                crate::args::ServerCmd::Run => crate::server_mode::run(&common, env),
                crate::args::ServerCmd::Start => crate::server_mode::start(&common, env),
                crate::args::ServerCmd::Stop => crate::server_mode::stop(&common, env),
                crate::args::ServerCmd::Adopt {
                    dry_run,
                    stop_stages,
                    core,
                } => crate::server_adopt::run(&common, env, dry_run, stop_stages, core.as_deref()),
                crate::args::ServerCmd::Service { write } => {
                    crate::server_service::run(&common, env, write)
                }
            };
        }
        // Hollywood's own ledger needs no studio at all (D13).
        Cmd::Hollywood(..) | Cmd::Card(..) | Cmd::TellHollywood { .. } => {
            return crate::hwood::run(common, cmd, env);
        }
        Cmd::PermsRevoke { .. } => {
            crate::proc_guard::check_cli_gate(
                crate::proc_guard::CliVerb::PermsRevoke,
                &common,
                env,
            )?;
            return Err(Fail::refused("perms revoke is not implemented in Phase 1"));
        }
        // `runners` is local detection only — no core, no ledger.
        Cmd::Runners { json } => {
            return match json {
                true => crate::runners::detect::report_json(env),
                false => crate::runners::detect::report(env),
            };
        }
        // `guide` prints backlot's own docs, compiled into the binary
        // — no studio, no ledger, no disk search.
        Cmd::Guide {
            ref topic,
            ref grep,
        } => {
            return crate::guide::run(topic.as_deref(), grep.as_deref());
        }
        // `repos` reads the directory itself; `in_use_studio` needs
        // the ledger base, which `repos::run` resolves best-effort.
        Cmd::Repos {
            json,
            ref from,
            all,
        } => {
            return crate::repos::run(env, &common, json, from.as_deref(), all);
        }
        // `studio prune` reads the ledgers and reaps finished work —
        // no studio event is appended.
        Cmd::StudioPrune {
            studio,
            all_finished,
            dry_run,
            force,
            json,
        } => {
            // A positional studio wins; otherwise the pinned
            // `BACKLOT_STUDIO`/`--studio` names it. `--all-finished`
            // deliberately ignores a pinned studio.
            let named = match studio {
                Some(name) => Some(name),
                None if all_finished => None,
                None => Some(crate::cli::studio_name(&common, env)?),
            };
            let base = base(env, &common)?;
            let base = match &named {
                Some(name) => crate::cli::resolve_base(
                    env,
                    &common,
                    &base,
                    &crate::cli::check_studio_name(name)?,
                )?,
                None => base,
            };
            return crate::prune::run(&base, named, all_finished, dry_run, force, json);
        }
        // `studio delete` closes terminals, prunes worktrees/branches,
        // records `studio.deleted` and archives the ledger.
        Cmd::StudioDelete {
            studio,
            dry_run,
            force,
            json,
        } => {
            return crate::delete::run(env, &common, studio, dry_run, force, json);
        }
        // `studio migrate` rewrites logs at the current schema
        // version — under the same locks, no event appended.
        Cmd::StudioMigrate {
            studio,
            all,
            dry_run,
        } => {
            check_stage_caller(&common, env)?;
            let named = match studio {
                Some(name) => Some(name),
                None if all => None,
                None => Some(crate::cli::studio_name(&common, env)?),
            };
            let base = base(env, &common)?;
            let base = match &named {
                Some(name) => crate::cli::resolve_base(
                    env,
                    &common,
                    &base,
                    &crate::cli::check_studio_name(name)?,
                )?,
                None => base,
            };
            return crate::migrate::run(&base, named, all, dry_run);
        }
        // `studio list` reads every core directory (or the pinned
        // one) — no studio name, no event appended. `--archived`
        // reads the ledgers under `archive/` instead.
        Cmd::StudioList { json, archived } => {
            return crate::studio_list::run(env, &common, json, archived);
        }
        // `studio restore` brings an archived ledger back as a
        // finished studio — nothing appended to a live ledger until
        // the copy lands.
        Cmd::StudioRestore {
            archived,
            new_name,
            dry_run,
            json,
        } => {
            check_stage_caller(&common, env)?;
            return crate::restore::run(
                env,
                &common,
                &archived,
                new_name.as_deref(),
                dry_run,
                json,
            );
        }
        _ => {}
    }
    let base = base(env, &common)?;
    // The first message's body must not be empty, whatever its source
    // — the same rule `--text` and `--stdin` carry. Checked before the
    // dry run's read so both paths refuse alike.
    if let Cmd::StudioNew {
        ask: Some(crate::args::TextSource::File(file)),
        ..
    } = &cmd
    {
        let body = std::fs::read_to_string(file)
            .map_err(|e| Fail::refused(format!("--ask-file {}: {e}", file.display())))?;
        if body.trim().is_empty() {
            return Err(Fail::refused("the ask is empty"));
        }
    }
    // The reads — including the cross-studio `--all` forms and the
    // `studio new` dry run — append nothing (R24, D19).
    if let Some(out) = crate::hwood_read::read(&common, &cmd, env, &base) {
        return out;
    }
    // G2: `--text`/`--stdin`/`--report -` land in a temp file cleaned up after exec_event.
    let temp = materialize(&mut cmd, stdin)?;
    let out = exec_event(common, cmd, env, &base);
    if let Some(path) = temp {
        let _ = std::fs::remove_file(&path);
        if let Some(dir) = path.parent().filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("backlot-report-"))
        }) {
            let _ = std::fs::remove_dir(dir);
        }
    }
    out
}

fn read_stdin_body(stdin: Option<&str>) -> Result<String, Fail> {
    match stdin {
        Some(text) => Ok(text.to_string()),
        None => std::io::read_to_string(std::io::stdin())
            .map_err(|e| Fail::refused(format!("cannot read stdin: {e}"))),
    }
}

/// `--text`/`--stdin`/`--report -` materialization (G2): stage into a
/// temp file removed after append; empty bodies refuse before writing.
fn materialize(cmd: &mut Cmd, stdin: Option<&str>) -> Result<Option<PathBuf>, Fail> {
    if let Cmd::Deliver { report, .. } = cmd {
        if report != std::path::Path::new("-") {
            return Ok(None);
        }
        let body = read_stdin_body(stdin)?;
        if body.trim().is_empty() {
            return Err(Fail::refused("the report body is empty"));
        }
        let path = stage_body("report", &body, true)?;
        *report = path.clone();
        return Ok(Some(path));
    }
    use crate::args::TextSource;
    let source = match cmd {
        Cmd::Tell { source, .. }
        | Cmd::ReqRevise { source, .. }
        | Cmd::Progress { source }
        | Cmd::Design(crate::args::DesignCmd::Note { source, .. }) => source,
        Cmd::StudioNew {
            ask: Some(source), ..
        } => source,
        _ => return Ok(None),
    };
    let body = match source {
        TextSource::File(_) => return Ok(None),
        TextSource::Text(text) => text.clone(),
        TextSource::Stdin => read_stdin_body(stdin)?,
    };
    if body.trim().is_empty() {
        return Err(Fail::refused("the message body is empty"));
    }
    let path = stage_body("message", &body, false)?;
    *source = TextSource::File(path.clone());
    Ok(Some(path))
}

#[cfg(test)]
thread_local! {
    /// Bodies staged on this thread, so a test checks only the files its own runs staged:
    /// other tests stage bodies in the same temp dir at the same time.
    pub(crate) static STAGED_BODIES: std::cell::RefCell<Vec<PathBuf>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

fn stage_body(label: &str, body: &str, nested_report: bool) -> Result<PathBuf, Fail> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let id = format!(
        "{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    );
    let path = if nested_report {
        let dir = std::env::temp_dir().join(format!("backlot-report-{id}"));
        std::fs::create_dir_all(&dir)
            .map_err(|e| Fail::refused(format!("cannot stage the {label} body: {e}")))?;
        dir.join("report.md")
    } else {
        std::env::temp_dir().join(format!("backlot-body-{id}.txt"))
    };
    std::fs::write(&path, body)
        .map_err(|e| Fail::refused(format!("cannot stage the {label} body: {e}")))?;
    #[cfg(test)]
    STAGED_BODIES.with(|s| s.borrow_mut().push(path.clone()));
    Ok(path)
}
