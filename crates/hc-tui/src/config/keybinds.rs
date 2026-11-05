// Derived from herdr src/config/keybinds.rs (Apache-2.0).

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use serde::{Deserialize, Serialize};

use super::Config;

pub type KeyCombo = (KeyCode, KeyModifiers);

/// Built-in prefix used when `keys.prefix` is unset or invalid.
pub(crate) const DEFAULT_PREFIX: KeyCombo = (KeyCode::Char('b'), KeyModifiers::CONTROL);

/// Minimal key shape the matcher needs, mirroring `input::TerminalKey`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalKey {
    pub code: KeyCode,
    pub modifiers: KeyModifiers,
    pub kind: KeyEventKind,
    pub shifted_codepoint: Option<u32>,
    pub generated_text: Option<String>,
}

impl TerminalKey {
    pub fn new(code: KeyCode, modifiers: KeyModifiers) -> Self {
        Self {
            code,
            modifiers,
            kind: KeyEventKind::Press,
            shifted_codepoint: None,
            generated_text: None,
        }
    }
}

impl From<KeyEvent> for TerminalKey {
    fn from(value: KeyEvent) -> Self {
        Self {
            code: value.code,
            modifiers: value.modifiers,
            kind: value.kind,
            shifted_codepoint: None,
            generated_text: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct LiveKeybindConfig {
    /// Every configured prefix key. The first entry is the primary prefix used
    /// for compact display; all entries enter prefix mode.
    pub prefix: Vec<KeyCombo>,
    pub keybinds: Keybinds,
}

impl LiveKeybindConfig {
    /// Whether `key` is one of the configured prefix keys.
    pub fn matches_prefix(&self, key: &TerminalKey) -> bool {
        self.prefix
            .iter()
            .any(|combo| terminal_key_matches_combo(key, *combo))
    }

    /// Primary prefix combo, used for the compact status bar.
    pub fn primary_prefix(&self) -> Option<KeyCombo> {
        self.prefix.first().copied()
    }

    /// Primary prefix rendered for the compact status bar.
    pub fn primary_prefix_label(&self) -> String {
        self.primary_prefix()
            .map(format_key_combo)
            .unwrap_or_else(|| format_key_combo(DEFAULT_PREFIX))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(untagged)]
pub enum BindingConfig {
    One(String),
    Many(Vec<String>),
}

impl Default for BindingConfig {
    fn default() -> Self {
        Self::One(String::new())
    }
}

impl BindingConfig {
    pub fn one(value: impl Into<String>) -> Self {
        Self::One(value.into())
    }

    pub fn empty() -> Self {
        Self::One(String::new())
    }

    pub(crate) fn values(&self) -> Vec<&str> {
        match self {
            Self::One(value) => vec![value.as_str()],
            Self::Many(values) => values.iter().map(String::as_str).collect(),
        }
    }

    pub(crate) fn into_values(self) -> Vec<String> {
        match self {
            Self::One(value) => vec![value],
            Self::Many(values) => values,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindingTrigger {
    Direct(KeyCombo),
    Prefix(KeyCombo),
}

impl BindingTrigger {
    pub fn combo(self) -> KeyCombo {
        match self {
            Self::Direct(combo) | Self::Prefix(combo) => combo,
        }
    }

    pub fn is_direct(self) -> bool {
        matches!(self, Self::Direct(_))
    }

    pub fn is_prefix(self) -> bool {
        matches!(self, Self::Prefix(_))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedBinding {
    pub trigger: BindingTrigger,
    pub label: String,
}

impl ResolvedBinding {
    fn matches_terminal_key(&self, key: &TerminalKey) -> bool {
        terminal_key_matches_combo(key, self.trigger.combo())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ActionKeybinds {
    pub bindings: Vec<ResolvedBinding>,
}

impl ActionKeybinds {
    pub fn matches_prefix_key(&self, key: &TerminalKey) -> bool {
        self.bindings
            .iter()
            .any(|binding| binding.trigger.is_prefix() && binding.matches_terminal_key(key))
    }

    pub fn matches_direct_key(&self, key: &TerminalKey) -> bool {
        self.bindings
            .iter()
            .any(|binding| binding.trigger.is_direct() && binding.matches_terminal_key(key))
    }

    pub fn labels(&self) -> Vec<String> {
        self.bindings
            .iter()
            .map(|binding| binding.label.clone())
            .collect()
    }

    pub fn label(&self) -> Option<String> {
        let labels = self.labels();
        if labels.is_empty() {
            None
        } else {
            Some(labels.join(" / "))
        }
    }

    pub fn prefix_rhs_label(&self) -> Option<String> {
        let labels: Vec<String> = self
            .bindings
            .iter()
            .filter(|binding| binding.trigger.is_prefix())
            .map(|binding| {
                binding
                    .label
                    .strip_prefix("prefix+")
                    .unwrap_or(&binding.label)
                    .to_string()
            })
            .collect();
        if labels.is_empty() {
            None
        } else {
            Some(labels.join(" / "))
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexedKeybind {
    pub trigger: BindingTrigger,
    pub label: String,
}

impl IndexedKeybind {
    pub fn matched_index(&self, key: &TerminalKey) -> Option<usize> {
        let combo = self.trigger.combo();
        let (expected_code, _) = normalize_key_combo(combo);
        let KeyCode::Char(key_number @ '1'..='9') = expected_code else {
            return None;
        };
        let legacy_shifted_number = matches!(key.code, KeyCode::Char(c)
            if shifted_number_symbol(c) == Some(key_number)
                && indexed_shifted_number_matches(key, combo, key_number));
        if terminal_key_matches_combo(key, combo) || legacy_shifted_number {
            Some((key_number as usize) - ('1' as usize))
        } else {
            None
        }
    }
}

/// Parsed keybinds for Herdr actions.
#[derive(Debug, Clone)]
pub struct NavigateKeybinds {
    pub workspace_up: ActionKeybinds,
    pub workspace_down: ActionKeybinds,
    pub pane_left: ActionKeybinds,
    pub pane_down: ActionKeybinds,
    pub pane_up: ActionKeybinds,
    pub pane_right: ActionKeybinds,
}

/// Parsed keybinds for Herdr actions.
#[derive(Debug, Clone)]
pub struct Keybinds {
    pub navigate: NavigateKeybinds,
    pub help: ActionKeybinds,
    pub settings: ActionKeybinds,
    pub new_workspace: ActionKeybinds,
    pub new_worktree: ActionKeybinds,
    pub open_worktree: ActionKeybinds,
    pub remove_worktree: ActionKeybinds,
    pub rename_workspace: ActionKeybinds,
    pub close_workspace: ActionKeybinds,
    pub workspace_picker: ActionKeybinds,
    pub goto: ActionKeybinds,
    pub detach: ActionKeybinds,
    pub reload_config: ActionKeybinds,
    pub open_notification_target: ActionKeybinds,
    pub previous_workspace: ActionKeybinds,
    pub next_workspace: ActionKeybinds,
    pub previous_agent: ActionKeybinds,
    pub next_agent: ActionKeybinds,
    pub focus_agent: Vec<IndexedKeybind>,
    pub new_tab: ActionKeybinds,
    pub rename_tab: ActionKeybinds,
    pub previous_tab: ActionKeybinds,
    pub next_tab: ActionKeybinds,
    pub move_tab_previous: ActionKeybinds,
    pub move_tab_next: ActionKeybinds,
    pub switch_tab: Vec<IndexedKeybind>,
    pub switch_workspace: Vec<IndexedKeybind>,
    pub close_tab: ActionKeybinds,
    pub rename_pane: ActionKeybinds,
    pub edit_scrollback: ActionKeybinds,
    pub clear_pane: ActionKeybinds,
    pub copy_mode: ActionKeybinds,
    pub focus_pane_left: ActionKeybinds,
    pub focus_pane_down: ActionKeybinds,
    pub focus_pane_up: ActionKeybinds,
    pub focus_pane_right: ActionKeybinds,
    pub swap_pane_left: ActionKeybinds,
    pub swap_pane_down: ActionKeybinds,
    pub swap_pane_up: ActionKeybinds,
    pub swap_pane_right: ActionKeybinds,
    pub cycle_pane_next: ActionKeybinds,
    pub cycle_pane_previous: ActionKeybinds,
    pub last_pane: ActionKeybinds,
    pub split_vertical: ActionKeybinds,
    pub split_horizontal: ActionKeybinds,
    pub close_pane: ActionKeybinds,
    pub zoom: ActionKeybinds,
    pub resize_mode: ActionKeybinds,
    pub resize_pane_left: ActionKeybinds,
    pub resize_pane_down: ActionKeybinds,
    pub resize_pane_up: ActionKeybinds,
    pub resize_pane_right: ActionKeybinds,
    pub toggle_sidebar: ActionKeybinds,
    pub custom_commands: Vec<CustomCommandKeybind>,
}

impl Default for Keybinds {
    fn default() -> Self {
        Config::default().keybinds()
    }
}

#[derive(Debug, Clone)]
pub struct CustomCommandKeybind {
    pub bindings: ActionKeybinds,
    pub label: String,
    pub command: String,
    pub action: CustomCommandAction,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CustomCommandAction {
    Shell,
    Pane,
    Popup,
    PluginAction,
}

#[path = "keybind_parsing.rs"]
mod parsing;
pub use parsing::*;

#[path = "keybind_matching.rs"]
mod matching;
pub use matching::*;
