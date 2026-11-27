//! The Call Sheet's footer hint row: the selected row's own keys on
//! the left, then the fixed right block. The slots keep one order and
//! a row's footer only lights the slots its own dispatch runs — a key
//! the row would eat is never advertised, and a key it does run stays
//! named here or sits in its `m` menu.

use super::rows::Target;
use super::sheet::Wait;
use crate::studio::StudioKey;

const RIGHT: [(&str, &str); 4] = [
    ("tab ←→ ↑↓ ⏎", ""),
    ("m", "menu"),
    ("?", "help"),
    ("esc", ""),
];
const RIGHT_READ: [(&str, &str); 4] = [
    ("tab ↑↓ ⏎", "read"),
    ("m", "menu"),
    ("?", "help"),
    ("esc", ""),
];
const RIGHT_MID: [(&str, &str); 3] = [("tab ←→ ↑↓ ⏎", ""), ("m", "menu"), ("esc", "")];
const RIGHT_READ_MID: [(&str, &str); 3] = [("tab ↑↓ ⏎", "read"), ("m", "menu"), ("esc", "")];
const RIGHT_BARE: [(&str, &str); 1] = [("↑↓ ⏎ esc", "")];

/// The row-scoped slots in their fixed order — `(key, word)` and compact bare key.
const SLOTS: [(&str, &str); 6] = [
    ("y/d", "answer"),
    ("a/x", "card"),
    ("f", "finish"),
    ("s", "start"),
    ("p", "peek"),
    ("i", "tell"),
];

