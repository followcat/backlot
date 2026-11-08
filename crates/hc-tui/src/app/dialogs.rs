// Derived from herdr src/client/shell/overlay_input.rs and mouse.rs (Apache-2.0).
use super::*;
use crate::{
    chrome::{
        dialogs::{Confirm, Rename},
        text_editor::TextEditor,
    },
    operations::{Action, Change, Creation, Edit, MachineOp},
};
use crossterm::event::{KeyCode, KeyModifiers};

pub(super) enum Target {
    NewTab(Box<Creation>, String),
    NewWorkspace(Box<Creation>, String),
    Workspace {
        key: String,
        proposal: crate::structure::Adoption,
    },
    Tab(Edit, bool, String),
    Pane(String, u64),
    MachineAdd,
    MachineRename {
        machine: String,
    },
    MachineRemove {
        machine: String,
    },
    /// Agent-menu targets — hc-tui extension (parity.md).
    SendLine {
        terminal: String,
        created_ms: u64,
    },
    AgentKill {
        terminal: String,
        created_ms: u64,
    },
    Close(Action),
}
pub(super) enum Body {
    Rename(Rename),
    Confirm(Confirm),
}
pub(super) struct Dialog {
    pub core: String,
    pub target: Target,
    pub body: Body,
}

impl App {
    pub(super) fn open_dialog(&mut self, target: Target, body: Body) {
        self.release_pane_mouse();
        self.chrome_drag = None;
        self.dialog = Some(Dialog {
            core: self
                .view
                .core
                .as_ref()
                .map(|c| c.id.clone())
                .unwrap_or_default(),
            target,
            body,
        });
    }
    pub(super) fn new_tab(&mut self) {
        let creation = match self.creation(None, false) {
            Ok(c) => c,
            Err(e) => {
                self.show_notice("New tab unavailable", &e);
                return;
            }
        };
        if self.shell.prompt_new_tab_name {
            let count = self
                .projection
                .snapshot
                .tabs
                .iter()
                .filter(|t| {
                    Some(&t.workspace_id) == self.projection.snapshot.focused_workspace_id.as_ref()
                })
                .count()
                + 1;
            let default = count.to_string();
            self.open_dialog(
                Target::NewTab(Box::new(creation), default.clone()),
                Body::Rename(Rename {
                    title: "new tab",
                    input: TextEditor::new(&default, true),
                }),
            );
        } else {
            self.submit_action(Action::Create(Box::new(creation)));
        }
    }
    /// `new_workspace` binding: prompt with the derived label when
    /// `prompt_new_workspace_name` is set, else create directly — matching
    /// the reference's `open_new_workspace_overlay`/direct create.
    pub(super) fn new_workspace(&mut self) {
        let (creation, suggested) = match self.workspace_creation() {
            Ok(pair) => pair,
            Err(e) => {
                self.show_notice("New workspace unavailable", &e);
                return;
            }
        };
        if self.shell.prompt_new_workspace_name {
            self.open_dialog(
                Target::NewWorkspace(Box::new(creation), suggested.clone()),
                Body::Rename(Rename {
                    title: "new workspace",
                    input: TextEditor::new(&suggested, true),
                }),
            );
        } else {
            self.submit_action(Action::Create(Box::new(creation)));
        }
    }

    /// `rename_workspace` on the focused workspace.
    pub(super) fn rename_workspace(&mut self) {
        if let Some(id) = self.projection.snapshot.focused_workspace_id.clone() {
            self.rename_workspace_for(&id);
        }
    }

    /// The workspace context-menu rename targets an explicit workspace; the
    /// label persists as the owned document's adoption for that space.
    pub(super) fn rename_workspace_for(&mut self, workspace_id: &str) {
        let Some(workspace) = self
            .projection
            .endpoints
            .iter()
            .filter_map(|e| e.snapshot.as_deref())
            .flat_map(|s| &s.workspaces)
            .find(|w| w.workspace_id == workspace_id)
        else {
            return;
        };
        let machine = self.workspace_machine(workspace_id);
        let raw_space = workspace_id
            .strip_prefix("space:")
            .and_then(|t| t.split_once(':'))
            .and_then(|(len, raw)| {
                (raw.len() == len.parse::<usize>().ok()?).then(|| raw.to_owned())
            })
            .unwrap_or_else(|| workspace_id.to_owned());
        let proposal = crate::structure::Adoption {
            machine,
            space: raw_space,
            tabs: Default::default(),
            name: workspace.label.clone(),
            custom_label: workspace.custom_label,
            ..Default::default()
        };
        let key = crate::project::layout_key(proposal.machine.as_deref(), workspace_id);
        self.open_dialog(
            Target::Workspace { key, proposal },
            Body::Rename(Rename {
                title: "rename workspace",
                input: TextEditor::new(&workspace.label, false),
            }),
        );
    }

