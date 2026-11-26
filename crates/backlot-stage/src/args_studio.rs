//! Parsers for the studio-level commands: `studio new`, `req revise`,
//! `plan publish`, and the one-file message commands.

use std::path::{Path, PathBuf};

use backlot_model::correct::parse_plan_kind;

use crate::args::*;
use crate::cli::Fail;

pub(crate) fn studio_new(common: &mut Common, it: &mut It) -> Result<(Common, Cmd), Fail> {
    let mut title = None;
    let mut repo = None;
    let mut cast_file = None;
    let mut mode = None;
    let mut dry_run = false;
    let mut interactive = false;
    let mut init = false;
    let mut no_git = false;
    // `--ask <text>` / `--ask-file <f>` — the wizard's first message
    // to the producer; one source only.
    let mut ask_text = None;
    let mut ask_file = None;
    while let Some(arg) = it.next() {
        if arg == "-h" || arg == "--help" {
            return Ok((std::mem::take(common), Cmd::Help(U_STUDIO)));
        }
        if common_flag(it, common, &arg, U_STUDIO)? {
            continue;
        }
        match arg.as_str() {
            "--title" => title = Some(it.value("--title", U_STUDIO)?),
            _ if eq(&arg, "--title").is_some() => title = eq(&arg, "--title").map(String::from),
            "--repo" => repo = Some(it.value("--repo", U_STUDIO)?),
            _ if eq(&arg, "--repo").is_some() => repo = eq(&arg, "--repo").map(String::from),
            "--cast" => cast_file = Some(PathBuf::from(it.value("--cast", U_STUDIO)?)),
            _ if eq(&arg, "--cast").is_some() => cast_file = eq(&arg, "--cast").map(PathBuf::from),
            "--mode" => mode = Some(parse_mode(&it.value("--mode", U_STUDIO)?)?),
            _ if eq(&arg, "--mode").is_some() => {
                mode = Some(parse_mode(eq(&arg, "--mode").unwrap())?)
            }
            "--ask" => ask_text = Some(it.value("--ask", U_STUDIO)?),
            _ if eq(&arg, "--ask").is_some() => ask_text = eq(&arg, "--ask").map(String::from),
            "--ask-file" => ask_file = Some(PathBuf::from(it.value("--ask-file", U_STUDIO)?)),
            _ if eq(&arg, "--ask-file").is_some() => {
                ask_file = eq(&arg, "--ask-file").map(PathBuf::from)
            }
            "--dry-run" => dry_run = true,
            "--interactive" => interactive = true,
            "--init" => init = true,
            "--no-git" => no_git = true,
            _ => return Err(usage(format!("unexpected argument '{arg}'"), U_STUDIO)),
        }
    }
    // The first message: `--ask` and `--ask-file` never combine, and
    // an empty `--ask` is refused now, as an empty `--text` is.
    if ask_text.is_some() && ask_file.is_some() {
        return Err(usage("pick one of --ask or --ask-file", U_STUDIO));
    }
    if let Some(text) = &ask_text
        && text.trim().is_empty()
    {
        return Err(usage("--ask must not be empty", U_STUDIO));
    }
    let ask = match (ask_text, ask_file) {
        (Some(text), None) => Some(TextSource::Text(text)),
        (None, Some(file)) => Some(TextSource::File(file)),
        _ => None,
    };
    if common.remote.is_some() {
        return Err(Fail::refused(
            "remote studio placement is Phase 2; create studios on the server host",
        ));
    }
    let cmd = match interactive {
        // The wizard resolves whatever the flags left unset — the
        // only pre-checks here are the ones that always hold.
        true => Cmd::StudioNew {
            title,
            repo: match repo {
                Some(r) => {
                    check_repo_absolute(&r)?;
                    Some(r)
                }
                None => None,
            },
            cast_file,
            dry_run,
            interactive: true,
            mode,
            init,
            no_git,
            ask,
        },
        false => {
            let repo = repo.ok_or_else(|| usage("missing --repo", U_STUDIO))?;
            check_repo_absolute(&repo)?;
            Cmd::StudioNew {
                title: Some(title.ok_or_else(|| usage("missing --title", U_STUDIO))?),
                repo: Some(repo),
                cast_file: Some(cast_file.ok_or_else(|| usage("missing --cast", U_STUDIO))?),
                dry_run,
                interactive: false,
                mode,
                init,
                no_git,
                ask,
            }
        }
    };
    Ok((std::mem::take(common), cmd))
}

/// `standard|experimental` for `--mode` (R14) — the cast file's
/// `mode` accepts the same two words through `CastFile.mode`.
fn parse_mode(v: &str) -> Result<backlot_model::event::StudioMode, Fail> {
    backlot_model::correct::parse_mode(v).ok_or_else(|| {
        usage(
            format!("bad --mode '{v}', want standard|experimental"),
            U_STUDIO,
        )
    })
}

