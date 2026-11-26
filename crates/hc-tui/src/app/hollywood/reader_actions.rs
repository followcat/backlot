//! Shared Reader actions (`y` raw OSC 52 copy, `o` focus/reopen,
//! `p` prompt-less peek, `v` sheet jump, `Ctrl+B` prefix chords,
//! scroll keys, pending card lock, and chip clicks).

use std::time::Instant;

use crossterm::event::{KeyCode, KeyModifiers};

use super::App;
use crate::app::input::{Decision, TerminalKey};
use crate::chrome::scroll::ScrollMetrics;
use crate::hollywood::{ReaderMeta, ReaderOAction, ReaderOrigin};
use crate::studio_cmds;

impl App {
    /// Handle `Ctrl+B` prefix chords while the Reader is in `Reading` mode so
    /// sequences like `Ctrl+B ?` reach global prefix actions cleanly.
    pub(crate) fn reader_handle_prefix(&mut self, key: &TerminalKey) -> bool {
        if key.input.action != hc_vt::Action::Press {
            return false;
        }
        let combo = crate::input::terminal_key_of(key);
        if self.prefix.armed() {
            match self.prefix.feed(key, &self.live) {
                Decision::Action(action) => self.on_action(action, self.area.0),
                Decision::Command(command) => self.run_custom_command(&command),
                Decision::Held | Decision::ToTerm(_) => {}
            }
            return true;
        }
        if self.live.matches_prefix(&combo) {
            let _ = self.prefix.feed(key, &self.live);
            return true;
        }
        false
    }

    /// Copy the exact unwrapped raw UTF-8 source text via OSC 52 + 2s copy toast.
    pub(crate) fn reader_copy_raw(&mut self, raw_body: &str, lines: &[String]) {
        let owned;
        let text = if raw_body.is_empty() {
            owned = lines.join("\n");
            owned.as_str()
        } else {
            raw_body
        };
        if text.is_empty() {
            return;
        }
        let sequence = crate::selection::osc52_sequence(text.as_bytes());
        let _ = (self.host_control)(sequence.as_bytes());
        if self.show_copy_feedback(Instant::now()) {
            self.sched.mark_dirty(Instant::now());
        }
    }

    /// Pending status text for an open actionable card (`card:<id>`).
    pub(crate) fn card_pending_label(&self, card: Option<&str>) -> Option<String> {
        let card = card?;
        let mark = crate::action::mark::card(card);
        self.actions
            .pending
            .iter()
            .find(|p| p.action.mark == mark)
            .map(|p| p.text.clone())
    }

    /// Execute `[o] focus` or `[o] reopen` according to author state.
    pub(crate) fn reader_run_o(&mut self, meta: &ReaderMeta) -> bool {
        match meta.o_action() {
            Some(ReaderOAction::Focus) => {
                let Some(terminal) = meta.role_terminal.clone() else {
                    return false;
                };
                self.hollywood_leave_if_open();
                self.hollywood.view = None;
                if let Some(s) = self.sheet.as_mut() {
                    s.view = None;
                }
                self.sheet = None;
                self.focus_pane(terminal);
                true
            }
            Some(ReaderOAction::Reopen) => {
                if let (ReaderOrigin::StudioExchange(key, _), Some(role)) =
                    (&meta.origin, meta.role.as_deref())
                {
                    self.run_action(studio_cmds::reopen(key, role));
                    true
                } else {
                    false
                }
            }
            None => false,
        }
    }

    /// Open existing read-only `open_peek` without ever setting `peek.prompt`.
    pub(crate) fn reader_open_peek(&mut self, meta: &ReaderMeta) -> bool {
        if !meta.can_peek() {
            return false;
        }
        let Some(terminal) = meta.role_terminal.clone() else {
            return false;
        };
        self.open_peek(terminal);
        true
    }

    /// Jump to Task & Delivery Sheet for the entry's studio and task, closing Reader.
    pub(crate) fn reader_open_sheet(&mut self, meta: &ReaderMeta) -> bool {
        let Some(key) = meta.sheet_studio_key() else {
            return false;
        };
        self.hollywood.view = None;
        self.sheet_open(key, meta.task.clone());
        true
    }

