//! Argument parser for `backlot hollywood switch`.

use super::{HollywoodCmd, U_HOLLYWOOD};
use crate::args::*;
use crate::cli::Fail;

pub(super) fn hswitch_args(common: &mut Common, it: &mut It) -> Result<(Common, Cmd), Fail> {
    let mut runner = None;
    let mut model = None;
    let mut effort = None;
    let mut timeout_s = None;
    let mut force = false;
    let mut no_wait = false;
    let mut json = false;
    let mut cancel = false;
    let mut status = false;
    while let Some(arg) = it.next() {
        if arg == "-h" || arg == "--help" {
            return Ok((std::mem::take(common), Cmd::Help(U_HOLLYWOOD)));
        }
        if common_flag(it, common, &arg, U_HOLLYWOOD)? {
            continue;
        }
        match arg.as_str() {
            "--runner" => runner = Some(it.value("--runner", U_HOLLYWOOD)?),
            _ if eq(&arg, "--runner").is_some() => runner = eq(&arg, "--runner").map(String::from),
            "--model" => model = Some(it.value("--model", U_HOLLYWOOD)?),
            _ if eq(&arg, "--model").is_some() => model = eq(&arg, "--model").map(String::from),
            "--effort" => effort = Some(it.value("--effort", U_HOLLYWOOD)?),
            _ if eq(&arg, "--effort").is_some() => effort = eq(&arg, "--effort").map(String::from),
            "--timeout" => {
                let raw = it.value("--timeout", U_HOLLYWOOD)?;
                timeout_s = Some(parse_timeout(&raw)?);
            }
            _ if eq(&arg, "--timeout").is_some() => {
                timeout_s = Some(parse_timeout(eq(&arg, "--timeout").unwrap())?);
            }
            "--force" => force = true,
            "--no-wait" => no_wait = true,
            "--json" => json = true,
            "--cancel" => cancel = true,
            "--status" => status = true,
            _ => return Err(usage(format!("unexpected argument '{arg}'"), U_HOLLYWOOD)),
        }
    }
    if cancel && status {
        return Err(usage(
            "--cancel and --status cannot be combined",
            U_HOLLYWOOD,
        ));
    }
    if (cancel || status)
        && (runner.is_some()
            || model.is_some()
            || effort.is_some()
            || timeout_s.is_some()
            || force
            || no_wait)
    {
        return Err(usage(
            "--cancel/--status cannot be combined with switch target flags",
            U_HOLLYWOOD,
        ));
    }
    if cancel {
        return Ok((
            std::mem::take(common),
            Cmd::Hollywood(HollywoodCmd::SwitchCancel { json }),
        ));
    }
    if status {
        return Ok((
            std::mem::take(common),
            Cmd::Hollywood(HollywoodCmd::SwitchStatus { json }),
        ));
    }
    Ok((
        std::mem::take(common),
        Cmd::Hollywood(HollywoodCmd::Switch {
            runner: runner.ok_or_else(|| usage("missing --runner", U_HOLLYWOOD))?,
            model: model.ok_or_else(|| usage("missing --model", U_HOLLYWOOD))?,
            effort,
            timeout_s,
            force,
            no_wait,
            json,
        }),
    ))
}

fn parse_timeout(raw: &str) -> Result<u64, Fail> {
    let secs = raw.parse::<u64>().map_err(|_| {
        usage(
            format!("bad --timeout '{raw}', want positive seconds"),
            U_HOLLYWOOD,
        )
    })?;
    if secs == 0 {
        return Err(usage("--timeout must be at least 1 second", U_HOLLYWOOD));
    }
    Ok(secs)
}
