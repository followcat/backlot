//! The Call Sheet's footer hint row: the selected row's own keys on
//! the left, then the fixed right block. The slots keep one order and
//! a row's footer only lights the slots its own dispatch runs — a key
//! the row would eat is never advertised, and a key it does run stays
//! named here or sits in its `m` menu.

use super::rows::Target;
use super::sheet::Wait;
use crate::chrome::text::display_width;
use crate::studio::StudioKey;

/// The footer's right block — the same keys in the same words on
/// every row, whatever the left part names.
const RIGHT: &str = "↑↓ ⏎   m menu   ? help   esc";
/// The right block on Talk/Headline rows where Enter opens the entry.
const RIGHT_READ: &str = "↑↓ ⏎ read   m menu   ? help   esc";
/// The right block when the width pinches: `? help` yields first.
const RIGHT_MID: &str = "↑↓ ⏎   m menu   esc";
const RIGHT_READ_MID: &str = "↑↓ ⏎ read   m menu   esc";
/// The floor — the block alone, bare. The old bare all-keys tier is
/// gone with it: the narrowest footer names nothing the selection
/// cannot run.
const RIGHT_BARE: &str = "↑↓ ⏎ esc";

/// The row-scoped slots in their fixed order — the words the full
/// footer shows, then the bare keys the narrow one shows.
const SLOTS: [(&str, &str); 6] = [
    ("y/d answer", "y/d"),
    ("a/x card", "a/x"),
    ("f finish", "f"),
    ("r start", "r"),
    ("p peek", "p"),
    ("i tell", "i"),
];