    /// Activate an action chip (`[k]`) on `self.hollywood.view`.
    pub(super) fn reader_activate_chip(&mut self, ch: char) {
        let Some((card, raw, lines, meta)) = self.hollywood.view.as_ref().map(|v| {
            (
                v.card.clone(),
                v.raw_body.clone(),
                v.lines.clone(),
                v.meta.clone(),
            )
        }) else {
            return;
        };
        match ch {
            't' => {
                let _ = self.reader_open_compose(false);
            }
            'r' => {
                if meta.kind == crate::hollywood::ReaderKind::Design && !meta.design_finalized() {
                    let _ = self.reader_open_compose(false);
                }
            }
            'y' => self.reader_copy_raw(&raw, &lines),
            'o' => {
                self.reader_run_o(&meta);
            }
            'p' => {
                self.reader_open_peek(&meta);
            }
            'v' => {
                self.reader_open_sheet(&meta);
            }
            'a' => {
                if let Some(id) = card
                    && self.card_pending_label(Some(&id)).is_none()
                {
                    self.hollywood_card_apply(&id);
                } else if meta.design_awaiting_user() {
                    self.reader_design_sign_off();
                }
            }
            'x' => {
                if let Some(id) = card
                    && self.card_pending_label(Some(&id)).is_none()
                {
                    self.hollywood_card_cancel(&id);
                }
            }
            _ => {}
        }
    }

    /// Jump forward/backward across parsed design section anchors in the open Reader.
    pub(crate) fn reader_design_step(&mut self, delta: isize) {
        let Some(view) = self.hollywood.view.as_ref() else {
            return;
        };
        if view.meta.kind != crate::hollywood::ReaderKind::Design {
            return;
        }
        let metrics = self.hollywood_hits.view.as_ref().and_then(|v| v.metrics);
        let mut sections = self
            .hollywood_hits
            .view
            .as_ref()
            .map(|v| v.sections.clone())
            .unwrap_or_default();
        if sections.is_empty() {
            let w = usize::from(self.area.0).saturating_sub(6).clamp(40, 88);
            sections = crate::hollywood::render_body_lines_with_sections(&view.lines, w);
        }
        if sections.is_empty() {
            return;
        }
        let cur = view.meta.design_section_idx.unwrap_or_else(|| {
            self.hollywood_hits.view.as_ref().map_or_else(
                || {
                    sections
                        .iter()
                        .rposition(|(_, ln)| *ln <= view.scroll)
                        .unwrap_or(0)
                },
                |vr| vr.active_section.min(sections.len() - 1),
            )
        });
        let next_idx = if delta < 0 {
            cur.saturating_sub(delta.unsigned_abs())
        } else {
            cur.saturating_add(delta as usize).min(sections.len() - 1)
        };
        let (label, target_line) = sections[next_idx].clone();
        let max_scroll = metrics.map_or(target_line, |m| m.max_offset_from_bottom);
        let target_scroll = target_line.min(max_scroll);
        if let Some(v) = self.hollywood.view.as_mut() {
            v.scroll = target_scroll;
            v.meta.design_section_idx = Some(next_idx);
            v.meta.design_section = Some(label);
            v.meta.nav_index = Some(next_idx + 1);
            v.meta.nav_total = Some(sections.len());
        }
        if let Some(vr) = self.hollywood_hits.view.as_mut() {
            vr.active_section = next_idx;
        }
    }

    /// Open sign-off confirmation when viewing an actionable design document.
    pub(crate) fn reader_design_sign_off(&mut self) {
        let Some(view) = self.hollywood.view.as_ref() else {
            return;
        };
        if !view.meta.design_awaiting_user() {
            return;
        }
        let Some(key) = view.meta.sheet_studio_key().or_else(|| {
            view.meta
                .studio
                .as_ref()
                .map(|id| crate::studio::StudioKey {
                    machine: view.meta.machine.clone(),
                    id: id.clone(),
                })
        }) else {
            return;
        };
        let skip = view.meta.design_phase == Some(backlot_model::design::DesignPhase::SuggestSkip);
        self.studio_dialog = Some(crate::studio_dialog::StudioDialog::design_sign(
            &key,
            skip,
            view.meta.file.as_deref(),
        ));
    }

