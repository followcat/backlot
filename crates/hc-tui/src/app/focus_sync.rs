//! Authoritative FocusState synchronization between `App::focus` and
//! legacy surface state (`panel.focused`, `tree_focus`, `cast_focus`,
//! `hollywood.mode`, and open modals).

use crate::focus::{CallSheetZone, LotLevel, ModalLayer, TellTarget, WorkspaceRegion};
use crate::hollywood::{LayerMode, Section, Target as SheetTarget};
use crate::panel::Target as PanelTarget;
use crate::studio::StudioKey;

use super::App;

impl App {
    /// Active non-exempt modal list pushed in open order.
    fn current_modal_layers(&self) -> Vec<ModalLayer> {
        let mut active = Vec::new();
        if self.popup.is_some() {
            active.push(ModalLayer::Popup);
        }
        if self.peek.is_some() {
            active.push(ModalLayer::Peek);
        }
        if self.dialog.is_some() {
            active.push(ModalLayer::Dialog);
        }
        if self.studio_dialog.is_some() {
            active.push(ModalLayer::StudioDialog);
        }
        if self.hw_switch.is_some() {
            active.push(ModalLayer::HwSwitch);
        }
        if self.new_studio.is_some() {
            active.push(ModalLayer::NewStudio);
        }
        if self.sheet.is_some() {
            active.push(ModalLayer::TaskSheet);
        }
        if self.hollywood.view.is_some() {
            active.push(ModalLayer::Reader);
        }
        if self.menu.is_some() {
            active.push(ModalLayer::ContextMenu);
        }
        if self.update_modal.is_some() {
            active.push(ModalLayer::UpdateCard);
        }
        if self.help {
            active.push(ModalLayer::HelpLauncher);
        }
        if self.navigator.is_some() {
            active.push(ModalLayer::Navigator);
        }
        if self.settings.is_some() {
            active.push(ModalLayer::Settings);
        }
        active
    }

    pub(crate) fn derived_call_sheet_zone(&self) -> CallSheetZone {
        match self.hollywood.selected.as_ref() {
            Some(SheetTarget::Input) => CallSheetZone::Compose,
            Some(t) if Section::of(t) == Section::Upper => CallSheetZone::Studios,
            Some(_) => CallSheetZone::Waiting,
            None => match self.hollywood.section {
                Section::Upper => CallSheetZone::Studios,
                Section::Lower => CallSheetZone::Waiting,
            },
        }
    }

    pub(crate) fn derived_lot_level(&self) -> LotLevel {
        if let Some(PanelTarget::Input(studio)) = self.panel.selected.as_ref()
            && self.panel.editing
        {
            return LotLevel::Compose {
                studio: studio.clone(),
                target: self.panel.tell_target.clone(),
            };
        }
        if let WorkspaceRegion::Lot(LotLevel::RowActions { studio, chip }) = &self.focus.workspace
            && self.panel.selected.as_ref().and_then(PanelTarget::studio) == Some(studio)
        {
            return LotLevel::RowActions {
                studio: studio.clone(),
                chip: *chip,
            };
        }
        match self.panel.selected.as_ref() {
            Some(PanelTarget::Studio(k) | PanelTarget::Block(k) | PanelTarget::Strip(k)) => {
                if let WorkspaceRegion::Lot(LotLevel::InsideStudio(cur)) = &self.focus.workspace
                    && cur == k
                {
                    LotLevel::InsideStudio(k.clone())
                } else {
                    LotLevel::Studios
                }
            }
            Some(target) => match target.studio() {
                Some(k) => LotLevel::InsideStudio(k.clone()),
                None => LotLevel::Studios,
            },
            None => LotLevel::Studios,
        }
    }

