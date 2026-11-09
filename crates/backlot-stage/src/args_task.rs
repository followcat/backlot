//! Parsers for the task-side commands: `task new`, `cast`, `deliver`,
//! `block`, `review`, `correct`, plus the `--json`-only flag pair.

use std::collections::BTreeMap;
use std::path::PathBuf;

use backlot_model::correct::parse_verdict;
use backlot_model::event::{ApprovalDecision, Priority, TaskStateChange};

use crate::args::*;
use crate::cli::Fail;

pub(crate) fn task_new(common: &mut Common, it: &mut It) -> Result<(Common, Cmd), Fail> {
    let mut title = None;
    let mut task_kind = None;
    let mut acceptance = None;
    let mut deps = Vec::new();
    while let Some(arg) = it.next() {
        if arg == "-h" || arg == "--help" {
            return Ok((std::mem::take(common), Cmd::Help(U_TASK)));
        }
        if common_flag(it, common, &arg, U_TASK)? {
            continue;
        }
        match arg.as_str() {
            "--title" => title = Some(it.value("--title", U_TASK)?),
            _ if eq(&arg, "--title").is_some() => title = eq(&arg, "--title").map(String::from),
            "--kind" => task_kind = Some(it.value("--kind", U_TASK)?),
            _ if eq(&arg, "--kind").is_some() => task_kind = eq(&arg, "--kind").map(String::from),
            "--acceptance-file" => {
                acceptance = Some(PathBuf::from(it.value("--acceptance-file", U_TASK)?))
            }
            _ if eq(&arg, "--acceptance-file").is_some() => {
                acceptance = eq(&arg, "--acceptance-file").map(PathBuf::from)
            }
            "--depends-on" => deps.push(it.value("--depends-on", U_TASK)?),
            _ if eq(&arg, "--depends-on").is_some() => {
                deps.push(eq(&arg, "--depends-on").unwrap().to_string())
            }
            _ => return Err(usage(format!("unexpected argument '{arg}'"), U_TASK)),
        }
    }
    Ok((
        std::mem::take(common),
        Cmd::TaskNew {
            title: title.ok_or_else(|| usage("missing --title", U_TASK))?,
            task_kind: task_kind.ok_or_else(|| usage("missing --kind", U_TASK))?,
            acceptance_file: acceptance
                .ok_or_else(|| usage("missing --acceptance-file", U_TASK))?,
            depends_on: deps,
        },
    ))
}

/// `task pause|resume|abandon|takeover`: one `--reason` on top of the
/// common `--task`.
pub(crate) fn task_state(
    common: &mut Common,
    it: &mut It,
    change: TaskStateChange,
) -> Result<(Common, Cmd), Fail> {
    let mut reason = None;
    while let Some(arg) = it.next() {
        if arg == "-h" || arg == "--help" {
            return Ok((std::mem::take(common), Cmd::Help(U_TASK)));
        }
        if common_flag(it, common, &arg, U_TASK)? {
            continue;
        }
        match arg.as_str() {
            "--reason" => reason = Some(it.value("--reason", U_TASK)?),
            _ if eq(&arg, "--reason").is_some() => reason = eq(&arg, "--reason").map(String::from),
            _ => return Err(usage(format!("unexpected argument '{arg}'"), U_TASK)),
        }
    }
    Ok((
        std::mem::take(common),
        Cmd::TaskState {
            change,
            reason: reason.ok_or_else(|| usage("missing --reason", U_TASK))?,
        },
    ))
}

/// `low|normal|high` for `--priority` and `--level`.
fn parse_level(v: &str, u: &'static str) -> Result<Priority, Fail> {
    v.parse::<Priority>()
        .map_err(|_| usage(format!("bad level '{v}', want low|normal|high"), u))
}

pub(crate) fn cast(common: &mut Common, it: &mut It) -> Result<(Common, Cmd), Fail> {
    let mut actor = None;
    let mut reason = None;
    let mut priority = None;
    while let Some(arg) = it.next() {
        if arg == "-h" || arg == "--help" {
            return Ok((std::mem::take(common), Cmd::Help(U_CAST)));
        }
        if common_flag(it, common, &arg, U_CAST)? {
            continue;
        }
        match arg.as_str() {
            "--actor" => actor = Some(it.value("--actor", U_CAST)?),
            _ if eq(&arg, "--actor").is_some() => actor = eq(&arg, "--actor").map(String::from),
            "--reason" => reason = Some(it.value("--reason", U_CAST)?),
            _ if eq(&arg, "--reason").is_some() => reason = eq(&arg, "--reason").map(String::from),
            "--priority" => priority = Some(parse_level(&it.value("--priority", U_CAST)?, U_CAST)?),
            _ if eq(&arg, "--priority").is_some() => {
                priority = Some(parse_level(eq(&arg, "--priority").unwrap(), U_CAST)?)
            }
            _ => return Err(usage(format!("unexpected argument '{arg}'"), U_CAST)),
        }
    }
    Ok((
        std::mem::take(common),
        Cmd::Cast {
            actor: actor.ok_or_else(|| usage("missing --actor", U_CAST))?,
            reason: reason.unwrap_or_default(),
            priority,
        },
    ))
}

