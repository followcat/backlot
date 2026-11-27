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

/// Focused Lot footer hint pairs (max 3 verbful `(key, verb)` pairs).
#[allow(clippy::too_many_arguments)]
pub(super) fn focused_hint_pairs(
    selected: Option<&Target>,
    editing: bool,
    confirm: bool,
    entered: bool,
    row_actions: bool,
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
    if row_actions {
        return fit_pairs(
            &[
                ("←→", "chip"),
                ("⏎", "run"),
                ("esc", if pane_refuses { "close" } else { "row" }),
            ],
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

    let raw: [(&'static str, &'static str); 3] = match selected {
        Some(Target::Input(_)) => [("i", "tell"), ("⏎", "compose"), ("esc", esc_verb)],
        Some(Target::ReopenProducer(_)) => [("o", "reopen"), ("m", "menu"), ("esc", esc_verb)],
        Some(Target::Talk(key, seq)) => {
            let has_file = super::data::studio(machines, key).is_some_and(|st| {
                st.studio
                    .exchange
                    .iter()
                    .any(|e| e.seq == *seq && e.file.is_some())
            });
            if has_file {
                [("⏎", "read"), ("i", "tell"), ("m", "menu")]
            } else {
                [("i", "tell"), ("m", "menu"), ("esc", esc_verb)]
            }
        }
        Some(Target::Person(key, role_name)) => {
            let role = super::data::studio(machines, key)
                .and_then(|st| st.studio.roles.iter().find(|r| r.role == *role_name));
            let state = role.map(|r| r.state.as_str());
            if state == Some("closed") {
                [("o", "reopen"), ("i", "tell"), ("m", "menu")]
            } else if state == Some("standby") {
                [("v", "sheet"), ("i", "tell"), ("m", "menu")]
            } else if role.is_some_and(|r| r.asks.as_ref().is_some_and(|a| !a.is_empty())) {
                [("r", "reply"), ("←→", "chip"), ("esc", esc_verb)]
            } else if role.is_some_and(|r| r.attention_seq.is_some() && r.can_answer_prompt) {
                [("y/d", "decide"), ("←→", "chip"), ("esc", esc_verb)]
            } else if entered && role.is_some_and(|r| r.terminal.is_some()) {
                [("⏎", "open"), ("←→", "chip"), ("esc", esc_verb)]
            } else if role.is_some_and(|r| r.terminal.is_some()) {
                [("⏎", "open"), ("p", "peek"), ("m", "menu")]
            } else {
                [("⏎", "open"), ("m", "menu"), ("esc", esc_verb)]
            }
        }
        Some(Target::Task(..)) => [("v", "sheet"), ("+-", "pri"), ("i", "tell")],
        Some(Target::OutwardCard(..)) => [("a/y", "apply"), ("x/d", "cancel"), ("⏎", "show")],
        Some(Target::Design(key)) => {
            match super::data::studio(machines, key)
                .and_then(|st| super::draw_design::active_design(st.studio))
            {
                Some(d)
                    if d.suggest_skip
                        || d.phase == backlot_model::design::DesignPhase::SuggestSkip =>
                {
                    [("r", "discuss"), ("o", "read"), ("a", "skip")]
                }
                Some(d) if d.waiting_user || d.phase.is_waiting_user() => {
                    [("r", "reply"), ("o", "read"), ("a", "sign")]
                }
                _ => [("o", "read"), ("m", "menu"), ("esc", esc_verb)],
            }
        }
        Some(Target::Studio(_)) => [
            ("⏎/→", if entered { "studio" } else { "enter" }),
            ("Tab", "urgent"),
            ("esc", esc_verb),
        ],
        Some(Target::Block(_) | Target::Strip(_)) => {
            [("⏎", "expand"), ("m", "menu"), ("esc", esc_verb)]
        }
        Some(Target::Fold(_) | Target::ClosedFold(_)) => {
            [("⏎", "open"), ("i", "tell"), ("esc", esc_verb)]
        }
        Some(Target::Stage(_)) => [("s", "start"), ("m", "menu"), ("esc", esc_verb)],
        Some(Target::Setup) => [("⏎", "setup"), ("?", "keys"), ("esc", esc_verb)],
        _ => [("↑↓", "move"), ("m", "menu"), ("esc", esc_verb)],
    };
    fit_pairs(&raw, width)
}
