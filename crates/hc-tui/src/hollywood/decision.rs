//! Opening-design decision helpers for the Call Sheet (`Wait::Decision`,
//! warm peach `?N` ask pill, Q1–Q3 detail lines, chips, and sign/skip confirm).

use backlot_model::design::DesignPhase;
use backlot_model::summary::{DesignSummary, StudioSummary};

use super::items::{Chip, WaitParts};
use crate::chrome::text::truncate_end;

/// One studio whose opening design is waiting on a user decision.
#[derive(Debug, Clone, Copy)]
pub(crate) struct DecisionItem<'a> {
    pub machine: Option<&'a str>,
    pub studio: &'a StudioSummary,
    pub design: &'a DesignSummary,
}

/// Active waiting design summary when the user must reply, discuss, sign, or skip.
pub(crate) fn waiting_design(studio: &StudioSummary) -> Option<&DesignSummary> {
    if studio.needs_upgrade || studio.state == "finished" {
        return None;
    }
    studio
        .design
        .as_ref()
        .filter(|d| d.waiting_user || d.phase.is_waiting_user())
}

/// Short round/skip badge text for the main row (`skip suggested` or `R<r>`).
pub(crate) fn badge_label(design: &DesignSummary) -> String {
    if design.suggest_skip || design.phase == DesignPhase::SuggestSkip {
        "skip suggested".to_owned()
    } else {
        format!("R{}", design.round)
    }
}

/// Format `N check` or `N checks`.
pub(crate) fn checks_count_phrase(n: usize) -> String {
    if n == 1 {
        "1 check".to_owned()
    } else {
        format!("{n} checks")
    }
}

/// Action chips painted on the selected `Wait::Decision` row.
pub(crate) fn chips(suggest_skip: bool, has_checks: bool) -> Vec<Chip> {
    let (reply_label, sign_label) = if suggest_skip {
        ("discuss", "skip")
    } else {
        ("reply", "sign")
    };
    let enter_label = if has_checks { "show" } else { "open" };
    vec![
        Chip {
            label: "read",
            key: "o",
            primary: false,
        },
        Chip {
            label: reply_label,
            key: "r",
            primary: false,
        },
        Chip {
            label: sign_label,
            key: "a",
            primary: false,
        },
        Chip {
            label: enter_label,
            key: "↵",
            primary: true,
        },
    ]
}

/// Header parts for `Wait::Decision` (`◆ DECISION  <studio>  <round/skip> [· +N checks] · <summary>`).
pub(crate) fn wait_parts(item: &DecisionItem<'_>, local: &str, multi: bool) -> WaitParts {
    let m = super::sheet::machine_label(item.machine, local, multi);
    let machine = if m.is_empty() {
        String::new()
    } else {
        format!("{m} · ")
    };
    let badge = badge_label(item.design);
    let checks = if item.design.proposed_checks.is_empty() {
        String::new()
    } else {
        format!(
            " · +{}",
            checks_count_phrase(item.design.proposed_checks.len())
        )
    };
    let blurb = if item.design.suggest_skip {
        item.design.skip_reason.as_deref()
    } else {
        item.design.summary.as_deref()
    }
    .map(str::trim)
    .filter(|s| !s.is_empty())
    .map(|s| format!(" · {}", truncate_end(s, 64)))
    .unwrap_or_default();
    WaitParts {
        glyph: "◆",
        verb: "DECISION".into(),
        gap: "  ",
        studio: item.studio.id.clone(),
        rest: format!(" · {machine}{badge}{checks}{blurb}"),
    }
}

/// Render proposed checks verbatim in a fenced code block so commands and
/// `expect` regexes are shown untouched ahead of sign-off.
pub(crate) fn format_proposed_checks_block(checks: &[backlot_model::event::CheckDecl]) -> String {
    if checks.is_empty() {
        return String::new();
    }
    let mut out = format!(
        "Signing off also activates {}:\n\n```",
        checks_count_phrase(checks.len())
    );
    for c in checks {
        out.push('\n');
        out.push_str(&format!("{} (timeout {}s)\n", c.name, c.timeout_s));
        out.push_str(&format!("  run: {}\n", c.run));
        if let Some(pat) = &c.expect {
            let min = c.effective_min_matches();
            if min == 1 {
                out.push_str(&format!("  expect: {pat}\n"));
            } else {
                out.push_str(&format!("  expect: {pat} (min {min})\n"));
            }
        }
    }
    out.push_str("```");
    out
}

/// Build the `↵` Decision show-view body: proposed checks first (verbatim),
/// followed by the summary/skip reason and open questions.
pub(crate) fn show_body(design: &DesignSummary) -> String {
    let mut sections = Vec::new();
    let checks = format_proposed_checks_block(&design.proposed_checks);
    if !checks.is_empty() {
        sections.push(checks);
    }
    if design.suggest_skip {
        if let Some(reason) = design.skip_reason.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            sections.push(format!("Skip suggested: {reason}"));
        }
    } else if let Some(summary) = design.summary.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        sections.push(summary.to_owned());
    }
    if !design.questions.is_empty() {
        let qs: Vec<String> = design
            .questions
            .iter()
            .map(|q| {
                let sec = q
                    .section_ref
                    .as_deref()
                    .filter(|s| !s.trim().is_empty())
                    .map(|s| format!(" · {s}"))
                    .unwrap_or_default();
                format!("- **{}**: {} → {}{}", q.id, q.prompt, q.recommendation, sec)
            })
            .collect();
        sections.push(qs.join("\n"));
    }
    sections.join("\n\n")
}

/// Up to 3 detail lines beneath a `Wait::Decision` row (`Q1..Q3` with rec + ref,
/// or skip reason / summary when there are no structured questions).
pub(crate) fn detail_lines(item: &DecisionItem<'_>) -> Vec<String> {
    if !item.design.questions.is_empty() {
        return item
            .design
            .questions
            .iter()
            .take(3)
            .map(|q| {
                let sec = q
                    .section_ref
                    .as_deref()
                    .filter(|s| !s.trim().is_empty())
                    .map(|s| format!(" · {s}"))
                    .unwrap_or_default();
                format!("{}: {} → {}{}", q.id, q.prompt, q.recommendation, sec)
            })
            .collect();
    }
    let fallback = if item.design.suggest_skip {
        item.design.skip_reason.as_deref()
    } else {
        item.design.summary.as_deref()
    };
    fallback
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| vec![s.to_owned()])
        .unwrap_or_default()
}
