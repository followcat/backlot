//! The archived-studios restore dialog's app side: the list's
//! keys, the `--dry-run` plan (with the server's confirm token), and
//! the real copy after it.

use crossterm::event::KeyCode;

use crate::app::input::TerminalKey;
use crate::studio_cmds;
use crate::studio_dialog::{RestoreDialog, RestoreStep, StudioDialog};

use super::App;

impl App {
    pub(crate) fn restore_key(
        &mut self,
        mut d: RestoreDialog,
        key: &TerminalKey,
        code: KeyCode,
    ) -> Option<StudioDialog> {
        if d.loading {
            return match code {
                KeyCode::Esc => None,
                _ => Some(StudioDialog::Restore(d)),
            };
        }
        match d.step {
            RestoreStep::List => match code {
                KeyCode::Esc => None,
                KeyCode::Up | KeyCode::Char('k') => {
                    d.move_selection(-1);
                    Some(StudioDialog::Restore(d))
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    d.move_selection(1);
                    Some(StudioDialog::Restore(d))
                }
                KeyCode::Enter => {
                    d.begin_name();
                    Some(StudioDialog::Restore(d))
                }
                _ => Some(StudioDialog::Restore(d)),
            },
            RestoreStep::Name => match code {
                KeyCode::Esc => None,
                KeyCode::Enter => self.restore_dry_run(d),
                _ => {
                    d.name.handle_key(key);
                    d.error = None;
                    Some(StudioDialog::Restore(d))
                }
            },
            RestoreStep::Confirm => match code {
                KeyCode::Esc => None,
                KeyCode::Enter if d.error.is_none() => {
                    self.restore_apply(&d);
                    None
                }
                // A stale server plan: Enter or `r` reviews it again.
                KeyCode::Enter | KeyCode::Char('r') if d.error.is_some() => self.restore_dry_run(d),
                KeyCode::Left => {
                    d.step = RestoreStep::Name;
                    d.body.clear();
                    d.confirm_token = None;
                    Some(StudioDialog::Restore(d))
                }
                _ => Some(StudioDialog::Restore(d)),
            },
        }
    }

    /// `studio restore <entry> --name <name> --dry-run` — its plan
    /// fills the dialog before anything is copied.
    pub(crate) fn restore_dry_run(&mut self, mut d: RestoreDialog) -> Option<StudioDialog> {
        let name = d.name.as_str().trim().to_owned();
        if name.is_empty() {
            d.error = Some("a name is required".into());
            return Some(StudioDialog::Restore(d));
        }
        let archived = d.selected().map(|e| e.name.clone())?;
        let mut action = studio_cmds::restore(d.machine.clone(), &archived, &name, true, None);
        // A server answers the plan with its token; a local or
        // remote-machine run keeps the plain output.
        if self.server_mode() && d.machine.is_none() {
            action.argv.push(crate::server_action::PLAN_FLAG.into());
        }
        d.loading = true;
        d.step = RestoreStep::Confirm;
        d.body.clear();
        d.error = None;
        d.confirm_token = None;
        self.studio_dialog_gen += 1;
        let epoch = self.studio_dialog_gen;
        let tx = self.dialog_fetch_tx.clone();
        let run = self.ns_runner();
        tokio::spawn(async move {
            let out = run(action.machine, action.argv, None).await;
            let _ = tx.send((epoch, out)).await;
        });
        Some(StudioDialog::Restore(d))
    }

    /// The real restore, after the dry run showed its plan.
    pub(crate) fn restore_apply(&mut self, d: &RestoreDialog) {
        let name = d.name.as_str().trim().to_owned();
        if let Some(entry) = d.selected() {
            self.run_action(studio_cmds::restore(
                d.machine.clone(),
                &entry.name,
                &name,
                false,
                d.confirm_token.as_deref(),
            ));
        }
    }
}
