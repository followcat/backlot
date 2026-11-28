//! CLI implementation of remembered permission grants: scoped approve,
//! `permit list` and `permit revoke`.

use std::path::{Path, PathBuf};

use backlot_model::event::{
    ApprovalDecision, EventKind, PERMISSION_PROMPT, PermitKind, PermitScope,
};
use backlot_model::permit::role_key;
use backlot_model::state::StudioState;
use backlot_model::{EventBy, NewEvent};

use crate::args::{ApprovalScope, Common, PermitCmd};
use crate::cli::{Env, Fail};
use crate::store::{self, Pending};

fn pending(kind: EventKind, by: &EventBy) -> Pending {
    Pending {
        event: NewEvent::new(by.clone(), kind),
        files: Vec::new(),
    }
}

/// What the prompt asks for. A hook signal distinguishes the two native
/// kinds; without one only a shell command can be remembered safely.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Target {
    Shell(String),
    Edit(String),
}

pub(crate) fn target_from_tool_input(input: &serde_json::Value) -> Option<Target> {
    if let Some(command) = input.get("command").and_then(|v| v.as_str()) {
        return Some(Target::Shell(command.to_string()));
    }
    input
        .get("file_path")
        .and_then(|v| v.as_str())
        .map(|p| Target::Edit(p.to_string()))
}

fn target_from_signal(dir: &Path, seq: u64) -> Option<Target> {
    let text = std::fs::read_to_string(dir.join("attention").join(format!("{seq}.signal"))).ok()?;
    let s: crate::signals::Signal = serde_json::from_str(&text).ok()?;
    target_from_tool_input(s.tool_input.as_ref()?)
}

fn target(dir: &Path, runner: &str, seq: u64) -> Option<Target> {
    if let Some(t) = target_from_signal(dir, seq) {
        return Some(t);
    }
    let screen = std::fs::read_to_string(dir.join("attention").join(format!("{seq}.txt"))).ok()?;
    crate::runners::prompt_detail(runner, &screen).map(Target::Shell)
}

fn guard_allows(command: &str, state: &StudioState, env: &Env) -> bool {
    let Some(studio) = state.studio.as_ref() else {
        return false;
    };
    let cwd = Path::new(&studio.repo);
    matches!(
        crate::guard_shell::classify_command_line(command, &studio.push, cwd, env),
        crate::guard_rules::GuardVerdict::Allow
    )
}

pub(crate) fn strip_git_location(rule: &str) -> String {
    let words: Vec<&str> = rule.split_whitespace().collect();
    if words.first() != Some(&"git") {
        return rule.to_string();
    }
    let mut out = vec!["git".to_string()];
    let mut i = 1;
    while i < words.len() {
        match words[i] {
            "-C" | "--git-dir" | "--work-tree" => i += 2,
            w if w.starts_with("--git-dir=") || w.starts_with("--work-tree=") => i += 1,
            w => {
                out.push(w.to_string());
                i += 1;
            }
        }
    }
    out.join(" ")
}

fn fixed_shell_rules() -> impl Iterator<Item = &'static str> {
    crate::runners::READ_ONLY
        .iter()
        .copied()
        .chain(crate::runners::WORKTREE_DIAGNOSTICS.iter().copied())
}

/// Whether a fixed pre-allow already covers this derived shell rule.
pub(crate) fn fixed_covers(rule: &str) -> bool {
    fixed_shell_rules().any(|r| r == rule || r == strip_git_location(rule))
}

