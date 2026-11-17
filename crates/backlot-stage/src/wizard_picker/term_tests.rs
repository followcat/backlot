//! The key mapping — crossterm's event in, the picker's [`Key`]
//! out. The event loop itself needs a terminal; this is the part a
//! test can drive.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use super::state::Key;
use super::term::map_key;

fn ev(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

#[test]
fn ordinary_keys_map() {
    assert_eq!(map_key(&ev(KeyCode::Char('a'))), Some(Key::Char('a')));
    assert_eq!(map_key(&ev(KeyCode::Backspace)), Some(Key::Backspace));
    assert_eq!(map_key(&ev(KeyCode::Left)), Some(Key::Left));
    assert_eq!(map_key(&ev(KeyCode::Right)), Some(Key::Right));
    assert_eq!(map_key(&ev(KeyCode::Up)), Some(Key::Up));
    assert_eq!(map_key(&ev(KeyCode::Down)), Some(Key::Down));
    assert_eq!(map_key(&ev(KeyCode::PageUp)), Some(Key::PgUp));
    assert_eq!(map_key(&ev(KeyCode::PageDown)), Some(Key::PgDn));
    assert_eq!(map_key(&ev(KeyCode::Home)), Some(Key::Home));
    assert_eq!(map_key(&ev(KeyCode::End)), Some(Key::End));
    assert_eq!(map_key(&ev(KeyCode::Tab)), Some(Key::Tab));
    assert_eq!(map_key(&ev(KeyCode::Enter)), Some(Key::Enter));
    assert_eq!(map_key(&ev(KeyCode::Esc)), Some(Key::Esc));
}

#[test]
fn ctrl_c_cancels() {
    let ev = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
    assert_eq!(map_key(&ev), Some(Key::CtrlC));
}

#[test]
fn uninteresting_events_map_to_none() {
    assert_eq!(map_key(&ev(KeyCode::BackTab)), None);
    assert_eq!(map_key(&ev(KeyCode::F(1))), None);
    let mut release = ev(KeyCode::Char('a'));
    release.kind = KeyEventKind::Release;
    assert_eq!(map_key(&release), None);
}
