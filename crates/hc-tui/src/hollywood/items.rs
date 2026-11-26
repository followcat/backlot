//! The waiting row's text model: each item's glyph, head text,
//! action chips and detail line — kept apart from the band's
//! painter so the words and the cells change independently.

use super::rows::Live;
use super::sheet::{PromptItem, ReadyItem, RoleItem, StageItem, StudioItem, Wait, machine_label};

/// One action chip's label and its key — `allow y` draws ` allow y `.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Chip {
    pub label: &'static str,
    pub key: &'static str,
    /// The row's primary action — lit `accent` on the selected row.
    pub primary: bool,
}

/// A chip's text at full and compact widths: ` allow y ` or `[y]`.
pub(crate) fn chip_text(chip: &Chip, compact: bool) -> String {
    if compact {
        format!("[{}]", chip.key)
    } else {
        format!(" {} {} ", chip.label, chip.key)
    }
}

/// The waiting row's chips: full ` label key ` or compact `[k]`.
pub(crate) fn chips_of(wait: &Wait<'_>) -> Vec<Chip> {
    match wait {
        // A prompt row only promises the keys that work: a runner with
        // no approval key offers peek and Enter, never `allow y`.
        Wait::Prompt(_) if !wait.can_answer() => vec![
            Chip {
                label: "peek",
                key: "p",
                primary: true,
            },
            Chip {
                label: "open",
                key: "↵",
                primary: false,
            },
        ],
        Wait::Prompt(_) => vec![
            Chip {
                label: "allow",
                key: "y",
                primary: true,
            },
            Chip {
                label: "deny",
                key: "d",
                primary: false,
            },
            Chip {
                label: "peek",
                key: "p",
                primary: false,
            },
            Chip {
                label: "open",
                key: "↵",
                primary: false,
            },
        ],
        Wait::Role(item) if item.asks => vec![
            Chip {
                label: "reply",
                key: "r",
                primary: true,
            },
            Chip {
                label: "ack",
                key: "x",
                primary: false,
            },
            Chip {
                label: "peek",
                key: "p",
                primary: false,
            },
            Chip {
                label: "open",
                key: "↵",
                primary: false,
            },
        ],
        Wait::Role(_) => vec![
            Chip {
                label: "open",
                key: "↵",
                primary: true,
            },
            Chip {
                label: "peek",
                key: "p",
                primary: false,
            },
        ],
        Wait::Hollywood(_) if !wait.can_answer() => vec![
            Chip {
                label: "peek",
                key: "p",
                primary: true,
            },
            Chip {
                label: "open",
                key: "↵",
                primary: false,
            },
        ],
        Wait::Hollywood(hollywood) if hollywood.attention_user => vec![
            Chip {
                label: "allow",
                key: "y",
                primary: true,
            },
            Chip {
                label: "deny",
                key: "d",
                primary: false,
            },
            Chip {
                label: "peek",
                key: "p",
                primary: false,
            },
            Chip {
                label: "open",
                key: "↵",
                primary: false,
            },
        ],
        Wait::Hollywood(_) => vec![
            Chip {
                label: "open",
                key: "↵",
                primary: true,
            },
            Chip {
                label: "peek",
                key: "p",
                primary: false,
            },
        ],
        Wait::NoRequirement(_) => vec![
            Chip {
                label: "open",
                key: "↵",
                primary: true,
            },
            Chip {
                label: "peek",
                key: "p",
                primary: false,
            },
        ],
        Wait::Stage(item) => {
            if item.offline {
                vec![Chip {
                    label: "check",
                    key: "↵",
                    primary: true,
                }]
            } else {
                vec![Chip {
                    label: "start",
                    key: "s",
                    primary: true,
                }]
            }
        }
        Wait::Card(_) | Wait::Outward(_) => vec![
            Chip {
                label: "apply",
                key: "a",
                primary: true,
            },
            Chip {
                label: "cancel",
                key: "x",
                primary: false,
            },
            Chip {
                label: "show",
                key: "↵",
                primary: false,
            },
        ],
        Wait::Ready(_) => vec![
            Chip {
                label: "finish",
                key: "f",
                primary: true,
            },
            Chip {
                label: "open",
                key: "↵",
                primary: false,
            },
        ],
    }
}

