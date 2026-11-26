//! Inline `t` compose workflow inside the Lot Reader (`hollywood.view`
//! and `sheet.view`), sharing `panel.drafts` + `panel.histories` (studio)
//! or `hollywood.input` + `hollywood.history` (Hollywood).

use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyModifiers};

use super::App;
use crate::app::input::TerminalKey;
use crate::hollywood::{InlineTellState, ReaderMeta, ReaderOrigin, reader_tell_target};
use crate::studio::StudioKey;
use crate::studio_cmds;

const SENT_HOLD: Duration = Duration::from_millis(1500);

#[derive(Clone)]
pub(super) enum ComposeTarget {
    Studio(StudioKey, String, Option<String>),
    Hollywood,
}

impl ComposeTarget {
    fn from_meta(meta: &ReaderMeta) -> Option<Self> {
        if !meta.can_tell() {
            return None;
        }
        match &meta.origin {
            ReaderOrigin::HollywoodExchange(_) => Some(Self::Hollywood),
            ReaderOrigin::StudioExchange(key, _)
            | ReaderOrigin::SheetDeliveries(key, _)
            | ReaderOrigin::Records(key)
            | ReaderOrigin::Design(key) => {
                let (target, seed) = reader_tell_target(meta);
                Some(Self::Studio(key.clone(), target, seed))
            }
            ReaderOrigin::Card(_) | ReaderOrigin::None => None,
        }
    }
}

fn sent_target_for_line(line: &str, default_target: &str) -> String {
    if let Some(rest) = line.strip_prefix("/tell ")
        && let Some((role, _)) = rest.trim_start().split_once(' ')
    {
        let role = role.trim();
        if !role.is_empty() {
            return role.strip_prefix("actor-").unwrap_or(role).to_owned();
        }
    }
    default_target.to_owned()
}

impl App {
    /// Target label, studio flag, and live shared editor for `draw_reader`.
    pub(crate) fn reader_compose_draw_ctx(
        &self,
        from_sheet: bool,
    ) -> Option<(
        String,
        bool,
        Option<&crate::chrome::text_editor::TextEditor>,
    )> {
        Some(match self.reader_compose_target_of(from_sheet)? {
            ComposeTarget::Studio(k, label, _) => (label, true, self.panel.draft(&k)),
            ComposeTarget::Hollywood => {
                ("hollywood".to_owned(), false, Some(&self.hollywood.input))
            }
        })
    }

    pub(super) fn reader_compose_target_of(&self, from_sheet: bool) -> Option<ComposeTarget> {
        let meta = if from_sheet {
            self.sheet
                .as_ref()
                .and_then(|s| s.view.as_ref())
                .map(|v| &v.meta)?
        } else {
            self.hollywood.view.as_ref().map(|v| &v.meta)?
        };
        ComposeTarget::from_meta(meta)
    }

    fn reader_tell_state(&self, from_sheet: bool) -> Option<&InlineTellState> {
        if from_sheet {
            self.sheet
                .as_ref()
                .and_then(|s| s.view.as_ref())
                .map(|v| &v.tell)
        } else {
            self.hollywood.view.as_ref().map(|v| &v.tell)
        }
    }

    fn set_reader_tell_state(&mut self, from_sheet: bool, next: InlineTellState) {
        if from_sheet {
            if let Some(sv) = self.sheet.as_mut().and_then(|s| s.view.as_mut()) {
                sv.tell = next;
            }
        } else if let Some(v) = self.hollywood.view.as_mut() {
            v.tell = next;
        }
    }

    /// Enter `Composing` from `Reading`/`Sent`/`Failed`, keeping any parked draft
    /// and seeding `/tell <role> ` only on an empty draft for eligible roles.
    pub(crate) fn reader_open_compose(&mut self, from_sheet: bool) -> bool {
        if matches!(
            self.reader_tell_state(from_sheet),
            Some(InlineTellState::Sending { .. })
        ) {
            return false;
        }
        let Some(target) = self.reader_compose_target_of(from_sheet) else {
            return false;
        };
        if let ComposeTarget::Studio(key, _, Some(seed)) = &target {
            let draft = self.panel.draft_mut(key);
            if draft.as_str().is_empty() {
                let _ = draft.insert(seed);
            }
        }
        self.set_reader_tell_state(from_sheet, InlineTellState::Composing);
        true
    }

