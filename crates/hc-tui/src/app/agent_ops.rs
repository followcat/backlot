// Agent-menu operations — an hc-tui extension documented in
// docs/parity.md. The reference TUI has no agent context menu; these
// items present the agent controls in the reference's menu style and
// reuse its dialog/confirm presentation.
use super::dialogs::{Body, Target};
use super::*;
use crate::chrome::menu::{ContextMenu, ContextMenuTarget};
use crate::chrome::{
    dialogs::{Confirm, Rename},
    text_editor::TextEditor,
};
use crate::operations::Action;

impl App {
    /// Right-click on an agent row opens the agent menu — the extension
    /// target (`ContextMenuTarget::Agent`).
    pub(super) fn open_agent_context_menu(&mut self, pane_id: &str, point: (u16, u16)) {
        self.menu = Some(ContextMenu {
            target: ContextMenuTarget::Agent {
                pane_id: pane_id.to_owned(),
            },
            x: point.0,
            y: point.1,
            highlighted: 0,
        });
    }

    /// "Send a line..." — the small text dialog; Enter dispatches
    /// `terminal.send` with the typed line.
    pub(super) fn send_line_for(&mut self, pane_id: &str) {
        let Some(term) = self.view.inventory.get(pane_id) else {
            return;
        };
        self.open_dialog(
            Target::SendLine {
                terminal: pane_id.to_owned(),
                created_ms: term.created_ms,
            },
            Body::Rename(Rename {
                title: "send a line",
                input: TextEditor::new("", false),
            }),
        );
    }

    /// "Kill" — the confirm dialog gates `terminal.kill`, matching the
    /// machine-remove presentation.
    pub(super) fn agent_kill_for(&mut self, pane_id: &str) {
        let Some(term) = self.view.inventory.get(pane_id) else {
            return;
        };
        let detail = term
            .labels
            .get("herdr.title")
            .filter(|label| !label.is_empty())
            .cloned()
            .unwrap_or_else(|| pane_id.to_owned());
        self.open_dialog(
            Target::AgentKill {
                terminal: pane_id.to_owned(),
                created_ms: term.created_ms,
            },
            Body::Confirm(Confirm {
                title: "Kill agent?".into(),
                detail,
            }),
        );
    }

    /// "Mark read" — write `herdr.seen = turn` through the seen worker,
    /// recording the optimistic marker so `share_seen` does not resend.
    pub(super) fn mark_seen_for(&mut self, pane_id: &str) {
        let Some(term) = self.view.inventory.get(pane_id) else {
            return;
        };
        self.seen_looked.insert(pane_id.to_owned(), term.turn);
        if !self.seen_inflight.insert((pane_id.to_owned(), term.turn)) {
            return;
        }
        if self
            .seen_tx
            .try_send(crate::app::seen::SeenWrite {
                terminal: pane_id.to_owned(),
                turn: term.turn,
            })
            .is_err()
        {
            self.seen_inflight.remove(&(pane_id.to_owned(), term.turn));
        }
    }

    /// "Mark unread" — the explicit lowering write `herdr.seen = turn-1`
    /// (`docs/labels.md`). At turn 0 nothing has finished, so there is
    /// nothing to mark.
    pub(super) fn mark_unseen_for(&mut self, pane_id: &str) {
        let Some(term) = self.view.inventory.get(pane_id) else {
            return;
        };
        let Some(mark) = term.turn.checked_sub(1) else {
            return;
        };
        // Leave `seen_looked` at the higher turn: the visible-pane share
        // pass must not raise the value again this turn.
        if !self.seen_inflight.insert((pane_id.to_owned(), mark)) {
            return;
        }
        if self
            .seen_tx
            .try_send(crate::app::seen::SeenWrite {
                terminal: pane_id.to_owned(),
                turn: mark,
            })
            .is_err()
        {
            self.seen_inflight.remove(&(pane_id.to_owned(), mark));
        }
    }

    /// The agent-menu dispatch — every item resolves its terminal at
    /// activation so a stale menu never acts on a vanished agent.
    pub(super) fn agent_menu_action(
        &mut self,
        pane_id: &str,
        action: crate::chrome::menu::ContextMenuAction,
    ) {
        use crate::chrome::menu::ContextMenuAction as A;
        let created_ms = self.view.inventory.get(pane_id).map(|t| t.created_ms);
        match action {
            A::SendLine => self.send_line_for(pane_id),
            A::Interrupt => {
                if let Some(created_ms) = created_ms {
                    self.submit_action(Action::Interrupt {
                        terminal: pane_id.to_owned(),
                        created_ms,
                    });
                }
            }
            A::MarkRead => self.mark_seen_for(pane_id),
            A::MarkUnread => self.mark_unseen_for(pane_id),
            A::Keep => {
                if let Some(created_ms) = created_ms {
                    self.submit_action(Action::Keep {
                        terminal: pane_id.to_owned(),
                        created_ms,
                    });
                }
            }
            A::Park => {
                if let Some(created_ms) = created_ms {
                    self.submit_action(Action::Park {
                        terminal: pane_id.to_owned(),
                        created_ms,
                    });
                }
            }
            A::Kill => self.agent_kill_for(pane_id),
            _ => {}
        }
    }
}
