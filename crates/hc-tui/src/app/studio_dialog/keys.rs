//! Dialog keys, mouse and submits — `Esc` cancels, `Enter`/`y`
//! runs the item's `run_action` (split from the open/prefill
//! paths to keep files small).

use crossterm::event::{KeyCode, KeyModifiers};

use crate::app::input::TerminalKey;
use crate::studio_dialog::{
    ConfirmFor, DialogField, ReasonFor, ReviseDialog, StudioDialog, TellDialog,
};
use crate::{studio_cmds, studio_dialog};

use super::App;

impl App {
    /// Keys while a studio dialog is open — the whole press is
    /// consumed, like the other modals. `Esc` always cancels.
    pub(crate) fn studio_dialog_key(&mut self, key: &TerminalKey) {
        let combo = crate::input::terminal_key_of(key);
        if combo.kind == crossterm::event::KeyEventKind::Release {
            return;
        }
        match self.studio_dialog.take() {
            Some(StudioDialog::Reason(mut d)) => {
                self.studio_dialog = match combo.code {
                    KeyCode::Esc => None,
                    KeyCode::Enter => {
                        let reason = d.reason.as_str().trim().to_owned();
                        if d.required && reason.is_empty() {
                            d.error = Some("a reason is required".to_owned());
                            Some(StudioDialog::Reason(d))
                        } else {
                            self.reason_submit(&d.for_, reason);
                            None
                        }
                    }
                    _ => {
                        d.reason.handle_key(key);
                        d.error = None;
                        Some(StudioDialog::Reason(d))
                    }
                };
            }
            Some(StudioDialog::Confirm(d)) => {
                if d.loading {
                    if combo.code != KeyCode::Esc {
                        self.studio_dialog = Some(StudioDialog::Confirm(d));
                    }
                    return;
                }
                // A stale server plan offers a re-review: Enter/`y`/`r`
                // run the dry run again instead of the commit.
                if d.error.is_some() {
                    match combo.code {
                        KeyCode::Enter | KeyCode::Char('y') | KeyCode::Char('r') => {
                            self.confirm_review(&d.for_);
                        }
                        KeyCode::Esc => {}
                        _ => self.studio_dialog = Some(StudioDialog::Confirm(d)),
                    }
                    return;
                }
                self.studio_dialog = None;
                match combo.code {
                    KeyCode::Enter | KeyCode::Char('y') if combo.modifiers.is_empty() => {
                        self.confirm_submit(&d.for_);
                    }
                    KeyCode::Enter => self.confirm_submit(&d.for_),
                    _ => {}
                }
            }
            Some(StudioDialog::Tell(d)) => {
                self.studio_dialog = self.tell_key(d, key, combo.code, combo.modifiers);
            }
            Some(StudioDialog::Revise(d)) => {
                self.studio_dialog = self.revise_key(d, key, combo.code, combo.modifiers);
            }
            Some(StudioDialog::Restart(d)) => {
                self.studio_dialog = self.restart_key(d, key, combo.code);
            }
            Some(StudioDialog::Recast(d)) => {
                self.studio_dialog = self.recast_key(d, key, combo.code, combo.modifiers);
            }
            Some(StudioDialog::Restore(d)) => {
                self.studio_dialog = self.restore_key(d, key, combo.code);
            }
            Some(StudioDialog::CastEdit(d)) => {
                self.studio_dialog = self.cast_edit_key(d, combo.code, combo.modifiers);
            }
            Some(StudioDialog::DesignSign(d)) => {
                self.studio_dialog = self.design_sign_key(d, combo.code, combo.modifiers);
            }
            Some(StudioDialog::Approval(d)) => {
                self.studio_dialog = self.approval_key(d, combo.code, combo.modifiers);
            }
            Some(StudioDialog::Permissions(d)) => {
                self.studio_dialog = self.permissions_key(d, combo.code);
            }
            None => {}
        }
    }