    /// Reconcile `App::focus` with any external modal/surface mutations
    /// (for tests or legacy paths that mutate `panel.focused`,
    /// `tree_focus`, or `hollywood.mode` directly).
    pub(crate) fn sync_focus(&mut self) {
        let active = self.current_modal_layers();
        self.focus.sync_modals(&active);

        if self.tree_focus.is_some() && !matches!(self.focus.workspace, WorkspaceRegion::Tree) {
            self.focus.workspace = WorkspaceRegion::Tree;
            return;
        }
        if self.tree_focus.is_none() && matches!(self.focus.workspace, WorkspaceRegion::Tree) {
            self.focus.workspace = WorkspaceRegion::Terminal;
        }
        if self.hollywood.expanded()
            && !matches!(self.focus.workspace, WorkspaceRegion::CallSheet(_))
        {
            self.focus.workspace = WorkspaceRegion::CallSheet(self.derived_call_sheet_zone());
            return;
        }
        if !self.hollywood.expanded()
            && matches!(self.focus.workspace, WorkspaceRegion::CallSheet(_))
        {
            self.focus.workspace = self
                .call_sheet_return
                .take()
                .unwrap_or(WorkspaceRegion::Terminal);
        }
        if self.panel.focused && !matches!(self.focus.workspace, WorkspaceRegion::Lot(_)) {
            let level = if self.panel.lot_studios_browse {
                LotLevel::Studios
            } else {
                self.derived_lot_level()
            };
            self.focus.workspace = WorkspaceRegion::Lot(level);
            return;
        }
        if !self.panel.focused && matches!(self.focus.workspace, WorkspaceRegion::Lot(_)) {
            self.focus.workspace = WorkspaceRegion::Terminal;
        }
        if matches!(self.focus.workspace, WorkspaceRegion::Lot(_)) {
            if self.panel.editing
                && !matches!(
                    self.focus.workspace,
                    WorkspaceRegion::Lot(LotLevel::Compose { .. })
                )
            {
                let studio = self
                    .panel
                    .selected
                    .as_ref()
                    .and_then(PanelTarget::studio)
                    .cloned()
                    .or_else(|| self.studio.current.clone())
                    .unwrap_or_else(|| StudioKey::local(""));
                self.focus.workspace = WorkspaceRegion::Lot(LotLevel::Compose {
                    studio,
                    target: self.panel.tell_target.clone(),
                });
            } else if !self.panel.editing && !self.panel.lot_studios_browse {
                self.focus.workspace = WorkspaceRegion::Lot(self.derived_lot_level());
            }
        }
        if matches!(self.focus.workspace, WorkspaceRegion::CallSheet(_)) {
            self.focus.workspace = WorkspaceRegion::CallSheet(self.derived_call_sheet_zone());
        }
    }

    /// Pre-key focus sync: reconciles external surface/modal/editing latches without
    /// promoting an active `LotLevel::Studios` session into `InsideStudio` before a key runs.
    pub(crate) fn sync_focus_before_key(&mut self) {
        let was_studios = matches!(
            self.focus.workspace,
            WorkspaceRegion::Lot(LotLevel::Studios)
        );
        self.sync_focus();
        if was_studios
            && self.panel.focused
            && !self.panel.editing
            && matches!(self.focus.workspace, WorkspaceRegion::Lot(_))
        {
            self.focus.workspace = WorkspaceRegion::Lot(LotLevel::Studios);
        }
    }

    /// Authoritatively transition `App::focus.workspace` and keep all legacy
    /// region latches (`panel.focused`, `tree_focus`, `cast_focus`,
    /// `hollywood.mode`) synchronized.
    pub(crate) fn set_workspace_focus(&mut self, next: WorkspaceRegion) {
        if !matches!(next, WorkspaceRegion::Tree)
            && let Some(row) = self.tree_focus.take()
        {
            self.tree_remembered = Some(row);
        }
        if !matches!(next, WorkspaceRegion::Cast)
            && let Some(cur) = self.cast_focus.take()
        {
            self.cast_remembered = Some(cur);
        }
        if !matches!(next, WorkspaceRegion::Lot(LotLevel::Studios)) {
            self.panel.lot_studios_browse = false;
        }
        if !matches!(next, WorkspaceRegion::Lot(_)) {
            self.panel.focused = false;
            self.panel.editing = false;
            self.panel.reset_history_nav();
        }
        if !matches!(next, WorkspaceRegion::CallSheet(_)) && self.hollywood.expanded() {
            self.hollywood.mode = LayerMode::Collapsed;
            self.hollywood.editing = false;
            self.hollywood.history.reset_nav();
            self.hollywood.confirm = None;
        }
        match &next {
            WorkspaceRegion::Terminal => {}
            WorkspaceRegion::Tree => {
                if self.tree_focus.is_none() {
                    let rows = self.tree_rows();
                    self.tree_focus = self
                        .tree_remembered
                        .clone()
                        .filter(|r| rows.contains(r))
                        .or_else(|| rows.first().cloned());
                }
            }
            WorkspaceRegion::Cast => {
                if self.cast_focus.is_none() {
                    let targets = self.cast_targets();
                    self.cast_focus = self
                        .cast_remembered
                        .clone()
                        .filter(|t| targets.contains(t))
                        .or_else(|| targets.first().cloned());
                }
            }
            WorkspaceRegion::Lot(level) => {
                self.panel.focused = true;
                self.panel.editing = matches!(level, LotLevel::Compose { .. });
                if let LotLevel::Compose { target, .. } = level {
                    self.panel.tell_target = target.clone();
                } else if matches!(level, LotLevel::Studios) {
                    self.panel.tell_target = TellTarget::default();
                    self.panel.compose_cursor_role = None;
                }
            }
            WorkspaceRegion::CallSheet(_) => {
                self.hollywood.mode = LayerMode::Expanded;
            }
        }
        self.focus.workspace = next;
        let active = self.current_modal_layers();
        self.focus.sync_modals(&active);
    }
}
