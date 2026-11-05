// Derived from herdr src/input/keybindings.rs and src/client/shell/input.rs
// key helpers (Apache-2.0).
//
//! Keybind resolution: the ported `resolve_*` chain and the `KeybindAction`
//! table, plus the `KeyInput`→`TerminalKey` edge adapter. Mode dispatch
//! lives in `app::input` mirroring `route_key_press`/`route_navigate_key`.

use crossterm::event::{KeyCode, KeyEventKind, KeyModifiers};
use hc_vt::{Action, Key, Mods};

use crate::config::{
    ActionKeybinds, CustomCommandKeybind, IndexedKeybind, Keybinds, TerminalKey,
    normalize_key_combo,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeybindDispatch {
    Direct,
    Prefix,
}

#[derive(Debug, Clone)]
pub enum KeybindMatch {
    Action(KeybindAction),
    Command(CustomCommandKeybind),
}

/// One herdr action a resolved binding names. The app executes supported
/// milestone actions; deferred bindings remain listed in docs/parity.md.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeybindAction {
    NewWorkspace,
    NewWorktree,
    OpenWorktree,
    RemoveWorktree,
    RenameWorkspace,
    CloseWorkspace,
    SwitchWorkspace(usize),
    SwitchTab(usize),
    FocusAgent(usize),
    WorkspacePicker,
    PreviousWorkspace,
    NextWorkspace,
    PreviousAgent,
    NextAgent,
    NewTab,
    RenameTab,
    PreviousTab,
    NextTab,
    MoveTabPrevious,
    MoveTabNext,
    CloseTab,
    RenamePane,
    FocusPaneLeft,
    FocusPaneDown,
    FocusPaneUp,
    FocusPaneRight,
    SwapPaneLeft,
    SwapPaneDown,
    SwapPaneUp,
    SwapPaneRight,
    SplitVertical,
    SplitHorizontal,
    ClosePane,
    EditScrollback,
    ClearPane,
    CopyMode,
    Zoom,
    EnterResizeMode,
    ResizePaneLeft,
    ResizePaneDown,
    ResizePaneUp,
    ResizePaneRight,
    ToggleSidebar,
    CyclePaneNext,
    CyclePanePrevious,
    LastPane,
    Help,
    Settings,
    ReloadConfig,
    OpenNotificationTarget,
    Detach,
    OpenNavigator,
}

pub fn resolve_direct_binding(keybinds: &Keybinds, key: &TerminalKey) -> Option<KeybindMatch> {
    resolve_exact_binding(keybinds, key, KeybindDispatch::Direct)
}

pub fn resolve_prefix_binding(keybinds: &Keybinds, key: &TerminalKey) -> Option<KeybindMatch> {
    resolve_exact_binding(keybinds, key, KeybindDispatch::Prefix).or_else(|| {
        generated_character_key(key).and_then(|generated_key| {
            resolve_exact_binding(keybinds, &generated_key, KeybindDispatch::Prefix)
        })
    })
}

