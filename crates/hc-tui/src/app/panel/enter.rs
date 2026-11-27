//! Studio entry and compose landing helpers for the Lot (`Alt+L` /
//! `Enter` / `→` / `Studios` level selection).

use super::App;
use crate::panel::Target;

impl App {
    /// Enter `LotLevel::Studios` selecting the first studio with waiting
    /// items, else first live studio, else `+ new` / first actionable row.
    pub(crate) fn enter_studios_level(&mut self) {
        self.panel.focused = true;
        self.panel.editing = false;
        self.panel.finish_confirm = None;
        self.panel.lot_studios_browse = false;
        self.panel.reset_history_nav();
        if let Some(target) = self.default_studios_target() {
            self.panel.selected = Some(target);
        }
        self.set_workspace_focus(crate::focus::WorkspaceRegion::Lot(
            crate::focus::LotLevel::Studios,
        ));
        self.panel_reveal_selected();
    }

    pub(crate) fn default_studios_target(&self) -> Option<Target> {
        let machines = self.panel_summaries();
        let studios = crate::panel::data::studios(&machines);
        let urgent = studios
            .iter()
            .find(|s| s.studio.waiting > 0 || s.studio.open_reviews > 0)
            .or_else(|| studios.iter().find(|s| s.studio.state != "finished"))
            .or_else(|| studios.first())
            .map(|s| s.key());
        let targets = self.panel_targets();
        if let Some(key) = urgent
            && let Some(t) = targets.iter().find(|t| t.studio() == Some(&key))
        {
            return Some(t.clone());
        }
        targets.into_iter().find(|t| *t != Target::None)
    }

    /// Enter `LotLevel::InsideStudio(key)` (§1.2.4): unfolds `key`,
    /// lands on (1) first Needs You item, (2) active pane's role or
    /// producer, or (3) bottom compose row.
    pub(crate) fn enter_studio_in_lot(&mut self, key: &crate::studio::StudioKey) {
        self.panel.focused = true;
        self.panel.editing = false;
        self.panel.finish_confirm = None;
        self.panel.lot_studios_browse = false;
        self.panel.reset_history_nav();
        self.panel.collapsed_blocks.remove(key);
        // Set `panel.selected` inside `key` first so `block_states::cursor_inside`
        // renders `key` unfolded when we inspect its rows.
        self.panel.selected = Some(Target::Studio(key.clone()));

        let machines = self.panel_summaries();
        let rows = self.panel_rows_at(&machines, self.panel_body_height());
        let targets: Vec<Target> = rows.iter().map(|r| r.target()).collect();

        let target = self
            .studio_priority_landing_target(&machines, &targets, key)
            .unwrap_or_else(|| Target::Studio(key.clone()));

        self.panel.selected = Some(target.clone());
        if let Some(idx) = targets.iter().position(|t| *t == target) {
            self.panel.selected_row = Some(idx);
            self.scroll_to_panel_row(&rows, idx);
        } else {
            self.panel.selected_row = None;
        }
        self.set_workspace_focus(crate::focus::WorkspaceRegion::Lot(
            crate::focus::LotLevel::InsideStudio(key.clone()),
        ));
    }

    /// Jump directly into inline Compose inside `studio`.
    pub(crate) fn enter_lot_compose(
        &mut self,
        studio: &crate::studio::StudioKey,
        target: crate::focus::TellTarget,
    ) {
        self.panel.collapsed_blocks.remove(studio);
        self.panel.selected = Some(Target::Input(studio.clone()));
        self.panel.editing = true;
        self.panel.finish_confirm = None;
        self.panel.reset_history_nav();
        self.set_workspace_focus(crate::focus::WorkspaceRegion::Lot(
            crate::focus::LotLevel::Compose {
                studio: studio.clone(),
                target,
            },
        ));
        self.panel_reveal_selected();
    }

    fn studio_priority_landing_target(
        &self,
        machines: &[crate::panel::data::MachineSummary],
        targets: &[Target],
        key: &crate::studio::StudioKey,
    ) -> Option<Target> {
        let studio = crate::panel::data::studio(machines, key)?;
        let has_target = |t: &Target| targets.contains(t);

        // 1a. User permission prompts
        for role in &studio.studio.roles {
            if role.attention_user && role.can_answer_prompt {
                let cand = Target::Person(key.clone(), role.role.clone());
                if has_target(&cand) {
                    return Some(cand);
                }
            }
        }
        // 1b. Open asks
        for role in &studio.studio.roles {
            if role.asks.as_ref().is_some_and(|a| !a.is_empty()) {
                let cand = Target::Person(key.clone(), role.role.clone());
                if has_target(&cand) {
                    return Some(cand);
                }
            }
        }
        // 1c. Escalated / user-owned attention / stuck / failed roles
        for role in &studio.studio.roles {
            if matches!(
                crate::studio::glyphs::Person::of(role),
                crate::studio::glyphs::Person::UserPrompt
                    | crate::studio::glyphs::Person::DirPrompt
                    | crate::studio::glyphs::Person::NeedsYou
                    | crate::studio::glyphs::Person::Asks
                    | crate::studio::glyphs::Person::Stuck
                    | crate::studio::glyphs::Person::Failed
            ) {
                let cand = Target::Person(key.clone(), role.role.clone());
                if has_target(&cand) {
                    return Some(cand);
                }
            }
        }
        // 1d. Waiting design or outward card
        if let Some(t) = targets
            .iter()
            .find(|t| matches!(t, Target::Design(k) | Target::OutwardCard(k, _) if k == key))
        {
            return Some(t.clone());
        }
        // 2a. Currently active terminal's member in `key`
        if let Some(focused) = self.focused.as_deref()
            && let Some(role) = studio
                .studio
                .roles
                .iter()
                .find(|r| r.terminal.as_deref() == Some(focused))
        {
            let cand = Target::Person(key.clone(), role.role.clone());
            if has_target(&cand) {
                return Some(cand);
            }
        }
        // 2b. `producer` or first live person in `key`
        let producer = Target::Person(key.clone(), "producer".into());
        if has_target(&producer) {
            return Some(producer);
        }
        if let Some(t) = targets
            .iter()
            .find(|t| matches!(t, Target::Person(k, _) if k == key))
        {
            return Some(t.clone());
        }
        // 3. Bottom compose row (`›`), else studio header
        let input = Target::Input(key.clone());
        if has_target(&input) {
            return Some(input);
        }
        targets
            .iter()
            .find(|t| matches!(t, Target::Studio(k) if k == key))
            .cloned()
    }
}