    /// Collapse `Sent` immediately to `Reading` on scroll / nav / Esc interactions.
    pub(crate) fn reader_dismiss_sent(&mut self, from_sheet: bool) {
        if matches!(
            self.reader_tell_state(from_sheet),
            Some(InlineTellState::Sent { .. })
        ) {
            self.set_reader_tell_state(from_sheet, InlineTellState::Reading);
        }
    }

    fn reader_reset_history(&mut self, target: &ComposeTarget) {
        match target {
            ComposeTarget::Studio(key, _, _) => self.panel.history_mut(key).reset_nav(),
            ComposeTarget::Hollywood => self.hollywood.history.reset_nav(),
        }
    }

    fn reader_history_step(&mut self, target: &ComposeTarget, up: bool) {
        match target {
            ComposeTarget::Studio(key, _, _) => {
                self.panel_sync_history(key);
                let current = self
                    .panel
                    .draft(key)
                    .map(|d| d.as_str().to_owned())
                    .unwrap_or_default();
                let next = if up {
                    self.panel.history_mut(key).step_up(&current)
                } else {
                    self.panel.history_mut(key).step_down()
                };
                if let Some(entry) = next {
                    let draft = self.panel.draft_mut(key);
                    draft.clear();
                    let _ = draft.insert(&entry);
                }
            }
            ComposeTarget::Hollywood => {
                self.sync_hollywood_history();
                let current = self.hollywood.input.as_str().to_owned();
                let next = if up {
                    self.hollywood.history.step_up(&current)
                } else {
                    self.hollywood.history.step_down()
                };
                if let Some(entry) = next {
                    self.hollywood.input.clear();
                    let _ = self.hollywood.input.insert(&entry);
                }
            }
        }
    }

    /// Handle keys for active compose/status bands (`Composing`, `Sending`, `Sent`, `Failed`).
    /// Returns `true` when consumed so `Reading` shortcuts never receive compose keys.
    pub(crate) fn reader_handle_tell_key(&mut self, key: &TerminalKey, from_sheet: bool) -> bool {
        let Some(state) = self.reader_tell_state(from_sheet).cloned() else {
            return false;
        };
        let combo = crate::input::terminal_key_of(key);
        let (code, modifiers) = crate::config::normalize_key_combo((combo.code, combo.modifiers));
        let plain = modifiers.is_empty();
        let press = key.input.action == hc_vt::Action::Press;

        match state {
            InlineTellState::Reading => false,
            InlineTellState::Composing => {
                if !matches!(
                    key.input.action,
                    hc_vt::Action::Press | hc_vt::Action::Repeat
                ) {
                    return true;
                }
                let Some(target) = self.reader_compose_target_of(from_sheet) else {
                    self.set_reader_tell_state(from_sheet, InlineTellState::Reading);
                    return false;
                };
                match code {
                    KeyCode::Esc if plain && press => {
                        self.reader_reset_history(&target);
                        self.set_reader_tell_state(from_sheet, InlineTellState::Reading);
                    }
                    KeyCode::Enter if plain && press => {
                        self.reader_submit_compose(from_sheet, target)
                    }
                    KeyCode::Tab if plain && press => {
                        if let ComposeTarget::Studio(studio_key, _, _) = &target {
                            self.panel_input_complete_for(studio_key);
                        }
                    }
                    KeyCode::Up if plain => self.reader_history_step(&target, true),
                    KeyCode::Down if plain => self.reader_history_step(&target, false),
                    KeyCode::Esc | KeyCode::Enter | KeyCode::Tab if plain => {}
                    _ => match &target {
                        ComposeTarget::Studio(studio_key, _, _) => {
                            let _ = self.panel.draft_mut(studio_key).handle_key(key);
                        }
                        ComposeTarget::Hollywood => {
                            let _ = self.hollywood.input.handle_key(key);
                        }
                    },
                }
                true
            }
            InlineTellState::Sending { .. } => {
                if press && plain && code == KeyCode::Esc {
                    self.set_reader_tell_state(from_sheet, InlineTellState::Reading);
                    return true;
                }
                press && plain && matches!(code, KeyCode::Char('t' | 'i'))
            }
            InlineTellState::Sent { .. } => {
                if press && plain && code == KeyCode::Esc {
                    self.set_reader_tell_state(from_sheet, InlineTellState::Reading);
                    return true;
                }
                if press && plain && matches!(code, KeyCode::Char('t' | 'i')) {
                    let _ = self.reader_open_compose(from_sheet);
                    return true;
                }
                let dismiss = matches!(
                    code,
                    KeyCode::Up
                        | KeyCode::Down
                        | KeyCode::Left
                        | KeyCode::Right
                        | KeyCode::PageUp
                        | KeyCode::PageDown
                        | KeyCode::Home
                        | KeyCode::End
                        | KeyCode::Char('j' | 'k' | ' ' | 'b' | 'g' | 'G' | '[' | ']')
                ) || (modifiers == KeyModifiers::CONTROL
                    && matches!(code, KeyCode::Char('d' | 'u')));
                if dismiss {
                    self.set_reader_tell_state(from_sheet, InlineTellState::Reading);
                }
                false
            }
            InlineTellState::Failed { .. } if press && plain => match code {
                KeyCode::Enter | KeyCode::Char('t' | 'i') => {
                    self.set_reader_tell_state(from_sheet, InlineTellState::Composing);
                    true
                }
                KeyCode::Esc => {
                    self.set_reader_tell_state(from_sheet, InlineTellState::Reading);
                    true
                }
                _ => false,
            },
            InlineTellState::Failed { .. } => false,
        }
    }

