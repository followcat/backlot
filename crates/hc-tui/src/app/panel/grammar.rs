//! Universal Lot grammar helpers (§1.2.3 & §1.2.4): bounded
//! cursor inside `InsideStudio`, inline action chip stepping
//! (`RowActions`), and `←`/`→` fold/enter rules. (`Tab`/`Shift+Tab`
//! section cycling lives in `app/panel/sections.rs`.)

use crate::focus::{LotLevel, TellTarget, WorkspaceRegion};
use crate::panel::chips::{self, RowChipKind};
use crate::panel::{Target, data};
use crate::studio::StudioKey;

use super::App;

impl App {
    /// `↑`/`↓`/`j`/`k`/`PageUp`/`PageDown` bounded inside `InsideStudio(studio)`:
    /// never crosses into another studio's rows.
    pub(super) fn panel_move_bounded(&mut self, studio: &StudioKey, delta: isize) {
        let machines = self.panel_summaries();
        let rows = self.panel_rows_at(&machines, self.panel_hits.body.height);
        let targets: Vec<Target> = rows.iter().map(|r| r.target()).collect();
        let lo = targets
            .iter()
            .position(|t| t.studio() == Some(studio) && *t != Target::None);
        let hi = targets
            .iter()
            .rposition(|t| t.studio() == Some(studio) && *t != Target::None);
        let (Some(lo), Some(hi)) = (lo, hi) else {
            return;
        };
        let mut index = self.panel_selected_index(&targets).clamp(lo, hi);
        if delta == isize::MIN {
            index = lo;
        } else if delta == isize::MAX {
            index = hi;
        } else {
            for _ in 0..delta.unsigned_abs() {
                let next = if delta > 0 {
                    (index + 1..=hi)
                        .find(|&i| targets[i] != targets[index] && targets[i] != Target::None)
                } else {
                    (lo..index)
                        .rev()
                        .find(|&i| targets[i] != targets[index] && targets[i] != Target::None)
                };
                match next {
                    Some(i) => index = i,
                    None => break,
                }
            }
        }
        while index > lo && targets[index - 1] == targets[index] {
            index -= 1;
        }
        self.panel.selected = Some(targets[index].clone());
        self.panel.selected_row = Some(index);
        self.panel.finish_confirm = None;
        self.panel.editing = false;
        self.panel.reset_history_nav();
        self.scroll_to_panel_row(&rows, index);
    }

    /// `←` / `→` (and `Tab`/`BackTab` in `RowActions`): navigate inline chips
    /// when the current row defines chips, or step out to `Studios` header on
    /// `←` from a chip-less row.
    pub(super) fn panel_row_chip_move(&mut self, studio: &StudioKey, delta: isize) {
        let targets = self.panel_targets();
        let index = self.panel_selected_index(&targets);
        let Some(selected) = targets.get(index).cloned() else {
            return;
        };
        let machines = self.panel_summaries();
        let chips = chips::row_chips(&machines, &selected);
        if chips.is_empty() {
            if delta < 0 {
                self.panel.selected = Some(Target::Studio(studio.clone()));
                self.set_workspace_focus(WorkspaceRegion::Lot(LotLevel::Studios));
                self.panel_reveal_selected();
            }
            return;
        }
        let next_chip = match &self.focus.workspace {
            WorkspaceRegion::Lot(LotLevel::RowActions { chip, .. }) => {
                ((*chip as isize) + delta).rem_euclid(chips.len() as isize) as usize
            }
            _ => {
                if delta >= 0 {
                    0
                } else {
                    chips.len().saturating_sub(1)
                }
            }
        };
        self.set_workspace_focus(WorkspaceRegion::Lot(LotLevel::RowActions {
            studio: studio.clone(),
            chip: next_chip,
        }));
    }

    /// Execute highlighted inline action chip in `Lot(RowActions)`.
    pub(super) fn panel_execute_chip(&mut self, studio: &StudioKey, chip_idx: usize) {
        let targets = self.panel_targets();
        let index = self.panel_selected_index(&targets);
        let Some(selected) = targets.get(index).cloned() else {
            return;
        };
        let machines = self.panel_summaries();
        let chips = chips::row_chips(&machines, &selected);
        let Some(chip) = chips.get(chip_idx).copied() else {
            return;
        };
        self.set_workspace_focus(WorkspaceRegion::Lot(LotLevel::InsideStudio(studio.clone())));
        match chip.kind {
            RowChipKind::Allow => self.panel_decide(true),
            RowChipKind::Deny => self.panel_decide(false),
            RowChipKind::Reply => match &selected {
                Target::Person(k, role) => {
                    self.enter_lot_compose(k, TellTarget::Role(role.clone()));
                }
                _ => self.panel_reply_selected(),
            },
            RowChipKind::Ack => self.panel_ack_selected(),
            RowChipKind::Switch => self.panel_switch_role_selected(),
            RowChipKind::Tell => match &selected {
                Target::Person(k, role) => {
                    self.enter_lot_compose(k, TellTarget::Role(role.clone()));
                }
                _ => self.enter_lot_compose(studio, TellTarget::default()),
            },
            RowChipKind::Peek => self.panel_peek_selected(),
            RowChipKind::Menu => self.panel_menu_selected(),
            RowChipKind::Open | RowChipKind::Show | RowChipKind::Read | RowChipKind::ViewSheet => {
                self.panel_activate();
            }
            RowChipKind::Reopen => self.panel_reopen_selected(),
            RowChipKind::Apply | RowChipKind::DesignSign => self.panel_card_apply(),
            RowChipKind::Cancel => self.panel_card_cancel(),
        }
    }

    /// `x` on a `Target::Person` with an open ask: acknowledges the ask via stage CLI.
    pub(super) fn panel_ack_selected(&mut self) {
        let targets = self.panel_targets();
        let index = self.panel_selected_index(&targets);
        let Some(Target::Person(key, role_name)) = targets.get(index).cloned() else {
            return;
        };
        if let Some(target) = self.ask_target(&key, &role_name) {
            self.run_action(crate::studio_cmds::ask_acknowledge(
                &key, &role_name, target,
            ));
        }
    }

    /// `w` on a quota/error role row: opens Recast dialog pre-targeted at that role.
    pub(super) fn panel_switch_role_selected(&mut self) {
        let machines = self.panel_summaries();
        let targets = self.panel_targets();
        let index = self.panel_selected_index(&targets);
        let Some(Target::Person(key, role_name)) = targets.get(index).cloned() else {
            return;
        };
        if let Some(studio) = data::studio(&machines, &key)
            && let Some(role) = studio.studio.roles.iter().find(|r| r.role == role_name)
        {
            self.recast_dialog_open(
                Some(&key),
                &role.role,
                &role.name,
                role.runner.clone(),
                role.model.clone(),
                role.effort.clone(),
            );
        }
    }
}