/// `--repo` must be absolute: a relative path would bind the studio
/// to whatever cwd the stage happened to run from. The check runs on
/// the client; canonicalization stays with the machine that owns the
/// path — a remote `studio new` must never resolve a hub path against
/// its own filesystem.
pub(crate) fn check_repo_absolute(repo: &str) -> Result<(), Fail> {
    if !Path::new(repo).is_absolute() {
        return Err(usage(
            format!("--repo must be an absolute path, got '{repo}'"),
            U_STUDIO,
        ));
    }
    Ok(())
}

/// An existing path is canonicalized so `..` and symlink spellings of
/// one folder cannot masquerade as two; a path that does not exist yet
/// is kept as spelled. Runs where the repository lives, never on a
/// client that only names a hub path.
pub(crate) fn canonical_repo(repo: &str) -> String {
    match Path::new(repo).canonicalize() {
        Ok(p) => p.to_string_lossy().into_owned(),
        Err(_) => repo.to_string(),
    }
}

/// Absolute check plus canonicalization — the local CLI and wizard
/// path.
pub(crate) fn repo_path(repo: &str) -> Result<String, Fail> {
    check_repo_absolute(repo)?;
    Ok(canonical_repo(repo))
}

/// `studio cast show [--json]` / `studio cast set --file <toml>
/// [--dry-run] [--json]`.
pub(crate) fn studio_cast(common: &mut Common, it: &mut It) -> Result<(Common, Cmd), Fail> {
    let Some(sub) = it.next() else {
        return Err(usage("missing studio cast subcommand", U_STUDIO_CAST));
    };
    let mut file = None;
    let mut dry_run = false;
    let mut json = false;
    let show = match sub.as_str() {
        "show" => true,
        "set" => false,
        "-h" | "--help" => return Ok((std::mem::take(common), Cmd::Help(U_STUDIO_CAST))),
        _ => {
            return Err(usage(
                format!("unknown studio cast subcommand '{sub}'"),
                U_STUDIO_CAST,
            ));
        }
    };
    while let Some(arg) = it.next() {
        if arg == "-h" || arg == "--help" {
            return Ok((std::mem::take(common), Cmd::Help(U_STUDIO_CAST)));
        }
        if common_flag(it, common, &arg, U_STUDIO_CAST)? {
            continue;
        }
        match arg.as_str() {
            "--json" => json = true,
            "--dry-run" if !show => dry_run = true,
            "--file" if !show => file = Some(PathBuf::from(it.value("--file", U_STUDIO_CAST)?)),
            _ if !show && eq(&arg, "--file").is_some() => {
                file = eq(&arg, "--file").map(PathBuf::from)
            }
            _ => return Err(usage(format!("unexpected argument '{arg}'"), U_STUDIO_CAST)),
        }
    }
    if show {
        Ok((
            std::mem::take(common),
            Cmd::StudioCast(CastCmd::Show { json }),
        ))
    } else {
        Ok((
            std::mem::take(common),
            Cmd::StudioCast(CastCmd::Set {
                file: file.ok_or_else(|| usage("missing --file", U_STUDIO_CAST))?,
                dry_run,
                json,
            }),
        ))
    }
}

/// `studio finish [--reason <r>] [--force]`: the reason is optional
/// and open tasks refuse the finish — the check is the model's.
/// `--force` finishes a studio with no requirement and no tasks
/// (L5); the empty-studio guard is the CLI's.
pub(crate) fn studio_finish(common: &mut Common, it: &mut It) -> Result<(Common, Cmd), Fail> {
    let mut reason = String::new();
    let mut force = false;
    while let Some(arg) = it.next() {
        if arg == "-h" || arg == "--help" {
            return Ok((std::mem::take(common), Cmd::Help(U_STUDIO)));
        }
        if common_flag(it, common, &arg, U_STUDIO)? {
            continue;
        }
        match arg.as_str() {
            "--reason" => reason = it.value("--reason", U_STUDIO)?,
            _ if eq(&arg, "--reason").is_some() => {
                reason = eq(&arg, "--reason").map(String::from).unwrap_or_default()
            }
            "--force" => force = true,
            _ => return Err(usage(format!("unexpected argument '{arg}'"), U_STUDIO)),
        }
    }
    Ok((std::mem::take(common), Cmd::StudioFinish { reason, force }))
}