    /// Activate an action chip (`[k]`) on an open Sheet Reader (`sheet.view`).
    pub(crate) fn reader_activate_sheet_chip(&mut self, ch: char) {
        let Some((raw, lines, meta)) = self
            .sheet
            .as_ref()
            .and_then(|s| s.view.as_ref())
            .map(|sv| (sv.raw_body.clone(), sv.lines.clone(), sv.meta.clone()))
        else {
            return;
        };
        match ch {
            't' => {
                let _ = self.reader_open_compose(true);
            }
            'y' => self.reader_copy_raw(&raw, &lines),
            'o' => {
                self.reader_run_o(&meta);
            }
            'p' => {
                self.reader_open_peek(&meta);
            }
            _ => {}
        }
    }

    /// Shared Reader scroll handler (`j`/`↓`, `k`/`↑`, `Space`/`PageDown`/`Ctrl+D`,
    /// `b`/`PageUp`/`Ctrl+U`, `g`/`Home`, `G`/`End`). Note plain `'d'` stays unbound.
    pub(crate) fn reader_scroll_key(
        scroll: &mut usize,
        code: KeyCode,
        modifiers: KeyModifiers,
        action: hc_vt::Action,
        page: usize,
        metrics: Option<ScrollMetrics>,
    ) -> bool {
        let plain = modifiers.is_empty();
        let ctrl = modifiers == KeyModifiers::CONTROL;
        let shift_or_plain = plain || modifiers == KeyModifiers::SHIFT;
        let active = action != hc_vt::Action::Release;
        let press = action == hc_vt::Action::Press;

        let handled = match code {
            KeyCode::Down | KeyCode::Char('j') if plain && active => {
                *scroll = scroll.saturating_add(1);
                true
            }
            KeyCode::Up | KeyCode::Char('k') if plain && active => {
                *scroll = scroll.saturating_sub(1);
                true
            }
            KeyCode::PageDown | KeyCode::Char(' ') if plain && active => {
                *scroll = scroll.saturating_add(page);
                true
            }
            KeyCode::Char('d') if ctrl && active => {
                *scroll = scroll.saturating_add(page);
                true
            }
            KeyCode::PageUp | KeyCode::Char('b') if plain && active => {
                *scroll = scroll.saturating_sub(page);
                true
            }
            KeyCode::Char('u') if ctrl && active => {
                *scroll = scroll.saturating_sub(page);
                true
            }
            KeyCode::Home | KeyCode::Char('g') if plain && press => {
                *scroll = 0;
                true
            }
            KeyCode::End | KeyCode::Char('G') if shift_or_plain && press => {
                *scroll = metrics.map_or(usize::MAX, |m| m.max_offset_from_bottom);
                true
            }
            _ => false,
        };
        if handled && let Some(m) = metrics {
            *scroll = (*scroll).min(m.max_offset_from_bottom);
        }
        handled
    }

    /// Nearest `Sent { until }` deadline across open Readers for `wake_deadline`.
    pub(crate) fn reader_sent_deadline(&self) -> Option<Instant> {
        use crate::hollywood::InlineTellState;
        let hw = self.hollywood.view.as_ref().and_then(|v| match &v.tell {
            InlineTellState::Sent { until, .. } => Some(*until),
            _ => None,
        });
        let sh = self
            .sheet
            .as_ref()
            .and_then(|s| s.view.as_ref())
            .and_then(|sv| match &sv.tell {
                InlineTellState::Sent { until, .. } => Some(*until),
                _ => None,
            });
        match (hw, sh) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    /// Collapse any expired `Sent` banner after 1.5 s.
    pub(crate) fn expire_reader_sent(&mut self, now: Instant) -> bool {
        use crate::hollywood::InlineTellState;
        let mut changed = false;
        if let Some(v) = self.hollywood.view.as_mut()
            && matches!(&v.tell, InlineTellState::Sent { until, .. } if now >= *until)
        {
            v.tell = InlineTellState::Reading;
            changed = true;
        }
        if let Some(sv) = self.sheet.as_mut().and_then(|s| s.view.as_mut())
            && matches!(&sv.tell, InlineTellState::Sent { until, .. } if now >= *until)
        {
            sv.tell = InlineTellState::Reading;
            changed = true;
        }
        changed
    }
}