    fn reader_submit_compose(&mut self, from_sheet: bool, target: ComposeTarget) {
        let line = match &target {
            ComposeTarget::Studio(key, _, _) => self
                .panel
                .draft(key)
                .map(|d| d.as_str().trim().to_owned())
                .unwrap_or_default(),
            ComposeTarget::Hollywood => self.hollywood.input.as_str().trim().to_owned(),
        };
        if line.is_empty() {
            self.reader_reset_history(&target);
            self.set_reader_tell_state(from_sheet, InlineTellState::Reading);
            return;
        }
        let display_target = match &target {
            ComposeTarget::Studio(key, default_target, _) => {
                if self.panel_mistyped_verb(&line) {
                    self.reject_input(
                        "looks like a mistyped /command — remove the leading letter or edit",
                    );
                    return;
                }
                self.panel_sync_history(key);
                if line != "/" {
                    self.panel.history_mut(key).record_send(&line);
                } else {
                    self.panel.reset_history_nav();
                }
                let label = sent_target_for_line(&line, default_target);
                self.last_dispatched_action_id = None;
                match line.strip_prefix('/') {
                    Some(rest) => self.panel_slash(key, rest),
                    None => self.run_action(studio_cmds::tell(key, "producer", &line)),
                }
                self.panel.clear_draft(key);
                label
            }
            ComposeTarget::Hollywood => {
                self.sync_hollywood_history();
                self.hollywood.history.record_send(&line);
                self.hollywood.input.clear();
                self.last_dispatched_action_id = None;
                self.run_action(studio_cmds::tell_hollywood(&line));
                "hollywood".to_owned()
            }
        };
        let next = if let Some(action_id) = self.last_dispatched_action_id.take()
            && self.actions.pending.iter().any(|p| p.id == action_id)
        {
            InlineTellState::Sending {
                action_id,
                target: display_target,
                text: line,
            }
        } else {
            InlineTellState::Reading
        };
        self.set_reader_tell_state(from_sheet, next);
    }

    /// Update any open Reader waiting on `action_id`; returns `true` when inline state handled it.
    pub(crate) fn reader_compose_on_reply(
        &mut self,
        id: u64,
        result: &Result<String, String>,
    ) -> bool {
        let now = Instant::now();
        for from_sheet in [false, true] {
            let matched = match self.reader_tell_state(from_sheet) {
                Some(InlineTellState::Sending {
                    action_id,
                    target,
                    text,
                }) if *action_id == id => Some((target.clone(), text.clone())),
                _ => None,
            };
            let Some((target, text)) = matched else {
                continue;
            };
            match result {
                Ok(_) => self.set_reader_tell_state(
                    from_sheet,
                    InlineTellState::Sent {
                        target,
                        until: now + SENT_HOLD,
                    },
                ),
                Err(err) => {
                    if let Some(t) = self.reader_compose_target_of(from_sheet) {
                        match t {
                            ComposeTarget::Studio(key, _, _) => {
                                let d = self.panel.draft_mut(&key);
                                d.clear();
                                let _ = d.insert(&text);
                            }
                            ComposeTarget::Hollywood => {
                                self.hollywood.input.clear();
                                let _ = self.hollywood.input.insert(&text);
                            }
                        }
                    }
                    self.set_reader_tell_state(
                        from_sheet,
                        InlineTellState::Failed {
                            err: crate::action::fail_body(err),
                        },
                    );
                }
            }
            return true;
        }
        false
    }
}