/// `studio reopen [--reason <r>]`: the person brings a finished studio
/// back — recasts, cast changes and new tasks work again. Refused from
/// a role terminal and by Hollywood's own command; Hollywood may ask
/// with a `reopen-studio` card.
pub(crate) fn studio_reopen(common: &mut Common, it: &mut It) -> Result<(Common, Cmd), Fail> {
    let mut reason = String::new();
    while let Some(arg) = it.next() {
        if arg == "-h" || arg == "--help" {
            return Ok((std::mem::take(common), Cmd::Help(U_STUDIO)));
        }
        if common_flag(it, common, &arg, U_STUDIO)? {
            continue;
        }
        match arg.as_str() {
            "--reason" => reason = it.value("--reason", U_STUDIO)?,
            _ if eq(&arg, "--reason").is_some() => {
                reason = eq(&arg, "--reason").map(String::from).unwrap_or_default()
            }
            _ => return Err(usage(format!("unexpected argument '{arg}'"), U_STUDIO)),
        }
    }
    Ok((std::mem::take(common), Cmd::StudioReopen { reason }))
}

/// `studio prune [<studio> | --all-finished] [--dry-run] [--force]`:
/// one finished studio, or every finished one. The scope rules are the
/// runner's — this only collects the flags.
pub(crate) fn studio_prune(common: &mut Common, it: &mut It) -> Result<(Common, Cmd), Fail> {
    let mut studio = None;
    let mut all_finished = false;
    let mut dry_run = false;
    let mut force = false;
    let mut json = false;
    while let Some(arg) = it.next() {
        if arg == "-h" || arg == "--help" {
            return Ok((std::mem::take(common), Cmd::Help(U_STUDIO)));
        }
        if common_flag(it, common, &arg, U_STUDIO)? {
            continue;
        }
        match arg.as_str() {
            "--all-finished" => all_finished = true,
            "--dry-run" => dry_run = true,
            "--force" => force = true,
            "--json" => json = true,
            _ if arg.starts_with('-') => {
                return Err(usage(format!("unexpected argument '{arg}'"), U_STUDIO));
            }
            _ if studio.is_none() => studio = Some(arg),
            _ => return Err(usage(format!("unexpected argument '{arg}'"), U_STUDIO)),
        }
    }
    if studio.is_some() && all_finished {
        return Err(usage("give a studio or --all-finished, not both", U_STUDIO));
    }
    if studio.is_none() && !all_finished {
        return Err(usage("missing studio (or --all-finished)", U_STUDIO));
    }
    Ok((
        std::mem::take(common),
        Cmd::StudioPrune {
            studio,
            all_finished,
            dry_run,
            force,
            json,
        },
    ))
}

/// `studio delete [<studio>] [--dry-run] [--force] [--json]`:
/// positional `<studio>` is optional and falls back to `--studio` /
/// `BACKLOT_STUDIO`.
pub(crate) fn studio_delete(common: &mut Common, it: &mut It) -> Result<(Common, Cmd), Fail> {
    let mut studio = None;
    let mut dry_run = false;
    let mut force = false;
    let mut json = false;
    while let Some(arg) = it.next() {
        if arg == "-h" || arg == "--help" {
            return Ok((std::mem::take(common), Cmd::Help(U_STUDIO)));
        }
        if common_flag(it, common, &arg, U_STUDIO)? {
            continue;
        }
        match arg.as_str() {
            "--dry-run" => dry_run = true,
            "--force" => force = true,
            "--json" => json = true,
            _ if arg.starts_with('-') => {
                return Err(usage(format!("unexpected argument '{arg}'"), U_STUDIO));
            }
            _ if studio.is_none() => studio = Some(arg),
            _ => return Err(usage(format!("unexpected argument '{arg}'"), U_STUDIO)),
        }
    }
    Ok((
        std::mem::take(common),
        Cmd::StudioDelete {
            studio,
            dry_run,
            force,
            json,
        },
    ))
}

/// `studio migrate [<studio> | --all] [--dry-run]` — rewrites the
/// ledger at the current schema version; appends nothing.
pub(crate) fn studio_migrate(common: &mut Common, it: &mut It) -> Result<(Common, Cmd), Fail> {
    let mut studio = None;
    let mut all = false;
    let mut dry_run = false;
    while let Some(arg) = it.next() {
        if arg == "-h" || arg == "--help" {
            return Ok((std::mem::take(common), Cmd::Help(U_STUDIO)));
        }
        if common_flag(it, common, &arg, U_STUDIO)? {
            continue;
        }
        match arg.as_str() {
            "--all" => all = true,
            "--dry-run" => dry_run = true,
            _ if arg.starts_with('-') => {
                return Err(usage(format!("unexpected argument '{arg}'"), U_STUDIO));
            }
            _ if studio.is_none() => studio = Some(arg),
            _ => return Err(usage(format!("unexpected argument '{arg}'"), U_STUDIO)),
        }
    }
    if studio.is_some() && all {
        return Err(usage("give a studio or --all, not both", U_STUDIO));
    }
    if all && common.studio.is_some() {
        return Err(usage("--all and --studio do not mix", U_STUDIO));
    }
    Ok((
        std::mem::take(common),
        Cmd::StudioMigrate {
            studio,
            all,
            dry_run,
        },
    ))
}

