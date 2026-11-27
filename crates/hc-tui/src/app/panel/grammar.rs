//! Universal Lot grammar helpers (§1.2.3 & §1.2.4): urgent studio Tab
//! cycling in `Studios`, bounded cursor & section cycling in `InsideStudio`,
//! inline action chip stepping (`RowActions`), and `←`/`→` fold/enter rules.

use crate::focus::{LotLevel, TellTarget, WorkspaceRegion};
use crate::panel::chips::{self, RowChipKind};
use crate::panel::{Target, data};
use crate::studio::StudioKey;

use super::App;

impl App {
    /// `Tab` / `Shift+Tab` inside `Lot(Studios)`: cycles studios with waiting
    /// items (`waiting > 0 || open_reviews > 0`), or all studio headers +
    /// `Setup` / `Stage` when ≤1 studio is urgent.
    pub(super) fn panel_studios_tab(&mut self, forward: bool) {
        let machines = self.panel_summaries();
        let targets = self.panel_targets();
        let headers: Vec<Target> = targets
            .iter()
            .filter(|t| {
                matches!(
                    t,
                    Target::Studio(_) | Target::Block(_) | Target::Stage(_) | Target::Setup
                )
            })
            .cloned()
            .collect();
        let mut deduped = Vec::new();
        for t in headers {
            if deduped.last() != Some(&t) {
                deduped.push(t);
            }
        }
        let urgent: Vec<Target> = deduped
            .iter()
            .filter(|t| {
                t.studio()
                    .and_then(|k| data::studio(&machines, k))
                    .is_some_and(|s| s.studio.waiting > 0 || s.studio.open_reviews > 0)
            })
            .cloned()
            .collect();
        let cycle = if urgent.len() >= 2 { &urgent } else { &deduped };
        if cycle.is_empty() {
            return;
        }
        let cur_studio = self.panel.selected.as_ref().and_then(Target::studio);
        let cur = self
            .panel
            .selected
            .as_ref()
            .and_then(|sel| {
                cycle
                    .iter()
                    .position(|t| t == sel || (cur_studio.is_some() && t.studio() == cur_studio))
            })
            .unwrap_or(0) as isize;
        let step = if forward { 1 } else { -1 };
        let next_idx = (cur + step).rem_euclid(cycle.len() as isize) as usize;
        self.panel.editing = false;
        self.panel.finish_confirm = None;
        self.panel.reset_history_nav();
        self.panel.selected = Some(cycle[next_idx].clone());
        self.set_workspace_focus(WorkspaceRegion::Lot(LotLevel::Studios));
        self.panel_reveal_selected();
    }

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
                    (index + 1..=hi).find(|&i| targets[i] != targets[index] && targets[i] != Target::None)
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

    /// `Tab` / `Shift+Tab` inside `InsideStudio(studio)`: cycle non-empty
    /// sections `Needs You` -> `People` -> `Tasks` -> `Talk` -> `Compose`
    /// without auto-arming `editing`.
    pub(super) fn panel_section_tab(&mut self, studio: &StudioKey, forward: bool) {
        let machines = self.panel_summaries();
        let targets = self.panel_targets();
        let is_needs_you = |t: &Target| match t {
            Target::Design(k) | Target::OutwardCard(k, _) => k == studio,
            Target::Person(k, role_name) if k == studio => data::studio(&machines, studio)
                .and_then(|s| s.studio.roles.iter().find(|r| r.role == *role_name))
                .is_some_and(|r| {
                    r.attention_user
                        || r.attention_seq.is_some()
                        || r.asks.as_ref().is_some_and(|a| !a.is_empty())
                        || r.failed.is_some()
                        || r.work == Some(backlot_model::summary::Work::Blocked)
                }),
            _ => false,
        };
        let needs_target = targets.iter().find(|t| is_needs_you(t)).cloned();
        let people_target = targets
            .iter()
            .find(|t| {
                matches!(t, Target::Person(k, _) | Target::ClosedFold(k) | Target::ReopenProducer(k) if k == studio && !is_needs_you(t))
            })
            .or_else(|| targets.iter().find(|t| matches!(t, Target::Person(k, _) if k == studio)))
            .cloned();
        let tasks_target = targets
            .iter()
            .find(|t| matches!(t, Target::Task(k, _) | Target::Fold(k) if k == studio))
            .cloned();
        let talk_target = targets
            .iter()
            .find(|t| matches!(t, Target::Talk(k, _) if k == studio))
            .cloned();
        let compose_target = targets
            .iter()
            .find(|t| matches!(t, Target::Input(k) if k == studio))
            .cloned();

        let mut sections = Vec::new();
        for cand in [
            needs_target,
            people_target,
            tasks_target,
            talk_target,
            compose_target,
        ]
        .into_iter()
        .flatten()
        {
            if !sections.contains(&cand) {
                sections.push(cand);
            }
        }
        if sections.is_empty() {
            return;
        }
        let section_of = |t: &Target| -> usize {
            if is_needs_you(t) {
                0
            } else if matches!(
                t,
                Target::Person(..) | Target::ClosedFold(..) | Target::ReopenProducer(..)
            ) {
                1
            } else if matches!(t, Target::Task(..) | Target::Fold(..)) {
                2
            } else if matches!(t, Target::Talk(..)) {
                3
            } else if matches!(t, Target::Input(..)) {
                4
            } else {
                0
            }
        };
        let cur_bucket = self.panel.selected.as_ref().map_or(0, section_of);
        let cur_idx = sections
            .iter()
            .position(|s| {
                self.panel.selected.as_ref() == Some(s) || section_of(s) >= cur_bucket
            })
            .unwrap_or(0) as isize;
        let step = if forward { 1 } else { -1 };
        let next_idx = (cur_idx + step).rem_euclid(sections.len() as isize) as usize;
        self.panel.editing = false;
        self.panel.finish_confirm = None;
        self.panel.reset_history_nav();
        self.panel.selected = Some(sections[next_idx].clone());
        self.panel_reveal_selected();
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
            self.run_action(crate::studio_cmds::ask_acknowledge(&key, &role_name, target));
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