    /// The reason dialog's Enter — the action its `for` names.
    fn reason_submit(&mut self, for_: &ReasonFor, reason: String) {
        let reason = Some(reason.as_str()).filter(|r| !r.is_empty());
        match for_ {
            ReasonFor::TaskVerb {
                key, task, verb, ..
            } => {
                self.run_action(studio_cmds::task(key, task, verb, reason));
            }
            ReasonFor::Finish { key } => {
                self.run_action(studio_cmds::finish(key, reason));
            }
            ReasonFor::CardCancel { machine, id } => {
                self.run_action(studio_cmds::card_cancel_on(machine.clone(), id, reason));
            }
        }
    }

    /// The confirm's `↵`/`y` — the action its `for` names.
    fn confirm_submit(&mut self, for_: &ConfirmFor) {
        match for_ {
            ConfirmFor::Close { key, role } => {
                self.run_action(studio_cmds::close(key, role));
            }
            ConfirmFor::StageStop { machine } => {
                self.run_action(studio_cmds::stage_stop(machine.clone()));
            }
            ConfirmFor::ClearAttention { key, role, seq } => {
                self.run_action(studio_cmds::clear_attention(key, role, *seq));
            }
            ConfirmFor::DirOverride { key, role, allow } => {
                self.run_action(studio_cmds::decide(key, role, *allow));
            }
            ConfirmFor::DeleteStudio {
                key,
                force,
                kept,
                token,
            } => {
                if !*force && !kept.is_empty() {
                    self.studio_dialog = Some(StudioDialog::delete_studio_force(key, kept));
                } else {
                    self.run_action(studio_cmds::delete_studio(key, *force, token.as_deref()));
                }
            }
            ConfirmFor::FinishEmpty { key } => {
                self.run_action(studio_cmds::finish_force(key));
            }
            ConfirmFor::Prune { key, token } => {
                self.run_action(studio_cmds::prune(key, token.as_deref()));
            }
            ConfirmFor::DesignSign { key, skip } => {
                self.run_action(studio_cmds::design_sign(key, *skip, None, None, false));
            }
        }
    }

    /// The stale-plan review: open the destructive dialog again, which
    /// fetches a fresh dry run and a fresh token.
    fn confirm_review(&mut self, for_: &ConfirmFor) {
        match for_ {
            ConfirmFor::DeleteStudio { key, .. } => self.delete_studio_open(key),
            ConfirmFor::Prune { key, .. } => self.prune_open(key),
            _ => {}
        }
    }

    /// The Tell dialog's keys — `Esc` cancels, `^s` submits
    /// anywhere, `Tab` walks body → Send, `Enter` on a single line
    /// or on Send submits (§2.3).
    fn tell_key(
        &mut self,
        mut d: TellDialog,
        key: &TerminalKey,
        code: KeyCode,
        mods: KeyModifiers,
    ) -> Option<StudioDialog> {
        match (code, mods) {
            (KeyCode::Esc, _) => return None,
            (KeyCode::Char('s'), m) if m == KeyModifiers::CONTROL => {
                return self.tell_submit(d);
            }
            (KeyCode::Tab, _) | (KeyCode::BackTab, _) => {
                d.field = if d.field == DialogField::Body {
                    DialogField::Send
                } else {
                    DialogField::Body
                };
            }
            (KeyCode::Enter, _) if d.field == DialogField::Send => {
                return self.tell_submit(d);
            }
            (KeyCode::Enter, _) if d.body.lines.len() == 1 => {
                return self.tell_submit(d);
            }
            _ => {
                if let Some(text) = key.text_commit.as_deref().filter(|t| !t.is_empty()) {
                    d.body.insert(text);
                } else {
                    d.body.key(code, mods);
                }
                d.error = None;
            }
        }
        Some(StudioDialog::Tell(d))
    }

    /// `backlot tell <role> --text <msg>` — empty refuses inline.
    fn tell_submit(&mut self, mut d: TellDialog) -> Option<StudioDialog> {
        let text = d.body.text();
        if text.trim().is_empty() {
            d.error = Some("an empty message sends nothing".to_owned());
            return Some(StudioDialog::Tell(d));
        }
        self.run_action(studio_cmds::tell(&d.key, &d.role, &text));
        None
    }

