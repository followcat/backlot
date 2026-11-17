//! The picker's terminal side: crossterm raw mode and the alternate
//! screen, the event loop that turns key events into [`Key`]s, and
//! the guard that puts the cooked terminal back on every way out —
//! a cancelled pick, a draw error, a panic alike. The picker only
//! ever runs when the wizard's stdin and stdout are a terminal.

use std::path::{Path, PathBuf};

use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::{cursor, event, execute, terminal};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use super::state::{Key, Outcome, Picker};
use super::{fs, render};

/// Raw mode and the alternate screen, undone in `Drop` — the cooked
/// terminal, the main screen and the cursor come back on a cancel,
/// on an error and on a panic alike.
struct Restore;

impl Drop for Restore {
    fn drop(&mut self) {
        let _ = terminal::disable_raw_mode();
        let _ = execute!(
            std::io::stdout(),
            terminal::LeaveAlternateScreen,
            cursor::Show
        );
    }
}

/// Run the picker on the wizard's terminal: `Some` is the picked
/// repository's absolute path, `None` a cancel — the wizard refuses
/// "cancelled" then. `in_use` is the "repositories in use" group.
pub fn pick(start: &Path, in_use: Vec<PathBuf>) -> std::io::Result<Option<PathBuf>> {
    terminal::enable_raw_mode()?;
    execute!(std::io::stdout(), terminal::EnterAlternateScreen)?;
    let _restore = Restore;
    let mut term = Terminal::new(CrosstermBackend::new(std::io::stdout()))?;
    let mut picker = Picker::new(fs::real(), fs::home(), start, in_use);
    loop {
        term.draw(|f| {
            let area = f.area();
            picker.set_page(render::view_height(area));
            if let Some(pos) = render::draw(&mut picker, area, f.buffer_mut()) {
                f.set_cursor_position(pos);
            }
        })?;
        match event::read()? {
            Event::Key(ev) => {
                if let Some(key) = map_key(&ev)
                    && let Some(outcome) = picker.key(key)
                {
                    return Ok(match outcome {
                        Outcome::Picked(path) => Some(path),
                        Outcome::Cancelled => None,
                    });
                }
            }
            Event::Resize(..) => {}
            _ => {}
        }
    }
}

/// A crossterm key event as a picker [`Key`] — `None` for what the
/// picker ignores: releases, BackTab, F-keys and the rest.
pub(crate) fn map_key(ev: &KeyEvent) -> Option<Key> {
    if ev.kind == KeyEventKind::Release {
        return None;
    }
    let ctrl = ev.modifiers.contains(KeyModifiers::CONTROL);
    Some(match ev.code {
        KeyCode::Char('c') if ctrl => Key::CtrlC,
        KeyCode::Char(c) => Key::Char(c),
        KeyCode::Backspace => Key::Backspace,
        KeyCode::Left => Key::Left,
        KeyCode::Right => Key::Right,
        KeyCode::Up => Key::Up,
        KeyCode::Down => Key::Down,
        KeyCode::PageUp => Key::PgUp,
        KeyCode::PageDown => Key::PgDn,
        KeyCode::Home => Key::Home,
        KeyCode::End => Key::End,
        KeyCode::Tab => Key::Tab,
        KeyCode::Enter => Key::Enter,
        KeyCode::Esc => Key::Esc,
        _ => return None,
    })
}