    /// Machine dialogs are an hc-tui extension — the reference TUI has no
    /// machine management surface (documented in docs/parity.md).
    pub(super) fn machine_add(&mut self) {
        self.open_dialog(
            Target::MachineAdd,
            Body::Rename(Rename {
                title: "add machine",
                input: TextEditor::new("", false),
            }),
        );
    }
    pub(super) fn machine_rename(&mut self, machine: String) {
        self.open_dialog(
            Target::MachineRename {
                machine: machine.clone(),
            },
            Body::Rename(Rename {
                title: "rename machine",
                input: TextEditor::new(&machine, true),
            }),
        );
    }
    pub(super) fn machine_remove(&mut self, machine: String) {
        self.open_dialog(
            Target::MachineRemove {
                machine: machine.clone(),
            },
            Body::Confirm(Confirm {
                title: "Remove machine?".into(),
                detail: machine,
            }),
        );
    }

    pub(super) fn rename_tab(&mut self) {
        let Some(tab) = self
            .projection
            .snapshot
            .tabs
            .iter()
            .find(|t| Some(&t.tab_id) == self.projection.snapshot.focused_tab_id.as_ref())
        else {
            return;
        };
        let body = Rename {
            title: "rename tab",
            input: TextEditor::new(&tab.label, false),
        };
        let Some(edit) = self.layout_edit(Change::Name(String::new())) else {
            return;
        };
        self.open_dialog(
            Target::Tab(edit, !tab.custom_label, tab.label.clone()),
            Body::Rename(body),
        );
    }
    pub(super) fn rename_pane(&mut self) {
        let Some(id) = self.focused.clone() else {
            return;
        };
        self.rename_pane_for(&id);
    }
    /// Context-menu pane rename targets an explicit pane.
    pub(super) fn rename_pane_for(&mut self, id: &str) {
        let id = id.to_owned();
        let Some(term) = self.view.inventory.get(&id) else {
            return;
        };
        let label = term
            .labels
            .get("herdr.title")
            .map(String::as_str)
            .filter(|s| !s.is_empty());
        let body = Rename {
            title: "rename pane",
            input: TextEditor::new(label.unwrap_or_default(), label.is_none()),
        };
        self.open_dialog(Target::Pane(id, term.created_ms), Body::Rename(body));
    }
    pub(super) fn insert_dialog_text(&mut self, text: &str) -> bool {
        if let Some(dialog) = &mut self.dialog {
            if let Body::Rename(rename) = &mut dialog.body {
                rename.input.insert(text);
            }
            true
        } else {
            false
        }
    }
    pub(super) fn dialog_key(&mut self, key: &input::TerminalKey) {
        let combo = crate::input::terminal_key_of(key);
        if combo.code == KeyCode::Enter {
            self.save_dialog();
            return;
        }
        if combo.code == KeyCode::Esc {
            if self
                .dialog
                .take()
                .is_some_and(|d| matches!(d.body, Body::Confirm(_)))
            {
                self.enter_workspace_picker();
            }
            return;
        }
        if let Some(Dialog {
            body: Body::Rename(rename),
            ..
        }) = &mut self.dialog
        {
            if key.text_commit.as_ref().is_none_or(String::is_empty)
                && ((combo.code == KeyCode::Char('c') && combo.modifiers == KeyModifiers::CONTROL)
                    || (combo.code == KeyCode::Backspace
                        && combo.modifiers.contains(KeyModifiers::SUPER)))
            {
                rename.input.clear();
            } else {
                rename.input.handle_key(key);
            }
        }
    }
    fn save_dialog(&mut self) {
        let Some(dialog) = self.dialog.take() else {
            return;
        };
        if self.view.core.as_ref().is_none_or(|c| c.id != dialog.core) {
            self.show_notice("Action unavailable", "Core changed; reopen the dialog");
            return;
        }
        let name = match dialog.body {
            Body::Rename(rename) => rename.input.trim().to_owned(),
            Body::Confirm(_) => String::new(),
        };
        let action = match dialog.target {
            Target::NewTab(mut creation, default) => {
                if let Change::NewTab { name: label, .. } = &mut creation.edit.change {
                    *label = if name == default { String::new() } else { name };
                }
                Some(Action::Create(creation))
            }
            Target::NewWorkspace(mut creation, suggested) => {
                // Same rule as the reference: empty input or the derived
                // label means the automatic name (custom_label stays off).
                let custom = !name.is_empty() && name != suggested;
                if let Some(adoption) = &mut creation.space_adoption {
                    adoption.name = if custom { name } else { suggested };
                    adoption.custom_label = custom;
                }
                Some(Action::Create(creation))
            }
            Target::Workspace { key, mut proposal } => {
                if name.is_empty() {
                    None
                } else {
                    proposal.name = name;
                    proposal.custom_label = true;
                    Some(Action::RenameSpace { key, proposal })
                }
            }
            Target::MachineAdd => {
                if name.is_empty() {
                    None
                } else {
                    Some(Action::Machine(MachineOp::Add { target: name }))
                }
            }
            Target::MachineRename { machine } => {
                if name.is_empty() {
                    None
                } else {
                    Some(Action::Machine(MachineOp::Rename {
                        machine,
                        new_name: name,
                    }))
                }
            }
            Target::MachineRemove { machine } => {
                Some(Action::Machine(MachineOp::Remove { machine }))
            }
            Target::SendLine {
                terminal,
                created_ms,
            } => Some(Action::SendLine {
                terminal,
                created_ms,
                text: name,
            }),
            Target::AgentKill {
                terminal,
                created_ms,
            } => Some(Action::Kill {
                terminal,
                created_ms,
                allow_gone: false,
            }),
            Target::Tab(mut edit, auto, original) => {
                if name.is_empty() || auto && name == original {
                    None
                } else {
                    edit.change = Change::Name(name);
                    Some(Action::Edit(edit))
                }
            }
            Target::Pane(terminal, created_ms) => Some(Action::RenamePane {
                terminal,
                created_ms,
                name,
            }),
            Target::Close(action) => {
                if !self.close_membership_valid(&action) {
                    self.show_notice(
                        "Action unavailable",
                        "Close target changed; reopen the confirmation",
                    );
                    return;
                }
                Some(action)
            }
        };
        if let Some(action) = action {
            self.submit_action(action);
        }
    }
    pub(super) fn dialog_mouse(&mut self, mouse: input::MouseInput) {
        if mouse.kind != input::MouseEventKind::Down(input::MouseButton::Left) {
            return;
        }
        let pos = ratatui::layout::Position::new(mouse.col, mouse.row);
        if !self.dialog_hits.area.contains(pos) {
            self.dialog = None;
            return;
        }
        if self.dialog_hits.cancel.contains(pos) {
            self.dialog = None;
            return;
        }
        if self.dialog_hits.primary.contains(pos) {
            self.save_dialog();
        } else if self.dialog_hits.clear.contains(pos) {
            if let Some(Dialog {
                body: Body::Rename(rename),
                ..
            }) = &mut self.dialog
            {
                rename.input.clear();
            }
        } else {
            self.dialog = None;
        }
    }
}

impl App {
    pub(super) fn render_dialog(&mut self, buf: &mut ratatui::buffer::Buffer) {
        let Some(dialog) = &self.dialog else {
            self.dialog_hits = Default::default();
            return;
        };
        use ratatui::style::Modifier;
        for cell in &mut buf.content {
            cell.modifier = Modifier::from_bits_truncate(cell.modifier.bits() & !0xf000);
            cell.set_style(cell.style().add_modifier(Modifier::DIM));
        }
        self.dialog_hits = match &dialog.body {
            Body::Rename(rename) => {
                crate::chrome::dialogs::render_rename(buf, rename, &self.shell.palette)
            }
            Body::Confirm(confirm) => {
                crate::chrome::dialogs::render_confirm(buf, confirm, &self.shell.palette)
            }
        }
        .unwrap_or_default();
        let pos = self
            .dialog_hits
            .cursor
            .as_ref()
            .filter(|c| c.visible)
            .map(|c| (c.x, c.y));
        self.want_cursor = draw::CursorWish {
            pos,
            anchor: pos,
            shape: hc_vt::CursorShape::Default,
            blinking: true,
        };
    }
}