/// Whether a path names a secret-shaped file or directory. This is a
/// conservative grant-time check, not a complete secret detector.
pub(crate) fn secret_path(path: &Path) -> bool {
    let lower = path.to_string_lossy().replace('\\', "/").to_lowercase();
    let leaf = path
        .file_name()
        .map(|n| n.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    lower.contains("/.ssh/")
        || lower.ends_with("/.ssh")
        || lower.contains("/.gnupg/")
        || lower.ends_with("/.gnupg")
        || lower.contains("/.config/secrets/")
        || lower.ends_with("/.docker/config.json")
        || lower.ends_with("/.kube/config")
        || leaf == ".env"
        || leaf.starts_with(".env.")
        || leaf == "credentials"
        || leaf == "auth.json"
        || lower.contains("/.claude/")
        || lower.ends_with("/.claude")
        || lower.contains("/.cursor/")
        || lower.ends_with("/.cursor")
        || lower.contains("/.commandcode/")
        || lower.ends_with("/.commandcode")
        || lower.contains("/.gemini/")
        || lower.ends_with("/.gemini")
        || lower.contains("/.config/cursor/")
        || lower.ends_with("/.config/cursor")
        || lower.contains("/.config/antigravity/")
        || lower.ends_with("/.config/antigravity")
        || lower.contains("/.config/devin/")
        || lower.ends_with("/.config/devin")
        || lower.contains("/.local/share/devin/")
        || lower.ends_with("/.local/share/devin")
}

fn deepest_canonical(path: &Path) -> PathBuf {
    let mut missing = Vec::new();
    let mut cur = path.to_path_buf();
    loop {
        if let Ok(real) = cur.canonicalize() {
            let mut out = real;
            for part in missing.iter().rev() {
                out.push(part);
            }
            return out;
        }
        let Some(name) = cur.file_name().map(|n| n.to_os_string()) else {
            return path.to_path_buf();
        };
        missing.push(name);
        if !cur.pop() {
            return path.to_path_buf();
        }
    }
}

/// The exact absolute edit rule a prompt may store: inside one of
/// `roots` after canonicalizing its deepest existing ancestor, with no
/// `..` component and no secret-shaped path. Relative prompt paths are
/// resolved from `cwd` (the role's work area).
pub(crate) fn edit_rule(path: &str, cwd: &Path, roots: &[PathBuf]) -> Result<String, Fail> {
    if path.trim().is_empty() || path.contains(['\n', '\r', '\0']) || path.contains('…') {
        return Err(Fail::refused(
            "an edit prompt without one clean path is once-only",
        ));
    }
    let raw = PathBuf::from(path);
    if raw
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(Fail::refused("an edit grant cannot contain `..`"));
    }
    let joined = if raw.is_absolute() {
        raw
    } else {
        cwd.join(raw)
    };
    if !joined.is_absolute() {
        return Err(Fail::refused("an edit grant needs an absolute path"));
    }
    let resolved = deepest_canonical(&joined);
    if secret_path(&resolved) {
        return Err(Fail::refused("an edit grant cannot name a secret path"));
    }
    let inside = roots.iter().any(|root| {
        let root = deepest_canonical(root);
        resolved == root || resolved.starts_with(root)
    });
    if !inside {
        return Err(Fail::refused(
            "an edit grant must stay inside the role's worktree or the studio repository",
        ));
    }
    Ok(resolved.to_string_lossy().into_owned())
}

fn grantable_rules(
    target: &Target,
    state: &StudioState,
    env: &Env,
    dir: &Path,
    person: &str,
) -> Result<Vec<(PermitKind, String, bool)>, Fail> {
    match target {
        Target::Edit(path) => {
            let Some(studio) = state.studio.as_ref() else {
                return Err(Fail::refused("the studio is not ready"));
            };
            let repo = PathBuf::from(&studio.repo);
            let area = crate::workareas::role_dir(dir, &studio.repo, person, studio.no_git);
            let rule = edit_rule(path, &area, &[area.clone(), repo])?;
            Ok(vec![(PermitKind::Edit, rule, false)])
        }
        Target::Shell(command) => {
            if !guard_allows(command, state, env) {
                return Err(Fail::refused(
                    "backlot's guard would stop this command, so it cannot be remembered",
                ));
            }
            let rules = crate::permit_rules::coverage(command).map_err(Fail::refused)?;
            Ok(rules
                .into_iter()
                .filter(|r| !fixed_covers(&r.rule))
                .map(|r| (PermitKind::Shell, r.rule, r.outward))
                .collect())
        }
    }
}

/// `approve --scope role|studio`: append the grants and the approval in
/// one locked batch. The grant seqs are deterministic (`p<seq>`), and
/// fixed read-only coverage is not duplicated into the ledger.
pub(crate) fn approve_scoped(
    common: &Common,
    env: &Env,
    base: &Path,
    person: &str,
    decision: ApprovalDecision,
    scope: ApprovalScope,
) -> Result<String, Fail> {
    crate::proc_guard::check_cli_gate(crate::proc_guard::CliVerb::Approve { person }, common, env)?;
    let caller = crate::cli::caller(common, env, false)?;
    approve_scoped_caller(base, env, &caller, person, decision, scope)
}

