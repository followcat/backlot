//! The waiting row's text model: each item's glyph, head text,
//! action chips and detail line — kept apart from the band's
//! painter so the words and the cells change independently.

use super::sheet::{PromptItem, ReadyItem, StageItem, Wait, machine_label};

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
        Wait::Card(_) => vec![
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
                key: "s",
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

/// The row's glyph and head text.
pub(crate) fn wait_head(wait: &Wait<'_>, local: &str, multi: bool) -> (String, &'static str) {
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
            (
                format!("PROMPT #{seq}  {} · {machine}{}", studio.id, role.name),
                "×",
            )
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
                (format!("OFFLINE {machine}"), "×")
            } else {
                (
                    format!(
                        "STAGE STOPPED {machine} · affects {studios} studio{}",
                        if *studios == 1 { "" } else { "s" }
                    ),
                    "◌",
                )
            }
        }
        Wait::Card(card) => (
            format!("CARD {}  {} · {}", card.id, card.studio, card.action),
            "◆",
        ),
        Wait::Ready(ReadyItem {
            machine, studio, ..
        }) => {
            let machine = machine_label(*machine, local, multi);
            let machine = if machine.is_empty() {
                String::new()
            } else {
                format!(" · {machine}")
            };
            (
                format!(
                    "READY {}{machine} · {}/{} tasks accepted",
                    studio.id, studio.tasks_done, studio.tasks_total
                ),
                "✓",
            )
        }
    }
}

/// The row's action label — what `pending` compares against.
pub(crate) fn target_label<'a>(wait: &'a Wait<'a>) -> &'a str {
    match wait {
        Wait::Prompt(item) => &item.role.role,
        Wait::Stage(item) => item.machine.unwrap_or("Local"),
        Wait::Card(card) => &card.id,
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
        _ => String::new(),
    }
}
