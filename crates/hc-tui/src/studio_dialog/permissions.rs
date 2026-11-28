//! The role Permissions list dialog: active remembered rules and revoke.

use crate::studio::StudioKey;
use backlot_model::summary::PermitSummary;

#[derive(Debug, Clone)]
pub(crate) struct PermissionsDialog {
    pub(crate) key: StudioKey,
    pub(crate) name: String,
    pub(crate) permits: Vec<PermitSummary>,
    pub(crate) selected: usize,
}

impl PermissionsDialog {
    pub(crate) fn title(&self) -> String {
        format!(
            "Permissions · {} · {}",
            self.name,
            crate::studio_dialog::StudioDialog::place(&self.key)
        )
    }

    pub(crate) fn move_selection(&mut self, delta: isize) {
        if self.permits.is_empty() {
            self.selected = 0;
            return;
        }
        let len = self.permits.len() as isize;
        self.selected = (self.selected as isize + delta).rem_euclid(len) as usize;
    }

    pub(crate) fn chosen(&self) -> Option<&PermitSummary> {
        self.permits.get(self.selected)
    }
}
