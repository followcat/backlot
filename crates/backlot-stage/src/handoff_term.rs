//! Active Hollywood terminal resolution, candidate naming/isolation, proof
//! disambiguation, and quota/rate/auth/connection sampling for Hollywood (§2.3).

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use backlot_model::hollywood::{
    HOLLYWOOD, HOLLYWOOD_TERMINAL, HWRole, HollywoodEvent, HollywoodKind,
};
use backlot_model::summary::Work;

use crate::connerr::{self, ConnPace};
use crate::handoff::{HBlockedKind, HandoffRecord};
use crate::ratelimit::{self, RatePace};
use crate::reconcile::LiveTerminal;
use crate::runners::{Launch, Prep};

/// Authoritative active Hollywood terminal lookup (§2.3.1):
/// 1. Exact `role.terminal` match (excluding `backlot.shell`, `hollywood-handoff`, `hollywood-previous`).
/// 2. Unique live terminal carrying `backlot.role=hollywood`.
/// 3. Fallback `name == HOLLYWOOD_TERMINAL` without a conflicting handoff/previous label.
pub(crate) fn active_hollywood_terminal<'a>(
    role: &HWRole,
    terminals: &'a [LiveTerminal],
) -> Option<&'a LiveTerminal> {
    let eligible = |t: &&LiveTerminal| -> bool {
        !t.labels.contains_key("backlot.shell")
            && !matches!(
                t.labels.get("backlot.role").map(String::as_str),
                Some("hollywood-handoff" | "hollywood-previous")
            )
    };
    if let Some(addr_or_name) = role.terminal.as_deref()
        && let Some(found) = terminals
            .iter()
            .filter(eligible)
            .find(|t| t.address == addr_or_name || t.name == addr_or_name)
    {
        return Some(found);
    }
    let labelled: Vec<&LiveTerminal> = terminals
        .iter()
        .filter(|t| {
            !t.labels.contains_key("backlot.shell")
                && t.labels.get("backlot.role").map(String::as_str) == Some(HOLLYWOOD)
        })
        .collect();
    if labelled.len() == 1 {
        return Some(labelled[0]);
    }
    terminals
        .iter()
        .filter(eligible)
        .find(|t| t.name == HOLLYWOOD_TERMINAL)
}

/// Pick the cleanest unused terminal name for a candidate or fresh Hollywood launch.
pub(crate) fn next_candidate_name(used: &BTreeSet<String>, generation: u64) -> String {
    if !used.contains(HOLLYWOOD_TERMINAL) {
        return HOLLYWOOD_TERMINAL.to_string();
    }
    let next_name = format!("{HOLLYWOOD_TERMINAL}-next");
    if !used.contains(&next_name) {
        return next_name;
    }
    let mut k = generation.max(1);
    loop {
        let cand = format!("{HOLLYWOOD_TERMINAL}-g{k}");
        if !used.contains(&cand) {
            return cand;
        }
        k += 1;
    }
}

/// Detect auth/login failure phrases when the terminal is idle/errored (§2.3.4).
pub(crate) fn auth_error_detail(term: &LiveTerminal, screen: &str, work: Work) -> Option<String> {
    if term.status.as_ref().is_some_and(|s| s.state == "working") || work == Work::Working {
        return None;
    }
    let status_msg = term
        .status
        .as_ref()
        .filter(|s| s.state == "error")
        .map(|s| s.msg.as_str())
        .unwrap_or("");
    let tail = ratelimit::fresh_tail(screen);
    for src in [status_msg, tail] {
        for line in src.lines().rev() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if matches_auth_phrase(&trimmed.to_lowercase()) {
                return Some(trimmed.chars().take(160).collect());
            }
        }
    }
    None
}

fn matches_auth_phrase(lower: &str) -> bool {
    [
        "not signed in",
        "please sign in",
        "sign in required",
        "authentication failed",
        "invalid api key",
        "unauthorized",
        "login required",
        "oauth token expired",
    ]
    .iter()
    .any(|p| lower.contains(p))
        || (lower.contains("401")
            && (lower.contains("error")
                || lower.contains("auth")
                || lower.contains("unauthorized")))
}