/// `studio list [--json] [--archived]` — every studio under every
/// core's state directory, or the ledgers under `archive/`;
/// read-only, so no studio name is required. Its own help says what
/// the default read covers and how `--remote <host>` widens it.
pub(crate) fn studio_list(common: &mut Common, it: &mut It) -> Result<(Common, Cmd), Fail> {
    let mut json = false;
    let mut archived = false;
    while let Some(arg) = it.next() {
        if arg == "-h" || arg == "--help" {
            return Ok((std::mem::take(common), Cmd::Help(U_STUDIO_LIST)));
        }
        if common_flag(it, common, &arg, U_STUDIO_LIST)? {
            continue;
        }
        match arg.as_str() {
            "--json" => json = true,
            "--archived" => archived = true,
            _ => return Err(usage(format!("unexpected argument '{arg}'"), U_STUDIO_LIST)),
        }
    }
    Ok((std::mem::take(common), Cmd::StudioList { json, archived }))
}

/// `studio restore <archived-name> [--name <new-name>] [--dry-run]`
/// (L7, M3): the positional names the `archive/<id>-<stamp>` entry.
/// The rename flag is `--name`, not `--as`: `--as` is every command's
/// caller role, so a second meaning would collide with `--as user`.
pub(crate) fn studio_restore(common: &mut Common, it: &mut It) -> Result<(Common, Cmd), Fail> {
    let mut archived = None;
    let mut new_name = None;
    let mut dry_run = false;
    let mut json = false;
    while let Some(arg) = it.next() {
        if arg == "-h" || arg == "--help" {
            return Ok((std::mem::take(common), Cmd::Help(U_STUDIO)));
        }
        if common_flag(it, common, &arg, U_STUDIO)? {
            continue;
        }
        match arg.as_str() {
            "--name" => new_name = Some(it.value("--name", U_STUDIO)?),
            _ if eq(&arg, "--name").is_some() => new_name = eq(&arg, "--name").map(String::from),
            "--dry-run" => dry_run = true,
            "--json" => json = true,
            _ if arg.starts_with('-') => {
                return Err(usage(format!("unexpected argument '{arg}'"), U_STUDIO));
            }
            _ if archived.is_none() => archived = Some(arg),
            _ => return Err(usage(format!("unexpected argument '{arg}'"), U_STUDIO)),
        }
    }
    Ok((
        std::mem::take(common),
        Cmd::StudioRestore {
            archived: archived.ok_or_else(|| usage("missing <archived-name>", U_STUDIO))?,
            new_name,
            dry_run,
            json,
        },
    ))
}

pub(crate) fn req_revise(common: &mut Common, it: &mut It) -> Result<(Common, Cmd), Fail> {
    let mut src = Source::default();
    let mut summary = None;
    while let Some(arg) = it.next() {
        if arg == "-h" || arg == "--help" {
            return Ok((std::mem::take(common), Cmd::Help(U_REQ)));
        }
        if common_flag(it, common, &arg, U_REQ)? {
            continue;
        }
        if src.flag(it, &arg, U_REQ)? {
            continue;
        }
        match arg.as_str() {
            "--summary" => summary = Some(it.value("--summary", U_REQ)?),
            _ if eq(&arg, "--summary").is_some() => {
                summary = eq(&arg, "--summary").map(String::from)
            }
            _ => return Err(usage(format!("unexpected argument '{arg}'"), U_REQ)),
        }
    }
    // `--text` exists only for `tell`; `req revise` takes `--file`
    // or `--stdin` (G2).
    if src.text.is_some() {
        return Err(usage("--text does not apply to req revise", U_REQ));
    }
    Ok((
        std::mem::take(common),
        Cmd::ReqRevise {
            source: src.one(U_REQ)?,
            summary: summary.ok_or_else(|| usage("missing --summary", U_REQ))?,
        },
    ))
}

pub(crate) fn plan_publish(common: &mut Common, it: &mut It) -> Result<(Common, Cmd), Fail> {
    let mut kind = None;
    let mut file = None;
    while let Some(arg) = it.next() {
        if arg == "-h" || arg == "--help" {
            return Ok((std::mem::take(common), Cmd::Help(U_PLAN)));
        }
        if common_flag(it, common, &arg, U_PLAN)? {
            continue;
        }
        match arg.as_str() {
            "--kind" => {
                let v = it.value("--kind", U_PLAN)?;
                kind = Some(
                    parse_plan_kind(&v)
                        .ok_or_else(|| usage(format!("bad --kind '{v}'"), U_PLAN))?,
                )
            }
            _ if eq(&arg, "--kind").is_some() => {
                let v = eq(&arg, "--kind").unwrap();
                kind = Some(
                    parse_plan_kind(v).ok_or_else(|| usage(format!("bad --kind '{v}'"), U_PLAN))?,
                )
            }
            "--file" => file = Some(PathBuf::from(it.value("--file", U_PLAN)?)),
            _ if eq(&arg, "--file").is_some() => file = eq(&arg, "--file").map(PathBuf::from),
            _ => return Err(usage(format!("unexpected argument '{arg}'"), U_PLAN)),
        }
    }
    Ok((
        std::mem::take(common),
        Cmd::PlanPublish {
            plan_kind: kind.ok_or_else(|| usage("missing --kind", U_PLAN))?,
            file: file.ok_or_else(|| usage("missing --file", U_PLAN))?,
        },
    ))
}

