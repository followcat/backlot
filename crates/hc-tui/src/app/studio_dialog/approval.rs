//! Key and submit handling for the permission approval choice dialog.

use crossterm::event::{KeyCode, KeyModifiers};

use super::App;
use crate::studio::StudioKey;
use crate::studio_cmds;
use crate::studio_dialog::{ApprovalChoice, ApprovalDialog, ApprovalTarget, StudioDialog};

impl App {
    /// Open the approval dialog for a studio role. `detail` is the summary's
    /// uncapped prompt target; the visible dialog derives its exact rule from it.
    pub(crate) fn approval_open_studio(
        &mut self,
        key: &StudioKey,
        role: &str,
        role_label: &str,
        detail: Option<&str>,
        deny: bool,
    ) {
        let mut d = ApprovalDialog::new(
            ApprovalTarget::Studio {
                key: key.clone(),
                role: role.to_string(),
            },
            role_label,
            detail,
            deny,
        );
        if deny {
            d.select_deny();
        }
        self.studio_dialog = Some(StudioDialog::Approval(d));
    }

    /// Hollywood's own approval dialog — same choices, its own ledger.
    pub(crate) fn approval_open_hollywood(&mut self, detail: Option<&str>, deny: bool) {
        let mut d = ApprovalDialog::new(ApprovalTarget::Hollywood, "hollywood", detail, deny);
        if deny {
            d.select_deny();
        }
        self.studio_dialog = Some(StudioDialog::Approval(d));
    }

    pub(super) fn approval_key(
        &mut self,
        mut d: ApprovalDialog,
        code: KeyCode,
        mods: KeyModifiers,
    ) -> Option<StudioDialog> {
        match code {
            KeyCode::Esc => None,
            KeyCode::Up | KeyCode::Char('k') | KeyCode::BackTab => {
                d.move_selection(-1);
                Some(StudioDialog::Approval(d))
            }
            KeyCode::Down | KeyCode::Char('j') | KeyCode::Tab => {
                d.move_selection(1);
                Some(StudioDialog::Approval(d))
            }
            KeyCode::Char('d') if mods.is_empty() => {
                self.approval_submit_choice(&d, ApprovalChoice::Deny);
                None
            }
            KeyCode::Enter | KeyCode::Char('y') => {
                self.approval_submit_choice(&d, d.chosen());
                None
            }
            _ => Some(StudioDialog::Approval(d)),
        }
    }

    pub(super) fn approval_submit(&mut self, d: &ApprovalDialog) {
        self.approval_submit_choice(d, d.chosen());
    }

    fn approval_submit_choice(&mut self, d: &ApprovalDialog, choice: ApprovalChoice) {
        match (&d.target, choice) {
            (ApprovalTarget::Studio { key, role }, ApprovalChoice::Once) => {
                self.run_action(studio_cmds::decide(key, role, true));
            }
            (ApprovalTarget::Studio { key, role }, ApprovalChoice::Deny) => {
                self.run_action(studio_cmds::decide(key, role, false));
            }
            (ApprovalTarget::Studio { key, role }, ApprovalChoice::Role) => {
                self.run_action(studio_cmds::decide_scoped(
                    key,
                    role,
                    backlot_stage::args::ApprovalScope::Role,
                ));
            }
            (ApprovalTarget::Studio { key, role }, ApprovalChoice::Studio) => {
                self.run_action(studio_cmds::decide_scoped(
                    key,
                    role,
                    backlot_stage::args::ApprovalScope::Studio,
                ));
            }
            (ApprovalTarget::Hollywood, ApprovalChoice::Once) => {
                self.run_action(studio_cmds::hollywood_approve(true));
            }
            (ApprovalTarget::Hollywood, ApprovalChoice::Deny) => {
                self.run_action(studio_cmds::hollywood_approve(false));
            }
            (ApprovalTarget::Hollywood, ApprovalChoice::Role) => {
                self.run_action(studio_cmds::hollywood_approve_scoped(
                    backlot_stage::args::ApprovalScope::Role,
                ));
            }
            (ApprovalTarget::Hollywood, ApprovalChoice::Studio) => {
                self.run_action(studio_cmds::hollywood_approve_scoped(
                    backlot_stage::args::ApprovalScope::Studio,
                ));
            }
        }
    }
}
