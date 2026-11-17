//! The update card's state and input: which card opens for the current
//! state, scrolling, skip and close. What the card shows is drawn by
//! `chrome::update_modal`; the installer lands in `update_run`.

use std::time::Instant;

use crossterm::event::{KeyCode, KeyModifiers};

use super::App;
use super::input::{MouseButton, MouseEventKind, MouseInput, TerminalKey};
use super::mouse::contains;
use crate::chrome::release_notes::{Callout, NotesState, Stream};
use crate::chrome::update_modal::{Button, ModalRender, ModalView, Phase};
use crate::update_install::{
    default_install_dir, dir_writable, manual_command, running_install_dir,
};
use crate::update_state::UpdateCheckState;

/// The version this binary reports — the "installed" side of the card.
pub(crate) const RUNNING: &str = env!("CARGO_PKG_VERSION");
/// Rows one wheel tick moves (herdr's release-notes step).
const WHEEL_ROWS: i32 = 3;

/// The open card: its phase, scroll and the directory it would install into.
pub(crate) struct UpdateModal {
    pub phase: Phase,
    pub scroll: u16,
    pub writable: bool,
    /// The install directory as the callout names it.
    pub install_text: String,
    /// The manual command for a read-only directory (or the plain
    /// `hc-tui --upgrade` when writable).
    pub chip: String,
    /// The last frame's hit map and scroll bounds.
    pub render: ModalRender,
}

/// The index of this binary's own notes in the cache, if cached.
pub(crate) fn running_notes(state: &UpdateCheckState) -> Option<usize> {
    state
        .releases
        .iter()
        .position(|release| release.version == RUNNING)
}

/// The card's view over the live state; `fetching` marks a notes fetch
/// still in flight.
pub(crate) fn view<'a>(
    modal: &'a UpdateModal,
    state: &'a UpdateCheckState,
    latest: Option<&'a str>,
    fetching: bool,
) -> ModalView<'a> {
    let scroll = modal.scroll;
    if modal.phase == Phase::WhatsNew {
        let releases =
            running_notes(state).map_or(&state.releases[..0], |i| &state.releases[i..=i]);
        return ModalView {
            phase: Phase::WhatsNew,
            installed: RUNNING,
            latest: RUNNING,
            writable: modal.writable,
            body: Stream {
                releases,
                installed: RUNNING,
                latest: RUNNING,
                notes: NotesState::Loaded,
                callout: None,
            },
            scroll,
        };
    }
    let latest = latest.unwrap_or(RUNNING);
    let tag = format!("v{latest}");
    let notes =
        if !state.releases.is_empty() && state.releases_for_tag.as_deref() == Some(tag.as_str()) {
            NotesState::Loaded
        } else if fetching {
            NotesState::Loading
        } else {
            NotesState::Offline
        };
    ModalView {
        phase: modal.phase,
        installed: RUNNING,
        latest,
        writable: modal.writable,
        body: Stream {
            releases: &state.releases,
            installed: RUNNING,
            latest,
            notes,
            callout: Some(Callout {
                writable: modal.writable,
                install_dir: &modal.install_text,
                chip: &modal.chip,
            }),
        },
        scroll,
    }
}

impl App {
    /// The card for the current state: the notes while an update is
    /// offered (a skipped one stays reachable), the read-only notes of
    /// this version once upgraded, otherwise a quiet up-to-date notice.
    pub(crate) fn open_update(&mut self) {
        let phase = if self.update_latest.is_some() {
            Phase::Opened
        } else if running_notes(&self.update_state).is_some() {
            Phase::WhatsNew
        } else {
            self.show_notice("Update", &format!("hc-tui v{RUNNING} is up to date"));
            return;
        };
        let exe = self.running_exe.clone();
        let dir = running_install_dir(exe.as_deref());
        let writable = dir.as_deref().is_some_and(dir_writable);
        let home = std::env::var_os("HOME");
        let chip = match dir.as_deref() {
            Some(dir) if !writable => {
                manual_command(dir, default_install_dir(home.as_deref()).as_deref())
            }
            _ => "hc-tui --upgrade".to_owned(),
        };
        let install_text = dir
            .as_deref()
            .map(|dir| dir.display().to_string())
            .unwrap_or_default();
        self.update_modal = Some(UpdateModal {
            phase,
            scroll: 0,
            writable,
            install_text,
            chip,
            render: ModalRender::default(),
        });
        if phase == Phase::Opened {
            self.kick_notes_fetch(true);
        }
        self.sched.mark_dirty(Instant::now());
    }