/// `backlot guide [<topic>] [--grep <word>]` — one optional topic
/// positional and one optional flag, either alone or together. No
/// studio is needed: the docs are compiled into the binary.
pub(crate) fn guide(common: &mut Common, it: &mut It) -> Result<(Common, Cmd), Fail> {
    let mut topic = None;
    let mut grep = None;
    while let Some(arg) = it.next() {
        if arg == "-h" || arg == "--help" {
            return Ok((std::mem::take(common), Cmd::Help(U_GUIDE)));
        }
        if common_flag(it, common, &arg, U_GUIDE)? {
            continue;
        }
        match arg.as_str() {
            "--grep" => grep = Some(it.value("--grep", U_GUIDE)?),
            _ if eq(&arg, "--grep").is_some() => grep = eq(&arg, "--grep").map(String::from),
            _ if arg.starts_with('-') => {
                return Err(usage(format!("unexpected argument '{arg}'"), U_GUIDE));
            }
            _ if topic.is_none() => topic = Some(arg),
            _ => return Err(usage(format!("unexpected argument '{arg}'"), U_GUIDE)),
        }
    }
    Ok((std::mem::take(common), Cmd::Guide { topic, grep }))
}

/// Commands taking only common flags (`stage start`, `stage run`).
pub(crate) fn common_only(
    common: &mut Common,
    it: &mut It,
    u: &'static str,
    cmd: Cmd,
) -> Result<(Common, Cmd), Fail> {
    while let Some(arg) = it.next() {
        if arg == "-h" || arg == "--help" {
            return Ok((std::mem::take(common), Cmd::Help(u)));
        }
        if common_flag(it, common, &arg, u)? {
            continue;
        }
        return Err(usage(format!("unexpected argument '{arg}'"), u));
    }
    Ok((std::mem::take(common), cmd))
}

/// `stage watch`: common flags plus an optional `--target` and
/// `--restart`.
pub(crate) fn stage_watch(common: &mut Common, it: &mut It) -> Result<(Common, Cmd), Fail> {
    let mut target = None;
    let mut restart = false;
    while let Some(arg) = it.next() {
        if arg == "-h" || arg == "--help" {
            return Ok((std::mem::take(common), Cmd::Help(U_STAGE)));
        }
        if common_flag(it, common, &arg, U_STAGE)? {
            continue;
        }
        match arg.as_str() {
            "--target" => target = Some(it.value("--target", U_STAGE)?),
            _ if eq(&arg, "--target").is_some() => target = eq(&arg, "--target").map(String::from),
            "--restart" => restart = true,
            _ => return Err(usage(format!("unexpected argument '{arg}'"), U_STAGE)),
        }
    }
    Ok((std::mem::take(common), Cmd::StageWatch { target, restart }))
}

/// `stage service`: `--print` (the default) or `--write`, the common
/// `--session`, and no `--remote` — a systemd user unit runs on the
/// local host.
pub(crate) fn stage_service(common: &mut Common, it: &mut It) -> Result<(Common, Cmd), Fail> {
    service_cmd(common, it, U_STAGE, |write| Cmd::StageService { write })
}

/// `server service`: `--print` (the default) or `--write`, the common
/// `--session`, and no `--remote`.
pub(crate) fn server_service(common: &mut Common, it: &mut It) -> Result<(Common, Cmd), Fail> {
    service_cmd(common, it, U_SERVER, |write| {
        Cmd::Server(ServerCmd::Service { write })
    })
}

fn service_cmd(
    common: &mut Common,
    it: &mut It,
    u: &'static str,
    make: impl FnOnce(bool) -> Cmd,
) -> Result<(Common, Cmd), Fail> {
    let mut print = false;
    let mut write = false;
    while let Some(arg) = it.next() {
        if arg == "-h" || arg == "--help" {
            return Ok((std::mem::take(common), Cmd::Help(u)));
        }
        if common_flag(it, common, &arg, u)? {
            continue;
        }
        match arg.as_str() {
            "--print" => print = true,
            "--write" => write = true,
            _ => return Err(usage(format!("unexpected argument '{arg}'"), u)),
        }
    }
    if print && write {
        return Err(usage("give --print or --write, not both", u));
    }
    if common.remote.is_some() {
        return Err(usage(
            "--remote does not apply: a systemd --user unit runs on the local host",
            u,
        ));
    }
    Ok((std::mem::take(common), make(write)))
}

