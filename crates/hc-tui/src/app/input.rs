// Derived in part from herdr src/input/model.rs and
// src/client/shell/input.rs (Apache-2.0).
//! Input handling: a neutral `UiEvent` stream (outer keys, paste, resize,
//! focus), the configurable reference prefix model, and the crossterm
//! translation layer. Deciding code (`Prefix`, `selector_key`) is pure and
//! unit-tested; crossterm only adapts bytes at the edge.

use hc_vt::{Action, Key, KeyInput, Mods};

pub(crate) const MAX_INPUT_BYTES: usize = 256 * 1024;

/// One outer-terminal event, already neutral. Mirrors the reference's
/// `RawInputEvent`: keys carry the full `TerminalKey` (with text
/// commits), IME-style text commits are their own event, mouse events
/// carry the full button/kind surface.
#[derive(Debug, Clone)]
pub enum UiEvent {
    /// A keyboard event for the inner terminal or the UI.
    Key(TerminalKey),
    /// Committed text (IME/commit-string input): sent to the pane as
    /// raw bytes, never parsed as UI keys.
    Text(String),
    /// Bracketed-paste content: forwarded to the inner terminal, never
    /// parsed as UI keys.
    Paste(Vec<u8>),
    /// The outer terminal was resized.
    Resize { cols: u16, rows: u16 },
    /// The outer terminal gained/lost focus.
    Focus(bool),
    /// A mouse event (button, wheel, drag, move) at a cell.
    Mouse(MouseInput),
    /// A sequence the decoder could not classify — the reference's
    /// `RawInputEvent::Unsupported`; dropped on arrival.
    Unsupported,
    /// A bounded input record was rejected; its discarded tail never
    /// becomes terminal input or a UI shortcut.
    InputRejected(&'static str),
}

/// The reference `TerminalKey`: a resolved key plus its text commit
/// and the raw bytes that produced it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalKey {
    /// The neutral key triple (`KeyCode`/`KeyModifiers`/`KeyEventKind`).
    pub input: KeyInput,
    /// Text this key also commits (kitty associated/generated text) —
    /// encoder input, sent to the pane when no binding claims the key.
    pub text_commit: Option<String>,
    /// The physical layout character a non-Latin Ctrl chord was
    /// rewritten from — the reference's `layout_key`.
    pub layout_key: Option<char>,
    /// The kitty shifted alternate codepoint — the reference's
    /// `shifted_codepoint`, for press/release matching.
    pub shifted_codepoint: Option<char>,
    /// The sequence that produced the event, for logs/echo.
    pub vt_bytes: Vec<u8>,
    pub(crate) physical_identity_hint: bool,
    pub(crate) repeat_count: u16,
    pub(crate) dispatch_id: u64,
}

impl TerminalKey {
    pub fn new(input: KeyInput) -> Self {
        TerminalKey {
            input,
            text_commit: None,
            layout_key: None,
            shifted_codepoint: None,
            vt_bytes: Vec::new(),
            physical_identity_hint: false,
            repeat_count: 1,
            dispatch_id: 0,
        }
    }

    /// The reference `model.rs` `with_text_commit`: attaches the typed
    /// text to plain character presses — `Char` with no modifiers, or an
    /// uppercase `Char` carrying Shift only. Control chords keep `None`.
    pub fn with_text_commit(mut self) -> Self {
        let has_text_only_modifiers = match self.input.key {
            Key::Char(ch) if ch.is_uppercase() => {
                self.input.mods == Mods::SHIFT || self.input.mods == Mods::NONE
            }
            Key::Char(_) => self.input.mods == Mods::NONE,
            _ => false,
        };
        if has_text_only_modifiers
            && self.input.action == Action::Press
            && let Key::Char(ch) = self.input.key
        {
            self.text_commit = Some(ch.to_string());
        }
        self
    }

    pub(crate) fn has_physical_identity(&self) -> bool {
        self.physical_identity_hint
    }
    // Native Windows key records are outside v1's platform scope.
    pub(crate) fn physical_key_id(&self) -> Option<u32> {
        None
    }
    pub(crate) fn with_action(mut self, action: Action) -> Self {
        self.input.action = action;
        if action == Action::Release {
            self.text_commit = None;
            self.repeat_count = 1;
        }
        self
    }

    pub fn with_vt_bytes(mut self, bytes: Vec<u8>) -> Self {
        self.vt_bytes = bytes;
        self
    }