pub fn resolve_non_indexed_action(
    keybinds: &Keybinds,
    key: &TerminalKey,
    dispatch: KeybindDispatch,
) -> Option<KeybindAction> {
    for (bindings, action) in [
        (&keybinds.help, KeybindAction::Help),
        (&keybinds.settings, KeybindAction::Settings),
        (&keybinds.workspace_picker, KeybindAction::WorkspacePicker),
        (&keybinds.new_workspace, KeybindAction::NewWorkspace),
        (&keybinds.new_worktree, KeybindAction::NewWorktree),
        (&keybinds.open_worktree, KeybindAction::OpenWorktree),
        (&keybinds.remove_worktree, KeybindAction::RemoveWorktree),
        (&keybinds.rename_workspace, KeybindAction::RenameWorkspace),
        (&keybinds.close_workspace, KeybindAction::CloseWorkspace),
        (
            &keybinds.previous_workspace,
            KeybindAction::PreviousWorkspace,
        ),
        (&keybinds.next_workspace, KeybindAction::NextWorkspace),
        (&keybinds.previous_agent, KeybindAction::PreviousAgent),
        (&keybinds.next_agent, KeybindAction::NextAgent),
        (&keybinds.new_tab, KeybindAction::NewTab),
        (&keybinds.rename_tab, KeybindAction::RenameTab),
        (&keybinds.previous_tab, KeybindAction::PreviousTab),
        (&keybinds.next_tab, KeybindAction::NextTab),
        (&keybinds.move_tab_previous, KeybindAction::MoveTabPrevious),
        (&keybinds.move_tab_next, KeybindAction::MoveTabNext),
        (&keybinds.close_tab, KeybindAction::CloseTab),
        (&keybinds.rename_pane, KeybindAction::RenamePane),
        (&keybinds.edit_scrollback, KeybindAction::EditScrollback),
        (&keybinds.clear_pane, KeybindAction::ClearPane),
        (&keybinds.copy_mode, KeybindAction::CopyMode),
        (&keybinds.focus_pane_left, KeybindAction::FocusPaneLeft),
        (&keybinds.focus_pane_down, KeybindAction::FocusPaneDown),
        (&keybinds.focus_pane_up, KeybindAction::FocusPaneUp),
        (&keybinds.focus_pane_right, KeybindAction::FocusPaneRight),
        (&keybinds.swap_pane_left, KeybindAction::SwapPaneLeft),
        (&keybinds.swap_pane_down, KeybindAction::SwapPaneDown),
        (&keybinds.swap_pane_up, KeybindAction::SwapPaneUp),
        (&keybinds.swap_pane_right, KeybindAction::SwapPaneRight),
        (&keybinds.last_pane, KeybindAction::LastPane),
        (&keybinds.cycle_pane_next, KeybindAction::CyclePaneNext),
        (
            &keybinds.cycle_pane_previous,
            KeybindAction::CyclePanePrevious,
        ),
        (&keybinds.split_vertical, KeybindAction::SplitVertical),
        (&keybinds.split_horizontal, KeybindAction::SplitHorizontal),
        (&keybinds.close_pane, KeybindAction::ClosePane),
        (&keybinds.zoom, KeybindAction::Zoom),
        (&keybinds.resize_mode, KeybindAction::EnterResizeMode),
        (&keybinds.resize_pane_left, KeybindAction::ResizePaneLeft),
        (&keybinds.resize_pane_down, KeybindAction::ResizePaneDown),
        (&keybinds.resize_pane_up, KeybindAction::ResizePaneUp),
        (&keybinds.resize_pane_right, KeybindAction::ResizePaneRight),
        (&keybinds.toggle_sidebar, KeybindAction::ToggleSidebar),
        (&keybinds.reload_config, KeybindAction::ReloadConfig),
        (
            &keybinds.open_notification_target,
            KeybindAction::OpenNotificationTarget,
        ),
        (&keybinds.detach, KeybindAction::Detach),
        (&keybinds.goto, KeybindAction::OpenNavigator),
    ] {
        if action_matches(bindings, key, dispatch) {
            return Some(action);
        }
    }
    None
}

pub fn resolve_custom_command(
    keybinds: &Keybinds,
    key: &TerminalKey,
    dispatch: KeybindDispatch,
) -> Option<CustomCommandKeybind> {
    keybinds
        .custom_commands
        .iter()
        .find(|binding| match dispatch {
            KeybindDispatch::Direct => binding.bindings.matches_direct_key(key),
            KeybindDispatch::Prefix => binding.bindings.matches_prefix_key(key),
        })
        .cloned()
}

pub fn resolve_indexed_action(
    keybinds: &Keybinds,
    key: &TerminalKey,
    dispatch: KeybindDispatch,
) -> Option<KeybindAction> {
    let actual_modifiers = normalize_key_combo((key.code, key.modifiers)).1;

    for exact_modifiers in [true, false] {
        let trigger_matches = |binding: &IndexedKeybind| {
            let dispatch_matches = match dispatch {
                KeybindDispatch::Direct => binding.trigger.is_direct(),
                KeybindDispatch::Prefix => binding.trigger.is_prefix(),
            };
            let expected_modifiers = normalize_key_combo(binding.trigger.combo()).1;
            dispatch_matches && (actual_modifiers == expected_modifiers) == exact_modifiers
        };

        for binding in &keybinds.switch_tab {
            if trigger_matches(binding)
                && let Some(index) = binding.matched_index(key)
            {
                return Some(KeybindAction::SwitchTab(index));
            }
        }
        for binding in &keybinds.switch_workspace {
            if trigger_matches(binding)
                && let Some(index) = binding.matched_index(key)
            {
                return Some(KeybindAction::SwitchWorkspace(index));
            }
        }
        for binding in &keybinds.focus_agent {
            if trigger_matches(binding)
                && let Some(index) = binding.matched_index(key)
            {
                return Some(KeybindAction::FocusAgent(index));
            }
        }
    }

    None
}

fn resolve_exact_binding(
    keybinds: &Keybinds,
    key: &TerminalKey,
    dispatch: KeybindDispatch,
) -> Option<KeybindMatch> {
    resolve_non_indexed_action(keybinds, key, dispatch)
        .map(KeybindMatch::Action)
        .or_else(|| resolve_custom_command(keybinds, key, dispatch).map(KeybindMatch::Command))
        .or_else(|| resolve_indexed_action(keybinds, key, dispatch).map(KeybindMatch::Action))
}