/// `server adopt [--dry-run] [--stop-stages] [--core <id>]`.
pub(crate) fn server_adopt(common: &mut Common, it: &mut It) -> Result<(Common, Cmd), Fail> {
    let mut dry_run = false;
    let mut stop_stages = false;
    let mut core = None;
    while let Some(arg) = it.next() {
        if arg == "-h" || arg == "--help" {
            return Ok((std::mem::take(common), Cmd::Help(U_SERVER)));
        }
        if common_flag(it, common, &arg, U_SERVER)? {
            continue;
        }
        match arg.as_str() {
            "--dry-run" => dry_run = true,
            "--stop-stages" => stop_stages = true,
            "--core" => core = Some(it.value("--core", U_SERVER)?),
            _ if eq(&arg, "--core").is_some() => core = eq(&arg, "--core").map(String::from),
            _ => return Err(usage(format!("unexpected argument '{arg}'"), U_SERVER)),
        }
    }
    if common.remote.is_some() {
        return Err(usage(
            "--remote does not apply: server adopt runs on the local host",
            U_SERVER,
        ));
    }
    Ok((
        std::mem::take(common),
        Cmd::Server(ServerCmd::Adopt {
            dry_run,
            stop_stages,
            core,
        }),
    ))
}

/// Commands taking just `--file`.
pub(crate) fn one_file(
    common: &mut Common,
    it: &mut It,
    u: &'static str,
    make: impl FnOnce(PathBuf) -> Cmd,
) -> Result<(Common, Cmd), Fail> {
    let mut file = None;
    while let Some(arg) = it.next() {
        if arg == "-h" || arg == "--help" {
            return Ok((std::mem::take(common), Cmd::Help(u)));
        }
        if common_flag(it, common, &arg, u)? {
            continue;
        }
        match arg.as_str() {
            "--file" => file = Some(PathBuf::from(it.value("--file", u)?)),
            _ if eq(&arg, "--file").is_some() => file = eq(&arg, "--file").map(PathBuf::from),
            _ => return Err(usage(format!("unexpected argument '{arg}'"), u)),
        }
    }
    Ok((
        std::mem::take(common),
        make(file.ok_or_else(|| usage("missing --file", u))?),
    ))
}

pub(crate) fn report(common: &mut Common, it: &mut It) -> Result<(Common, Cmd), Fail> {
    // `report show --studio <s> --file <relpath>` (G3) is the
    // read-only stream; a first `show` word takes that branch and
    // the actor's `--node` form keeps its own parse below.
    if it.peek() == Some("show") {
        let _ = it.next();
        return report_show(common, it);
    }
    let mut node = None;
    let mut file = None;
    while let Some(arg) = it.next() {
        if arg == "-h" || arg == "--help" {
            return Ok((std::mem::take(common), Cmd::Help(U_REPORT)));
        }
        if common_flag(it, common, &arg, U_REPORT)? {
            continue;
        }
        match arg.as_str() {
            "--node" => node = Some(it.value("--node", U_REPORT)?),
            _ if eq(&arg, "--node").is_some() => node = eq(&arg, "--node").map(String::from),
            "--file" => file = Some(PathBuf::from(it.value("--file", U_REPORT)?)),
            _ if eq(&arg, "--file").is_some() => file = eq(&arg, "--file").map(PathBuf::from),
            _ => return Err(usage(format!("unexpected argument '{arg}'"), U_REPORT)),
        }
    }
    Ok((
        std::mem::take(common),
        Cmd::Report {
            node: node.ok_or_else(|| usage("missing --node", U_REPORT))?,
            file: file.ok_or_else(|| usage("missing --file", U_REPORT))?,
        },
    ))
}

/// `report show`: `--file` plus the common flags only (`--studio`
/// names the studio); `--node` is the actor form's and stays out.
fn report_show(common: &mut Common, it: &mut It) -> Result<(Common, Cmd), Fail> {
    let mut file = None;
    while let Some(arg) = it.next() {
        if arg == "-h" || arg == "--help" {
            return Ok((std::mem::take(common), Cmd::Help(U_REPORT)));
        }
        if common_flag(it, common, &arg, U_REPORT)? {
            continue;
        }
        match arg.as_str() {
            "--file" => file = Some(PathBuf::from(it.value("--file", U_REPORT)?)),
            _ if eq(&arg, "--file").is_some() => file = eq(&arg, "--file").map(PathBuf::from),
            _ => return Err(usage(format!("unexpected argument '{arg}'"), U_REPORT)),
        }
    }
    Ok((
        std::mem::take(common),
        Cmd::ReportShow {
            file: file.ok_or_else(|| usage("missing --file", U_REPORT))?,
        },
    ))
}

