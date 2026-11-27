//! Parsers for `backlot studio check add|remove` (user-only).

use std::path::PathBuf;

use crate::args::{Common, It, common_flag, eq, usage};
use crate::args_usage::U_STUDIO_CHECK;
use crate::cli::Fail;

/// Subcommands under `backlot studio check`.
#[derive(Debug)]
pub enum CheckCmd {
    Add {
        name: String,
        run: String,
        timeout_s: Option<u64>,
        expect: Option<String>,
        min_matches: Option<u32>,
        preflight: bool,
    },
    Remove {
        name: String,
    },
}

/// Parse `backlot studio check add|remove ...`.
pub(crate) fn studio_check(common: &mut Common, it: &mut It) -> Result<(Common, crate::args::Cmd), Fail> {
    let Some(sub) = it.next() else {
        return Err(usage("missing studio check subcommand", U_STUDIO_CHECK));
    };
    match sub.as_str() {
        "add" => parse_add(common, it),
        "remove" => parse_remove(common, it),
        "-h" | "--help" => Ok((std::mem::take(common), crate::args::Cmd::Help(U_STUDIO_CHECK))),
        _ => Err(usage(
            format!("unknown studio check subcommand '{sub}'"),
            U_STUDIO_CHECK,
        )),
    }
}

fn parse_add(common: &mut Common, it: &mut It) -> Result<(Common, crate::args::Cmd), Fail> {
    let mut name: Option<String> = None;
    let mut run: Option<String> = None;
    let mut timeout_s: Option<u64> = None;
    let mut expect: Option<String> = None;
    let mut min_matches: Option<u32> = None;
    let mut preflight = false;
    while let Some(arg) = it.next() {
        if arg == "-h" || arg == "--help" {
            return Ok((std::mem::take(common), crate::args::Cmd::Help(U_STUDIO_CHECK)));
        }
        if common_flag(it, common, &arg, U_STUDIO_CHECK)? {
            continue;
        }
        match arg.as_str() {
            "--name" => name = Some(it.value("--name", U_STUDIO_CHECK)?),
            _ if eq(&arg, "--name").is_some() => name = eq(&arg, "--name").map(String::from),
            "--run" => run = Some(it.value("--run", U_STUDIO_CHECK)?),
            _ if eq(&arg, "--run").is_some() => run = eq(&arg, "--run").map(String::from),
            "--timeout" => {
                timeout_s = Some(parse_u64(&it.value("--timeout", U_STUDIO_CHECK)?, "--timeout")?);
            }
            _ if eq(&arg, "--timeout").is_some() => {
                timeout_s = Some(parse_u64(eq(&arg, "--timeout").unwrap(), "--timeout")?);
            }
            "--expect" => expect = Some(it.value("--expect", U_STUDIO_CHECK)?),
            _ if eq(&arg, "--expect").is_some() => expect = eq(&arg, "--expect").map(String::from),
            "--min-matches" => {
                min_matches = Some(parse_u32(
                    &it.value("--min-matches", U_STUDIO_CHECK)?,
                    "--min-matches",
                )?);
            }
            _ if eq(&arg, "--min-matches").is_some() => {
                min_matches = Some(parse_u32(
                    eq(&arg, "--min-matches").unwrap(),
                    "--min-matches",
                )?);
            }
            "--preflight" => preflight = true,
            _ => return Err(usage(format!("unexpected argument '{arg}'"), U_STUDIO_CHECK)),
        }
    }
    let _ = PathBuf::new();
    Ok((
        std::mem::take(common),
        crate::args::Cmd::StudioCheck(CheckCmd::Add {
            name: name.ok_or_else(|| usage("missing --name", U_STUDIO_CHECK))?,
            run: run.ok_or_else(|| usage("missing --run", U_STUDIO_CHECK))?,
            timeout_s,
            expect,
            min_matches,
            preflight,
        }),
    ))
}

fn parse_remove(common: &mut Common, it: &mut It) -> Result<(Common, crate::args::Cmd), Fail> {
    let mut name: Option<String> = None;
    while let Some(arg) = it.next() {
        if arg == "-h" || arg == "--help" {
            return Ok((std::mem::take(common), crate::args::Cmd::Help(U_STUDIO_CHECK)));
        }
        if common_flag(it, common, &arg, U_STUDIO_CHECK)? {
            continue;
        }
        match arg.as_str() {
            "--name" => name = Some(it.value("--name", U_STUDIO_CHECK)?),
            _ if eq(&arg, "--name").is_some() => name = eq(&arg, "--name").map(String::from),
            _ => return Err(usage(format!("unexpected argument '{arg}'"), U_STUDIO_CHECK)),
        }
    }
    Ok((
        std::mem::take(common),
        crate::args::Cmd::StudioCheck(CheckCmd::Remove {
            name: name.ok_or_else(|| usage("missing --name", U_STUDIO_CHECK))?,
        }),
    ))
}

fn parse_u64(raw: &str, flag: &str) -> Result<u64, Fail> {
    raw.parse::<u64>()
        .map_err(|_| usage(format!("bad {flag} '{raw}'"), U_STUDIO_CHECK))
}

fn parse_u32(raw: &str, flag: &str) -> Result<u32, Fail> {
    raw.parse::<u32>()
        .map_err(|_| usage(format!("bad {flag} '{raw}'"), U_STUDIO_CHECK))
}
