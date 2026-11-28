//! Safe auto-answering of permission prompts already covered by active
//! grants plus the fixed read-only permission set.

use backlot_model::event::{ApprovalDecision, PermitKind};
use backlot_model::permit::{grants_for, role_key};
use std::path::Path;

use backlot_model::state::StudioState;

fn squash(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Whether a prompt's visible command is the same command the hook
/// request named. Any mismatch — or a screen that names no command —
/// stops auto-answering.
pub(crate) fn screen_matches(runner: &str, screen: &str, command: &str) -> bool {
    let Some(block) = crate::permit::prompt_block(runner, screen) else {
        return false;
    };
    let block = format!(" {} ", squash(&block));
    let command = format!(" {} ", squash(command));
    block.contains(&command)
}

fn fixed_covers(rule: &str) -> bool {
    crate::permit_cmd::fixed_covers(rule)
}

/// The active edit grant exactly matching a canonical edit rule.
pub(crate) fn covering_edit_grant(
    state: &StudioState,
    role: &str,
    rule: &str,
) -> Option<Vec<String>> {
    let key = role_key(state, role);
    let grant = grants_for(state, &key)
        .into_iter()
        .find(|g| g.kind == PermitKind::Edit && g.rule == rule)?;
    Some(vec![grant.id.clone()])
}

/// Whether the edit target is visible in the prompt screen. The exact
/// canonical rule or the prompt's original spelling both count; a screen
/// naming neither is never auto-answered.
pub(crate) fn edit_screen_matches(screen: &str, original: &str, rule: &str) -> bool {
    screen.contains(original) || screen.contains(rule)
}

/// Shell coverage against an explicit grant set and guard context — the
/// shared core used by studio roles and Hollywood.
pub(crate) fn covering_shell_grants_in(
    grants: &[&backlot_model::event::PermitRecord],
    command: &str,
    policy: &backlot_model::push_policy::PushPolicy,
    cwd: &Path,
    env: &crate::cli::Env,
) -> Option<Vec<String>> {
    if !matches!(
        crate::guard_shell::classify_command_line(command, policy, cwd, env),
        crate::guard_rules::GuardVerdict::Allow
    ) {
        return None;
    }
    let derived = crate::permit_rules::coverage(command).ok()?;
    let mut used = Vec::new();
    for segment in derived {
        if fixed_covers(&segment.rule) {
            continue;
        }
        let grant = grants.iter().find(|g| {
            g.kind == PermitKind::Shell
                && (g.rule == segment.rule
                    || g.rule == crate::permit_cmd::strip_git_location(&segment.rule))
        })?;
        if !used.iter().any(|id| id == &grant.id) {
            used.push(grant.id.clone());
        }
    }
    Some(used)
}

/// The active grant ids covering every segment of `command`, or `None`
/// when the command is once-only, partly uncovered, or a guard would
/// not plainly allow it. Fixed read-only rules need no grant id.
pub(crate) fn covering_shell_grants(
    state: &StudioState,
    role: &str,
    command: &str,
    env: &crate::cli::Env,
) -> Option<Vec<String>> {
    let studio = state.studio.as_ref()?;
    if !matches!(
        crate::guard_shell::classify_command_line(
            command,
            &studio.push,
            std::path::Path::new(&studio.repo),
            env
        ),
        crate::guard_rules::GuardVerdict::Allow
    ) {
        return None;
    }
    let derived = crate::permit_rules::coverage(command).ok()?;
    let key = role_key(state, role);
    let grants = grants_for(state, &key);
    let mut used = Vec::new();
    for segment in derived {
        if fixed_covers(&segment.rule) {
            continue;
        }
        let grant = grants.iter().find(|g| {
            g.kind == PermitKind::Shell
                && (g.rule == segment.rule
                    || g.rule == crate::permit_cmd::strip_git_location(&segment.rule))
        })?;
        if !used.iter().any(|id| id == &grant.id) {
            used.push(grant.id.clone());
        }
    }
    Some(used)
}

/// The runner allow key for an auto-answer, through the same caps table
/// a recorded decision uses.
pub(crate) fn allow_key(runner: &str) -> Option<&'static str> {
    crate::permit::approval_keys(runner, ApprovalDecision::Allow)
}
