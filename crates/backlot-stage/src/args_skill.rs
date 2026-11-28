//! Parser for `backlot skill` — the Agent Skills file shipped in the
//! binary: `install`, `status`, `show`, `uninstall`.

use crate::args::*;
use crate::args_studio::common_only;
use crate::cli::Fail;

pub(crate) fn skill(common: &mut Common, it: &mut It) -> Result<(Common, Cmd), Fail> {
    let Some(sub) = it.next() else {
        return Err(usage("missing skill subcommand", U_SKILL));
    };
    match sub.as_str() {
        "install" => install(common, it),
        "status" | "show" | "uninstall" => {
            let cmd = match sub.as_str() {
                "status" => SkillCmd::Status,
                "show" => SkillCmd::Show,
                _ => SkillCmd::Uninstall,
            };
            common_only(common, it, U_SKILL, Cmd::Skill(cmd))
        }
        "-h" | "--help" => Ok((std::mem::take(common), Cmd::Help(U_SKILL))),
        _ => Err(usage(format!("unknown skill subcommand '{sub}'"), U_SKILL)),
    }
}

fn install(common: &mut Common, it: &mut It) -> Result<(Common, Cmd), Fail> {
    let mut agents = Vec::new();
    let mut dry_run = false;
    let mut force = false;
    while let Some(arg) = it.next() {
        if arg == "-h" || arg == "--help" {
            return Ok((std::mem::take(common), Cmd::Help(U_SKILL)));
        }
        if common_flag(it, common, &arg, U_SKILL)? {
            continue;
        }
        match arg.as_str() {
            "--agent" => agents.extend(split_agents(&it.value("--agent", U_SKILL)?)),
            _ if eq(&arg, "--agent").is_some() => {
                agents.extend(split_agents(eq(&arg, "--agent").unwrap()))
            }
            "--dry-run" => dry_run = true,
            "--force" => force = true,
            _ => return Err(usage(format!("unexpected argument '{arg}'"), U_SKILL)),
        }
    }
    Ok((
        std::mem::take(common),
        Cmd::Skill(SkillCmd::Install {
            agents,
            dry_run,
            force,
        }),
    ))
}

/// `--agent claude-code,codex` — one name or a comma-separated list.
fn split_agents(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect()
}