pub(crate) fn tell(common: &mut Common, it: &mut It) -> Result<(Common, Cmd), Fail> {
    let mut role = None;
    let mut src = Source::default();
    let mut text = None;
    let mut ask = false;
    while let Some(arg) = it.next() {
        if arg == "-h" || arg == "--help" {
            return Ok((std::mem::take(common), Cmd::Help(U_TELL)));
        }
        if common_flag(it, common, &arg, U_TELL)? {
            continue;
        }
        if src.flag(it, &arg, U_TELL)? {
            continue;
        }
        match arg.as_str() {
            "--ask" => ask = true,
            _ if arg.starts_with('-') => {
                return Err(usage(format!("unexpected argument '{arg}'"), U_TELL));
            }
            _ if role.is_none() => role = Some(arg),
            _ if text.is_none() => text = Some(arg),
            _ => return Err(usage(format!("unexpected argument '{arg}'"), U_TELL)),
        }
    }
    // `tell hollywood "<text>"` is the user's line to the overseer —
    // its own ledger, no file, no studio. `--text` answers the same
    // line without a positional word (G2).
    if role.as_deref() == Some("hollywood") {
        if src.file.is_some() || src.stdin {
            return Err(usage(
                "--file and --stdin do not apply to tell hollywood",
                U_TELL,
            ));
        }
        let line = match (text, src.text) {
            (Some(_), Some(_)) => {
                return Err(usage("give the text once, not twice", U_TELL));
            }
            (Some(t), None) | (None, Some(t)) => t,
            (None, None) => return Err(usage("missing <text>", U_TELL)),
        };
        if line.trim().is_empty() {
            return Err(usage("the text must not be empty", U_TELL));
        }
        return Ok((std::mem::take(common), Cmd::TellHollywood { text: line }));
    }
    if let Some(extra) = text {
        return Err(usage(format!("unexpected argument '{extra}'"), U_TELL));
    }
    Ok((
        std::mem::take(common),
        Cmd::Tell {
            role: role.ok_or_else(|| usage("missing role", U_TELL))?,
            source: src.one(U_TELL)?,
            ask,
        },
    ))
}

/// `progress (<line> | --file <f> | --text <msg> | --stdin)`: one-line progress note.
pub(crate) fn progress(common: &mut Common, it: &mut It) -> Result<(Common, Cmd), Fail> {
    let mut src = Source::default();
    let mut line = None;
    while let Some(arg) = it.next() {
        if arg == "-h" || arg == "--help" {
            return Ok((std::mem::take(common), Cmd::Help(U_PROGRESS)));
        }
        if common_flag(it, common, &arg, U_PROGRESS)? {
            continue;
        }
        if src.flag(it, &arg, U_PROGRESS)? {
            continue;
        }
        match arg.as_str() {
            _ if arg.starts_with('-') => {
                return Err(usage(format!("unexpected argument '{arg}'"), U_PROGRESS));
            }
            _ if line.is_none() => line = Some(arg),
            _ => return Err(usage(format!("unexpected argument '{arg}'"), U_PROGRESS)),
        }
    }
    let has_flag_src = src.file.is_some() || src.text.is_some() || src.stdin;
    let source = match (line, has_flag_src) {
        (Some(_), true) => {
            return Err(usage(
                "give a positional line or --file/--text/--stdin, not both",
                U_PROGRESS,
            ));
        }
        (Some(text), false) => {
            if text.trim().is_empty() {
                return Err(usage("progress line must not be empty", U_PROGRESS));
            }
            TextSource::Text(text)
        }
        (None, _) => src.one(U_PROGRESS)?,
    };
    Ok((std::mem::take(common), Cmd::Progress { source }))
}

/// `runners [--json]` — the detection table or its G4 document.
pub(crate) fn runners(common: &mut Common, it: &mut It) -> Result<(Common, Cmd), Fail> {
    let mut json = false;
    while let Some(arg) = it.next() {
        if arg == "-h" || arg == "--help" {
            return Ok((std::mem::take(common), Cmd::Help(U_RUNNERS)));
        }
        if common_flag(it, common, &arg, U_RUNNERS)? {
            continue;
        }
        match arg.as_str() {
            "--json" => json = true,
            _ => return Err(usage(format!("unexpected argument '{arg}'"), U_RUNNERS)),
        }
    }
    Ok((std::mem::take(common), Cmd::Runners { json }))
}