    /// The reference's VT identity uses the original layout character,
    /// even when Ctrl resolved the shortcut to its base-layout key.
    /// Lease/swallow matching goes through it.
    pub(crate) fn identity(&self) -> Key {
        self.layout_key.map_or(self.input.key, Key::Char)
    }
}

/// A neutral mouse event — the full `MouseEvent` surface the framer
/// can report; `App::on_mouse` answers the subset the chrome uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MouseInput {
    /// What happened.
    pub kind: MouseEventKind,
    /// Zero-based cell column.
    pub col: u16,
    /// Zero-based cell row.
    pub row: u16,
    /// Held modifiers.
    pub mods: Mods,
    /// Original one-based SGR coordinates, interpreted after host mode negotiation.
    pub sgr_position: Option<(u32, u32)>,
}

/// The reference `MouseEventKind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseEventKind {
    Down(MouseButton),
    Up(MouseButton),
    Drag(MouseButton),
    /// Motion (also used for extended-button drags crossterm-style
    /// button flags can't name).
    Moved,
    ScrollUp,
    ScrollDown,
    ScrollLeft,
    ScrollRight,
}

/// The reference `MouseButton`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Middle,
    Right,
}

/// What one key resolves to.
#[derive(Debug, Clone)]
pub enum Decision {
    /// A resolved keybinding action (see `KeybindAction`).
    Action(crate::input::KeybindAction),
    /// A configured custom command — M3a does not spawn
    /// shell/popup/plugin commands, so it resolves but stays inert.
    Command(crate::config::CustomCommandKeybind),
    /// Forward to the inner terminal — the full semantic key (generated
    /// text, shifted alternate, layout key, raw bytes) travels with it.
    ToTerm(TerminalKey),
    /// Arm/transition without a decision yet.
    Held,
}

/// The herdr prefix machine (ported from client/shell/input.rs's
/// `route_key_press` Prefix branch): any configured prefix key arms; the
/// next *press* resolves an action through the keybind table; an unknown
/// press disarms silently; a second prefix-key press forwards a literal
/// prefix to the terminal.
///
/// Armed state survives non-press events: under kitty keyboard reporting a
/// held prefix emits repeats and every key emits a release — none of those
/// may disarm the prefix before its resolving press. Any press the UI
/// consumed (the prefix itself or an action key) is remembered so its
/// later release/repeat is swallowed instead of leaking an orphan event
/// into an inner kitty-reporting program.
#[derive(Debug, Default)]
pub struct Prefix {
    armed: bool,
    /// Presses the UI ate: the physical/named keys awaiting release.
    /// Keyed by the source VT identity, including the original layout
    /// character. Release-time modifiers may differ (Ctrl released
    /// before B). Repeats stay swallowed; Release pops the entry.
    swallowed: std::collections::VecDeque<Key>,
}

/// Bound on remembered swallowed presses — always popped on release.
const SWALLOWED_MAX: usize = 8;

impl Prefix {
    /// Whether the next press resolves a prefix action.
    pub fn armed(&self) -> bool {
        self.armed
    }

    /// Forgets a pending prefix (mode change, pane switch). Swallowed
    /// presses stay: their releases may still arrive.
    pub fn reset(&mut self) {
        self.armed = false;
    }

    /// Remembers a press the UI consumed (navigate moves, help toggles) so
    /// its release/repeat never reaches the inner terminal.
    pub fn swallow(&mut self, key: &TerminalKey) {
        if key.input.action != Action::Press {
            return;
        }
        if self.swallowed.len() >= SWALLOWED_MAX {
            self.swallowed.pop_front();
        }
        self.swallowed.push_back(key.identity());
    }

    /// A non-press event whose press the UI consumed: swallowed. The
    /// matching release pops the entry; repeats keep it. Matching is on
    /// the key alone — release-time modifiers are unreliable.
    pub(crate) fn release_swallowed(&mut self, key: &TerminalKey) -> bool {
        let pos = self.swallowed.iter().position(|&k| k == key.identity());
        match (pos, key.input.action) {
            (Some(pos), Action::Release) => {
                self.swallowed.remove(pos);
                true
            }
            (Some(_), _) => true, // a repeat of a consumed press
            (None, _) => false,
        }
    }

    /// Does `key` press any configured prefix key?
    fn is_prefix_press(key: &TerminalKey, live: &crate::config::LiveKeybindConfig) -> bool {
        key.input.action == Action::Press
            && live.matches_prefix(&crate::input::terminal_key_of(key))
    }