/// Default focused chip index when moving onto a waiting row: cards
/// land on `show ↵` so Enter opens the detail view; other rows land
/// on their primary chip (`allow y`, `peek p`, `start s`, `finish f`,
/// `open ↵`).
pub(crate) fn default_chip_index(wait: &Wait<'_>, chips: &[Chip]) -> usize {
    match wait {
        Wait::Card(_) | Wait::Outward(_) => chips.iter().position(|c| c.key == "↵").unwrap_or(0),
        _ => chips.iter().position(|c| c.primary).unwrap_or(0),
    }
}

/// The live row's glyph and text: `… sending to hollywood`,
/// `⠶ hollywood: <doing>`, `⚠ hollywood asks: <prompt>`, and
/// `○ hollywood is idle and has not answered · open ↵` when the
/// screen is idle and the line still has no reply.
pub(crate) fn live_head(live: &Live<'_>) -> (String, &'static str) {
    match live {
        Live::Pending => ("sending to hollywood".into(), "…"),
        Live::Working(Some(doing)) => (format!("hollywood: {doing}"), "⠶"),
        Live::Working(None) => ("hollywood is working".into(), "⠶"),
        Live::Prompt(prompt) => (format!("hollywood asks: {prompt}"), "⚠"),
        Live::Idle => (
            "hollywood is idle and has not answered · open ↵".into(),
            "○",
        ),
    }
}

fn first_sentence(text: &str) -> &str {
    let end = text
        .char_indices()
        .find(|(_, c)| matches!(c, '.' | '?' | '!' | '\n'))
        .map(|(i, c)| i + c.len_utf8())
        .unwrap_or(text.len());
    text[..end].trim()
}

fn ask_age(studio: &backlot_model::summary::StudioSummary, role: &str) -> Option<String> {
    let ts = studio
        .exchange
        .iter()
        .filter(|e| e.ask && e.who == role)
        .filter_map(|e| e.ts_ms)
        .max()?;
    super::reader_meta::age_label(Some(ts), crate::update_state::now_ms())
}

