//! Key and submit handling for the 3-choice opening-design sign-off modal
//! (`StudioDialog::DesignSign`).

use crossterm::event::{KeyCode, KeyModifiers};

use super::App;
use crate::studio_cmds;
use crate::studio_dialog::{DesignSignChoice, DesignSignDialog, StudioDialog};

impl App {
    pub(super) fn design_sign_key(
        &mut self,
        mut d: DesignSignDialog,
        code: KeyCode,
        mods: KeyModifiers,
    ) -> Option<StudioDialog> {
        match code {
            KeyCode::Esc => None,
            KeyCode::Up | KeyCode::Char('k') | KeyCode::BackTab => {
                d.move_selection(-1);
                Some(StudioDialog::DesignSign(d))
            }
            KeyCode::Down | KeyCode::Char('j') | KeyCode::Tab => {
                d.move_selection(1);
                Some(StudioDialog::DesignSign(d))
            }
            KeyCode::PageUp => {
                d.scroll_by(-3);
                Some(StudioDialog::DesignSign(d))
            }
            KeyCode::PageDown => {
                d.scroll_by(3);
                Some(StudioDialog::DesignSign(d))
            }
            KeyCode::Char('y') if mods.is_empty() => match d.chosen() {
                DesignSignChoice::SignWithChecks | DesignSignChoice::SignWithoutChecks => {
                    self.design_sign_submit(&d);
                    None
                }
                DesignSignChoice::Cancel => Some(StudioDialog::DesignSign(d)),
            },
            KeyCode::Enter => {
                self.design_sign_submit(&d);
                None
            }
            _ => Some(StudioDialog::DesignSign(d)),
        }
    }

    pub(super) fn design_sign_submit(&mut self, d: &DesignSignDialog) {
        match d.chosen() {
            DesignSignChoice::SignWithChecks => {
                self.run_action(studio_cmds::design_sign(&d.key, false, None, None, false));
            }
            DesignSignChoice::SignWithoutChecks => {
                self.run_action(studio_cmds::design_sign(&d.key, false, None, None, true));
            }
            DesignSignChoice::Cancel => {}
        }
    }
}