/// `reopen`/`close --person <role key>`: common flags plus the key.
pub(crate) fn person(
    common: &mut Common,
    it: &mut It,
    u: &'static str,
    make: impl FnOnce(String) -> Cmd,
) -> Result<(Common, Cmd), Fail> {
    let mut person = None;
    while let Some(arg) = it.next() {
        if arg == "-h" || arg == "--help" {
            return Ok((std::mem::take(common), Cmd::Help(u)));
        }
        if common_flag(it, common, &arg, u)? {
            continue;
        }
        match arg.as_str() {
            "--person" => person = Some(it.value("--person", u)?),
            _ if eq(&arg, "--person").is_some() => person = eq(&arg, "--person").map(String::from),
            _ => return Err(usage(format!("unexpected argument '{arg}'"), u)),
        }
    }
    Ok((
        std::mem::take(common),
        make(person.ok_or_else(|| usage("missing --person", u))?),
    ))
}

/// `priority --level`: the task is the common `--task`.
pub(crate) fn priority(common: &mut Common, it: &mut It) -> Result<(Common, Cmd), Fail> {
    let mut level = None;
    while let Some(arg) = it.next() {
        if arg == "-h" || arg == "--help" {
            return Ok((std::mem::take(common), Cmd::Help(U_PRIORITY)));
        }
        if common_flag(it, common, &arg, U_PRIORITY)? {
            continue;
        }
        match arg.as_str() {
            "--level" => level = Some(parse_level(&it.value("--level", U_PRIORITY)?, U_PRIORITY)?),
            _ if eq(&arg, "--level").is_some() => {
                level = Some(parse_level(eq(&arg, "--level").unwrap(), U_PRIORITY)?)
            }
            _ => return Err(usage(format!("unexpected argument '{arg}'"), U_PRIORITY)),
        }
    }
    Ok((
        std::mem::take(common),
        Cmd::Priority {
            level: level.ok_or_else(|| usage("missing --level", U_PRIORITY))?,
        },
    ))
}

pub(crate) fn deliver(common: &mut Common, it: &mut It) -> Result<(Common, Cmd), Fail> {
    let mut commits = None;
    let mut report = None;
    let mut checks = Vec::new();
    while let Some(arg) = it.next() {
        if arg == "-h" || arg == "--help" {
            return Ok((std::mem::take(common), Cmd::Help(U_DELIVER)));
        }
        if common_flag(it, common, &arg, U_DELIVER)? {
            continue;
        }
        match arg.as_str() {
            "--commits" => commits = Some(it.value("--commits", U_DELIVER)?),
            _ if eq(&arg, "--commits").is_some() => {
                commits = eq(&arg, "--commits").map(String::from)
            }
            "--report" => report = Some(PathBuf::from(it.value("--report", U_DELIVER)?)),
            _ if eq(&arg, "--report").is_some() => report = eq(&arg, "--report").map(PathBuf::from),
            "--check" => checks.push(parse_check(&it.value("--check", U_DELIVER)?, U_DELIVER)?),
            _ if eq(&arg, "--check").is_some() => {
                checks.push(parse_check(eq(&arg, "--check").unwrap(), U_DELIVER)?)
            }
            _ => return Err(usage(format!("unexpected argument '{arg}'"), U_DELIVER)),
        }
    }
    Ok((
        std::mem::take(common),
        Cmd::Deliver {
            commits: commits.ok_or_else(|| usage("missing --commits", U_DELIVER))?,
            report: report.ok_or_else(|| usage("missing --report", U_DELIVER))?,
            checks,
        },
    ))
}