/// Which slots the selected row answers, in `SLOTS`' order: `y/d` on
/// a prompt, `a/x` on a card, `f` on a ready studio (its frame
/// included), `r` on a stage row whose machine can start, `p` on
/// prompts and studio frames, and `i` while an agent is configured —
/// the tell line is the sheet's own, and the key dies without one.
/// Every lit slot is one the row's dispatch would actually run.
fn slots(selected: Option<&Target>, waits: &[Wait<'_>], agent: bool) -> [bool; 6] {
    // A frame's `f` follows its studio: the READY row's wait is the
    // marker both read.
    let ready = |key: &StudioKey| {
        waits
            .iter()
            .any(|w| w.target() == Target::Ready(key.clone()))
    };
    match selected {
        Some(Target::Prompt(..)) => [true, false, false, false, true, agent],
        Some(Target::Card(_)) => [false, true, false, false, false, agent],
        Some(Target::Stage(_, offline)) => [false, false, false, !offline, false, agent],
        Some(Target::Ready(key)) | Some(Target::Studio(key)) => {
            [false, false, ready(key), false, true, agent]
        }
        _ => [false, false, false, false, false, agent],
    }
}

/// The footer for the selected row: the longest tier that fits
/// `width` — full words, then bare keys, then the block alone; `""`
/// when even the block is too wide. `agent` is the `[hollywood]`
/// gate the `i tell` slot shares with the key's dispatch.
pub(crate) fn for_row(
    selected: Option<&Target>,
    waits: &[Wait<'_>],
    agent: bool,
    width: usize,
) -> String {
    let mask = slots(selected, waits, agent);
    let mut full = String::new();
    let mut compact = String::new();
    for (i, lit) in mask.iter().enumerate() {
        if !lit {
            continue;
        }
        let (word, key) = SLOTS[i];
        if !full.is_empty() {
            full.push_str("   ");
            compact.push(' ');
        }
        full.push_str(word);
        compact.push_str(key);
    }
    let join = |left: &str, right: &str| {
        if left.is_empty() {
            right.to_owned()
        } else {
            format!("{left}   {right}")
        }
    };
    let (right, right_mid) = match selected {
        Some(Target::Headline | Target::Talk(_)) => (RIGHT_READ, RIGHT_READ_MID),
        _ => (RIGHT, RIGHT_MID),
    };
    [
        join(&full, right),
        join(&compact, right_mid),
        RIGHT_BARE.to_owned(),
    ]
    .into_iter()
    .find(|tier| display_width(tier) <= width)
    .unwrap_or_default()
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
            for_row(Some(&selected), &waits, true, 200),
            "y/d answer   p peek   i tell   ↑↓ ⏎   m menu   ? help   esc"
        );
    }

    #[test]
    fn rows_without_own_keys_name_only_the_tell_and_the_block() {
        let list = machines(Some(&summary()));
        let waits = waits_of(&list);
        // With an agent the tell line is the sheet's own — every row
        // without keys of its own still names it.
        let with_tell = "i tell   ↑↓ ⏎   m menu   ? help   esc";
        for selected in [Target::Calm, Target::Input] {
            assert_eq!(
                for_row(Some(&selected), &waits, true, 200),
                with_tell,
                "{selected:?}"
            );
        }
        assert_eq!(for_row(None, &waits, true, 200), with_tell, "no selection");
        // Headline and Talk rows spell out `⏎ read` so Enter's
        // read-only CardView open is advertised in context.
        let with_read = "i tell   ↑↓ ⏎ read   m menu   ? help   esc";
        for selected in [Target::Headline, Target::Talk(7)] {
            assert_eq!(
                for_row(Some(&selected), &waits, true, 200),
                with_read,
                "{selected:?}"
            );
        }
        // Without an agent `i` is a dead key: the Setup row — and
        // every other one — names nothing but the block.
        let bare = "↑↓ ⏎   m menu   ? help   esc";
        for selected in [Target::Calm, Target::Input, Target::Setup] {
            assert_eq!(
                for_row(Some(&selected), &waits, false, 200),
                bare,
                "{selected:?}"
            );
        }
        assert_eq!(
            for_row(Some(&Target::Headline), &waits, false, 200),
            "↑↓ ⏎ read   m menu   ? help   esc"
        );
        assert_eq!(for_row(None, &waits, false, 200), bare, "no selection");
    }

    #[test]
    fn a_card_row_names_apply_and_cancel() {
        let list = machines(Some(&summary()));
        let waits = waits_of(&list);
        let selected = Target::Card("c3".into());
        assert_eq!(
            for_row(Some(&selected), &waits, true, 200),
            "a/x card   i tell   ↑↓ ⏎   m menu   ? help   esc"
        );
    }

    #[test]
    fn a_down_stage_row_names_r_start_an_offline_one_does_not() {
        let list = machines(Some(&summary()));
        let waits = waits_stage_down(&list);
        let down = Target::Stage(None, false);
        assert_eq!(
            for_row(Some(&down), &waits, true, 200),
            "r start   i tell   ↑↓ ⏎   m menu   ? help   esc"
        );
        let offline = Target::Stage(Some("host-b".into()), true);
        assert_eq!(
            for_row(Some(&offline), &waits, true, 200),
            "i tell   ↑↓ ⏎   m menu   ? help   esc"
        );
    }

    #[test]
    fn a_ready_studio_names_f_finish_from_its_row_and_frame() {
        let mut ready = summary();
        ready.studios[0].ready_to_finish = true;
        let list = machines(Some(&ready));
        let waits = waits_of(&list);
        let key = StudioKey::local("acme-shop");
        for selected in [Target::Ready(key.clone()), Target::Studio(key)] {
            assert_eq!(
                for_row(Some(&selected), &waits, true, 200),
                "f finish   p peek   i tell   ↑↓ ⏎   m menu   ? help   esc",
                "{selected:?}"
            );
        }
    }

    #[test]
    fn a_live_studio_frame_names_peek_but_never_finish() {
        let list = machines(Some(&summary()));
        let waits = waits_of(&list);
        let selected = Target::Studio(StudioKey::local("acme-shop"));
        assert_eq!(
            for_row(Some(&selected), &waits, true, 200),
            "p peek   i tell   ↑↓ ⏎   m menu   ? help   esc"
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
        // The compact tier keeps the row's keys and the menu hint.
        assert_eq!(
            for_row(Some(&selected), &waits, true, 36),
            "y/d p i   ↑↓ ⏎   m menu   esc"
        );
        // The floor is the bare block — the old bare all-keys tier is
        // gone, and past it the footer is empty.
        assert_eq!(for_row(Some(&selected), &waits, true, 8), "↑↓ ⏎ esc");
        assert_eq!(for_row(Some(&selected), &waits, true, 7), "");
    }
}
