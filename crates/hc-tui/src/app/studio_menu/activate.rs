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
        self.run_studio_act(menu, item.act);
    }

    /// One `StudioAct` on a built menu — the `Ctrl+B ?` launcher
    /// calls this directly (S5): its rows carry the same
    /// `StudioMenu`/`StudioAct` pair a menu press resolves to, so
    /// actions, dialogs and focuses flow through this single arm.
    pub(crate) fn run_studio_act(&mut self, menu: &StudioMenu, act: StudioAct) {
        match act {
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
            StudioAct::Restart => {
                if let Some(role) = menu.role.as_ref() {
                    let resumable = menu.studio.is_some() && role.runner_resumes();
                    self.studio_dialog = Some(StudioDialog::Restart(
                        crate::studio_dialog::RestartDialog::new(
                            menu.studio.clone(),
                            &role.role,
                            &role.name,
                            resumable,
                        ),
                    ));
                }
            }
            StudioAct::Recast => {
                if let Some(role) = menu.role.as_ref() {
                    self.recast_dialog_open(
                        menu.studio.as_ref(),
                        &role.role,
                        &role.name,
                        role.runner.clone(),
                        role.model.clone(),
                        role.effort.clone(),
                    );
                }
            }
            StudioAct::Prune => {
                if let Some(key) = &menu.studio {
                    self.prune_open(key);
                }
            }
            StudioAct::Archived => {
                self.open_archived(menu.machine().map(str::to_owned));
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
                if self.can_start_stage(machine.as_deref()) {
                    self.run_action(studio_cmds::stage_start(machine));
                }
            }
            StudioAct::StageLog => {
                self.open_stage_log(menu.machine().map(str::to_owned));
            }
            StudioAct::ToggleFinishedGroup => {
                let machine = menu.machine().map(str::to_owned);
                self.toggle_finished_group(machine);
            }
            StudioAct::StageStop => {
                let machine = menu.machine().map(str::to_owned);
                self.studio_dialog = Some(StudioDialog::confirm(
                    format!("Stop the stage · {}", machine.as_deref().unwrap_or("Local")),
                    "Every studio on this machine stops advancing until the stage starts again. \
                     Running roles and the ledger keep going. Stop it?"
                        .to_owned(),
                    ConfirmFor::StageStop { machine },
                ));
            }
            StudioAct::Finish => {
                if let Some(key) = &menu.studio {
                    // L5: an empty studio earns a plain-words confirm and
                    // `--force`; a worked one keeps the reason dialog.
                    self.studio_dialog = Some(if menu.empty {
                        StudioDialog::confirm(
                            format!(
                                "Finish studio · {}",
                                studio_dialog::StudioDialog::place(key)
                            ),
                            "Nothing was built yet (no requirement, no tasks). Finish anyway?"
                                .to_owned(),
                            ConfirmFor::FinishEmpty { key: key.clone() },
                        )
                    } else {
                        StudioDialog::reason(
                            format!(
                                "Finish studio · {}",
                                studio_dialog::StudioDialog::place(key)
                            ),
                            false,
                            ReasonFor::Finish { key: key.clone() },
                        )
                    });
                }
            }
            StudioAct::DeleteStudio => {
                if let Some(key) = &menu.studio {
                    self.delete_studio_open(key);
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
            // The lot menu's surfaces (S5 §2.3): no `backlot` command
            // behind them — the sheet, the dock, S4's dialog and the
            // Studio settings tab open directly.
            StudioAct::CallSheet => self.hollywood_open(),
            StudioAct::ToggleDock => self.panel_toggle(),
            // A Machine row preselects its own endpoint (U6c); the
            // lot menu's `New studio…` keeps the active machine.
            StudioAct::NewStudio => self.new_studio_open(menu.machine().map(str::to_owned)),
            StudioAct::HollywoodSettings => self.open_studio_settings(),
            // U6c's machine row actions: install/upgrade runs the
            // official installer on the target machine (managed
            // binaries refuse first), restart chains stop → start,
            // and the endpoint ops map to the existing machine calls.
            StudioAct::MachineInstall => {
                self.machine_install(menu.machine().map(str::to_owned));
            }
            StudioAct::StageRestart => {
                self.machine_stage_restart(menu.machine().map(str::to_owned));
            }
            StudioAct::MachineConnect => {
                if let Some(machine) = menu.machine() {
                    self.submit_action(crate::operations::Action::Machine(
                        crate::operations::MachineOp::Connect {
                            machine: machine.to_owned(),
                        },
                    ));
                }
            }
            StudioAct::MachineRename => {
                if let Some(machine) = menu.machine() {
                    self.machine_rename(machine.to_owned());
                }
            }
            StudioAct::MachineRemove => {
                if let Some(machine) = menu.machine() {
                    self.machine_remove(machine.to_owned());
                }
            }
            StudioAct::MachineAdd => self.machine_add(),
            StudioAct::HollywoodShow => match menu.kind {
                crate::studio_menu::MenuKind::Hollywood(Some(seq)) => {
                    self.run_action(studio_cmds::hollywood_show(seq));
                }
                crate::studio_menu::MenuKind::Hollywood(None) => self.hollywood_headline_open(),
                _ => {}
            },
            StudioAct::FocusHollywood => self.hollywood_focus_terminal(),
            StudioAct::ReportShow => {
                if let (Some(key), crate::studio_menu::MenuKind::StudioTalk(seq), Some(file)) =
                    (&menu.studio, menu.kind, menu.talk_file.as_deref())
                {
                    self.run_action(studio_cmds::report_show(key, seq, file));
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
        let studio_title = menu
            .studio
            .as_ref()
            .map(|key| self.studio_peek_title(key, &role.name));
        self.open_peek(terminal);
        if let Some(peek) = self.peek.as_mut() {
            peek.studio_title = studio_title;
            // A prompt the runner cannot answer is not answered by the
            // peek's `y`/`d` either: the terminal is where it is.
            if role.attention_user
                && role.can_answer_prompt
                && let Some(key) = menu.studio.as_ref()
            {
                peek.prompt = Some((key.clone(), role.role.clone()));
            }
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
