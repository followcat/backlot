//! Process-tree guard for CLI approval/card/revocation gates and
//! self-approval prevention across runner hooks (push-perms Hole 3).

use std::collections::BTreeSet;

use backlot_model::hollywood::is_hollywood_terminal_name;

use crate::args::Common;
use crate::cli::{Env, Fail};

const MAX_ANCESTOR_HOPS: usize = 64;

/// Role identity recovered from an ancestor process environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RoleAncestor {
    pub pid: u32,
    pub role: Option<String>,
    pub terminal: Option<String>,
    pub studio: Option<String>,
}

impl RoleAncestor {
    pub(crate) fn effective_role(&self) -> Option<&str> {
        if let Some(r) = self.role.as_deref().filter(|s| !s.is_empty()) {
            return Some(r);
        }
        if self
            .terminal
            .as_deref()
            .is_some_and(is_hollywood_terminal_name)
        {
            return Some(backlot_model::hollywood::HOLLYWOOD);
        }
        if let (Some(term), Some(studio)) = (self.terminal.as_deref(), self.studio.as_deref()) {
            let prefix = format!("bl-{studio}-");
            if let Some(rest) = term.strip_prefix(&prefix)
                && !rest.is_empty()
            {
                return Some(rest);
            }
        }
        None
    }
}

/// CLI verb guarded against execution inside a role terminal process tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CliVerb<'a> {
    CardMutate,
    PermsRevoke,
    ServerControl,
    HollywoodControl,
    Approve { person: &'a str },
}

/// Ancestor-process reader seam: `(ppid, env_pairs)` for a PID.
pub(crate) type AncestorRead<'a> = &'a dyn Fn(u32) -> Option<(u32, Vec<(String, String)>)>;

#[cfg(target_os = "linux")]
fn parse_linux_ppid(stat: &str) -> Option<u32> {
    let rparen = stat.rfind(')')?;
    let after = stat.get(rparen + 1..)?.trim_start();
    let mut parts = after.split_whitespace();
    let _state = parts.next()?;
    parts.next()?.parse::<u32>().ok()
}

#[cfg(target_os = "linux")]
fn current_ppid() -> u32 {
    std::fs::read_to_string("/proc/self/stat")
        .ok()
        .and_then(|s| parse_linux_ppid(&s))
        .unwrap_or(0)
}

#[cfg(target_os = "macos")]
fn current_ppid() -> u32 {
    let me = std::process::id().to_string();
    std::process::Command::new("ps")
        .args(["-o", "ppid=", "-p", &me])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| {
            String::from_utf8_lossy(&o.stdout)
                .trim()
                .parse::<u32>()
                .ok()
        })
        .unwrap_or(0)
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn current_ppid() -> u32 {
    0
}

#[cfg(target_os = "linux")]
pub(crate) fn real_proc_entry(pid: u32) -> Option<(u32, Vec<(String, String)>)> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let ppid = parse_linux_ppid(&stat)?;
    let mut vars = Vec::new();
    if let Ok(raw) = std::fs::read(format!("/proc/{pid}/environ")) {
        for chunk in raw.split(|b| *b == 0) {
            let s = String::from_utf8_lossy(chunk);
            if let Some((k, v)) = s.split_once('=')
                && k.starts_with("BACKLOT_")
            {
                vars.push((k.to_string(), v.to_string()));
            }
        }
    }
    Some((ppid, vars))
}

