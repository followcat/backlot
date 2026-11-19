//! The command dispatch `cli::run` drives (split out under the file
//! size guideline): parse → identity → the no-ledger arms → the
//! reads → G2's `--text`/`--stdin` materialization → `exec_event`.

use std::path::PathBuf;

use crate::args::{Cmd, parse};
use crate::cli::{Env, Fail, base, check_identity, check_stage_caller, exec_event};

/// `run` with stdin already read — the tests' seam: `None` reads the
/// real stream, and only when a `--stdin` command asks for it (G2).
pub(crate) fn run_stdin(args: &[String], env: &Env, stdin: Option<&str>) -> Result<String, Fail> {
    let (common, mut cmd) = parse(args)?;
    if let Cmd::Help(u) = cmd {
        return Ok(format!("{u}\n{}", crate::args::COMMON));
    }
    check_identity(&common, env)?;
    if let Cmd::StudioNew {
        interactive: true,
        title,
        repo,
        cast_file,
        dry_run,
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
            },
            env,
        );
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
        // Hollywood's own ledger needs no studio at all (D13).
        Cmd::Hollywood(..) | Cmd::Card(..) | Cmd::TellHollywood { .. } => {
            return crate::hwood::run(common, cmd, env);
        }
        // `runners` is local detection only — no core, no ledger.
        Cmd::Runners { json } => {
            return match json {
                true => crate::runners::detect::report_json(env),
                false => crate::runners::detect::report(env),
            };
        }
        // `repos` reads the directory itself; `in_use_studio` needs
        // the ledger base, which `repos::run` resolves best-effort.
        Cmd::Repos { json, ref from } => {
            return crate::repos::run(env, &common, json, from.as_deref());
        }
        // `studio prune` reads the ledgers and reaps finished work —
        // no studio event is appended.
        Cmd::StudioPrune {
            studio,
            all_finished,
            dry_run,
            force,
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
            return crate::prune::run(&base, named, all_finished, dry_run, force);
        }
        _ => {}
    }
    let base = base(env, &common)?;
    // The reads — including the cross-studio `--all` forms and the
    // `studio new` dry run — append nothing (R24, D19).
    if let Some(out) = crate::hwood_read::read(&common, &cmd, env, &base) {
        return out;
    }
    // G2: a `--text`/`--stdin` body lands in a temp file the `--file`
    // path then copies; `commands` only ever sees files.
    let temp = materialize(&mut cmd, stdin)?;
    let out = exec_event(common, cmd, env, &base);
    if let Some(path) = temp {
        let _ = std::fs::remove_file(path);
    }
    out
}

/// `--text`/`--stdin` materialization (G2): the body a `--file`
/// command copies lands in a temp file the caller removes after the
/// append. An empty body is refused before any file is written.
fn materialize(cmd: &mut Cmd, stdin: Option<&str>) -> Result<Option<PathBuf>, Fail> {
    use crate::args::TextSource;
    let source = match cmd {
        Cmd::Tell { source, .. } | Cmd::ReqRevise { source, .. } => source,
        _ => return Ok(None),
    };
    let body = match source {
        TextSource::File(_) => return Ok(None),
        TextSource::Text(text) => text.clone(),
        TextSource::Stdin => match stdin {
            Some(text) => text.to_string(),
            None => std::io::read_to_string(std::io::stdin())
                .map_err(|e| Fail::refused(format!("cannot read stdin: {e}")))?,
        },
    };
    if body.trim().is_empty() {
        return Err(Fail::refused("the message body is empty"));
    }
    let path = body_file(&body)?;
    *source = TextSource::File(path.clone());
    Ok(Some(path))
}

/// The unique temp file a `--text`/`--stdin` body is staged in.
fn body_file(body: &str) -> Result<PathBuf, Fail> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let path = std::env::temp_dir().join(format!(
        "backlot-body-{}-{}.txt",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::write(&path, body)
        .map_err(|e| Fail::refused(format!("cannot stage the message body: {e}")))?;
    Ok(path)
}
