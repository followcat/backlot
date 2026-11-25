//! The stale-plan reply for destructive studio dialogs (B1b): a
//! server plan that changed under the confirm is shown in the dialog,
//! not a toast, and reviewing again fetches a fresh plan and token.

use crate::action::Pending;
use crate::app::App;
use crate::studio::StudioKey;
use crate::studio_dialog::{ConfirmFor, StudioDialog};

impl App {
    /// A destructive commit refused because the plan the dialog showed
    /// went stale. Keep the dialog open with the server's own line;
    /// Enter/`r` review the plan again.
    pub(in crate::app) fn stale_plan_reply(
        &mut self,
        pending: &Pending,
        result: &Result<String, String>,
    ) -> bool {
        let Err(err) = result else { return false };
        if !crate::server_action::is_stale_plan(err) {
            return false;
        }
        let argv = &pending.action.argv;
        if argv.first().map(String::as_str) != Some("studio") {
            return false;
        }
        let line = err
            .lines()
            .find(|l| !l.trim().is_empty())
            .unwrap_or_default()
            .trim()
            .to_owned();
        let machine = pending.action.machine.clone();
        let place = |key: &StudioKey| crate::studio_dialog::StudioDialog::place(key);
        match argv.get(1).map(String::as_str) {
            Some("delete") => {
                let Some(id) = argv.get(2).cloned() else {
                    return false;
                };
                let key = StudioKey { machine, id };
                let for_ = ConfirmFor::DeleteStudio {
                    key: key.clone(),
                    force: false,
                    kept: Vec::new(),
                    token: None,
                };
                self.studio_dialog = Some(StudioDialog::confirm_stale(
                    format!("Delete studio · {}", place(&key)),
                    "The plan you reviewed is out of date. Review it again before deleting.".into(),
                    line,
                    for_,
                ));
            }
            Some("prune") => {
                let Some(id) = argv.get(2).cloned() else {
                    return false;
                };
                let key = StudioKey { machine, id };
                let for_ = ConfirmFor::Prune {
                    key: key.clone(),
                    token: None,
                };
                self.studio_dialog = Some(StudioDialog::confirm_stale(
                    format!("Free work areas · {}", place(&key)),
                    "The plan you reviewed is out of date. Review it again before pruning.".into(),
                    line,
                    for_,
                ));
            }
            Some("restore") => {
                let Some(archived) = argv.get(2).cloned() else {
                    return false;
                };
                let name = argv
                    .iter()
                    .position(|a| a == "--name")
                    .and_then(|at| argv.get(at + 1))
                    .cloned()
                    .unwrap_or_default();
                let entries = self.studio_index.archived(machine.as_deref()).to_vec();
                let mut d = crate::studio_dialog::RestoreDialog::new(machine, entries);
                d.sel = d
                    .entries
                    .iter()
                    .position(|e| e.name == archived)
                    .unwrap_or(0);
                d.name = crate::chrome::text_editor::TextEditor::new(&name, false);
                d.step = crate::studio_dialog::RestoreStep::Confirm;
                d.error = Some(line);
                self.studio_dialog = Some(StudioDialog::Restore(d));
            }
            _ => return false,
        }
        true
    }
}