#[cfg(target_os = "macos")]
pub(crate) fn real_proc_entry(pid: u32) -> Option<(u32, Vec<(String, String)>)> {
    let pid_s = pid.to_string();
    let ppid_out = std::process::Command::new("ps")
        .args(["-o", "ppid=", "-p", &pid_s])
        .output()
        .ok()?;
    if !ppid_out.status.success() {
        return None;
    }
    let ppid = String::from_utf8_lossy(&ppid_out.stdout)
        .trim()
        .parse::<u32>()
        .ok()?;
    let mut vars = Vec::new();
    if let Ok(eww) = std::process::Command::new("ps")
        .args(["eww", "-o", "command=", "-p", &pid_s])
        .output()
        && eww.status.success()
    {
        let line = String::from_utf8_lossy(&eww.stdout);
        for tok in line.split_whitespace() {
            if let Some((k, v)) = tok.split_once('=')
                && k.starts_with("BACKLOT_")
            {
                vars.push((k.to_string(), v.to_string()));
            }
        }
    }
    Some((ppid, vars))
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn real_proc_entry(_pid: u32) -> Option<(u32, Vec<(String, String)>)> {
    None
}

pub(crate) fn find_role_ancestor(start_ppid: u32, read: AncestorRead<'_>) -> Option<RoleAncestor> {
    let mut pid = start_ppid;
    let mut visited = BTreeSet::new();
    for _ in 0..MAX_ANCESTOR_HOPS {
        if pid <= 1 || !visited.insert(pid) {
            break;
        }
        let (ppid, vars) = read(pid)?;
        let mut role = None;
        let mut terminal = None;
        let mut studio = None;
        for (k, v) in vars {
            match k.as_str() {
                "BACKLOT_ROLE" if !v.is_empty() => role = Some(v),
                "BACKLOT_TERMINAL" if !v.is_empty() => terminal = Some(v),
                "BACKLOT_STUDIO" if !v.is_empty() => studio = Some(v),
                _ => {}
            }
        }
        let is_term = role.is_some()
            || terminal
                .as_deref()
                .is_some_and(|t| is_hollywood_terminal_name(t) || t.starts_with("bl-"));
        if is_term {
            return Some(RoleAncestor {
                pid,
                role,
                terminal,
                studio,
            });
        }
        if ppid == pid {
            break;
        }
        pid = ppid;
    }
    None
}

/// Enforce process-tree and `BACKLOT_ROLE` gates for `card apply|cancel`,
/// `perms revoke`, and `approve`.
pub(crate) fn check_cli_gate(verb: CliVerb<'_>, common: &Common, env: &Env) -> Result<(), Fail> {
    check_cli_gate_with(verb, common, env, &real_proc_entry, current_ppid())
}

pub(crate) fn check_cli_gate_with(
    verb: CliVerb<'_>,
    common: &Common,
    env: &Env,
    read: AncestorRead<'_>,
    start_ppid: u32,
) -> Result<(), Fail> {
    let anc = find_role_ancestor(start_ppid, read);
    let env_role = env.get("BACKLOT_ROLE").filter(|s| !s.is_empty());
    match verb {
        CliVerb::CardMutate => refuse_role_gate(
            env_role,
            anc.as_ref(),
            "a card is applied or cancelled by the user",
            "",
        ),
        CliVerb::PermsRevoke => {
            if env_role.is_some() || anc.is_some() {
                return Err(Fail::refused(
                    "permission revocation is the user's — cannot run from a role terminal (BACKLOT_ROLE)",
                ));
            }
            Ok(())
        }
        CliVerb::ServerControl => refuse_role_gate(
            env_role,
            anc.as_ref(),
            "the server is started, stopped or adopted by the person",
            "",
        ),
        CliVerb::HollywoodControl => refuse_role_gate(
            env_role,
            anc.as_ref(),
            "switching hollywood is the person's call",
            "; propose a setup card with 'backlot hollywood propose setup ...' instead",
        ),
        CliVerb::Approve { person } => {
            if let Some(a) = anc {
                let anc_role = a.effective_role().ok_or_else(|| {
                    Fail::refused(format!(
                        "cannot determine ancestor role for approve (pid {})",
                        a.pid
                    ))
                })?;
                let Some(er) = env_role else {
                    return Err(Fail::refused(format!(
                        "approve from role '{anc_role}' terminal requires matching BACKLOT_ROLE"
                    )));
                };
                if er != anc_role {
                    return Err(Fail::refused(format!(
                        "BACKLOT_ROLE '{er}' disagrees with role terminal ancestor '{anc_role}'"
                    )));
                }
                if let Some(flag_role) = common.role.as_deref()
                    && flag_role != anc_role
                {
                    return Err(Fail::refused(format!(
                        "--as '{flag_role}' disagrees with role terminal ancestor '{anc_role}'"
                    )));
                }
                if !matches!(anc_role, "producer" | "director") {
                    return Err(Fail::refused(format!(
                        "role '{anc_role}' cannot approve; only user, producer, or director can"
                    )));
                }
                if person == anc_role {
                    return Err(Fail::refused(format!(
                        "role '{anc_role}' cannot approve its own prompt"
                    )));
                }
            } else if let Some(er) = env_role
                && person == er
            {
                return Err(Fail::refused(format!(
                    "role '{er}' cannot approve its own prompt"
                )));
            }
            Ok(())
        }
    }
}

fn refuse_role_gate(
    env_role: Option<&str>,
    anc: Option<&RoleAncestor>,
    headline: &str,
    suffix: &str,
) -> Result<(), Fail> {
    if env_role.is_some() {
        return Err(Fail::refused(format!(
            "{headline} — BACKLOT_ROLE is set{suffix}"
        )));
    }
    if let Some(a) = anc {
        return Err(Fail::refused(format!(
            "{headline} — role terminal ancestor (pid {}, BACKLOT_ROLE={}){suffix}",
            a.pid,
            a.effective_role().unwrap_or("unknown")
        )));
    }
    Ok(())
}

fn is_env_assign(word: &str) -> bool {
    let Some((k, _)) = word.split_once('=') else {
        return false;
    };
    !k.is_empty() && !k.starts_with('-') && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Inspect one already-split shell segment (program + args, before wrapper unwrapping)
/// for Hole 3 self-approval patterns: `BACKLOT_*`, `env -i`, `--ignore-environment`,
/// `backlot card apply|cancel`, `backlot perms revoke`, and unauthorised `backlot approve`.
pub(crate) fn check_segment_self_approval(words: &[String], role: Option<&str>) -> Option<String> {
    if words.iter().any(|w| w.contains("BACKLOT_")) {
        return Some("touching BACKLOT_* environment".into());
    }
    if words.iter().any(|w| w == "--ignore-environment") {
        return Some("environment clearing (--ignore-environment)".into());
    }
    let mut i = 0usize;
    while i < words.len() {
        let w = words[i].as_str();
        if is_env_assign(w) {
            i += 1;
            continue;
        }
        match w.rsplit('/').next().unwrap_or(w) {
            "env" => {
                i += 1;
                while i < words.len() {
                    let f = words[i].as_str();
                    if f == "--" {
                        i += 1;
                        break;
                    }
                    if is_env_assign(f) {
                        i += 1;
                        continue;
                    }
                    if f == "--ignore-environment"
                        || (f.starts_with('-') && !f.starts_with("--") && f[1..].contains('i'))
                    {
                        return Some("environment clearing (env -i)".into());
                    }
                    if matches!(f, "-u" | "--unset" | "-C" | "-S") {
                        i += 2;
                        continue;
                    }
                    if f.starts_with('-') {
                        i += 1;
                        continue;
                    }
                    break;
                }
            }
            "command" | "nohup" | "exec" => i += 1,
            "timeout" | "sudo" | "doas" => {
                i += 1;
                while i < words.len() && words[i].starts_with('-') {
                    if matches!(words[i].as_str(), "-u" | "-g" | "-k") {
                        i += 2;
                    } else {
                        i += 1;
                    }
                }
            }
            "backlot" => return check_backlot_args(&words[i + 1..], role),
            _ => return None,
        }
    }
    None
}

fn check_backlot_args(args: &[String], role: Option<&str>) -> Option<String> {
    let mut positionals: Vec<&str> = Vec::new();
    let mut saw_as = false;
    let mut person: Option<&str> = None;
    let mut i = 0usize;
    while i < args.len() {
        let a = args[i].as_str();
        if a == "--as" {
            saw_as = true;
            i += 2;
        } else if a.starts_with("--as=") {
            saw_as = true;
            i += 1;
        } else if a == "--person" {
            person = args.get(i + 1).map(String::as_str);
            i += 2;
        } else if let Some(p) = a.strip_prefix("--person=") {
            person = Some(p);
            i += 1;
        } else if matches!(
            a,
            "--studio" | "--task" | "--session" | "--remote" | "--decision" | "--reason"
        ) {
            i += 2;
        } else if a.starts_with('-') {
            i += 1;
        } else {
            positionals.push(a);
            i += 1;
        }
    }
    match positionals.as_slice() {
        ["card", "apply" | "cancel", ..] => {
            Some("self-approval blocked (backlot card apply|cancel)".into())
        }
        ["perms", "revoke", ..] => Some("self-approval blocked (backlot perms revoke)".into()),
        ["server", "run" | "start" | "stop" | "adopt" | "service", ..] => {
            Some("server control is the person's (backlot server)".into())
        }
        ["hollywood", "switch" | "setup", ..] => {
            Some("switching hollywood is the person's (backlot hollywood switch|setup)".into())
        }
        ["approve", ..] => {
            if !matches!(role, Some("producer" | "director")) {
                return Some("self-approval blocked (backlot approve)".into());
            }
            if saw_as {
                return Some("self-approval blocked (backlot approve --as)".into());
            }
            if person.is_some() && person == role {
                return Some("self-approval blocked (cannot approve own role)".into());
            }
            None
        }
        _ => None,
    }
}
