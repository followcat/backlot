//! Menu activation — `context_menu_activate`'s studio arm maps
//! each `StudioAct` to a `run_action`, a native dialog or a
//! navigation (split from the open paths to keep files small).

use crate::studio_dialog::{ConfirmFor, ReasonFor, StudioDialog};
use crate::studio_menu::{StudioAct, StudioMenu};
use crate::{studio_cmds, studio_dialog};

use super::App;

impl App {
    /// `context_menu_activate`'s studio arm — a dimmed row does
    /// nothing; the rest dispatch through `run_action` or the
    /// dialogs.
    pub(crate) fn studio_menu_activate(&mut self, menu: &StudioMenu, index: usize) {
        let Some(item) = menu.items().get(index).cloned() else {
            return;
        };
        if item.dim.is_some() {
            return;
        }
        match item.act {
            StudioAct::FocusTerminal => self.studio_menu_focus(menu),
            StudioAct::Peek => self.studio_menu_peek(menu),
            StudioAct::Sheet => {
                if let Some(key) = &menu.studio {
                    self.sheet_open(key.clone(), None);
                }
            }
            StudioAct::TaskSheet => {
                let task = menu
                    .task
                    .as_ref()
                    .map(|t| t.id.clone())
                    .or_else(|| menu.role.as_ref().and_then(|r| r.task.clone()));
                if let (Some(key), Some(task)) = (&menu.studio, task) {
                    self.sheet_open(key.clone(), Some(task));
                }
            }
            StudioAct::Decide(allow) => {
                if let (Some(key), Some(role)) = (&menu.studio, menu.role.as_ref()) {
                    self.run_action(studio_cmds::decide(key, &role.role, allow));
                }
            }
            StudioAct::DirOverride(allow) => {
                if let (Some(key), Some(role)) = (&menu.studio, menu.role.as_ref()) {
                    self.studio_dialog = Some(StudioDialog::confirm(
                        format!(
                            "Answer prompt myself · {}",
                            studio_dialog::StudioDialog::place(key)
                        ),
                        format!(
                            "director normally reviews {}'s worktree prompts. Record a user {} decision anyway?",
                            role.name,
                            if allow { "allow" } else { "deny" }
                        ),
                        ConfirmFor::DirOverride {
                            key: key.clone(),
                            role: role.role.clone(),
                            allow,
                        },
                    ));
                }
            }
            StudioAct::TaskVerb(verb) => {
                let task = menu
                    .task
                    .as_ref()
                    .map(|t| t.id.clone())
                    .or_else(|| menu.role.as_ref().and_then(|r| r.task.clone()));
                if let (Some(key), Some(task)) = (&menu.studio, task) {
                    self.studio_dialog = Some(StudioDialog::reason(
                        format!(
                            "{} task {} · {}",
                            verb_title(verb),
                            task,
                            studio_dialog::StudioDialog::place(key)
                        ),
                        true,
                        ReasonFor::TaskVerb {
                            key: key.clone(),
                            task,
                            verb,
                        },
                    ));
                }
            }
            StudioAct::Priority(level) => {
                let task = menu
                    .task
                    .as_ref()
                    .map(|t| t.id.clone())
                    .or_else(|| menu.role.as_ref().and_then(|r| r.task.clone()));
                if let (Some(key), Some(task)) = (&menu.studio, task) {
                    self.run_action(studio_cmds::priority(key, &task, &level.to_string()));
                }
            }
            StudioAct::Reopen => {
                if let (Some(key), Some(role)) = (&menu.studio, menu.role.as_ref()) {
                    self.run_action(studio_cmds::reopen(key, &role.role));
                }
            }
            StudioAct::Close => {
                if let (Some(key), Some(role)) = (&menu.studio, menu.role.as_ref()) {
                    self.studio_dialog = Some(StudioDialog::confirm(
                        format!(
                            "Close {} · {}",
                            role.name,
                            studio_dialog::StudioDialog::place(key)
                        ),
                        format!(
                            "{}'s terminal is removed; the role stays in the ledger and can reopen. Close it?",
                            role.name
                        ),
                        ConfirmFor::Close {
                            key: key.clone(),
                            role: role.role.clone(),
                        },
                    ));
                }
            }
            StudioAct::ClearAttention => {
                if let (Some(key), Some(role), Some(seq)) = (
                    &menu.studio,
                    menu.role.as_ref(),
                    menu.role.as_ref().and_then(|r| r.attention_seq),
                ) {
                    self.studio_dialog = Some(StudioDialog::confirm(
                        format!(
                            "Clear attention #{seq} · {}",
                            studio_dialog::StudioDialog::place(key)
                        ),
                        format!(
                            "drop {}'s open prompt without answering it — the role keeps whatever it was doing. Clear #{seq}?",
                            role.name
                        ),
                        ConfirmFor::ClearAttention {
                            key: key.clone(),
                            role: role.role.clone(),
                            seq,
                        },
                    ));
                }
            }
            StudioAct::StageStart => {
                let machine = menu.machine().map(str::to_owned);
                self.run_action(studio_cmds::stage_start(machine));
            }
            StudioAct::StageStop => {
                let machine = menu.machine().map(str::to_owned);
                self.studio_dialog = Some(StudioDialog::confirm(
                    format!("Stop stage · {}", machine.as_deref().unwrap_or("Local")),
                    "the stage's terminal exits; the studio's terminals and ledger keep running. Stop it?"
                        .to_owned(),
                    ConfirmFor::StageStop { machine },
                ));
            }
            StudioAct::Finish => {
                if let Some(key) = &menu.studio {
                    self.studio_dialog = Some(StudioDialog::reason(
                        format!(
                            "Finish studio · {}",
                            studio_dialog::StudioDialog::place(key)
                        ),
                        false,
                        ReasonFor::Finish { key: key.clone() },
                    ));
                }
            }
            StudioAct::Tell => self.tell_dialog_open(menu),
            StudioAct::Revise => self.revise_dialog_open(menu),
            StudioAct::Records => {
                if let Some(key) = &menu.studio {
                    self.run_action(studio_cmds::records(key));
                }
            }
            StudioAct::CardApply => {
                if let Some(id) = &menu.card {
                    self.run_action(studio_cmds::card_apply(id));
                }
            }
            StudioAct::CardShow => {
                if let Some(id) = &menu.card {
                    self.run_action(studio_cmds::card_show(id));
                }
            }
            StudioAct::CardCancel => {
                if let Some(id) = &menu.card {
                    self.studio_dialog = Some(StudioDialog::reason(
                        format!("Cancel card {id}"),
                        false,
                        ReasonFor::CardCancel { id: id.clone() },
                    ));
                }
            }
        }
    }

