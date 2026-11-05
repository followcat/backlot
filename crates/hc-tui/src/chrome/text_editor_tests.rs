// Derived from herdr src/client/shell/text_editor.rs tests (Apache-2.0).
use super::*;
use hc_vt::{Action, Key, KeyInput, Mods};

fn press(editor: &mut TextEditor, key: Key, mods: Mods) {
    let event = crate::app::input::TerminalKey::new(KeyInput {
        key,
        mods,
        action: Action::Press,
    });
    assert!(editor.handle_key(&event).is_some());
    assert!(
        editor.cursor == editor.len()
            || editor
                .grapheme_indices(true)
                .any(|(i, _)| i == editor.cursor)
    );
}

#[test]
fn source_cursor_editing_word_kills_and_yank() {
    use Key::*;
    for (code, mods, text, cursor, killed) in [
        (ArrowLeft, Mods::NONE, "one two", 3, ""),
        (Char('b'), Mods::CTRL, "one two", 3, ""),
        (ArrowRight, Mods::NONE, "one two", 5, ""),
        (Home, Mods::NONE, "one two", 0, ""),
        (End, Mods::NONE, "one two", 7, ""),
        (Backspace, Mods::NONE, "onetwo", 3, ""),
        (Delete, Mods::NONE, "one wo", 4, ""),
        (Char('b'), Mods::ALT, "one two", 0, ""),
        (Char('f'), Mods::ALT, "one two", 7, ""),
        (Char('u'), Mods::CTRL, "two", 0, "one "),
        (Char('k'), Mods::CTRL, "one ", 4, "two"),
        (Char('w'), Mods::CTRL, "two", 0, "one "),
    ] {
        let mut editor = TextEditor::from("one two");
        editor.cursor = 4;
        press(&mut editor, code, mods);
        assert_eq!(
            (editor.as_str(), editor.cursor, editor.killed.as_str()),
            (text, cursor, killed)
        );
    }
    let mut editor = TextEditor::from("one two");
    press(&mut editor, Char('u'), Mods::CTRL);
    press(&mut editor, Char('y'), Mods::CTRL);
    assert_eq!(editor.as_str(), "one two");
}

#[test]
fn grapheme_edit_and_viewport_never_split_combining_or_wide_text() {
    let mut editor = TextEditor::default();
    editor.insert("e");
    editor.insert("\u{301}");
    press(&mut editor, Key::Backspace, Mods::NONE);
    assert!(editor.is_empty());
    editor.insert("one 👩‍💻 中");
    let (text, cursor) = editor.viewport(5);
    assert_eq!(text, " 中");
    assert_eq!(cursor, 3);
    press(&mut editor, Key::Backspace, Mods::NONE);
    press(&mut editor, Key::Backspace, Mods::NONE);
    press(&mut editor, Key::Backspace, Mods::NONE);
    assert_eq!(editor.as_str(), "one ");
}

#[test]
fn host_generated_text_is_authoritative_and_controls_are_normalized() {
    let mut editor = TextEditor::default();
    let mut event = crate::app::input::TerminalKey::new(KeyInput {
        key: Key::Char('/'),
        mods: Mods::SHIFT,
        action: Action::Press,
    });
    event.text_commit = Some("?\r\n中\tX\0".into());
    assert_eq!(editor.handle_key(&event), Some(true));
    assert_eq!(editor.as_str(), "? 中 X");
}