    /// The Revise dialog's keys — `^e` the `$EDITOR`, `Tab` walks
    /// body → summary → Send, `^s` or Enter on Send submits (§2.4).
    fn revise_key(
        &mut self,
        mut d: ReviseDialog,
        key: &TerminalKey,
        code: KeyCode,
        mods: KeyModifiers,
    ) -> Option<StudioDialog> {
        match (code, mods) {
            (KeyCode::Esc, _) => return None,
            (KeyCode::Char('e'), m) if m == KeyModifiers::CONTROL => {
                if d.body.is_none() {
                    d.error = Some("still loading current requirement…".to_owned());
                    return Some(StudioDialog::Revise(d));
                }
                self.studio_dialog = Some(StudioDialog::Revise(d));
                self.revise_editor();
                return self.studio_dialog.take();
            }
            (KeyCode::Char('s'), m) if m == KeyModifiers::CONTROL => {
                return self.revise_submit(d);
            }
            (KeyCode::Tab, _) => {
                d.field = match d.field {
                    DialogField::Body => DialogField::Summary,
                    DialogField::Summary => DialogField::Send,
                    DialogField::Send => DialogField::Body,
                };
            }
            (KeyCode::BackTab, _) => {
                d.field = match d.field {
                    DialogField::Body => DialogField::Send,
                    DialogField::Summary => DialogField::Body,
                    DialogField::Send => DialogField::Summary,
                };
            }
            (KeyCode::Enter, _) if d.field == DialogField::Send => {
                return self.revise_submit(d);
            }
            (KeyCode::Enter, _) if d.field == DialogField::Summary => {
                d.field = DialogField::Send;
            }
            _ => match d.field {
                DialogField::Body => {
                    if let Some(body) = d.body.as_mut() {
                        if let Some(text) = key.text_commit.as_deref().filter(|t| !t.is_empty()) {
                            body.insert(text);
                        } else {
                            body.key(code, mods);
                        }
                    }
                    d.error = None;
                }
                DialogField::Summary => {
                    d.summary.handle_key(key);
                    d.error = None;
                }
                DialogField::Send => {}
            },
        }
        Some(StudioDialog::Revise(d))
    }

    /// `backlot req revise --stdin --summary <s>` — the body pipes
    /// through the runner; blocks while loading, on missing summary,
    /// and on first submit when the base requirement read failed.
    fn revise_submit(&mut self, mut d: ReviseDialog) -> Option<StudioDialog> {
        if d.body.is_none() {
            d.error = Some("still loading current requirement…".to_owned());
            return Some(StudioDialog::Revise(d));
        }
        let summary = d.summary.as_str().trim().to_owned();
        if summary.is_empty() {
            d.error = Some("a one-line summary is required".to_owned());
            d.field = DialogField::Summary;
            return Some(StudioDialog::Revise(d));
        }
        if d.prefill_failed && !d.confirm_overwrite {
            d.confirm_overwrite = true;
            d.error = Some("base read failed — press Ctrl+S / Send again to overwrite".to_owned());
            return Some(StudioDialog::Revise(d));
        }
        let body = d.body.as_ref().map(|b| b.text()).unwrap_or_default();
        self.run_action(studio_cmds::revise(&d.key, &summary, body));
        None
    }

