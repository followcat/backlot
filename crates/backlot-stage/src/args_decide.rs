//! Argument parsers for `backlot decide` and `backlot repo-note`.

use std::path::PathBuf;

use crate::args::*;
use crate::cli::Fail;

/// `decide --id AD-<n> --file <path> [--supersedes AD-<m>] [--notify <roles>|active-actors]`
pub(crate) fn decide(common: &mut Common, it: &mut It) -> Result<(Common, Cmd), Fail> {
    let mut id = None;
    let mut file = None;
    let mut supersedes = None;
    let mut notify = None;
    while let Some(arg) = it.next() {
        if arg == "-h" || arg == "--help" {
            return Ok((std::mem::take(common), Cmd::Help(U_DECIDE)));
        }
        if common_flag(it, common, &arg, U_DECIDE)? {
            continue;
        }
        match arg.as_str() {
            "--id" => id = Some(it.value("--id", U_DECIDE)?),
            _ if eq(&arg, "--id").is_some() => id = eq(&arg, "--id").map(String::from),
            "--file" => file = Some(PathBuf::from(it.value("--file", U_DECIDE)?)),
            _ if eq(&arg, "--file").is_some() => file = eq(&arg, "--file").map(PathBuf::from),
            "--supersedes" => supersedes = Some(it.value("--supersedes", U_DECIDE)?),
            _ if eq(&arg, "--supersedes").is_some() => {
                supersedes = eq(&arg, "--supersedes").map(String::from)
            }
            "--notify" => notify = Some(it.value("--notify", U_DECIDE)?),
            _ if eq(&arg, "--notify").is_some() => notify = eq(&arg, "--notify").map(String::from),
            _ => return Err(usage(format!("unexpected argument '{arg}'"), U_DECIDE)),
        }
    }
    let id = id.ok_or_else(|| usage("missing --id", U_DECIDE))?;
    if backlot_model::decisions::parse_decision_id(&id).is_none() {
        return Err(usage(
            format!("invalid --id '{id}': expected AD-<n>"),
            U_DECIDE,
        ));
    }
    if let Some(ref sup) = supersedes {
        if backlot_model::decisions::parse_decision_id(sup).is_none() {
            return Err(usage(
                format!("invalid --supersedes '{sup}': expected AD-<m>"),
                U_DECIDE,
            ));
        }
        if sup == &id {
            return Err(usage(format!("{id} cannot supersede itself"), U_DECIDE));
        }
    }
    if let Some(ref raw) = notify
        && (raw.trim().is_empty() || raw.split(',').any(|p| p.trim().is_empty()))
    {
        return Err(usage("--notify must list non-empty roles", U_DECIDE));
    }
    Ok((
        std::mem::take(common),
        Cmd::Decide {
            id,
            file: file.ok_or_else(|| usage("missing --file", U_DECIDE))?,
            supersedes,
            notify,
        },
    ))
}

/// `repo-note propose|show|remove`
pub(crate) fn repo_note(common: &mut Common, it: &mut It) -> Result<(Common, Cmd), Fail> {
    let Some(sub) = it.next() else {
        return Err(usage("missing repo-note subcommand", U_REPO_NOTE));
    };
    if sub == "-h" || sub == "--help" {
        return Ok((std::mem::take(common), Cmd::Help(U_REPO_NOTE)));
    }
    match sub.as_str() {
        "propose" => {
            let mut id = None;
            let mut text = None;
            let mut why = None;
            while let Some(arg) = it.next() {
                if arg == "-h" || arg == "--help" {
                    return Ok((std::mem::take(common), Cmd::Help(U_REPO_NOTE)));
                }
                if common_flag(it, common, &arg, U_REPO_NOTE)? {
                    continue;
                }
                match arg.as_str() {
                    "--id" => id = Some(it.value("--id", U_REPO_NOTE)?),
                    _ if eq(&arg, "--id").is_some() => id = eq(&arg, "--id").map(String::from),
                    "--text" => text = Some(it.value("--text", U_REPO_NOTE)?),
                    _ if eq(&arg, "--text").is_some() => {
                        text = eq(&arg, "--text").map(String::from)
                    }
                    "--why" => why = Some(it.value("--why", U_REPO_NOTE)?),
                    _ if eq(&arg, "--why").is_some() => why = eq(&arg, "--why").map(String::from),
                    _ => return Err(usage(format!("unexpected argument '{arg}'"), U_REPO_NOTE)),
                }
            }
            Ok((
                std::mem::take(common),
                Cmd::RepoNote(RepoNoteCmd::Propose {
                    id: id.ok_or_else(|| usage("missing --id", U_REPO_NOTE))?,
                    text: text.ok_or_else(|| usage("missing --text", U_REPO_NOTE))?,
                    why: why.ok_or_else(|| usage("missing --why", U_REPO_NOTE))?,
                }),
            ))
        }
        "show" => {
            let mut repo = None;
            while let Some(arg) = it.next() {
                if arg == "-h" || arg == "--help" {
                    return Ok((std::mem::take(common), Cmd::Help(U_REPO_NOTE)));
                }
                if common_flag(it, common, &arg, U_REPO_NOTE)? {
                    continue;
                }
                match arg.as_str() {
                    "--repo" => repo = Some(it.value("--repo", U_REPO_NOTE)?),
                    _ if eq(&arg, "--repo").is_some() => {
                        repo = eq(&arg, "--repo").map(String::from)
                    }
                    _ => return Err(usage(format!("unexpected argument '{arg}'"), U_REPO_NOTE)),
                }
            }
            Ok((
                std::mem::take(common),
                Cmd::RepoNote(RepoNoteCmd::Show { repo }),
            ))
        }
        "remove" => {
            let mut id = None;
            let mut repo = None;
            while let Some(arg) = it.next() {
                if arg == "-h" || arg == "--help" {
                    return Ok((std::mem::take(common), Cmd::Help(U_REPO_NOTE)));
                }
                if common_flag(it, common, &arg, U_REPO_NOTE)? {
                    continue;
                }
                match arg.as_str() {
                    "--id" => id = Some(it.value("--id", U_REPO_NOTE)?),
                    _ if eq(&arg, "--id").is_some() => id = eq(&arg, "--id").map(String::from),
                    "--repo" => repo = Some(it.value("--repo", U_REPO_NOTE)?),
                    _ if eq(&arg, "--repo").is_some() => {
                        repo = eq(&arg, "--repo").map(String::from)
                    }
                    _ => return Err(usage(format!("unexpected argument '{arg}'"), U_REPO_NOTE)),
                }
            }
            Ok((
                std::mem::take(common),
                Cmd::RepoNote(RepoNoteCmd::Remove {
                    id: id.ok_or_else(|| usage("missing --id", U_REPO_NOTE))?,
                    repo,
                }),
            ))
        }
        _ => Err(usage(
            format!("unknown repo-note subcommand '{sub}'"),
            U_REPO_NOTE,
        )),
    }
}
