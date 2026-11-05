//! Read-only application tests and shared fixtures.
mod cases_1;

use super::*;
use crate::input::KeybindAction;

fn live() -> crate::config::LiveKeybindConfig {
    crate::config::Config::default().live_keybinds()
}

fn press(key: Key, mods: Mods) -> TerminalKey {
    TerminalKey::new(KeyInput {
        key,
        mods,
        action: Action::Press,
    })
}

fn assert_held(decision: Decision) {
    assert!(matches!(decision, Decision::Held), "{decision:?}");
}

fn assert_term(decision: Decision, input: &TerminalKey) {
    match decision {
        Decision::ToTerm(k) => {
            assert_eq!(k.input, input.input);
        }
        other => panic!("expected ToTerm, got {other:?}"),
    }
}

fn assert_action(decision: Decision, action: KeybindAction) {
    match decision {
        Decision::Action(found) => assert_eq!(found, action),
        other => panic!("expected Action({action:?}), got {other:?}"),
    }
}

fn at(key: Key, mods: Mods, action: Action) -> TerminalKey {
    TerminalKey::new(KeyInput { key, mods, action })
}