    /// The dialog's press routing — Send/Cancel/body/summary hit
    /// rects; a press outside cancels like Esc.
    pub(crate) fn studio_dialog_mouse(&mut self, mouse: crate::app::input::MouseInput) {
        use crate::app::input::{MouseButton, MouseEventKind};
        use ratatui::layout::Position;
        let contains = |rect: ratatui::layout::Rect| {
            !rect.is_empty()
                && rect.contains(Position {
                    x: mouse.col,
                    y: mouse.row,
                })
        };
        if mouse.kind != MouseEventKind::Down(MouseButton::Left) {
            return;
        }
        if contains(self.studio_dialog_hits.cancel) {
            self.studio_dialog = None;
            return;
        }
        if contains(self.studio_dialog_hits.send) {
            match self.studio_dialog.take() {
                Some(StudioDialog::Reason(d)) => {
                    let reason = d.reason.as_str().trim().to_owned();
                    if d.required && reason.is_empty() {
                        self.studio_dialog =
                            Some(StudioDialog::Reason(studio_dialog::ReasonDialog {
                                error: Some("a reason is required".into()),
                                ..d
                            }));
                    } else {
                        self.reason_submit(&d.for_, reason);
                    }
                }
                Some(StudioDialog::Confirm(d)) => {
                    if d.loading {
                        self.studio_dialog = Some(StudioDialog::Confirm(d));
                    } else if d.error.is_some() {
                        self.confirm_review(&d.for_);
                    } else {
                        self.confirm_submit(&d.for_);
                    }
                }
                Some(StudioDialog::Tell(d)) => {
                    self.studio_dialog = self.tell_submit(d);
                }
                Some(StudioDialog::Revise(d)) => {
                    self.studio_dialog = self.revise_submit(d);
                }
                Some(StudioDialog::Restart(d)) => {
                    self.studio_dialog = self.restart_submit(&d);
                }
                Some(StudioDialog::Recast(mut d)) => {
                    if !self.recast_advance(&mut d) {
                        self.studio_dialog = Some(StudioDialog::Recast(d));
                    }
                }
                Some(StudioDialog::Restore(mut d)) => match d.step {
                    crate::studio_dialog::RestoreStep::List => {
                        d.begin_name();
                        self.studio_dialog = Some(StudioDialog::Restore(d));
                    }
                    crate::studio_dialog::RestoreStep::Name => {
                        self.studio_dialog = self.restore_dry_run(d);
                    }
                    crate::studio_dialog::RestoreStep::Confirm => {
                        if d.error.is_some() {
                            self.studio_dialog = self.restore_dry_run(d);
                        } else if !d.loading {
                            self.restore_apply(&d);
                        } else {
                            self.studio_dialog = Some(StudioDialog::Restore(d));
                        }
                    }
                },
                Some(StudioDialog::CastEdit(d)) => {
                    if d.ready && !d.busy {
                        self.cast_edit_apply(&d);
                    } else {
                        self.studio_dialog = Some(StudioDialog::CastEdit(d));
                    }
                }
                Some(StudioDialog::DesignSign(d)) => {
                    self.design_sign_submit(&d);
                }
                Some(StudioDialog::Approval(d)) => {
                    self.approval_submit(&d);
                }
                Some(StudioDialog::Permissions(d)) => {
                    if let Some(permit) = d.chosen().map(|p| p.id.clone()) {
                        self.run_action(studio_cmds::permit_revoke(&d.key, &permit));
                    }
                }
                None => {}
            }
            return;
        }
        if let Some(idx) = self
            .studio_dialog_hits
            .choices
            .iter()
            .position(|rect| contains(*rect))
        {
            match self.studio_dialog.take() {
                Some(StudioDialog::DesignSign(mut d)) => {
                    d.selected = idx;
                    self.design_sign_submit(&d);
                    return;
                }
                Some(StudioDialog::Approval(mut d)) => {
                    d.selected = idx;
                    self.approval_submit(&d);
                    return;
                }
                Some(StudioDialog::Permissions(mut d)) => {
                    d.selected = idx;
                    self.studio_dialog = Some(StudioDialog::Permissions(d));
                    return;
                }
                other => self.studio_dialog = other,
            }
        }
        if contains(self.studio_dialog_hits.area) {
            // Inside: a click on the body or summary moves the focus.
            if let Some(StudioDialog::Tell(d)) = self.studio_dialog.as_mut() {
                d.field = if contains(self.studio_dialog_hits.body) {
                    DialogField::Body
                } else {
                    d.field
                };
            }
            if let Some(StudioDialog::Revise(d)) = self.studio_dialog.as_mut() {
                if contains(self.studio_dialog_hits.body) {
                    d.field = DialogField::Body;
                } else if contains(self.studio_dialog_hits.summary) {
                    d.field = DialogField::Summary;
                }
            }
            return;
        }
        self.studio_dialog = None;
    }
}