/// Scoped approval for an already validated user caller — shared by the
/// local CLI and the server queue.
pub(crate) fn approve_scoped_caller(
    base: &Path,
    env: &Env,
    caller: &crate::cli::Caller,
    person: &str,
    decision: ApprovalDecision,
    scope: ApprovalScope,
) -> Result<String, Fail> {
    if caller.role != "user" {
        return Err(Fail::refused("remembering a permission is the user's"));
    }
    if decision != ApprovalDecision::Allow {
        return Err(Fail::refused("only an allow decision can be remembered"));
    }
    let dir = store::studio_dir(base, &caller.studio);
    let by = EventBy {
        role: caller.role.clone(),
        session: caller.session.clone(),
        via: None,
    };
    let events = store::append_many(&dir, |state, next_seq| {
        let attn = state
            .open_attention()
            .find(|a| a.role == person && a.reason == PERMISSION_PROMPT)
            .ok_or_else(|| Fail::refused(format!("no permission prompt open on '{person}'")))?;
        let runner = state
            .roles
            .get(person)
            .and_then(|r| r.actor.as_ref())
            .map(|a| a.runner.clone())
            .unwrap_or_else(|| "cmd".into());
        let target = target(&dir, &runner, attn.seq).ok_or_else(|| {
            Fail::refused("the saved prompt names no command or edit path, so it is once-only")
        })?;
        let rules = grantable_rules(&target, state, env, &dir, person)?;
        if rules.is_empty() {
            let approval = pending(
                EventKind::RoleApproval {
                    role: person.to_string(),
                    decision,
                },
                &by,
            );
            return Ok(vec![approval]);
        }
        if scope == ApprovalScope::Studio && rules.iter().any(|(_, _, outward)| *outward) {
            return Err(Fail::refused(
                "an outward permit can be remembered only for its role",
            ));
        }
        let key = role_key(state, person);
        let role = (scope == ApprovalScope::Role).then_some(key);
        let mut out = Vec::new();
        for (i, (kind, rule, outward)) in rules.iter().enumerate() {
            out.push(pending(
                EventKind::PermitGranted {
                    id: format!("p{}", next_seq + i as u64),
                    scope: match scope {
                        ApprovalScope::Role => PermitScope::Role,
                        ApprovalScope::Studio => PermitScope::Studio,
                        ApprovalScope::Once => unreachable!(),
                    },
                    role: role.clone(),
                    permit_kind: *kind,
                    rule: rule.clone(),
                    outward: *outward,
                    from: attn.seq,
                    runner: runner.clone(),
                },
                &by,
            ));
        }
        out.push(pending(
            EventKind::RoleApproval {
                role: person.to_string(),
                decision,
            },
            &by,
        ));
        Ok(out)
    })?;
    let granted = events
        .iter()
        .filter(|e| matches!(e.kind, EventKind::PermitGranted { .. }))
        .count();
    if granted == 0 {
        Ok("approved; fixed permissions already cover it\n".into())
    } else {
        Ok(format!(
            "approved; remembered {granted} permit{}\n",
            if granted == 1 { "" } else { "s" }
        ))
    }
}

fn permit_json(state: &StudioState, role: Option<&str>) -> serde_json::Value {
    let key = role.map(|r| role_key(state, r));
    let rows: Vec<_> = state
        .permits
        .values()
        .filter(|p| key.as_deref().is_none_or(|k| p.reaches(k)))
        .map(|p| {
            serde_json::json!({
                "id": p.id,
                "scope": p.scope,
                "role": p.role,
                "kind": p.kind,
                "rule": p.rule,
                "outward": p.outward,
                "from": p.from,
                "runner": p.runner,
                "seq": p.seq,
                "ts_ms": p.ts_ms,
            })
        })
        .collect();
    serde_json::Value::Array(rows)
}

/// List grants for a validated user caller; the server queue shares this.
pub(crate) fn permit_list_base(
    base: &Path,
    caller: &crate::cli::Caller,
    role: Option<&str>,
) -> Result<serde_json::Value, Fail> {
    if caller.role != "user" {
        return Err(Fail::refused("permit administration is the user's"));
    }
    let dir = store::studio_dir(base, &caller.studio);
    let state = crate::cli::load(&dir)?;
    Ok(permit_json(&state, role))
}

fn permit_log_json(dir: &Path, role: &str) -> serde_json::Value {
    let path = crate::signals::signals_path(dir, role);
    let Ok(text) = std::fs::read_to_string(path) else {
        return serde_json::Value::Array(Vec::new());
    };
    let mut rows: Vec<serde_json::Value> = text
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|v| v.get("event").and_then(|e| e.as_str()) == Some("PermitAutoAllow"))
        .collect();
    if rows.len() > 100 {
        rows = rows.split_off(rows.len() - 100);
    }
    rows.reverse();
    serde_json::Value::Array(rows)
}