/// `approve`: the permission-prompt decision (D9) for `--person` —
/// recording it is refused without an open `permission prompt`
/// attention on that role, the check is the model's.
pub(crate) fn approve(common: &mut Common, it: &mut It) -> Result<(Common, Cmd), Fail> {
    let mut person = None;
    let mut decision = None;
    while let Some(arg) = it.next() {
        if arg == "-h" || arg == "--help" {
            return Ok((std::mem::take(common), Cmd::Help(U_APPROVE)));
        }
        if common_flag(it, common, &arg, U_APPROVE)? {
            continue;
        }
        match arg.as_str() {
            "--person" => person = Some(it.value("--person", U_APPROVE)?),
            _ if eq(&arg, "--person").is_some() => person = eq(&arg, "--person").map(String::from),
            "--decision" => {
                let v = it.value("--decision", U_APPROVE)?;
                decision = Some(parse_decision(&v).ok_or_else(|| {
                    usage(format!("bad --decision '{v}', want allow|deny"), U_APPROVE)
                })?)
            }
            _ if eq(&arg, "--decision").is_some() => {
                let v = eq(&arg, "--decision").unwrap_or_default();
                decision = Some(parse_decision(v).ok_or_else(|| {
                    usage(format!("bad --decision '{v}', want allow|deny"), U_APPROVE)
                })?)
            }
            _ => return Err(usage(format!("unexpected argument '{arg}'"), U_APPROVE)),
        }
    }
    Ok((
        std::mem::take(common),
        Cmd::Approve {
            person: person.ok_or_else(|| usage("missing --person", U_APPROVE))?,
            decision: decision.ok_or_else(|| usage("missing --decision", U_APPROVE))?,
        },
    ))
}

fn parse_decision(v: &str) -> Option<ApprovalDecision> {
    match v {
        "allow" => Some(ApprovalDecision::Allow),
        "deny" => Some(ApprovalDecision::Deny),
        _ => None,
    }
}

pub(crate) fn block(common: &mut Common, it: &mut It) -> Result<(Common, Cmd), Fail> {
    let mut reason = None;
    while let Some(arg) = it.next() {
        if arg == "-h" || arg == "--help" {
            return Ok((std::mem::take(common), Cmd::Help(U_BLOCK)));
        }
        if common_flag(it, common, &arg, U_BLOCK)? {
            continue;
        }
        match arg.as_str() {
            "--reason" => reason = Some(it.value("--reason", U_BLOCK)?),
            _ if eq(&arg, "--reason").is_some() => reason = eq(&arg, "--reason").map(String::from),
            _ => return Err(usage(format!("unexpected argument '{arg}'"), U_BLOCK)),
        }
    }
    Ok((
        std::mem::take(common),
        Cmd::Block {
            reason: reason.ok_or_else(|| usage("missing --reason", U_BLOCK))?,
        },
    ))
}

pub(crate) fn review(common: &mut Common, it: &mut It) -> Result<(Common, Cmd), Fail> {
    let mut delivery = None;
    let mut verdict = None;
    let mut conclusion = None;
    let mut findings = None;
    let mut upstream = None;
    while let Some(arg) = it.next() {
        if arg == "-h" || arg == "--help" {
            return Ok((std::mem::take(common), Cmd::Help(U_REVIEW)));
        }
        if common_flag(it, common, &arg, U_REVIEW)? {
            continue;
        }
        match arg.as_str() {
            "--delivery" => {
                let v = it.value("--delivery", U_REVIEW)?;
                delivery = Some(
                    v.parse::<u32>()
                        .map_err(|_| usage(format!("bad --delivery '{v}'"), U_REVIEW))?,
                )
            }
            _ if eq(&arg, "--delivery").is_some() => {
                let v = eq(&arg, "--delivery").unwrap();
                delivery = Some(
                    v.parse::<u32>()
                        .map_err(|_| usage(format!("bad --delivery '{v}'"), U_REVIEW))?,
                )
            }
            "--verdict" => {
                let v = it.value("--verdict", U_REVIEW)?;
                verdict = Some(
                    parse_verdict(&v)
                        .ok_or_else(|| usage(format!("bad --verdict '{v}'"), U_REVIEW))?,
                )
            }
            _ if eq(&arg, "--verdict").is_some() => {
                let v = eq(&arg, "--verdict").unwrap();
                verdict = Some(
                    parse_verdict(v)
                        .ok_or_else(|| usage(format!("bad --verdict '{v}'"), U_REVIEW))?,
                )
            }
            "--conclusion" => conclusion = Some(it.value("--conclusion", U_REVIEW)?),
            _ if eq(&arg, "--conclusion").is_some() => {
                conclusion = eq(&arg, "--conclusion").map(String::from)
            }
            "--findings" => findings = Some(PathBuf::from(it.value("--findings", U_REVIEW)?)),
            _ if eq(&arg, "--findings").is_some() => {
                findings = eq(&arg, "--findings").map(PathBuf::from)
            }
            "--upstream-changes" => {
                upstream = Some(PathBuf::from(it.value("--upstream-changes", U_REVIEW)?))
            }
            _ if eq(&arg, "--upstream-changes").is_some() => {
                upstream = eq(&arg, "--upstream-changes").map(PathBuf::from)
            }
            _ => return Err(usage(format!("unexpected argument '{arg}'"), U_REVIEW)),
        }
    }
    Ok((
        std::mem::take(common),
        Cmd::Review {
            delivery: delivery.ok_or_else(|| usage("missing --delivery", U_REVIEW))?,
            verdict: verdict.ok_or_else(|| usage("missing --verdict", U_REVIEW))?,
            conclusion: conclusion.ok_or_else(|| usage("missing --conclusion", U_REVIEW))?,
            findings,
            upstream_changes: upstream,
        },
    ))
}

