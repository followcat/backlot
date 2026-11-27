//! Lot header focus hint and focused 3-pair verbful footer hints (`S3 §5`).

use crate::chrome::text::display_width;

use super::PanelMode;
use super::config::StudioKeys;
use super::data::MachineSummary;
use super::rows::Target;

/// Right-aligned hint on the `lot` title row:
/// - Focused overlay: `esc back` (`esc close` when pane refuses input)
/// - Focused beside panes: `esc leave`
/// - Unfocused: `<focus_lot> focus` when room >= 22 cols, else bare `<focus_lot>`
///   (drops completely when `focus_lot` is unbound).
pub(super) fn header_hint(
    focused: bool,
    overlay: bool,
    pane_refuses: bool,
    _mode: PanelMode,
    keys: &StudioKeys,
    panel_width: u16,
    room: usize,
) -> Option<String> {
    if focused {
        let full = if pane_refuses {
            "esc close"
        } else if overlay {
            "esc back"
        } else {
            "esc leave"
        };
        if display_width(full) <= room {
            return Some(full.to_string());
        }
        return (3 <= room).then(|| "esc".to_string());
    }
    let key = keys.focus_lot_label()?;
    let full = format!("{key} focus");
    if panel_width >= 22 && display_width(&full) <= room {
        Some(full)
    } else if display_width(&key) <= room {
        Some(key)
    } else {
        None
    }
}

fn fit_pairs(
    pairs: &[(&'static str, &'static str)],
    width: usize,
) -> Vec<(&'static str, &'static str)> {
    let mut take = pairs.len().min(3);
    while take > 0 {
        if crate::chrome::render::hint_bar_width(&pairs[..take]) as usize <= width {
            return pairs[..take].to_vec();
        }
        take -= 1;
    }
    if width >= 3 {
        vec![("esc", "")]
    } else {
        Vec::new()
    }
}

/// Fit prioritized `(priority, (key, label))` hints into `width`, first
/// compacting `tab/⇧tab sec` -> `tab/⇧tab` if tight, then dropping the
/// lowest-priority item first (rightmost on ties) while keeping display order.
fn fit_prioritized(
    items: &[(u8, (&'static str, &'static str))],
    width: usize,
) -> Vec<(&'static str, &'static str)> {
    let raw: Vec<_> = items.iter().map(|(_, p)| *p).collect();
    if crate::chrome::render::hint_bar_width(&raw) as usize <= width {
        return raw;
    }
    let mut active: Vec<(u8, (&'static str, &'static str))> = items
        .iter()
        .map(|&(pri, pair)| {
            if pair.0 == "tab/⇧tab" && !pair.1.is_empty() {
                (pri, (pair.0, ""))
            } else {
                (pri, pair)
            }
        })
        .collect();
    loop {
        let cur: Vec<_> = active.iter().map(|(_, p)| *p).collect();
        if crate::chrome::render::hint_bar_width(&cur) as usize <= width {
            return cur;
        }
        if active.len() <= 1 {
            break;
        }
        let min_pri = active.iter().map(|(p, _)| *p).min().unwrap_or(0);
        if let Some(drop_at) = active.iter().rposition(|(p, _)| *p == min_pri) {
            active.remove(drop_at);
        }
    }
    if width >= 3 {
        vec![("esc", "")]
    } else {
        Vec::new()
    }
}