    /// `Focus producer` / `Focus terminal` — the same landing an
    /// Enter press gives the row.
    fn studio_menu_focus(&mut self, menu: &StudioMenu) {
        if let Some(role) = menu.role.as_ref() {
            if let Some(terminal) = role.terminal.clone() {
                self.focus_pane(terminal);
            }
            return;
        }
        if let Some(key) = &menu.studio {
            self.focus_studio(key);
        }
    }

    /// `Peek screen` — the role's live screen, prompt pair included
    /// like `p` on the row.
    fn studio_menu_peek(&mut self, menu: &StudioMenu) {
        let Some(role) = menu.role.as_ref() else {
            return;
        };
        let Some(terminal) = role.terminal.clone() else {
            self.show_notice("Peek unavailable", "the role has no screen to peek");
            return;
        };
        self.open_peek(terminal);
        if role.attention_user
            && let (Some(peek), Some(key)) = (self.peek.as_mut(), menu.studio.as_ref())
        {
            peek.prompt = Some((key.clone(), role.role.clone()));
        }
    }
}

/// The reason dialog's verb word — `Pause task t1 · …`.
fn verb_title(verb: &str) -> &'static str {
    match verb {
        "pause" => "Pause",
        "resume" => "Resume",
        "abandon" => "Abandon",
        _ => "Take over",
    }
}
