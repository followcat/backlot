//! Opening-design decision helpers for the Call Sheet (`Wait::Decision`,
//! warm peach `?N` ask pill, Q1–Q3 detail lines, chips, and sign/skip confirm).

use backlot_model::design::DesignPhase;
use backlot_model::summary::{DesignSummary, StudioSummary};

use super::items::{Chip, WaitParts};
use crate::chrome::text::truncate_end;
use crate::studio::StudioKey;
use crate::studio_dialog::{ConfirmFor, StudioDialog};

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

/// Action chips painted on the selected `Wait::Decision` row.
pub(crate) fn chips(suggest_skip: bool) -> Vec<Chip> {
    let (reply_label, sign_label) = if suggest_skip {
        ("discuss", "skip")
    } else {
        ("reply", "sign")
    };
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
            label: "open",
            key: "↵",
            primary: true,
        },
    ]
}

/// Header parts for `Wait::Decision` (`◆ DECISION  <studio>  <round/skip> · <summary>`).
pub(crate) fn wait_parts(item: &DecisionItem<'_>, local: &str, multi: bool) -> WaitParts {
    let m = super::sheet::machine_label(item.machine, local, multi);
    let machine = if m.is_empty() {
        String::new()
    } else {
        format!("{m} · ")
    };
    let badge = badge_label(item.design);
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
        rest: format!(" · {machine}{badge}{blurb}"),
    }
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

/// Confirmation modal built on `a` (`sign off` vs `skip`) naming the frozen file.
pub(crate) fn confirm_dialog(key: &StudioKey, design: &DesignSummary) -> StudioDialog {
    let place = StudioDialog::place(key);
    let frozen = format!("design/v{}-final.md", design.version);
    let suggest_skip = design.suggest_skip || design.phase == DesignPhase::SuggestSkip;
    let (title, body) = if suggest_skip {
        (
            format!("Skip design · {place}"),
            format!(
                "Mode: skip opening design for v{}.\nFreeze: {} (from {}).",
                design.version, frozen, design.doc_file
            ),
        )
    } else {
        (
            format!("Sign off design · {place} R{}", design.round),
            format!(
                "Mode: sign off opening design v{} R{}.\nFreeze: {} (from {}).",
                design.version, design.round, frozen, design.doc_file
            ),
        )
    };
    StudioDialog::confirm(
        title,
        body,
        ConfirmFor::DesignSign {
            key: key.clone(),
            suggest_skip,
        },
    )
}