/// Whether any waiting item demands the user's direct attention (red urgency).
pub(crate) fn any_user_urgent(waits: &[Wait<'_>]) -> bool {
    waits.iter().any(|w| {
        matches!(w, Wait::Prompt(_) | Wait::Stage(_))
            || matches!(w, Wait::Role(r) if !r.asks)
            || matches!(w, Wait::Hollywood(h) if h.attention_user)
    })
}

/// Structured pieces of a waiting row's first line so painters can style
/// the verb, studio id, and trailing detail independently.
pub(crate) struct WaitParts {
    pub glyph: &'static str,
    pub verb: String,
    pub gap: &'static str,
    pub studio: String,
    pub rest: String,
}

/// The semantic colour a waiting row's indicator, glyph, and verb share.
pub(crate) fn glyph_colour(
    wait: &Wait<'_>,
    palette: &crate::chrome::palette::Palette,
) -> ratatui::style::Color {
    match wait {
        Wait::Prompt(_) => palette.red,
        Wait::Role(item) if item.asks => palette.peach,
        Wait::Role(_) => palette.red,
        Wait::Hollywood(hollywood) if hollywood.attention_user => palette.red,
        Wait::Hollywood(_) => palette.peach,
        Wait::NoRequirement(_) => palette.accent,
        Wait::Stage(item) if item.offline => palette.red,
        Wait::Stage(_) => palette.peach,
        Wait::Card(_) | Wait::Outward(_) => palette.accent,
        Wait::Ready(_) => palette.teal,
    }
}

/// Structured pieces of a waiting item's first line.
pub(crate) fn wait_parts(wait: &Wait<'_>, local: &str, multi: bool) -> WaitParts {
    match wait {
        Wait::Prompt(PromptItem {
            machine,
            studio,
            role,
        }) => {
            let seq = role.attention_seq.unwrap_or(0);
            let machine = machine_label(*machine, local, multi);
            let machine = if machine.is_empty() {
                String::new()
            } else {
                format!("{machine} · ")
            };
            WaitParts {
                glyph: "×",
                verb: format!("PROMPT #{seq}"),
                gap: "  ",
                studio: studio.id.clone(),
                rest: format!(" · {machine}{}", role.name),
            }
        }
        Wait::Role(RoleItem {
            machine,
            studio,
            role,
            asks,
        }) => {
            let machine = machine_label(*machine, local, multi);
            let machine = if machine.is_empty() {
                String::new()
            } else {
                format!("{machine} · ")
            };
            if *asks {
                let age = ask_age(studio, &role.role)
                    .map(|a| format!(" · {a}"))
                    .unwrap_or_default();
                let question = role.asks.as_deref().unwrap_or("a question");
                WaitParts {
                    glyph: "?",
                    verb: "ASKS".into(),
                    gap: " ",
                    studio: studio.id.clone(),
                    rest: format!(
                        " · {machine}{}{age} — {}",
                        role.name,
                        first_sentence(question)
                    ),
                }
            } else {
                WaitParts {
                    glyph: "×",
                    verb: "NEEDS YOU".into(),
                    gap: " ",
                    studio: studio.id.clone(),
                    rest: format!(" · {machine}{}", role.name),
                }
            }
        }
        Wait::Hollywood(hollywood) if hollywood.attention_user => WaitParts {
            glyph: "⚠",
            verb: "HOLLYWOOD".into(),
            gap: " ",
            studio: String::new(),
            rest: format!(
                "asks: {}",
                hollywood
                    .attention_detail
                    .as_deref()
                    .or(hollywood.attention.as_deref())
                    .unwrap_or("permission prompt")
            ),
        },
        Wait::Hollywood(_) => WaitParts {
            glyph: "!",
            verb: "HOLLYWOOD".into(),
            gap: " ",
            studio: String::new(),
            rest: "blocked".into(),
        },
        Wait::NoRequirement(StudioItem { machine, studio }) => {
            let machine = machine_label(*machine, local, multi);
            let machine = if machine.is_empty() {
                String::new()
            } else {
                format!(" · {machine}")
            };
            WaitParts {
                glyph: "✦",
                verb: "NEW".into(),
                gap: " ",
                studio: studio.id.clone(),
                rest: format!("{machine} · no requirement"),
            }
        }
        Wait::Stage(StageItem {
            machine,
            offline,
            studios,
        }) => {
            let machine = machine_label(*machine, local, multi);
            let machine = if machine.is_empty() {
                local.to_string()
            } else {
                machine.to_string()
            };
            if *offline {
                WaitParts {
                    glyph: "×",
                    verb: "OFFLINE".into(),
                    gap: " ",
                    studio: machine,
                    rest: String::new(),
                }
            } else {
                WaitParts {
                    glyph: "◌",
                    verb: "STAGE STOPPED".into(),
                    gap: " ",
                    studio: machine,
                    rest: format!(
                        " · affects {studios} studio{}",
                        if *studios == 1 { "" } else { "s" }
                    ),
                }
            }
        }
        Wait::Card(card) => WaitParts {
            glyph: "◆",
            verb: format!("CARD {}", card.id),
            gap: "  ",
            studio: card.studio.clone(),
            rest: format!(" · {}", card.action),
        },
        Wait::Outward(item) => {
            let machine = machine_label(item.machine, local, multi);
            let rest = if machine.is_empty() {
                format!(" · {}", item.card.command)
            } else {
                format!(" · {machine} · {}", item.card.command)
            };
            WaitParts {
                glyph: "◆",
                verb: format!("CARD {}", item.card.id),
                gap: "  ",
                studio: item.studio.id.clone(),
                rest,
            }
        }
        Wait::Ready(ReadyItem {
            machine, studio, ..
        }) => {
            let machine = machine_label(*machine, local, multi);
            let machine = if machine.is_empty() {
                String::new()
            } else {
                format!(" · {machine}")
            };
            WaitParts {
                glyph: "✓",
                verb: "READY".into(),
                gap: " ",
                studio: studio.id.clone(),
                rest: format!(
                    "{machine} · {}/{} tasks accepted",
                    studio.tasks_done, studio.tasks_total
                ),
            }
        }
    }
}

/// The row's glyph and head text.
#[cfg(test)]
pub(crate) fn wait_head(wait: &Wait<'_>, local: &str, multi: bool) -> (String, &'static str) {
    let p = wait_parts(wait, local, multi);
    (
        format!("{}{}{}{}", p.verb, p.gap, p.studio, p.rest),
        p.glyph,
    )
}

/// The second line the needs-you rows carry — a role's what or
/// question, Hollywood's block, a new studio's next step.
pub(crate) fn detail_text(wait: &Wait<'_>) -> Option<String> {
    match wait {
        // A prompt row whose runner cannot answer says where to answer
        // instead of naming keys that would send nothing; the command
        // the picker asks about stays the detail.
        Wait::Prompt(item) if !item.role.can_answer_prompt => {
            Some(match item.role.attention_detail.as_deref() {
                Some(ask) => format!("{ask} · answer in its terminal (Enter)"),
                None => "answer in its terminal (Enter)".into(),
            })
        }
        Wait::Role(item) if item.asks => Some(
            item.role
                .asks
                .clone()
                .unwrap_or_else(|| "a question".into()),
        ),
        Wait::Role(item) => Some(
            item.role
                .attention_detail
                .as_deref()
                .or(item.role.attention.as_deref())
                .unwrap_or("needs you")
                .to_string(),
        ),
        // A runner with no approval key cannot take the decision here:
        // the row says where to answer instead of naming `y`/`d`.
        Wait::Hollywood(_) if !wait.can_answer() => Some("answer in its terminal (Enter)".into()),
        Wait::Hollywood(hollywood) if hollywood.attention_user => {
            Some("allow once with y, deny with d, peek with p, open with Enter".into())
        }
        Wait::Hollywood(_) => Some("blocked on a person — open its terminal".into()),
        Wait::NoRequirement(item) => Some(format!(
            "next: {}",
            crate::panel::data::next_step(item.studio).unwrap_or("tell the producer what to build")
        )),
        _ => None,
    }
}

/// The row's action label — what `pending` compares against.
pub(crate) fn target_label<'a>(wait: &'a Wait<'a>) -> &'a str {
    match wait {
        Wait::Prompt(item) => &item.role.role,
        Wait::Role(item) => &item.role.role,
        Wait::Hollywood(_) => "hollywood",
        Wait::NoRequirement(item) => &item.studio.id,
        Wait::Stage(item) => item.machine.unwrap_or("Local"),
        Wait::Card(card) => &card.id,
        Wait::Outward(item) => &item.card.id,
        Wait::Ready(item) => &item.studio.id,
    }
}

/// The 40-column read: `× #7 director` for a prompt — the kind word
/// and the studio go when the row cannot hold them.
pub(crate) fn ultra_head(wait: &Wait<'_>, _local: &str, _multi: bool) -> String {
    match wait {
        Wait::Prompt(item) => format!(
            "#{} {}",
            item.role.attention_seq.unwrap_or(0),
            item.role.name
        ),
        Wait::Role(item) if item.asks => item.role.name.clone(),
        Wait::Role(item) => format!("needs {}", item.role.name),
        Wait::Hollywood(_) => "hollywood".into(),
        Wait::NoRequirement(item) => item.studio.id.clone(),
        _ => String::new(),
    }
}