/// Recent read-only auto-allows for a validated user caller.
pub(crate) fn permit_log_base(
    base: &Path,
    caller: &crate::cli::Caller,
    role: &str,
) -> Result<serde_json::Value, Fail> {
    if caller.role != "user" {
        return Err(Fail::refused("permit administration is the user's"));
    }
    if role.is_empty()
        || role.len() > 64
        || !role
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
    {
        return Err(Fail::usage("bad role name"));
    }
    let dir = store::studio_dir(base, &caller.studio);
    Ok(permit_log_json(&dir, role))
}

/// Revoke one grant for a validated user caller; the server queue shares this.
pub(crate) fn permit_revoke_base(
    base: &Path,
    caller: &crate::cli::Caller,
    id: &str,
) -> Result<String, Fail> {
    if caller.role != "user" {
        return Err(Fail::refused("permit administration is the user's"));
    }
    let dir = store::studio_dir(base, &caller.studio);
    let by = EventBy {
        role: caller.role.clone(),
        session: caller.session.clone(),
        via: caller.via.clone(),
    };
    store::append(&dir, |_, _| {
        Ok::<_, Fail>(pending(
            EventKind::PermitRevoked {
                id: id.to_string(),
                reason: "revoked by user".into(),
            },
            &by,
        ))
    })?;
    Ok(format!("revoked permit {id}"))
}

/// `permit list` / `permit revoke`.
pub(crate) fn permit(
    common: &Common,
    env: &Env,
    base: &Path,
    cmd: &PermitCmd,
) -> Result<String, Fail> {
    crate::proc_guard::check_cli_gate(crate::proc_guard::CliVerb::PermsRevoke, common, env)?;
    let caller = crate::cli::caller(common, env, false)?;
    if caller.role != "user" {
        return Err(Fail::refused("permit administration is the user's"));
    }
    let dir = store::studio_dir(base, &caller.studio);
    match cmd {
        PermitCmd::List { role, json } => {
            let value = permit_list_base(base, &caller, role.as_deref())?;
            if *json {
                Ok(format!(
                    "{}\n",
                    serde_json::to_string_pretty(&value).unwrap_or_default()
                ))
            } else {
                let mut out = String::new();
                for row in value.as_array().into_iter().flatten() {
                    let scope = row["scope"].as_str().unwrap_or("role");
                    let who = row["role"].as_str().unwrap_or("studio");
                    let outward = if row["outward"].as_bool() == Some(true) {
                        " outward"
                    } else {
                        ""
                    };
                    out.push_str(&format!(
                        "{} {} {} {:?} {:?}{} (from attention {})\n",
                        row["id"].as_str().unwrap_or_default(),
                        scope,
                        who,
                        row["kind"].as_str().unwrap_or_default(),
                        row["rule"].as_str().unwrap_or_default(),
                        outward,
                        row["from"].as_u64().unwrap_or_default(),
                    ));
                }
                if out.is_empty() {
                    out.push_str("no active permits\n");
                }
                Ok(out)
            }
        }
        PermitCmd::Log { role, json } => {
            let value = permit_log_base(base, &caller, role)?;
            if *json {
                Ok(format!(
                    "{}\n",
                    serde_json::to_string_pretty(&value).unwrap_or_default()
                ))
            } else {
                let mut out = String::new();
                for row in value.as_array().into_iter().flatten() {
                    out.push_str(&format!(
                        "{} {} {} {}\n",
                        row["ts_ms"].as_u64().unwrap_or_default(),
                        row["decision"].as_str().unwrap_or("allow"),
                        row["reason"].as_str().unwrap_or_default(),
                        row["command"].as_str().unwrap_or_default(),
                    ));
                }
                if out.is_empty() {
                    out.push_str("no read-only auto-allows\n");
                }
                Ok(out)
            }
        }
        PermitCmd::Revoke { id } => {
            let by = EventBy {
                role: caller.role,
                session: caller.session,
                via: None,
            };
            store::append(&dir, |_, _| {
                Ok::<_, Fail>(pending(
                    EventKind::PermitRevoked {
                        id: id.clone(),
                        reason: "revoked by user".into(),
                    },
                    &by,
                ))
            })?;
            Ok(format!("revoked permit {id}\n"))
        }
    }
}