    /// Opens the what's-new card once after an upgrade, when this
    /// version's notes are cached and not yet shown.
    pub(super) fn maybe_open_whats_new(&mut self) {
        let shown = self.update_state.show_on_startup_version.as_deref() == Some(RUNNING);
        if !shown && running_notes(&self.update_state).is_some() && self.update_modal.is_none() {
            self.open_update();
        }
    }

    pub(super) fn handle_update_key(&mut self, key: &TerminalKey) {
        let combo = crate::input::terminal_key_of(key);
        let (code, mods) = crate::config::normalize_key_combo((combo.code, combo.modifiers));
        let Some(modal) = self.update_modal.as_ref() else {
            return;
        };
        let (phase, writable) = (modal.phase, modal.writable);
        let rows = i32::from(modal.render.rows.max(1));
        let plain = mods.is_empty();
        let ctrl = mods == KeyModifiers::CONTROL;
        let scrollable = matches!(phase, Phase::Opened | Phase::WhatsNew);
        match code {
            KeyCode::Esc => self.close_update(),
            KeyCode::Char('q') if plain => self.close_update(),
            KeyCode::Char('s') if plain && phase == Phase::Opened => self.skip_update(),
            KeyCode::Enter | KeyCode::Char('u') if plain && phase == Phase::Opened && writable => {}
            KeyCode::Down | KeyCode::Char('j') if plain && scrollable => self.scroll_update(1),
            KeyCode::Up | KeyCode::Char('k') if plain && scrollable => self.scroll_update(-1),
            KeyCode::PageDown | KeyCode::Char(' ') if plain && scrollable => {
                self.scroll_update(rows);
            }
            KeyCode::Char('d') if ctrl && scrollable => self.scroll_update(rows),
            KeyCode::PageUp | KeyCode::Char('u') if ctrl && scrollable => self.scroll_update(-rows),
            KeyCode::Home | KeyCode::Char('g') if plain && scrollable => self.scroll_update_to(0),
            KeyCode::End | KeyCode::Char('G') if scrollable => self.scroll_update_to(i32::MAX),
            _ => {}
        }
        self.sched.mark_dirty(Instant::now());
    }

    /// The card owns the mouse: its buttons act, the wheel scrolls, and
    /// everything else (outside clicks included) is swallowed.
    pub(super) fn handle_update_mouse(&mut self, mouse: MouseInput) {
        let point = (mouse.col, mouse.row);
        match mouse.kind {
            MouseEventKind::ScrollUp => self.scroll_update(-WHEEL_ROWS),
            MouseEventKind::ScrollDown => self.scroll_update(WHEEL_ROWS),
            MouseEventKind::Down(MouseButton::Left) => {
                let hit = self.update_modal.as_ref().and_then(|modal| {
                    modal
                        .render
                        .buttons
                        .iter()
                        .find(|(rect, _)| contains(*rect, point))
                        .map(|(_, button)| *button)
                });
                if let Some(button) = hit {
                    self.press_update_button(button);
                }
            }
            _ => {}
        }
        self.sched.mark_dirty(Instant::now());
    }

    fn press_update_button(&mut self, button: Button) {
        match button {
            Button::Close => self.close_update(),
            Button::Skip => self.skip_update(),
            Button::Update => {}
        }
    }

    fn scroll_update(&mut self, delta: i32) {
        if let Some(modal) = self.update_modal.as_mut() {
            let max = i32::from(modal.render.max_scroll);
            modal.scroll = (i32::from(modal.scroll) + delta).clamp(0, max) as u16;
        }
    }

    fn scroll_update_to(&mut self, row: i32) {
        if let Some(modal) = self.update_modal.as_mut() {
            modal.scroll = row.clamp(0, i32::from(modal.render.max_scroll)) as u16;
        }
    }

    /// Esc / later / close. Closing the read-only card marks this
    /// version's what's-new as shown.
    fn close_update(&mut self) {
        let Some(modal) = self.update_modal.take() else {
            return;
        };
        if modal.phase == Phase::WhatsNew {
            self.update_state.show_on_startup_version = Some(RUNNING.to_owned());
            self.persist_update_state();
        }
        self.sched.mark_dirty(Instant::now());
    }

    /// `s skip`: the version stops advertising everywhere (badge, dot,
    /// toast, modebar) until a newer release appears.
    fn skip_update(&mut self) {
        let Some(latest) = self.update_latest.clone() else {
            return;
        };
        self.update_modal = None;
        self.update_state.skipped_version = Some(latest.clone());
        self.update_state.seen_toast_version = Some(latest);
        if self.notice.as_ref().is_some_and(|notice| notice.update) {
            self.notice = None;
        }
        self.persist_update_state();
        self.sched.mark_dirty(Instant::now());
    }
}