/// Which slots the selected row answers, in `SLOTS`' order: `y/d` on
/// a prompt (Hollywood's own only while `hollywood_asking`), `a/x` on
/// a card, `f` on a ready studio (its frame included), `s` on a stage
/// row whose machine can start — or on a studio frame while its
/// machine's stage is down — `p` on prompts, studio frames and
/// Hollywood's live row, and `i` while an agent is configured — the
/// tell line is the sheet's own, and the key dies without one.
/// Every lit slot is one the row's dispatch would actually run.
fn slots(
    selected: Option<&Target>,
    waits: &[Wait<'_>],
    agent: bool,
    hollywood_asking: bool,
) -> [bool; 6] {
    // A frame's `f` follows its studio: the READY row's wait is the
    // marker both read.
    let ready = |key: &StudioKey| {
        waits
            .iter()
            .any(|w| w.target() == Target::Ready(key.clone()))
    };
    // A frame's `s` follows its machine: the STAGE STOPPED row's wait
    // names the machine whose stage the frame's `s` starts.
    let stage_down = |key: &StudioKey| {
        waits.iter().any(|w| {
            matches!(
                w,
                Wait::Stage(item)
                    if !item.offline && item.machine == key.machine.as_deref()
            )
        })
    };
    // A prompt row's `y/d` lights only while the runner can answer it;
    // otherwise the row is Enter and `p` only.
    let prompt_answers = |target: &Target| {
        waits
            .iter()
            .find(|w| &w.target() == target)
            .is_some_and(|w| w.can_answer())
    };
    match selected {
        Some(target @ Target::Prompt(..)) => {
            [prompt_answers(target), false, false, false, true, agent]
        }
        Some(Target::Card(_) | Target::OutwardCard(..)) => {
            [false, true, false, false, false, agent]
        }
        Some(Target::Stage(_, offline)) => [false, false, false, !offline, false, agent],
        // R3-N5: a READY row has no peek of its own — `p` stays on
        // prompt rows and studio frames only.
        Some(Target::Ready(key)) => [false, false, ready(key), false, false, agent],
        Some(Target::Studio(key)) => [false, false, ready(key), stage_down(key), true, agent],
        // Hollywood's prompt / live row answers only while its prompt stands;
        // `p` and the tell line are always live.
        Some(Target::Hollywood | Target::HollywoodLive) => {
            [hollywood_asking, false, false, false, true, agent]
        }
        _ => [false, false, false, false, false, agent],
    }
}

fn pairs_width(pairs: &[(&str, &str)]) -> usize {
    usize::from(crate::chrome::render::hint_bar_width(pairs))
}

#[cfg(test)]
fn join_pairs(pairs: &[(&str, &str)]) -> String {
    pairs
        .iter()
        .map(|(k, l)| {
            if l.is_empty() {
                (*k).to_string()
            } else if k.is_empty() {
                (*l).to_string()
            } else {
                format!("{k} {l}")
            }
        })
        .collect::<Vec<_>>()
        .join("  ")
}

/// Structured footer pairs for the selected row.
pub(crate) fn pairs_for_row(
    selected: Option<&Target>,
    editing: bool,
    waits: &[Wait<'_>],
    agent: bool,
    hollywood_asking: bool,
    width: usize,
) -> Vec<(&'static str, &'static str)> {
    if editing && matches!(selected, Some(Target::Input)) {
        let wide: &[(&str, &str)] = &[
            ("⏎", "send"),
            ("↑↓", "history"),
            ("tab", "next zone"),
            ("esc", "rows"),
            ("alt+y", "close"),
        ];
        let mid: &[(&str, &str)] = &[
            ("⏎", "send"),
            ("↑↓", "hist"),
            ("tab", "zone"),
            ("esc", "rows"),
        ];
        let bare: &[(&str, &str)] = &[("⏎ ↑↓ esc", "")];
        return [wide, mid, bare]
            .into_iter()
            .find(|tier| pairs_width(tier) <= width)
            .unwrap_or(&[])
            .to_vec();
    }
    let mask = slots(selected, waits, agent, hollywood_asking);
    let mut full: Vec<(&'static str, &'static str)> = if let Some(Target::Decision(..)) = selected {
        let skip = waits
            .iter()
            .find(|w| selected == Some(&w.target()))
            .is_some_and(|w| matches!(w, Wait::Decision(item) if item.design.suggest_skip));
        let mut pairs = vec![
            ("o", "read"),
            ("r", if skip { "discuss" } else { "reply" }),
            ("a", if skip { "skip" } else { "sign" }),
        ];
        if agent {
            pairs.push(("i", "tell"));
        }
        pairs
    } else {
        Vec::new()
    };
    let mut compact: Vec<(&'static str, &'static str)> = Vec::new();
    let mut compact_keys = String::new();
    if full.is_empty() {
        for (i, lit) in mask.iter().enumerate() {
            if !lit {
                continue;
            }
            let (key, word) = SLOTS[i];
            full.push((key, word));
            if !compact_keys.is_empty() {
                compact_keys.push(' ');
            }
            compact_keys.push_str(key);
        }
    }
    let compact_str: &'static str = if compact_keys.is_empty() {
        ""
    } else {
        let joined: String = full.iter().map(|(k, _)| *k).collect::<Vec<_>>().join(" ");
        // Map static compact combinations without leaking; fall through to exact slot keys.
        match joined.as_str() {
            "y/d p i" => "y/d p i",
            _ => "",
        }
    };
    let (right, right_mid) = match selected {
        Some(Target::Headline | Target::Talk(_)) => (&RIGHT_READ[..], &RIGHT_READ_MID[..]),
        _ => (&RIGHT[..], &RIGHT_MID[..]),
    };
    let mut full_row = full.clone();
    full_row.extend_from_slice(right);
    if pairs_width(&full_row) <= width {
        return full_row;
    }
    if !compact_str.is_empty() {
        compact.push((compact_str, ""));
    } else {
        for (key, _) in &full {
            compact.push((*key, ""));
        }
    }
    compact.extend_from_slice(right_mid);
    if pairs_width(&compact) <= width {
        return compact;
    }
    if pairs_width(&RIGHT_BARE) <= width {
        return RIGHT_BARE.to_vec();
    }
    Vec::new()
}

/// Structured footer pairs for confirm prompts.
pub(crate) fn pairs_for_confirm(width: usize) -> Vec<(&'static str, &'static str)> {
    let full: &[(&str, &str)] = &[("y", "confirm"), ("n / esc", "cancel")];
    let short: &[(&str, &str)] = &[("y/n/esc", "")];
    [full, short]
        .into_iter()
        .find(|tier| pairs_width(tier) <= width)
        .unwrap_or(&[])
        .to_vec()
}

/// Structured footer pairs accounting for active Hollywood handoff.
pub(crate) fn pairs_for_row_with_handoff(
    selected: Option<&Target>,
    editing: bool,
    waits: &[Wait<'_>],
    agent: bool,
    hollywood_asking: bool,
    width: usize,
) -> Vec<(&'static str, &'static str)> {
    if !editing {
        let hw = waits.iter().find_map(|w| match w {
            Wait::Hollywood(h) => Some(*h),
            _ => None,
        });
        if let Some(hw) = hw
            && super::handoff_ui::is_handoff_active(hw)
        {
            let candidates: &[&[(&'static str, &'static str)]] =
                match super::handoff_ui::handoff_phase(hw) {
                    Some("verifying") => &[
                        &[
                            ("↑↓", "row"),
                            ("←→", "chip"),
                            ("tab", "zone"),
                            ("p", "peek candidate"),
                            ("x", "cancel switch"),
                            ("w", "switch agent"),
                            ("esc", "close"),
                        ],
                        &[
                            ("p", "peek candidate"),
                            ("x", "cancel switch"),
                            ("w", "switch"),
                            ("esc", ""),
                        ],
                        &[("p", "peek"), ("x", "cancel"), ("esc", "")],
                    ],
                    Some("needs_approval") => &[
                        &[
                            ("y/d", "answer"),
                            ("p", "peek"),
                            ("x", "cancel switch"),
                            ("w", "switch agent"),
                            ("esc", ""),
                        ],
                        &[
                            ("y/d", "answer"),
                            ("p", "peek"),
                            ("x", "cancel"),
                            ("esc", ""),
                        ],
                    ],
                    _ => &[
                        &[
                            ("↑↓", "row"),
                            ("←→", "chip"),
                            ("tab", "zone"),
                            ("x", "cancel switch"),
                            ("w", "switch agent"),
                            ("esc", "close"),
                        ],
                        &[("x", "cancel switch"), ("w", "switch"), ("esc", "")],
                        &[("x", "cancel"), ("esc", "")],
                    ],
                };
            if let Some(fit) = candidates.iter().find(|tier| pairs_width(tier) <= width) {
                return (*fit).to_vec();
            }
        }
        if matches!(selected, Some(Target::Hollywood | Target::HollywoodLive))
            && hw.is_some_and(super::handoff_ui::is_quota_or_error)
        {
            let candidates: &[&[(&'static str, &'static str)]] = &[
                &[
                    ("w", "switch agent"),
                    ("p", "peek"),
                    ("i", "tell"),
                    ("tab ←→ ↑↓ ⏎", ""),
                    ("m", "menu"),
                    ("?", "help"),
                    ("esc", ""),
                ],
                &[
                    ("w", "switch"),
                    ("p", "peek"),
                    ("i", "tell"),
                    ("tab ↑↓ ⏎", ""),
                    ("esc", ""),
                ],
            ];
            if let Some(fit) = candidates.iter().find(|tier| pairs_width(tier) <= width) {
                return (*fit).to_vec();
            }
        }
        if matches!(selected, Some(Target::Title)) && agent {
            let mut with_w = vec![("w", "switch agent")];
            let base = pairs_for_row(selected, editing, waits, agent, hollywood_asking, width);
            with_w.extend(base.iter().copied());
            if pairs_width(&with_w) <= width {
                return with_w;
            }
            return base;
        }
    }
    pairs_for_row(selected, editing, waits, agent, hollywood_asking, width)
}

/// Text form of the selected row's footer (retained for existing tests).
#[cfg(test)]
pub(crate) fn for_row(
    selected: Option<&Target>,
    editing: bool,
    waits: &[Wait<'_>],
    agent: bool,
    hollywood_asking: bool,
    width: usize,
) -> String {
    join_pairs(&pairs_for_row(
        selected,
        editing,
        waits,
        agent,
        hollywood_asking,
        width,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hollywood::sheet::{Conn, Status, waiting_items};
    use crate::hollywood::tests::{machines, summary, with_user_prompt};
    use crate::panel::data::MachineSummary;

    /// The waits of `list` with every machine online and its stage
    /// live.
    fn waits_of(list: &[MachineSummary]) -> Vec<Wait<'_>> {
        let status: &Status = &|_| (Conn::Online, true);
        waiting_items(list, status)
    }

    /// The waits with every machine's stage down.
    fn waits_stage_down(list: &[MachineSummary]) -> Vec<Wait<'_>> {
        let status: &Status = &|_| (Conn::Online, false);
        waiting_items(list, status)
    }

    #[test]
    fn a_prompt_row_names_its_answers() {
        let list = vec![MachineSummary {
            machine: None,
            summary: with_user_prompt("acme-shop", "actor-alice"),
        }];
        let waits = waits_of(&list);
        let selected = Target::Prompt(StudioKey::local("acme-shop"), "actor-alice".into());
        assert_eq!(
            for_row(Some(&selected), false, &waits, true, false, 200),
            "y/d answer  p peek  i tell  tab ←→ ↑↓ ⏎  m menu  ? help  esc"
        );
    }

    #[test]
    fn rows_without_own_keys_name_only_the_tell_and_the_block() {
        let list = machines(Some(&summary()));
        let waits = waits_of(&list);
        let with_tell = "i tell  tab ←→ ↑↓ ⏎  m menu  ? help  esc";
        for selected in [Target::Calm, Target::Input] {
            assert_eq!(
                for_row(Some(&selected), false, &waits, true, false, 200),
                with_tell,
                "{selected:?}"
            );
        }
        assert_eq!(
            for_row(None, false, &waits, true, false, 200),
            with_tell,
            "no selection"
        );
        let with_read = "i tell  tab ↑↓ ⏎ read  m menu  ? help  esc";
        for selected in [Target::Headline, Target::Talk(7)] {
            assert_eq!(
                for_row(Some(&selected), false, &waits, true, false, 200),
                with_read,
                "{selected:?}"
            );
        }
        let bare = "tab ←→ ↑↓ ⏎  m menu  ? help  esc";
        for selected in [Target::Calm, Target::Input, Target::Setup] {
            assert_eq!(
                for_row(Some(&selected), false, &waits, false, false, 200),
                bare,
                "{selected:?}"
            );
        }
        assert_eq!(
            for_row(Some(&Target::Headline), false, &waits, false, false, 200),
            "tab ↑↓ ⏎ read  m menu  ? help  esc"
        );
        assert_eq!(
            for_row(None, false, &waits, false, false, 200),
            bare,
            "no selection"
        );
    }

    #[test]
    fn a_card_row_names_apply_and_cancel() {
        let list = machines(Some(&summary()));
        let waits = waits_of(&list);
        let selected = Target::Card("c3".into());
        assert_eq!(
            for_row(Some(&selected), false, &waits, true, false, 200),
            "a/x card  i tell  tab ←→ ↑↓ ⏎  m menu  ? help  esc"
        );
    }

    #[test]
    fn a_down_stage_row_names_s_start_an_offline_one_does_not() {
        let list = machines(Some(&summary()));
        let waits = waits_stage_down(&list);
        let down = Target::Stage(None, false);
        assert_eq!(
            for_row(Some(&down), false, &waits, true, false, 200),
            "s start  i tell  tab ←→ ↑↓ ⏎  m menu  ? help  esc"
        );
        let offline = Target::Stage(Some("host-b".into()), true);
        assert_eq!(
            for_row(Some(&offline), false, &waits, true, false, 200),
            "i tell  tab ←→ ↑↓ ⏎  m menu  ? help  esc"
        );
    }

    #[test]
    fn a_ready_studio_names_f_finish_from_its_row_and_frame() {
        let mut ready = summary();
        ready.studios[0].ready_to_finish = true;
        let list = machines(Some(&ready));
        let waits = waits_of(&list);
        let key = StudioKey::local("acme-shop");
        assert_eq!(
            for_row(
                Some(&Target::Ready(key.clone())),
                false,
                &waits,
                true,
                false,
                200
            ),
            "f finish  i tell  tab ←→ ↑↓ ⏎  m menu  ? help  esc"
        );
        assert_eq!(
            for_row(Some(&Target::Studio(key)), false, &waits, true, false, 200),
            "f finish  p peek  i tell  tab ←→ ↑↓ ⏎  m menu  ? help  esc"
        );
    }

    #[test]
    fn a_live_studio_frame_names_peek_but_never_finish() {
        let list = machines(Some(&summary()));
        let waits = waits_of(&list);
        let selected = Target::Studio(StudioKey::local("acme-shop"));
        assert_eq!(
            for_row(Some(&selected), false, &waits, true, false, 200),
            "p peek  i tell  tab ←→ ↑↓ ⏎  m menu  ? help  esc"
        );
    }

    #[test]
    fn the_narrow_tiers_compact_then_drop_the_left() {
        let list = vec![MachineSummary {
            machine: None,
            summary: with_user_prompt("acme-shop", "actor-alice"),
        }];
        let waits = waits_of(&list);
        let selected = Target::Prompt(StudioKey::local("acme-shop"), "actor-alice".into());
        assert_eq!(
            for_row(Some(&selected), false, &waits, true, false, 36),
            "y/d p i  tab ←→ ↑↓ ⏎  m menu  esc"
        );
        assert_eq!(
            for_row(Some(&selected), false, &waits, true, false, 8),
            "↑↓ ⏎ esc"
        );
        assert_eq!(for_row(Some(&selected), false, &waits, true, false, 7), "");
    }
}