/// Sample rate-limit, quota, auth, connection, and agent errors for active Hollywood.
/// Returns `(blocked_state, optional_retry_continue)` so caller decides whether to send `continue`.
pub(crate) fn sample_hollywood_error(
    term: &LiveTerminal,
    screen: &str,
    work: Work,
    rate_pace: &mut Option<RatePace>,
    conn_pace: &mut Option<ConnPace>,
    now_ms: u64,
) -> (Option<HBlockedKind>, Option<&'static str>) {
    if let Some(hit) = ratelimit::limited(term, screen, work, now_ms) {
        *conn_pace = None;
        let detail = screen_error_line(term, screen).unwrap_or_else(|| "rate limited".into());
        if hit.escalate_now {
            *rate_pace = None;
            return (Some(HBlockedKind::OutOfQuota { detail }), None);
        }
        let mut pace = rate_pace.take().unwrap_or(RatePace {
            retries: 0,
            due_ms: now_ms.saturating_add(hit.retry_wait_ms),
            reset_at_ms: hit.reset_at_ms,
        });
        if pace.reset_at_ms.is_none() {
            pace.reset_at_ms = hit.reset_at_ms;
        }
        if hit.auto_continue {
            let deadline = pace.reset_at_ms.unwrap_or(pace.due_ms);
            if now_ms >= deadline {
                *rate_pace = None;
                return (Some(HBlockedKind::OutOfQuota { detail }), None);
            }
            *rate_pace = Some(pace);
            return (Some(HBlockedKind::RateLimited { detail }), None);
        }
        match ratelimit::step(pace, pace.retries < ratelimit::MAX_RETRIES, now_ms) {
            ratelimit::Step::Wait => {
                *rate_pace = Some(pace);
                return (Some(HBlockedKind::RateLimited { detail }), None);
            }
            ratelimit::Step::Send => {
                pace.retries += 1;
                pace.due_ms =
                    now_ms.saturating_add(hit.retry_wait_ms.max(ratelimit::RETRY_MARGIN_MS));
                *rate_pace = Some(pace);
                return (
                    Some(HBlockedKind::RateLimited { detail }),
                    Some(ratelimit::CONTINUE),
                );
            }
            ratelimit::Step::Escalate => {
                return (Some(HBlockedKind::OutOfQuota { detail }), None);
            }
        }
    }
    if ratelimit::weekly_choice_dialog(screen) {
        *rate_pace = None;
        *conn_pace = None;
        let detail =
            screen_error_line(term, screen).unwrap_or_else(|| "weekly quota reached".into());
        return (Some(HBlockedKind::OutOfQuota { detail }), None);
    }
    *rate_pace = None;
    if let Some(auth) = auth_error_detail(term, screen, work) {
        *conn_pace = None;
        return (Some(HBlockedKind::AuthError { detail: auth }), None);
    }
    if connerr::matches(term, screen, work) {
        let detail = screen_error_line(term, screen).unwrap_or_else(|| "connection error".into());
        let mut pace = conn_pace.take().unwrap_or(ConnPace {
            retries: 0,
            due_ms: now_ms,
        });
        match connerr::step(pace, pace.retries < connerr::MAX_CONN_RETRIES, now_ms) {
            connerr::Step::Wait => {
                *conn_pace = Some(pace);
                return (Some(HBlockedKind::ConnectionError { detail }), None);
            }
            connerr::Step::Send => {
                pace.retries += 1;
                pace.due_ms = now_ms.saturating_add(ratelimit::RETRY_MARGIN_MS);
                *conn_pace = Some(pace);
                return (
                    Some(HBlockedKind::ConnectionError { detail }),
                    Some(ratelimit::CONTINUE),
                );
            }
            connerr::Step::Escalate => {
                return (Some(HBlockedKind::ConnectionError { detail }), None);
            }
        }
    }
    *conn_pace = None;
    if ratelimit::errored(term) {
        let detail = screen_error_line(term, screen).unwrap_or_else(|| "agent error".into());
        return (Some(HBlockedKind::AgentError { detail }), None);
    }
    (None, None)
}

fn screen_error_line(term: &LiveTerminal, screen: &str) -> Option<String> {
    if let Some(s) = term
        .status
        .as_ref()
        .filter(|s| s.state == "error" && !s.msg.trim().is_empty())
    {
        return Some(s.msg.trim().chars().take(160).collect());
    }
    ratelimit::fresh_tail(screen)
        .lines()
        .rev()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with('>'))
        .map(|l| l.chars().take(160).collect())
}

