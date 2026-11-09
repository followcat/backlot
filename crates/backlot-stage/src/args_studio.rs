//! Parsers for the studio-level commands: `studio new`, `req revise`,
//! `plan publish`, and the one-file message commands.

use std::path::PathBuf;

use backlot_model::correct::parse_plan_kind;

use crate::args::*;
use crate::cli::Fail;

pub(crate) fn studio_new(common: &mut Common, it: &mut It) -> Result<(Common, Cmd), Fail> {
    let mut title = None;
    let mut repo = None;
    let mut cast_file = None;
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
            _ => return Err(usage(format!("unexpected argument '{arg}'"), U_STUDIO)),
        }
    }
    let cmd = Cmd::StudioNew {
        title: title.ok_or_else(|| usage("missing --title", U_STUDIO))?,
        repo: repo.ok_or_else(|| usage("missing --repo", U_STUDIO))?,
        cast_file: cast_file.ok_or_else(|| usage("missing --cast", U_STUDIO))?,
    };
    Ok((std::mem::take(common), cmd))
}

pub(crate) fn req_revise(common: &mut Common, it: &mut It) -> Result<(Common, Cmd), Fail> {
    let mut file = None;
    let mut summary = None;
    while let Some(arg) = it.next() {
        if arg == "-h" || arg == "--help" {
            return Ok((std::mem::take(common), Cmd::Help(U_REQ)));
        }
        if common_flag(it, common, &arg, U_REQ)? {
            continue;
        }
        match arg.as_str() {
            "--file" => file = Some(PathBuf::from(it.value("--file", U_REQ)?)),
            _ if eq(&arg, "--file").is_some() => file = eq(&arg, "--file").map(PathBuf::from),
            "--summary" => summary = Some(it.value("--summary", U_REQ)?),
            _ if eq(&arg, "--summary").is_some() => {
                summary = eq(&arg, "--summary").map(String::from)
            }
            _ => return Err(usage(format!("unexpected argument '{arg}'"), U_REQ)),
        }
    }
    Ok((
        std::mem::take(common),
        Cmd::ReqRevise {
            file: file.ok_or_else(|| usage("missing --file", U_REQ))?,
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

pub(crate) fn tell(common: &mut Common, it: &mut It) -> Result<(Common, Cmd), Fail> {
    let mut role = None;
    let mut file = None;
    while let Some(arg) = it.next() {
        if arg == "-h" || arg == "--help" {
            return Ok((std::mem::take(common), Cmd::Help(U_TELL)));
        }
        if common_flag(it, common, &arg, U_TELL)? {
            continue;
        }
        match arg.as_str() {
            "--file" => file = Some(PathBuf::from(it.value("--file", U_TELL)?)),
            _ if eq(&arg, "--file").is_some() => file = eq(&arg, "--file").map(PathBuf::from),
            _ if arg.starts_with('-') => {
                return Err(usage(format!("unexpected argument '{arg}'"), U_TELL));
            }
            _ if role.is_none() => role = Some(arg),
            _ => return Err(usage(format!("unexpected argument '{arg}'"), U_TELL)),
        }
    }
    Ok((
        std::mem::take(common),
        Cmd::Tell {
            role: role.ok_or_else(|| usage("missing role", U_TELL))?,
            file: file.ok_or_else(|| usage("missing --file", U_TELL))?,
        },
    ))
}
