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
    let mut dry_run = false;
    let mut interactive = false;
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
            "--dry-run" => dry_run = true,
            "--interactive" => interactive = true,
            _ => return Err(usage(format!("unexpected argument '{arg}'"), U_STUDIO)),
        }
    }
    let cmd = match interactive {
        // The wizard resolves whatever the flags left unset — the
        // only pre-checks here are the ones that always hold.
        true => Cmd::StudioNew {
            title,
            repo: repo.map(|r| repo_path(&r)).transpose()?,
            cast_file,
            dry_run,
            interactive: true,
        },
        false => Cmd::StudioNew {
            title: Some(title.ok_or_else(|| usage("missing --title", U_STUDIO))?),
            repo: Some(repo_path(
                &repo.ok_or_else(|| usage("missing --repo", U_STUDIO))?,
            )?),
            cast_file: Some(cast_file.ok_or_else(|| usage("missing --cast", U_STUDIO))?),
            dry_run,
            interactive: false,
        },
    };
    Ok((std::mem::take(common), cmd))
}

/// `--repo` must be absolute: a relative path would bind the studio
/// to whatever cwd the stage happened to run from. An existing path
/// is canonicalized so `..` and symlink spellings of one folder
/// cannot masquerade as two; a path that does not exist yet is kept
/// as spelled.
pub(crate) fn repo_path(repo: &str) -> Result<String, Fail> {
    let path = Path::new(repo);
    if !path.is_absolute() {
        return Err(usage(
            format!("--repo must be an absolute path, got '{repo}'"),
            U_STUDIO,
        ));
    }
    Ok(match path.canonicalize() {
        Ok(p) => p.to_string_lossy().into_owned(),
        Err(_) => repo.into(),
    })
}

/// `studio finish`: an optional `--reason`, refused while any task
/// is open — the check is the model's, this only collects the text.
pub(crate) fn studio_finish(common: &mut Common, it: &mut It) -> Result<(Common, Cmd), Fail> {
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
    Ok((std::mem::take(common), Cmd::StudioFinish { reason }))
}

/// `studio prune [<studio> | --all-finished] [--dry-run] [--force]`:
/// one finished studio, or every finished one. The scope rules are the
/// runner's — this only collects the flags.
pub(crate) fn studio_prune(common: &mut Common, it: &mut It) -> Result<(Common, Cmd), Fail> {
    let mut studio = None;
    let mut all_finished = false;
    let mut dry_run = false;
    let mut force = false;
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
    let mut print = false;
    let mut write = false;
    while let Some(arg) = it.next() {
        if arg == "-h" || arg == "--help" {
            return Ok((std::mem::take(common), Cmd::Help(U_STAGE)));
        }
        if common_flag(it, common, &arg, U_STAGE)? {
            continue;
        }
        match arg.as_str() {
            "--print" => print = true,
            "--write" => write = true,
            _ => return Err(usage(format!("unexpected argument '{arg}'"), U_STAGE)),
        }
    }
    if print && write {
        return Err(usage("give --print or --write, not both", U_STAGE));
    }
    if common.remote.is_some() {
        return Err(usage(
            "--remote does not apply: a systemd --user unit runs on the local host",
            U_STAGE,
        ));
    }
    Ok((std::mem::take(common), Cmd::StageService { write }))
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
        },
    ))
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

/// `repos [--json] [--from <dir>]` — the G4 directory listing.
pub(crate) fn repos(common: &mut Common, it: &mut It) -> Result<(Common, Cmd), Fail> {
    let mut json = false;
    let mut from = None;
    while let Some(arg) = it.next() {
        if arg == "-h" || arg == "--help" {
            return Ok((std::mem::take(common), Cmd::Help(U_REPOS)));
        }
        if common_flag(it, common, &arg, U_REPOS)? {
            continue;
        }
        match arg.as_str() {
            "--json" => json = true,
            "--from" => from = Some(it.value("--from", U_REPOS)?),
            _ if eq(&arg, "--from").is_some() => from = eq(&arg, "--from").map(String::from),
            _ => return Err(usage(format!("unexpected argument '{arg}'"), U_REPOS)),
        }
    }
    Ok((std::mem::take(common), Cmd::Repos { json, from }))
}