/// Focused Lot footer hint pairs (verbful `(key, verb)` pairs, dropping
/// least-useful hints first under narrow dock widths).
pub(super) fn focused_hint_pairs(
    selected: Option<&Target>,
    editing: bool,
    confirm: bool,
    machines: &[MachineSummary],
    overlay: bool,
    pane_refuses: bool,
    width: usize,
) -> Vec<(&'static str, &'static str)> {
    let esc_verb: &'static str = if pane_refuses {
        "close"
    } else if overlay {
        "back"
    } else {
        "leave"
    };
    if confirm {
        return fit_pairs(
            &[("y", "finish"), ("d", "cancel"), ("esc", esc_verb)],
            width,
        );
    }
    if editing && matches!(selected, Some(Target::Input(_))) {
        let wide = [("⏎", "send"), ("↑↓", "history"), ("esc", "done")];
        if crate::chrome::render::hint_bar_width(&wide) as usize <= width {
            return wide.to_vec();
        }
        let mid = [("⏎ ↑↓", ""), ("esc", "")];
        if crate::chrome::render::hint_bar_width(&mid) as usize <= width {
            return mid.to_vec();
        }
        return if width >= 3 {
            vec![("esc", "")]
        } else {
            Vec::new()
        };
    }

    const TAB: (u8, (&str, &str)) = (2, ("tab/⇧tab", "sec"));
    let items: Vec<(u8, (&'static str, &'static str))> = match selected {
        Some(Target::Input(_)) => vec![(3, ("i/⏎", "compose")), TAB, (3, ("esc", esc_verb))],
        Some(Target::ReopenProducer(_)) => {
            vec![
                (3, ("o", "reopen")),
                TAB,
                (1, ("m", "menu")),
                (3, ("esc", esc_verb)),
            ]
        }
        Some(Target::Talk(key, seq)) => {
            let has_file = super::data::studio(machines, key).is_some_and(|st| {
                st.studio
                    .exchange
                    .iter()
                    .any(|e| e.seq == *seq && e.file.is_some())
            });
            if has_file {
                vec![
                    (3, ("⏎", "read")),
                    (2, ("i", "tell")),
                    TAB,
                    (1, ("m", "menu")),
                ]
            } else {
                vec![
                    (3, ("i", "tell")),
                    TAB,
                    (1, ("m", "menu")),
                    (3, ("esc", esc_verb)),
                ]
            }
        }
        Some(Target::Person(key, role_name)) => {
            let role = super::data::studio(machines, key)
                .and_then(|st| st.studio.roles.iter().find(|r| r.role == *role_name));
            let state = role.map(|r| r.state.as_str());
            if state == Some("closed") {
                vec![
                    (3, ("o", "reopen")),
                    (2, ("i", "tell")),
                    TAB,
                    (1, ("m", "menu")),
                ]
            } else if state == Some("standby") {
                vec![
                    (3, ("v", "sheet")),
                    (2, ("i", "tell")),
                    TAB,
                    (1, ("m", "menu")),
                ]
            } else if role.is_some_and(|r| r.attention_seq.is_some() && r.can_answer_prompt) {
                vec![
                    (3, ("y/d", "decide")),
                    (2, ("p", "peek")),
                    TAB,
                    (1, ("m", "menu")),
                ]
            } else if role.is_some_and(|r| r.terminal.is_some()) {
                vec![
                    (3, ("⏎", "open")),
                    (2, ("p", "peek")),
                    TAB,
                    (1, ("m", "menu")),
                ]
            } else {
                vec![
                    (3, ("⏎", "open")),
                    TAB,
                    (1, ("m", "menu")),
                    (3, ("esc", esc_verb)),
                ]
            }
        }
        Some(Target::Task(..)) => {
            vec![
                (3, ("v", "sheet")),
                (2, ("+-", "pri")),
                TAB,
                (1, ("i", "tell")),
            ]
        }
        Some(Target::OutwardCard(..)) => vec![
            (3, ("a/y", "apply")),
            (3, ("x/d", "cancel")),
            (2, ("⏎", "show")),
            (1, ("tab/⇧tab", "sec")),
        ],
        Some(Target::Design(key)) => {
            match super::data::studio(machines, key)
                .and_then(|st| super::draw_design::active_design(st.studio))
            {
                Some(d)
                    if d.suggest_skip
                        || d.phase == backlot_model::design::DesignPhase::SuggestSkip =>
                {
                    vec![
                        (3, ("r", "discuss")),
                        (2, ("o", "read")),
                        (3, ("a", "skip")),
                        (1, ("tab/⇧tab", "sec")),
                    ]
                }
                Some(d) if d.waiting_user || d.phase.is_waiting_user() => vec![
                    (3, ("r", "reply")),
                    (2, ("o", "read")),
                    (3, ("a", "sign")),
                    (1, ("tab/⇧tab", "sec")),
                ],
                _ => vec![
                    (3, ("o", "read")),
                    TAB,
                    (1, ("m", "menu")),
                    (3, ("esc", esc_verb)),
                ],
            }
        }
        Some(Target::Studio(_)) => {
            vec![
                (3, ("⏎", "focus")),
                TAB,
                (1, ("m", "menu")),
                (3, ("esc", esc_verb)),
            ]
        }
        Some(Target::Block(_) | Target::Strip(_)) => {
            vec![
                (3, ("⏎", "expand")),
                TAB,
                (1, ("m", "menu")),
                (3, ("esc", esc_verb)),
            ]
        }
        Some(Target::Fold(_) | Target::ClosedFold(_)) => {
            vec![
                (3, ("⏎", "open")),
                (2, ("i", "tell")),
                TAB,
                (3, ("esc", esc_verb)),
            ]
        }
        Some(Target::Stage(_)) => {
            vec![
                (3, ("s", "start")),
                TAB,
                (1, ("m", "menu")),
                (3, ("esc", esc_verb)),
            ]
        }
        Some(Target::Setup) => {
            vec![
                (3, ("⏎", "setup")),
                TAB,
                (1, ("?", "keys")),
                (3, ("esc", esc_verb)),
            ]
        }
        _ => vec![
            (3, ("↑↓", "move")),
            (1, ("tab/⇧tab", "sec")),
            (2, ("m", "menu")),
            (3, ("esc", esc_verb)),
        ],
    };
    fit_prioritized(&items, width)
}