fn generated_character_key(key: &TerminalKey) -> Option<TerminalKey> {
    let mut characters = key.generated_text.as_deref()?.chars();
    let character = characters.next()?;
    if character.is_control() || characters.next().is_some() {
        return None;
    }
    Some(TerminalKey::new(
        KeyCode::Char(character),
        KeyModifiers::empty(),
    ))
}

fn action_matches(bindings: &ActionKeybinds, key: &TerminalKey, dispatch: KeybindDispatch) -> bool {
    match dispatch {
        KeybindDispatch::Direct => bindings.matches_direct_key(key),
        KeybindDispatch::Prefix => bindings.matches_prefix_key(key),
    }
}

/// The reference's `is_ctrl_bracket_key`: kitty may report Ctrl+[ on its own
/// while other hosts fold it into Esc — both cancel Navigate.
pub(crate) fn is_ctrl_bracket_key(key: &TerminalKey) -> bool {
    key.code == KeyCode::Char('[') && key.modifiers == KeyModifiers::CONTROL
}

/// The parsed `app::input::TerminalKey` → the matcher's `TerminalKey`,
/// carrying generated text, the shifted alternate and the layout key so
/// GUI binding matching sees exactly what the parser reported.
pub(crate) fn terminal_key_of(parsed: &crate::app::input::TerminalKey) -> TerminalKey {
    let input = &parsed.input;
    let code = match input.key {
        Key::Char(c) => KeyCode::Char(c),
        Key::Enter => KeyCode::Enter,
        Key::Tab => KeyCode::Tab,
        Key::BackTab => KeyCode::BackTab,
        Key::Backspace => KeyCode::Backspace,
        Key::Escape => KeyCode::Esc,
        Key::Space => KeyCode::Char(' '),
        Key::Delete => KeyCode::Delete,
        Key::Insert => KeyCode::Insert,
        Key::Home => KeyCode::Home,
        Key::End => KeyCode::End,
        Key::PageUp => KeyCode::PageUp,
        Key::PageDown => KeyCode::PageDown,
        Key::ArrowUp => KeyCode::Up,
        Key::ArrowDown => KeyCode::Down,
        Key::ArrowLeft => KeyCode::Left,
        Key::ArrowRight => KeyCode::Right,
        Key::F(n) => KeyCode::F(n),
        Key::Functional(code) => functional_keycode(code),
    };
    let mut modifiers = KeyModifiers::empty();
    if input.mods.0 & Mods::SHIFT.0 != 0 {
        modifiers |= KeyModifiers::SHIFT;
    }
    if input.mods.0 & Mods::CTRL.0 != 0 {
        modifiers |= KeyModifiers::CONTROL;
    }
    if input.mods.0 & Mods::ALT.0 != 0 {
        modifiers |= KeyModifiers::ALT;
    }
    if input.mods.0 & Mods::SUPER.0 != 0 {
        modifiers |= KeyModifiers::SUPER;
    }
    if input.mods.0 & Mods::HYPER.0 != 0 {
        modifiers |= KeyModifiers::HYPER;
    }
    if input.mods.0 & Mods::META.0 != 0 {
        modifiers |= KeyModifiers::META;
    }
    let kind = match input.action {
        Action::Press => KeyEventKind::Press,
        Action::Repeat => KeyEventKind::Repeat,
        Action::Release => KeyEventKind::Release,
    };
    TerminalKey {
        code,
        modifiers,
        kind,
        shifted_codepoint: parsed.shifted_codepoint.map(u32::from),
        generated_text: parsed.text_commit.clone(),
    }
}

// Derived from herdr src/input/parse.rs (Apache-2.0).
fn functional_keycode(code: u32) -> crossterm::event::KeyCode {
    use crossterm::event::{KeyCode as K, MediaKeyCode as M};
    match code {
        57358 => K::CapsLock,
        57359 => K::ScrollLock,
        57360 => K::NumLock,
        57361 => K::PrintScreen,
        57362 => K::Pause,
        57363 => K::Menu,
        57427 => K::KeypadBegin,
        57428 => K::Media(M::Play),
        57429 => K::Media(M::Pause),
        57430 => K::Media(M::PlayPause),
        57431 => K::Media(M::Reverse),
        57432 => K::Media(M::Stop),
        57433 => K::Media(M::FastForward),
        57434 => K::Media(M::Rewind),
        57435 => K::Media(M::TrackNext),
        57436 => K::Media(M::TrackPrevious),
        57437 => K::Media(M::Record),
        57438 => K::Media(M::LowerVolume),
        57439 => K::Media(M::RaiseVolume),
        57440 => K::Media(M::MuteVolume),
        _ => K::Null,
    }
}
