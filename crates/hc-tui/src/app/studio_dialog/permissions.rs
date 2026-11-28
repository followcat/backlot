//! Keys and submits for a role's remembered-permission list.

use crossterm::event::KeyCode;

use super::App;
use crate::studio_cmds;
use crate::studio_dialog::{PermissionsDialog, StudioDialog};

impl App {
    pub(super) fn permissions_key(
        &mut self,
        mut d: PermissionsDialog,
        code: KeyCode,
    ) -> Option<StudioDialog> {
        match code {
            KeyCode::Esc => None,
            KeyCode::Up | KeyCode::Char('k') | KeyCode::BackTab => {
                d.move_selection(-1);
                Some(StudioDialog::Permissions(d))
            }
            KeyCode::Down | KeyCode::Char('j') | KeyCode::Tab => {
                d.move_selection(1);
                Some(StudioDialog::Permissions(d))
            }
            KeyCode::Enter | KeyCode::Char('r') => {
                if let Some(permit) = d.chosen() {
                    self.run_action(studio_cmds::permit_revoke(&d.key, &permit.id));
                }
                None
            }
            _ => Some(StudioDialog::Permissions(d)),
        }
    }
}
