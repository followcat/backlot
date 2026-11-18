// Derived from herdr src/input/keybind_help.rs (Apache-2.0).
use std::borrow::Cow;

use crossterm::event::{KeyCode, KeyModifiers};

use crate::{
    app::input::TerminalKey,
    config::{ActionKeybinds, IndexedKeybind, Keybinds},
};

pub(crate) type KeybindHelpEntry = (String, Cow<'static, str>);
pub(crate) type KeybindHelpGroup = (&'static str, Vec<KeybindHelpEntry>);

/// The character a key types, for overlay shortcuts and text fields.
pub(crate) fn keybind_help_text_char(key: &TerminalKey) -> Option<char> {
    let combo = crate::input::terminal_key_of(key);
    if !combo.modifiers.difference(KeyModifiers::SHIFT).is_empty() {
        return None;
    }
    // Text the host says the key produced is authoritative: kitty hosts may
    // report Shift+/ as `/` with text `?` and no shifted alternate.
    if let Some(text) = key.text_commit.as_deref() {
        let mut chars = text.chars();
        if let (Some(character), None) = (chars.next(), chars.next())
            && !character.is_control()
        {
            return Some(character);
        }
    }
    if let Some(character) = key.shifted_codepoint {
        return Some(character);
    }
    let KeyCode::Char(character) = combo.code else {
        return None;
    };
    Some(character)
}

fn entry(key: impl Into<String>, label: &'static str) -> KeybindHelpEntry {
    (key.into(), Cow::Borrowed(label))
}

fn binding_label(bindings: &ActionKeybinds) -> String {
    bindings.label().unwrap_or_else(|| "unset".to_owned())
}

fn indexed_label(bindings: &[IndexedKeybind]) -> String {
    if bindings.is_empty() {
        return "unset".to_owned();
    }
    let mut parts = Vec::new();
    let mut index = 0;
    while index < bindings.len() {
        if let Some(prefix) = indexed_range_prefix(&bindings[index..]) {
            parts.push(format!("{prefix}1..9"));
            index += 9;
        } else {
            parts.push(bindings[index].label.clone());
            index += 1;
        }
    }
    parts.join(" / ")
}

fn indexed_range_prefix(bindings: &[IndexedKeybind]) -> Option<&str> {
    let run = bindings.get(..9)?;
    let prefix = run[0].label.strip_suffix('1')?;
    for (offset, binding) in run.iter().enumerate() {
        let digit = char::from(b'1' + offset as u8);
        if binding.label.strip_suffix(digit) != Some(prefix) {
            return None;
        }
    }
    Some(prefix)
}

pub(crate) fn keybind_help_groups(
    keybinds: &Keybinds,
    prefixes: &[crate::config::KeyCombo],
) -> Vec<KeybindHelpGroup> {
    let mut groups = vec![
        (
            "global",
            vec![
                entry(crate::config::format_prefix_combos(prefixes), "prefix mode"),
                entry("ctrl+g", "lock / unlock keyboard shortcuts"),
                entry(binding_label(&keybinds.help), "keybinds"),
                entry(binding_label(&keybinds.settings), "settings"),
                entry(binding_label(&keybinds.detach), "detach"),
                entry(binding_label(&keybinds.reload_config), "reload config"),
                entry(
                    binding_label(&keybinds.open_notification_target),
                    "open notification target",
                ),
            ],
        ),
        (
            "navigation",
            vec![
                entry("esc", "back"),
                entry(
                    format!(
                        "{} / {}",
                        binding_label(&keybinds.navigate.workspace_up),
                        binding_label(&keybinds.navigate.workspace_down)
                    ),
                    "workspace list",
                ),
                entry(
                    format!(
                        "{} / {} / {} / {} / left / right",
                        binding_label(&keybinds.navigate.pane_left),
                        binding_label(&keybinds.navigate.pane_down),
                        binding_label(&keybinds.navigate.pane_up),
                        binding_label(&keybinds.navigate.pane_right)
                    ),
                    "move focus",
                ),
                entry("tab / shift+tab", "cycle pane"),
                entry("enter", "open workspace"),
                entry("1..9", "switch workspace"),
            ],
        ),
        (
            "workspaces / tabs",
            vec![
                entry(
                    binding_label(&keybinds.workspace_picker),
                    "workspace navigation",
                ),
                entry(binding_label(&keybinds.goto), "session navigator"),
                entry(binding_label(&keybinds.new_workspace), "new workspace"),
                entry(binding_label(&keybinds.new_worktree), "new worktree"),
                entry(binding_label(&keybinds.open_worktree), "open worktree"),
                entry(
                    binding_label(&keybinds.remove_worktree),
                    "delete worktree checkout",
                ),
                entry(
                    binding_label(&keybinds.rename_workspace),
                    "rename workspace",
                ),
                entry(binding_label(&keybinds.close_workspace), "close workspace"),
                entry(
                    binding_label(&keybinds.previous_workspace),
                    "previous workspace",
                ),
                entry(binding_label(&keybinds.next_workspace), "next workspace"),
                entry(
                    indexed_label(&keybinds.switch_workspace),
                    "switch workspace 1-9",
                ),
                entry(binding_label(&keybinds.previous_agent), "previous agent"),
                entry(binding_label(&keybinds.next_agent), "next agent"),
                entry(indexed_label(&keybinds.focus_agent), "focus agent 1-9"),
                entry(binding_label(&keybinds.new_tab), "new tab"),
                entry(binding_label(&keybinds.rename_tab), "rename tab"),
                entry(binding_label(&keybinds.previous_tab), "previous tab"),
                entry(binding_label(&keybinds.next_tab), "next tab"),
                entry(binding_label(&keybinds.move_tab_previous), "move tab left"),
                entry(binding_label(&keybinds.move_tab_next), "move tab right"),
                entry(indexed_label(&keybinds.switch_tab), "switch tab 1-9"),
                entry(binding_label(&keybinds.close_tab), "close tab"),
            ],
        ),
        (
            "panes",
            vec![
                entry(binding_label(&keybinds.split_vertical), "split vertical"),
                entry(
                    binding_label(&keybinds.split_horizontal),
                    "split horizontal",
                ),
                entry(binding_label(&keybinds.close_pane), "close pane"),
                entry(binding_label(&keybinds.rename_pane), "rename pane"),
                entry(binding_label(&keybinds.edit_scrollback), "edit scrollback"),
                entry(binding_label(&keybinds.clear_pane), "clear pane"),
                entry(binding_label(&keybinds.copy_mode), "copy mode"),
                entry(binding_label(&keybinds.zoom), "zoom pane"),
                entry(binding_label(&keybinds.resize_mode), "resize mode"),
                entry(
                    binding_label(&keybinds.resize_pane_left),
                    "resize pane left",
                ),
                entry(
                    binding_label(&keybinds.resize_pane_down),
                    "resize pane down",
                ),
                entry(binding_label(&keybinds.resize_pane_up), "resize pane up"),
                entry(
                    binding_label(&keybinds.resize_pane_right),
                    "resize pane right",
                ),
                entry(binding_label(&keybinds.toggle_sidebar), "toggle sidebar"),
                entry(binding_label(&keybinds.focus_pane_left), "focus pane left"),
                entry(binding_label(&keybinds.focus_pane_down), "focus pane down"),
                entry(binding_label(&keybinds.focus_pane_up), "focus pane up"),
                entry(
                    binding_label(&keybinds.focus_pane_right),
                    "focus pane right",
                ),
                entry(binding_label(&keybinds.cycle_pane_next), "cycle pane next"),
                entry(
                    binding_label(&keybinds.cycle_pane_previous),
                    "cycle pane previous",
                ),
                entry(binding_label(&keybinds.last_pane), "last pane"),
            ],
        ),
    ];

    if !keybinds.custom_commands.is_empty() {
        groups.push((
            "custom",
            keybinds
                .custom_commands
                .iter()
                .map(|binding| {
                    (
                        binding.label.clone(),
                        binding
                            .description
                            .clone()
                            .map(Cow::Owned)
                            .unwrap_or(Cow::Borrowed("custom command")),
                    )
                })
                .collect(),
        ));
    }
    groups.push((
        "backlot",
        vec![entry(binding_label(&keybinds.update), "update")],
    ));
    groups
}

/// Backlot's own sections (S0): the dock and call-sheet toggles, the
/// four studio keys with the herdr action each one shadows on this
/// config, and the two-line glyph legend. Studio mode only — plain
/// mode never builds them, so its help stays cell-identical to
/// herdr's.
pub(crate) fn backlot_help_groups(
    keybinds: &Keybinds,
    prefixes: &[crate::config::KeyCombo],
    panel: &crate::panel::PanelConfig,
    sheet: crate::hollywood::CallSheetKeys,
    panel_tail: Option<crate::config::KeyCombo>,
) -> Vec<KeybindHelpGroup> {
    let chord = |tail: Option<crate::config::KeyCombo>| {
        tail.map(|combo| {
            format!(
                "{} {}",
                crate::config::format_prefix_combos(prefixes),
                crate::config::format_key_combo(combo)
            )
        })
        .unwrap_or_else(|| "unset".to_owned())
    };
    let shadow = |combo: Option<crate::config::KeyCombo>| -> String {
        combo
            .and_then(|(code, modifiers)| {
                crate::input::resolve_direct_binding(
                    keybinds,
                    &crate::config::TerminalKey::new(code, modifiers),
                )
            })
            .map(|hit| match hit {
                crate::input::KeybindMatch::Action(action) => {
                    format!("  (shadows herdr: {})", action_name(action))
                }
                crate::input::KeybindMatch::Command(_) => "  (shadows a custom command)".to_owned(),
            })
            .unwrap_or_default()
    };
    let studio_key = |combo: Option<crate::config::KeyCombo>| {
        combo
            .map(crate::config::format_key_combo)
            .unwrap_or_else(|| "unset".to_owned())
    };
    let groups = vec![
        (
            "backlot",
            vec![
                entry(chord(panel_tail), "studio dock"),
                (
                    sheet.label(&crate::config::format_prefix_combos(prefixes)),
                    Cow::Borrowed("call sheet") as Cow<'static, str>,
                ),
            ],
        ),
        (
            "studio",
            vec![
                (
                    studio_key(panel.keys.previous_studio),
                    Cow::Owned(format!(
                        "previous studio{}",
                        shadow(panel.keys.previous_studio)
                    )),
                ),
                (
                    studio_key(panel.keys.next_studio),
                    Cow::Owned(format!("next studio{}", shadow(panel.keys.next_studio))),
                ),
                (
                    studio_key(panel.keys.previous_role),
                    Cow::Owned(format!("previous role{}", shadow(panel.keys.previous_role))),
                ),
                (
                    studio_key(panel.keys.next_role),
                    Cow::Owned(format!("next role{}", shadow(panel.keys.next_role))),
                ),
            ],
        ),
        (
            "glyph legend",
            vec![
                entry(
                    "people",
                    "◐ work  ○ idle  ◌ slot  × USER/dir  ! fail  ✓ closed",
                ),
                entry("tasks", "· wait  ◐ run   ◈ rev   ✓ pass  = pause  ↑/↓ prio"),
            ],
        ),
    ];
    groups
}

/// The config key a resolved herdr action carries — the shadow note's
/// name for it.
fn action_name(action: crate::input::KeybindAction) -> &'static str {
    use crate::input::KeybindAction as A;
    match action {
        A::NewWorkspace => "new_workspace",
        A::NewWorktree => "new_worktree",
        A::OpenWorktree => "open_worktree",
        A::RemoveWorktree => "remove_worktree",
        A::RenameWorkspace => "rename_workspace",
        A::CloseWorkspace => "close_workspace",
        A::SwitchWorkspace(_) => "switch_workspace",
        A::SwitchTab(_) => "switch_tab",
        A::FocusAgent(_) => "focus_agent",
        A::WorkspacePicker => "workspace_picker",
        A::PreviousWorkspace => "previous_workspace",
        A::NextWorkspace => "next_workspace",
        A::PreviousAgent => "previous_agent",
        A::NextAgent => "next_agent",
        A::NewTab => "new_tab",
        A::RenameTab => "rename_tab",
        A::PreviousTab => "previous_tab",
        A::NextTab => "next_tab",
        A::MoveTabPrevious => "move_tab_previous",
        A::MoveTabNext => "move_tab_next",
        A::CloseTab => "close_tab",
        A::RenamePane => "rename_pane",
        A::FocusPaneLeft => "focus_pane_left",
        A::FocusPaneDown => "focus_pane_down",
        A::FocusPaneUp => "focus_pane_up",
        A::FocusPaneRight => "focus_pane_right",
        A::SwapPaneLeft => "swap_pane_left",
        A::SwapPaneDown => "swap_pane_down",
        A::SwapPaneUp => "swap_pane_up",
        A::SwapPaneRight => "swap_pane_right",
        A::SplitVertical => "split_vertical",
        A::SplitHorizontal => "split_horizontal",
        A::ClosePane => "close_pane",
        A::EditScrollback => "edit_scrollback",
        A::ClearPane => "clear_pane",
        A::CopyMode => "copy_mode",
        A::Zoom => "zoom",
        A::EnterResizeMode => "resize_mode",
        A::ResizePaneLeft => "resize_pane_left",
        A::ResizePaneDown => "resize_pane_down",
        A::ResizePaneUp => "resize_pane_up",
        A::ResizePaneRight => "resize_pane_right",
        A::ToggleSidebar => "toggle_sidebar",
        A::CyclePaneNext => "cycle_pane_next",
        A::CyclePanePrevious => "cycle_pane_previous",
        A::LastPane => "last_pane",
        A::Help => "help",
        A::Settings => "settings",
        A::ReloadConfig => "reload_config",
        A::OpenNotificationTarget => "open_notification_target",
        A::Detach => "detach",
        A::OpenNavigator => "goto",
        A::Update => "update",
    }
}

pub(crate) fn filter_keybind_help_groups(
    groups: Vec<KeybindHelpGroup>,
    query: &str,
) -> Vec<KeybindHelpGroup> {
    if query.is_empty() {
        return groups;
    }
    let query = query.to_lowercase();
    groups
        .into_iter()
        .filter_map(|(group, entries)| {
            let entries = entries
                .into_iter()
                .filter(|(key, label)| {
                    key.to_lowercase().contains(&query) || label.to_lowercase().contains(&query)
                })
                .collect::<Vec<_>>();
            (!entries.is_empty()).then_some((group, entries))
        })
        .collect()
}
