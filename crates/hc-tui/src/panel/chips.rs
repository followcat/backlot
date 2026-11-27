//! Single-source-of-truth inline action chips for Lot rows and Cast rows
//! (§1.2.3 & §1.2.5): chip definitions are shared between keyboard chip
//! cycling (`LotLevel::RowActions`), Cast chip cycling, visual chip pill
//! rendering (`« chip »`), and hint bars.

use backlot_model::summary::RoleSummary;

use super::Target;
use super::data::{self, MachineSummary};

pub(crate) fn is_role_quota_or_error(role: &RoleSummary) -> bool {
    role.failed.is_some()
        || (role.work == Some(backlot_model::summary::Work::Blocked)
            && !role.attention_user
            && role.attention_seq.is_none()
            && role.asks.as_ref().is_none_or(|a| a.is_empty()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RowChipKind {
    Allow,
    Deny,
    Reply,
    Ack,
    Switch,
    Tell,
    Peek,
    Menu,
    Open,
    Reopen,
    Apply,
    Cancel,
    DesignSign,
    Show,
    ViewSheet,
    Read,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RowChip {
    pub(crate) key: char,
    pub(crate) label: &'static str,
    pub(crate) kind: RowChipKind,
}

/// Ordered action chips for a role row (shared between Lot `RowActions` and Cast).
pub(crate) fn row_chips_for_person(role: &RoleSummary) -> Vec<RowChip> {
    if role.attention_user && role.can_answer_prompt {
        return vec![
            RowChip {
                key: 'y',
                label: "allow y",
                kind: RowChipKind::Allow,
            },
            RowChip {
                key: 'd',
                label: "deny d",
                kind: RowChipKind::Deny,
            },
            RowChip {
                key: 'p',
                label: "peek p",
                kind: RowChipKind::Peek,
            },
            RowChip {
                key: '↵',
                label: "open ↵",
                kind: RowChipKind::Open,
            },
        ];
    }
    if role.asks.as_ref().is_some_and(|a| !a.is_empty()) {
        return vec![
            RowChip {
                key: 'r',
                label: "reply r",
                kind: RowChipKind::Reply,
            },
            RowChip {
                key: 'x',
                label: "ack x",
                kind: RowChipKind::Ack,
            },
            RowChip {
                key: 't',
                label: "tell t",
                kind: RowChipKind::Tell,
            },
            RowChip {
                key: '↵',
                label: "open ↵",
                kind: RowChipKind::Open,
            },
        ];
    }
    if is_role_quota_or_error(role) {
        return vec![
            RowChip {
                key: 'w',
                label: "switch w",
                kind: RowChipKind::Switch,
            },
            RowChip {
                key: 't',
                label: "tell t",
                kind: RowChipKind::Tell,
            },
            RowChip {
                key: 'p',
                label: "peek p",
                kind: RowChipKind::Peek,
            },
            RowChip {
                key: '↵',
                label: "open ↵",
                kind: RowChipKind::Open,
            },
        ];
    }
    if role.state == "closed" {
        return vec![
            RowChip {
                key: 'o',
                label: "reopen o",
                kind: RowChipKind::Reopen,
            },
            RowChip {
                key: 'm',
                label: "menu m",
                kind: RowChipKind::Menu,
            },
        ];
    }
    if role.state == "standby" {
        return vec![
            RowChip {
                key: 't',
                label: "tell t",
                kind: RowChipKind::Tell,
            },
            RowChip {
                key: 'p',
                label: "peek p",
                kind: RowChipKind::Peek,
            },
            RowChip {
                key: 'm',
                label: "menu m",
                kind: RowChipKind::Menu,
            },
        ];
    }
    vec![
        RowChip {
            key: 't',
            label: "tell t",
            kind: RowChipKind::Tell,
        },
        RowChip {
            key: 'p',
            label: "peek p",
            kind: RowChipKind::Peek,
        },
        RowChip {
            key: 'm',
            label: "menu m",
            kind: RowChipKind::Menu,
        },
        RowChip {
            key: '↵',
            label: "open ↵",
            kind: RowChipKind::Open,
        },
    ]
}

/// Ordered action chips for any Lot `Target` (`Vec::new()` for chip-less rows
/// such as `Fold`, `ClosedFold`, `Studio`, `Input`, `Setup`, `Stage`, `Block`).
pub(crate) fn row_chips(machines: &[MachineSummary], target: &Target) -> Vec<RowChip> {
    match target {
        Target::Person(key, role_name) => data::studio(machines, key)
            .and_then(|s| s.studio.roles.iter().find(|r| r.role == *role_name))
            .map(row_chips_for_person)
            .unwrap_or_default(),
        Target::OutwardCard(..) => vec![
            RowChip {
                key: 'a',
                label: "apply a",
                kind: RowChipKind::Apply,
            },
            RowChip {
                key: 'x',
                label: "cancel x",
                kind: RowChipKind::Cancel,
            },
            RowChip {
                key: '↵',
                label: "show ↵",
                kind: RowChipKind::Show,
            },
        ],
        Target::Design(key) => {
            let waiting = data::studio(machines, key)
                .and_then(|s| super::draw_design::active_design(s.studio))
                .is_some_and(|d| d.waiting_user || d.phase.is_waiting_user());
            if waiting {
                vec![
                    RowChip {
                        key: 'r',
                        label: "reply r",
                        kind: RowChipKind::Reply,
                    },
                    RowChip {
                        key: 'a',
                        label: "sign a",
                        kind: RowChipKind::DesignSign,
                    },
                    RowChip {
                        key: '↵',
                        label: "read ↵",
                        kind: RowChipKind::Read,
                    },
                ]
            } else {
                vec![RowChip {
                    key: '↵',
                    label: "read ↵",
                    kind: RowChipKind::Read,
                }]
            }
        }
        Target::Task(..) => vec![
            RowChip {
                key: 'v',
                label: "sheet ↵",
                kind: RowChipKind::ViewSheet,
            },
            RowChip {
                key: 'm',
                label: "menu m",
                kind: RowChipKind::Menu,
            },
        ],
        Target::Talk(key, seq) => {
            let has_file = data::studio(machines, key).is_some_and(|s| {
                s.studio
                    .exchange
                    .iter()
                    .any(|e| e.seq == *seq && e.file.is_some())
            });
            if has_file {
                vec![
                    RowChip {
                        key: '↵',
                        label: "read ↵",
                        kind: RowChipKind::Read,
                    },
                    RowChip {
                        key: 't',
                        label: "tell t",
                        kind: RowChipKind::Tell,
                    },
                ]
            } else {
                vec![RowChip {
                    key: 't',
                    label: "tell t",
                    kind: RowChipKind::Tell,
                }]
            }
        }
        Target::Studio(..)
        | Target::Block(..)
        | Target::Strip(..)
        | Target::Fold(..)
        | Target::ClosedFold(..)
        | Target::Stage(..)
        | Target::Setup
        | Target::Input(..)
        | Target::ReopenProducer(..)
        | Target::None => Vec::new(),
    }
}

/// Render right-aligned inline action chips onto `rect`, highlighting `active`
/// with inverted `« label »` accent pill delimiters (§1.2.5 / §1.5).
pub(crate) fn draw_row_chips(
    buf: &mut ratatui::buffer::Buffer,
    rect: ratatui::layout::Rect,
    chips: &[RowChip],
    active: usize,
    palette: &crate::chrome::palette::Palette,
) {
    use ratatui::style::{Modifier, Style};
    if rect.width < 10 || rect.height == 0 || chips.is_empty() {
        return;
    }
    let idx = active.min(chips.len().saturating_sub(1));
    let contrast = if palette.panel_bg == ratatui::style::Color::Reset {
        palette.surface_dim
    } else {
        palette.panel_bg
    };
    let active_style = Style::default()
        .fg(contrast)
        .bg(palette.accent)
        .add_modifier(Modifier::BOLD);
    let dim_style = Style::default().fg(palette.text).bg(palette.surface0);
    let active_text = format!("« {} »", chips[idx].label);
    let active_w = crate::chrome::text::display_width(&active_text) as u16;
    let min_left = 10u16.min(rect.width.saturating_sub(active_w));
    let avail = rect.width.saturating_sub(min_left);
    if active_w > avail {
        let x = rect.right().saturating_sub(rect.width.min(active_w));
        crate::chrome::render::put_text(
            buf,
            x,
            rect.y,
            rect.right().saturating_sub(x),
            &active_text,
            active_style,
        );
        return;
    }
    // Build chips fitting inside `avail`, prioritizing the active chip and its neighbors.
    let mut parts: Vec<(String, Style)> = vec![(active_text, active_style)];
    let mut used = active_w;
    for (i, chip) in chips.iter().enumerate() {
        if i == idx {
            continue;
        }
        let text = format!(" [{}]", chip.label);
        let w = crate::chrome::text::display_width(&text) as u16;
        if used + w <= avail {
            used += w;
            if i < idx {
                parts.insert(
                    parts.len().saturating_sub(1),
                    (format!("[{}] ", chip.label), dim_style),
                );
            } else {
                parts.push((text, dim_style));
            }
        }
    }
    let mut x = rect.right().saturating_sub(used);
    for (text, style) in parts {
        let w = crate::chrome::text::display_width(&text) as u16;
        crate::chrome::render::put_text(buf, x, rect.y, w, &text, style);
        x = x.saturating_add(w);
    }
}