/// `--delivery <n>` flag shared by `review` and `finding`.
fn parse_delivery(it: &mut It, u: &'static str, arg: &str) -> Result<Option<u32>, Fail> {
    let v = if arg == "--delivery" {
        Some(it.value("--delivery", u)?)
    } else {
        eq(arg, "--delivery").map(String::from)
    };
    v.map(|v| {
        v.parse::<u32>()
            .map_err(|_| usage(format!("bad --delivery '{v}'"), u))
    })
    .transpose()
}

pub(crate) fn finding(common: &mut Common, it: &mut It) -> Result<(Common, Cmd), Fail> {
    let mut delivery = None;
    let mut findings = None;
    while let Some(arg) = it.next() {
        if arg == "-h" || arg == "--help" {
            return Ok((std::mem::take(common), Cmd::Help(U_FINDING)));
        }
        if common_flag(it, common, &arg, U_FINDING)? {
            continue;
        }
        match arg.as_str() {
            "--delivery" => delivery = parse_delivery(it, U_FINDING, &arg)?,
            _ if eq(&arg, "--delivery").is_some() => {
                delivery = parse_delivery(it, U_FINDING, &arg)?
            }
            "--findings" => findings = Some(PathBuf::from(it.value("--findings", U_FINDING)?)),
            _ if eq(&arg, "--findings").is_some() => {
                findings = eq(&arg, "--findings").map(PathBuf::from)
            }
            _ => return Err(usage(format!("unexpected argument '{arg}'"), U_FINDING)),
        }
    }
    Ok((
        std::mem::take(common),
        Cmd::Finding {
            delivery: delivery.ok_or_else(|| usage("missing --delivery", U_FINDING))?,
            findings: findings.ok_or_else(|| usage("missing --findings", U_FINDING))?,
        },
    ))
}

pub(crate) fn correct(common: &mut Common, it: &mut It) -> Result<(Common, Cmd), Fail> {
    let mut event = None;
    let mut reason = None;
    let mut set = BTreeMap::new();
    while let Some(arg) = it.next() {
        if arg == "-h" || arg == "--help" {
            return Ok((std::mem::take(common), Cmd::Help(U_CORRECT)));
        }
        if common_flag(it, common, &arg, U_CORRECT)? {
            continue;
        }
        match arg.as_str() {
            "--event" => {
                let v = it.value("--event", U_CORRECT)?;
                event = Some(
                    v.parse::<u64>()
                        .map_err(|_| usage(format!("bad --event '{v}'"), U_CORRECT))?,
                )
            }
            _ if eq(&arg, "--event").is_some() => {
                let v = eq(&arg, "--event").unwrap();
                event = Some(
                    v.parse::<u64>()
                        .map_err(|_| usage(format!("bad --event '{v}'"), U_CORRECT))?,
                )
            }
            "--reason" => reason = Some(it.value("--reason", U_CORRECT)?),
            _ if eq(&arg, "--reason").is_some() => reason = eq(&arg, "--reason").map(String::from),
            "--set" => {
                let (k, v) = parse_set(&it.value("--set", U_CORRECT)?, U_CORRECT)?;
                set.insert(k, v);
            }
            _ if eq(&arg, "--set").is_some() => {
                let (k, v) = parse_set(eq(&arg, "--set").unwrap(), U_CORRECT)?;
                set.insert(k, v);
            }
            _ => return Err(usage(format!("unexpected argument '{arg}'"), U_CORRECT)),
        }
    }
    if set.is_empty() {
        return Err(usage("missing --set", U_CORRECT));
    }
    Ok((
        std::mem::take(common),
        Cmd::Correct {
            event: event.ok_or_else(|| usage("missing --event", U_CORRECT))?,
            reason: reason.unwrap_or_default(),
            set,
        },
    ))
}

/// `status` / `records`: only `--json` on top of the common flags.
pub(crate) fn flags_only(
    common: &mut Common,
    it: &mut It,
    u: &'static str,
    make: impl FnOnce(bool) -> Cmd,
) -> Result<(Common, Cmd), Fail> {
    let mut json = false;
    while let Some(arg) = it.next() {
        if arg == "-h" || arg == "--help" {
            return Ok((std::mem::take(common), Cmd::Help(u)));
        }
        if common_flag(it, common, &arg, u)? {
            continue;
        }
        match arg.as_str() {
            "--json" => json = true,
            _ => return Err(usage(format!("unexpected argument '{arg}'"), u)),
        }
    }
    Ok((std::mem::take(common), make(json)))
}