/// `repos [--json] [--from <dir>] [--all]` — the G4 directory listing.
/// Dot-directories are hidden unless `--all` asks for them.
pub(crate) fn repos(common: &mut Common, it: &mut It) -> Result<(Common, Cmd), Fail> {
    let mut json = false;
    let mut from = None;
    let mut all = false;
    while let Some(arg) = it.next() {
        if arg == "-h" || arg == "--help" {
            return Ok((std::mem::take(common), Cmd::Help(U_REPOS)));
        }
        if common_flag(it, common, &arg, U_REPOS)? {
            continue;
        }
        match arg.as_str() {
            "--json" => json = true,
            "--all" => all = true,
            "--from" => from = Some(it.value("--from", U_REPOS)?),
            _ if eq(&arg, "--from").is_some() => from = eq(&arg, "--from").map(String::from),
            _ => return Err(usage(format!("unexpected argument '{arg}'"), U_REPOS)),
        }
    }
    Ok((std::mem::take(common), Cmd::Repos { json, from, all }))
}

/// `ask --file <f>` or `ask --run [--why <text>] (-- <prog> <args...> | "<cmd>")`.
pub(crate) fn ask(common: &mut Common, it: &mut It) -> Result<(Common, Cmd), Fail> {
    // `ask acknowledged --person <role> --target <seq>` is the user's
    // receipt for a person question — not the role's `ask --file` flow.
    if it
        .peek()
        .is_some_and(|a| a == "acknowledged" || a == "acknowledge")
    {
        it.next();
        let mut role = None;
        let mut target = None;
        while let Some(arg) = it.next() {
            if common_flag(it, common, &arg, U_ASK)? {
                continue;
            }
            match arg.as_str() {
                "--person" => role = Some(it.value("--person", U_ASK)?),
                "--target" => {
                    target =
                        Some(it.value("--target", U_ASK)?.parse::<u64>().map_err(|_| {
                            usage("--target must be a message sequence number", U_ASK)
                        })?)
                }
                _ if eq(&arg, "--person").is_some() => {
                    role = eq(&arg, "--person").map(String::from)
                }
                _ if eq(&arg, "--target").is_some() => {
                    target = eq(&arg, "--target").and_then(|v| v.parse::<u64>().ok())
                }
                _ => return Err(usage(format!("unexpected argument '{arg}'"), U_ASK)),
            }
        }
        return Ok((
            std::mem::take(common),
            Cmd::AskAcknowledge {
                role: role.ok_or_else(|| usage("missing --person", U_ASK))?,
                target: target.ok_or_else(|| usage("missing or bad --target", U_ASK))?,
            },
        ));
    }
    let mut file = None;
    let mut run = false;
    let mut why = None;
    let mut raw_cmd = None;
    let mut run_argv = Vec::new();
    while let Some(arg) = it.next() {
        if arg == "--" {
            if !run {
                return Err(usage(
                    "unexpected `--` (pass --run for outward commands)",
                    U_ASK,
                ));
            }
            while let Some(a) = it.next() {
                run_argv.push(a);
            }
            break;
        }
        if arg == "-h" || arg == "--help" {
            return Ok((std::mem::take(common), Cmd::Help(U_ASK)));
        }
        if common_flag(it, common, &arg, U_ASK)? {
            continue;
        }
        match arg.as_str() {
            "--file" => file = Some(PathBuf::from(it.value("--file", U_ASK)?)),
            _ if eq(&arg, "--file").is_some() => file = eq(&arg, "--file").map(PathBuf::from),
            "--run" => run = true,
            "--why" => why = Some(it.value("--why", U_ASK)?),
            _ if eq(&arg, "--why").is_some() => why = eq(&arg, "--why").map(String::from),
            _ if run && raw_cmd.is_none() && !arg.starts_with('-') => raw_cmd = Some(arg),
            _ => return Err(usage(format!("unexpected argument '{arg}'"), U_ASK)),
        }
    }
    if file.is_some() && run {
        return Err(usage("pick one of --file or --run", U_ASK));
    }
    if !run {
        let file = file.ok_or_else(|| usage("missing --file", U_ASK))?;
        return Ok((std::mem::take(common), Cmd::Ask { file }));
    }
    if let Some(ref w) = why
        && w.trim().is_empty()
    {
        return Err(usage("--why must not be empty", U_ASK));
    }
    let argv = if !run_argv.is_empty() {
        if raw_cmd.is_some() {
            return Err(usage(
                "pick either `-- <prog> ...` or a command string, not both",
                U_ASK,
            ));
        }
        run_argv
    } else if let Some(cmd_str) = raw_cmd {
        let parsed = crate::guard_shell_lex::split_shell_segments_checked(&cmd_str)
            .map_err(|r| Fail::refused(format!("invalid outward command: {r}")))?;
        if !parsed.nested.is_empty() || parsed.segments.len() != 1 {
            return Err(Fail::refused("ask --run expects a single simple command"));
        }
        parsed.segments.into_iter().next().unwrap_or_default()
    } else {
        return Err(usage("missing command after --run", U_ASK));
    };
    if argv.is_empty() {
        return Err(usage("missing command after --run", U_ASK));
    }
    Ok((
        std::mem::take(common),
        Cmd::AskRun {
            argv,
            why,
            cwd: None,
            command: None,
        },
    ))
}