/// Redirect candidate hook/signals/cursor state away from live Hollywood (§2.3.2).
pub(crate) fn apply_handoff_isolation(
    launch: &mut Launch,
    hdir: &Path,
    handoff_id: &str,
) -> (Option<PathBuf>, Option<PathBuf>) {
    let roles_dir = hdir.join("roles").join(HOLLYWOOD);
    let cand_sig = roles_dir.join(format!("handoff-{handoff_id}-signals.jsonl"));
    let mut sig_used = false;
    let mut cursor_dir = None;

    if let Some(hooks) = launch.hooks.as_mut() {
        let cand_settings = roles_dir.join(format!("handoff-{handoff_id}-claude-settings.json"));
        let old_sig = hooks.signals.to_string_lossy().into_owned();
        let new_sig = cand_sig.to_string_lossy().into_owned();
        hooks.content = hooks.content.replace(&old_sig, &new_sig);
        let old_settings = hooks.settings.to_string_lossy().into_owned();
        let new_settings = cand_settings.to_string_lossy().into_owned();
        for a in &mut launch.argv {
            if *a == old_settings {
                *a = new_settings.clone();
            }
        }
        hooks.settings = cand_settings;
        hooks.signals = cand_sig.clone();
        sig_used = true;
    }

    if let Some(prep) = launch.prep.as_mut() {
        match prep {
            Prep::Cmd(p) => {
                p.signals = cand_sig.clone();
                sig_used = true;
            }
            Prep::Agy(p) => {
                p.signals = cand_sig.clone();
                sig_used = true;
            }
            Prep::Devin(p) => {
                p.signals = cand_sig.clone();
                sig_used = true;
            }
            Prep::Cline(p) => {
                let old = p.signals.to_string_lossy().into_owned();
                let new = cand_sig.to_string_lossy().into_owned();
                p.hook_content = p.hook_content.replace(&old, &new);
                p.signals = cand_sig.clone();
                sig_used = true;
            }
            Prep::Cursor(p) => {
                let iso = roles_dir.join(format!("cursor-handoff-{handoff_id}"));
                p.root = iso.clone();
                launch.env.insert(
                    "CURSOR_CONFIG_DIR".into(),
                    iso.join("config").to_string_lossy().into_owned(),
                );
                launch.env.insert(
                    "CURSOR_DATA_DIR".into(),
                    iso.join("data").to_string_lossy().into_owned(),
                );
                launch.env.insert(
                    "CURSOR_SANDBOX_POLICY_DIR".into(),
                    iso.join("sandbox").to_string_lossy().into_owned(),
                );
                cursor_dir = Some(iso);
            }
        }
    }
    (sig_used.then_some(cand_sig), cursor_dir)
}

/// Check whether `e` proves candidate Hollywood wrote `hollywood.reported` or `role.progress`.
pub(crate) fn matches_candidate_proof(
    e: &HollywoodEvent,
    rec: &HandoffRecord,
    old_live: bool,
) -> bool {
    if e.seq <= rec.started_seq || e.by.role != HOLLYWOOD {
        return false;
    }
    if !matches!(
        e.kind,
        HollywoodKind::Reported { .. } | HollywoodKind::RoleProgress { .. }
    ) {
        return false;
    }
    let old_differs = rec
        .old_terminal
        .as_deref()
        .is_some_and(|old| e.by.session.as_deref() == Some(old));
    if old_differs {
        return false;
    }
    let via_match = e.by.via.as_deref() == Some(format!("handoff:{}", rec.id).as_str());
    let sess_match = rec
        .candidate_terminal
        .as_deref()
        .is_some_and(|t| e.by.session.as_deref() == Some(t))
        || rec
            .candidate_name
            .as_deref()
            .is_some_and(|n| e.by.session.as_deref() == Some(n));
    via_match || sess_match || !old_live
}

pub(crate) fn labels_for_candidate(handoff_id: &str, tab: &str) -> BTreeMap<String, String> {
    BTreeMap::from([
        ("backlot.role".into(), "hollywood-handoff".into()),
        ("backlot.handoff_id".into(), handoff_id.into()),
        ("backlot.space".into(), HOLLYWOOD.into()),
        ("herdr.space".into(), "backlot".into()),
        ("herdr.tab".into(), tab.into()),
        ("herdr.seen".into(), "0".into()),
    ])
}

pub(crate) fn cleanup_candidate_artifacts(rec: &HandoffRecord) {
    if let Some(sig) = rec.candidate_signals.as_deref() {
        let _ = fs::remove_file(sig);
    }
    if let Some(dir) = rec.candidate_cursor_dir.as_deref() {
        let _ = fs::remove_dir_all(dir);
    }
}

pub(crate) fn tail_has_hook_activity(tail: &str) -> bool {
    tail.lines().any(|line| {
        serde_json::from_str::<serde_json::Value>(line)
            .ok()
            .is_some_and(|v| {
                matches!(
                    v["event"].as_str(),
                    Some("UserPromptSubmit" | "PreToolUse" | "PostToolUse" | "Stop")
                )
            })
    })
}
