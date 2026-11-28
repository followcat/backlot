//! Safe auto-answering of permission prompts already covered by active
//! grants plus the fixed read-only permission set.

use backlot_model::event::{ApprovalDecision, PermitKind};
use backlot_model::permit::{grants_for, role_key};
use backlot_model::state::StudioState;

fn squash(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Whether a prompt's visible command is the same command the hook
/// request named. Any mismatch — or a screen that names no command —
/// stops auto-answering.
pub(crate) fn screen_matches(runner: &str, screen: &str, command: &str) -> bool {
    crate::runners::prompt_detail(runner, screen)
        .is_some_and(|shown| squash(&shown) == squash(command))
}

fn fixed_covers(rule: &str) -> bool {
    crate::permit_cmd::fixed_covers(rule)
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