    /// Feed one key event against the resolved keybind config. Releases and
    /// repeats of keys the UI never consumed pass through untouched; only
    /// presses arm or resolve.
    pub fn feed(&mut self, key: &TerminalKey, live: &crate::config::LiveKeybindConfig) -> Decision {
        // Releases/repeats: a consumed press's tail is swallowed; any
        // other non-press event forwards (its press reached the
        // terminal). Neither touches the armed state.
        if key.input.action != Action::Press {
            if self.release_swallowed(key) {
                return Decision::Held;
            }
            return Decision::ToTerm(key.clone());
        }

        if self.armed {
            self.armed = false;
            if key.input.key == Key::Escape
                || (key.input.key == Key::Char('[') && key.input.mods == Mods::CTRL)
            {
                self.swallow(key);
                return Decision::Held;
            }
            if Self::is_prefix_press(key, live) {
                // Double prefix: a literal prefix belongs to the terminal —
                // its release follows normally, nothing swallowed.
                self.unswallow(key);
                return Decision::ToTerm(key.clone());
            }
            // The action key's press is consumed: swallow its tail.
            self.swallow(key);
            let matched = crate::input::terminal_key_of(key);
            return match crate::input::resolve_prefix_binding(&live.keybinds, &matched) {
                Some(crate::input::KeybindMatch::Action(action)) => Decision::Action(action),
                Some(crate::input::KeybindMatch::Command(command)) => Decision::Command(command),
                // Esc and anything unbound: disarm, swallow (documented).
                None => Decision::Held,
            };
        }
        if Self::is_prefix_press(key, live) {
            self.armed = true;
            self.swallow(key);
            return Decision::Held;
        }
        // A forwarded press makes any stale swallowed entry for the same
        // key stale — its pending release now belongs to the terminal.
        self.unswallow(key);
        Decision::ToTerm(key.clone())
    }

    /// Drops swallowed entries for `key`'s key — called when a fresh
    /// press forwards, so its release cannot be eaten by an older
    /// consumed press's leftover entry.
    fn unswallow(&mut self, key: &TerminalKey) {
        self.swallowed.retain(|&k| k != key.identity());
    }
}

/// One Navigate-mode outcome (the workspace picker the reference enters on
/// `WorkspacePicker`): move the pending highlight, accept it, select an
/// index directly, or cancel back to the focused terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Navigate {
    /// Move the pending workspace highlight by delta rows.
    Move(i32),
    /// Confirm the highlighted workspace.
    Accept,
    /// Direct-select workspace index (`1`-`9` → `0`-`8`).
    Select(usize),
    /// Leave Navigate back to the focused terminal.
    Cancel,
}

/// Navigate-mode key routing, ported from `route_navigate_key`: Esc or any
/// configured prefix key cancels; the configured `navigate_workspace_up`/
/// `down` bindings move the pending highlight; Enter accepts; `1`..`9`
/// select a workspace row directly. Everything else is ignored — the
/// previewed terminal is observed, never typed into.
pub fn navigate_key(
    key: &TerminalKey,
    live: &crate::config::LiveKeybindConfig,
) -> Option<Navigate> {
    // Presses and repeats both route: a held navigation key keeps
    // moving (the reference reprocesses consumed-lease repeats).
    if !matches!(key.input.action, Action::Press | Action::Repeat) {
        return None;
    }
    let key = crate::input::terminal_key_of(key);
    if key.code == crossterm::event::KeyCode::Esc
        || crate::input::is_ctrl_bracket_key(&key)
        || live.matches_prefix(&key)
    {
        return Some(Navigate::Cancel);
    }
    let nav = &live.keybinds.navigate;
    if nav.workspace_up.matches_direct_key(&key) {
        return Some(Navigate::Move(-1));
    }
    if nav.workspace_down.matches_direct_key(&key) {
        return Some(Navigate::Move(1));
    }
    let (code, mods) = crate::config::normalize_key_combo((key.code, key.modifiers));
    if code == crossterm::event::KeyCode::Enter && mods.is_empty() {
        return Some(Navigate::Accept);
    }
    if let crossterm::event::KeyCode::Char(digit @ '1'..='9') = code
        && mods.is_empty()
    {
        return Some(Navigate::Select(usize::from(digit as u8 - b'1')));
    }
    None
}

#[cfg(test)]
#[path = "input_tests/mod.rs"]
mod tests;

#[cfg(test)]
#[path = "input_layout_tests.rs"]
mod layout_tests;
