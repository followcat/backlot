// Derived from herdr src/app/window_title.rs and terminal_effects.rs (Apache-2.0).
use super::*;
use crate::config::{WindowTitlePart as Part, WindowTitleToken as Token};

impl App {
    pub(super) fn update_window_title(&mut self) {
        let Some(template) = &self.shell.window_title else {
            return;
        };
        let live_title = self.pane.vt().and_then(|vt| vt.title().ok());
        let snapshot = &self.projection.snapshot;
        if self.view.core.is_none() && snapshot.workspaces.is_empty() {
            return;
        }
        let workspace = snapshot
            .workspaces
            .iter()
            .find(|workspace| workspace.focused);
        let tab = snapshot.tabs.iter().find(|tab| tab.focused);
        let terminal = self
            .focused
            .as_deref()
            .and_then(|id| self.view.inventory.terminals().get(id));
        let hostname =
            super::hostname::for_endpoint(&self.view, &self.projection.active_endpoint_id)
                .unwrap_or("");
        let pane_label = terminal
            .and_then(|term| term.labels.get("herdr.title"))
            .map(String::as_str)
            .unwrap_or("");
        let terminal_title = live_title
            .as_deref()
            .filter(|title| !title.is_empty())
            .or_else(|| terminal.map(|term| term.title.as_str()))
            .and_then(crate::chrome::title_text::stripped_terminal_title)
            .unwrap_or_default();
        let mut title = String::new();
        for part in template.parts() {
            title.push_str(match part {
                Part::Literal(text) => text,
                Part::Token(Token::Hostname) => hostname,
                Part::Token(Token::Workspace) => workspace
                    .map(|workspace| workspace.label.as_str())
                    .unwrap_or(""),
                Part::Token(Token::Tab) => tab.map(|tab| tab.label.as_str()).unwrap_or(""),
                Part::Token(Token::Pane) => pane_label,
                Part::Token(Token::TerminalTitle) => &terminal_title,
            });
        }
        let title = crate::config::window_title::sanitize_window_title_text(&title)
            .unwrap_or_else(|| "herdr".into());
        if self.last_window_title.as_deref() != Some(&title)
            && (self.host_control)(format!("\x1b]0;{title}\x07").as_bytes()).is_ok()
        {
            self.last_window_title = Some(title);
        }
    }
}
