//! CLI rendering helpers for shared quota accounts in `backlot status` and `status --all`.

use std::collections::BTreeSet;
use std::fmt::Write as _;

use backlot_model::summary::{AccountRoleSummary, AccountSummary, QuotaWindow};

use crate::ratelimit::format_reset_local;
use crate::render::{fmt_ms, hh_mm};

fn is_shared_across_studios(acct: &AccountSummary) -> bool {
    let studios: BTreeSet<&str> = acct.roles.iter().map(|r| r.studio.as_str()).collect();
    studios.len() > 1
}

/// Return `[account_id]` badge text when a role's account is non-default, capped,
/// stalled, or shared across multiple studios / Hollywood.
pub(crate) fn role_account_badge<'a>(
    studio: &str,
    role: &str,
    accounts: &'a [AccountSummary],
) -> Option<&'a str> {
    let acct = accounts
        .iter()
        .find(|a| a.roles.iter().any(|r| r.studio == studio && r.role == role))?;
    if !acct.id.ends_with(":default")
        || acct.max_active.is_some()
        || acct.stalled
        || is_shared_across_studios(acct)
    {
        Some(acct.id.as_str())
    } else {
        None
    }
}

/// Compact `accounts:` footer for single-studio `backlot status`.
pub(crate) fn studio_accounts_footer(
    studio: &str,
    accounts: &[AccountSummary],
    now_ms: u64,
) -> String {
    let touching: Vec<&AccountSummary> = accounts
        .iter()
        .filter(|a| a.roles.iter().any(|r| r.studio == studio))
        .collect();
    if touching.is_empty() {
        return String::new();
    }
    let mut out = String::from("  accounts:\n");
    for acct in touching {
        let state_str = if acct.stalled {
            if let Some(reset) = acct.reset_at_ms {
                format!("STALLED resets {}", format_reset_local(reset, now_ms))
            } else {
                "STALLED".into()
            }
        } else {
            "ok".into()
        };
        let last_stall = acct
            .last_stall_ms
            .map(|ts| format!(" · last stall {}", hh_mm(ts)))
            .unwrap_or_default();
        let cap_str = acct
            .max_active
            .map(|cap| format!("/{cap}"))
            .unwrap_or_default();
        let here = acct.roles.iter().filter(|r| r.studio == studio).count();
        let elsewhere_roles: Vec<&AccountRoleSummary> =
            acct.roles.iter().filter(|r| r.studio != studio).collect();
        let mut others: Vec<&str> = elsewhere_roles
            .iter()
            .map(|r| r.studio.as_str())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        others.sort_by(|a, b| (*a != "hollywood").cmp(&(*b != "hollywood")).then(a.cmp(b)));
        let sharing = if elsewhere_roles.is_empty() {
            format!("{here} here")
        } else {
            format!(
                "{here} here + {} elsewhere ({})",
                elsewhere_roles.len(),
                others.join(", ")
            )
        };
        let _ = writeln!(
            out,
            "    {}: {}{} · {}{} active ({} working) · {}",
            acct.id, state_str, last_stall, acct.active, cap_str, acct.working, sharing
        );
    }
    out
}

/// Full `accounts:` block for `backlot status --all`, rendered directly after `hollywood:`.
pub(crate) fn accounts_block(accounts: &[AccountSummary], now_ms: u64) -> String {
    if accounts.is_empty() {
        return String::new();
    }
    let mut out = String::from("accounts:\n");
    for acct in accounts {
        let src = if acct.source.is_empty() {
            String::new()
        } else {
            format!(" ({})", acct.source)
        };
        let status = if acct.stalled {
            match acct.reset_at_ms {
                Some(reset) if reset > now_ms => format!(
                    "STALLED · resets {} (in {})",
                    format_reset_local(reset, now_ms),
                    fmt_ms(reset - now_ms)
                ),
                Some(reset) => format!("STALLED · resets {}", format_reset_local(reset, now_ms)),
                None => "STALLED".into(),
            }
        } else {
            "ok".into()
        };
        let last_stall = acct.last_stall_ms.map(hh_mm).unwrap_or_else(|| "—".into());
        let window_tag = match acct.window {
            Some(QuotaWindow::Short) => " [short]",
            Some(QuotaWindow::Long) => " [long]",
            Some(QuotaWindow::Unknown) | None => "",
        };
        let label = format!("{}{src}", acct.id);
        let _ = writeln!(
            out,
            "  {label:<26} {status} · last stall {last_stall}{window_tag}"
        );
        let active_str = match (acct.max_active, acct.room) {
            (Some(cap), Some(room)) => {
                format!(
                    "active {}/{} ({} working, {room} free)",
                    acct.active, cap, acct.working
                )
            }
            (Some(cap), None) => {
                format!("active {}/{} ({} working)", acct.active, cap, acct.working)
            }
            _ => format!("active {} ({} working)", acct.active, acct.working),
        };
        let shared = match acct.roles.len() {
            0 => String::new(),
            1 => ":".into(),
            n => format!(" · shared by {n} roles:"),
        };
        let _ = writeln!(out, "    {active_str}{shared}");

        if !acct.roles.is_empty() {
            let formatted: Vec<String> = acct.roles.iter().map(format_role_item).collect();
            let mut line = String::from("    ");
            for (idx, item) in formatted.iter().enumerate() {
                let sep = if idx == 0 { "" } else { ", " };
                if idx > 0 && line.len() + sep.len() + item.len() > 88 {
                    let _ = writeln!(out, "{line},");
                    line = format!("    {item}");
                } else {
                    if idx > 0 {
                        line.push_str(", ");
                    }
                    line.push_str(item);
                }
            }
            let _ = writeln!(out, "{line}");
        }
    }
    out
}

fn format_role_item(r: &AccountRoleSummary) -> String {
    let name = if r.studio == "hollywood" && r.role == "hollywood" {
        "hollywood".to_string()
    } else {
        format!("{}/{}", r.studio, r.role)
    };
    let state = match (r.working, r.active, r.held, r.stalled) {
        (true, _, _, true) => "working, stalled",
        (true, _, _, false) => "working",
        (false, true, _, true) => "live, stalled",
        (false, true, _, false) => "live",
        (false, false, true, true) => "held, stalled",
        (false, false, true, false) => "held",
        (false, false, false, true) => "stalled",
        (false, false, false, false) => "standby",
    };
    format!("{name} ({state})")
}
