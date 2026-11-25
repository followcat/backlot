//! The Call Sheet's row actions (S1 §5, S3 §3): each key's
//! `studio_cmds::Action` on the right machine — a remote studio's
//! prompt, stage and finish actions go through `RemoteRunner` by
//! the tracker's `machine`; cards and `tell` stay on the local core.
//! Pending, toast and failure all ride the one `ActionTracker`.

use super::App;
use crate::hollywood::{Confirm, Target};
use crate::studio::StudioKey;
use crate::studio_cmds;

impl App {
    /// `y`/`d` on a prompt row: a user-owned prompt answers `backlot
    /// approve` at once; a director-routed one takes the D9 override
    /// confirm first (§2.2).
    pub(crate) fn hollywood_prompt_decision(
        &mut self,
        studio: &StudioKey,
        role: &str,
        allow: bool,
    ) {
        let machines = self.panel_summaries();
        let found = crate::panel::data::studio(&machines, studio)
            .and_then(|s| s.studio.roles.iter().find(|r| r.role == role));
        // A runner that cannot answer sends nothing: the key records no
        // decision and the row already says to answer in the terminal.
        if found.is_some_and(|r| !r.can_answer_prompt) {
            return;
        }
        let owned = found.is_some_and(|r| r.attention_user);
        if owned {
            self.run_action(studio_cmds::decide(studio, role, allow));
        } else {
            self.studio_dialog = Some(crate::studio_dialog::StudioDialog::dir_override(
                studio, role, allow,
            ));
        }
    }

    /// Whether Hollywood's live row owns a prompt — the only state
    /// its `y`/`d` answer.
    fn hollywood_asking(&self) -> bool {
        let machines = self.panel_summaries();
        crate::hollywood::hollywood_asking(&machines)
    }

    /// `p` on the live row: Hollywood's own screen, read-only.
    fn hollywood_peek_live(&mut self) {
        match self.hollywood_terminal() {
            Some(terminal) => self.open_peek(terminal),
            None => self.show_notice(
                "Hollywood",
                "bl-hollywood is not running — the stage starts it",
            ),
        }
    }

    /// `p` on a prompt row or a frame's on-set role: the role's live
    /// screen, read-only — machine-qualified for a remote studio. The
    /// peek carries the prompt only while the role actually owns one:
    /// an unprompted seat would otherwise answer `y`/`d` into thin
    /// air.
    fn hollywood_peek(&mut self, studio: &StudioKey, role: &str) {
        let machines = self.panel_summaries();
        let found = crate::panel::data::studio(&machines, studio).and_then(|s| {
            s.studio.roles.iter().find(|r| r.role == role).map(|r| {
                (
                    r.terminal.clone(),
                    r.attention_user && r.can_answer_prompt,
                    r.name.clone(),
                )
            })
        });
        match found {
            Some((Some(terminal), attention_user, role_name)) => {
                let studio_title = self.studio_peek_title(studio, &role_name);
                self.open_peek(terminal);
                if let Some(peek) = self.peek.as_mut() {
                    peek.studio_title = Some(studio_title);
                    peek.prompt = attention_user.then(|| (studio.clone(), role.to_string()));
                }
            }
            Some((None, _, _)) | None => {
                self.show_notice("Hollywood", "the role has no screen to peek")
            }
        }
    }

    /// `p` on a studio frame or its READY row: the role the frame
    /// already calls "on set" — the live worker, else the producer.
    /// When that seat owns a user prompt the peek carries it like the
    /// prompt row's does.
    fn hollywood_peek_on_set(&mut self, studio: &StudioKey) {
        let machines = self.panel_summaries();
        let role = crate::panel::data::studio(&machines, studio)
            .and_then(|s| crate::hollywood::on_set(s.studio).map(|r| r.role.clone()));
        match role {
            Some(role) => self.hollywood_peek(studio, &role),
            None => self.show_notice("Hollywood", "the studio has no one on set to peek"),
        }
    }

    /// `s` on a stage row or a stopped-stage frame: `backlot stage
    /// start` on the machine — the same command the panel's `s` runs.
    pub(super) fn hollywood_stage_start(&mut self, machine: Option<String>) {
        if !self.can_start_stage(machine.as_deref()) {
            return;
        }
        self.run_action(studio_cmds::stage_start(machine));
    }

    /// Whether the studio `key` names is `ready_to_finish` — the one
    /// state a frame's `f` arms the finish confirm on. A document that
    /// no longer lists it (a publish raced the press) reads not ready.
    fn hollywood_studio_ready(&self, key: &StudioKey) -> bool {
        let machines = self.panel_summaries();
        crate::panel::data::studio(&machines, key).is_some_and(|s| s.studio.ready_to_finish)
    }

    /// `f` on a ready row: `backlot studio finish`, on the studio's
    /// own machine, after the inline confirm answers `y`.
    fn hollywood_finish(&mut self, key: &StudioKey) {
        self.run_action(studio_cmds::finish(key, None));
    }

    fn card_machine(&self, id: &str) -> Option<String> {
        if let Some(crate::hollywood::Target::OutwardCard(key, cid)) =
            self.hollywood.selected.as_ref()
            && self.hollywood.expanded()
            && cid == id
        {
            return key.machine.clone();
        }
        if let Some(crate::panel::Target::OutwardCard(key, cid)) = self.panel.selected.as_ref()
            && cid == id
        {
            return key.machine.clone();
        }
        let machines = self.panel_summaries();
        if crate::hollywood::local_summary(&machines)
            .and_then(|s| s.hollywood.as_ref())
            .is_some_and(|h| h.cards.iter().any(|c| c.id == id))
        {
            return None;
        }
        machines
            .iter()
            .find(|m| {
                m.summary
                    .studios
                    .iter()
                    .any(|s| s.outward_cards.iter().any(|c| c.id == id))
            })
            .and_then(|m| m.machine.clone())
    }

    pub(super) fn hollywood_card_apply_on(&mut self, machine: Option<String>, id: &str) {
        self.run_action(studio_cmds::card_apply_on(machine, id));
    }

    pub(super) fn hollywood_card_cancel_on(&mut self, machine: Option<String>, id: &str) {
        self.studio_dialog = Some(crate::studio_dialog::StudioDialog::reason(
            format!("Cancel card {id}"),
            false,
            crate::studio_dialog::ReasonFor::CardCancel {
                machine,
                id: id.to_owned(),
            },
        ));
    }

    pub(super) fn hollywood_card_show_on(&mut self, machine: Option<String>, id: &str) {
        self.run_action(studio_cmds::card_show_on(machine, id));
    }

    /// `a` / Apply on the selected card: `backlot card apply <id>`.
    pub(super) fn hollywood_card_apply(&mut self, id: &str) {
        let machine = self.card_machine(id);
        self.hollywood_card_apply_on(machine, id);
    }

    /// `x` / Cancel on the selected card: the reason dialog asks
    /// first (§2.1 — a card cancel's reason is optional).
    pub(super) fn hollywood_card_cancel(&mut self, id: &str) {
        let machine = self.card_machine(id);
        self.hollywood_card_cancel_on(machine, id);
    }

    /// A plain key on the selected row — the Call Sheet's own
    /// dispatch (S1 §5). Returns true when the key was the row's;
    /// the footer's slot mask is tested against this answer.
    pub(crate) fn hollywood_row_key(&mut self, code: crossterm::event::KeyCode) -> bool {
        use crossterm::event::KeyCode;
        let targets = self.hollywood_targets();
        let index = self.hollywood_selected_index(&targets);
        match (targets.get(index).cloned(), code) {
            (Some(Target::Prompt(studio, role)), KeyCode::Char('y')) => {
                self.hollywood_prompt_decision(&studio, &role, true);
            }
            (Some(Target::Prompt(studio, role)), KeyCode::Char('d')) => {
                self.hollywood_prompt_decision(&studio, &role, false);
            }
            (Some(Target::Prompt(studio, role)), KeyCode::Char('p')) => {
                self.hollywood_peek(&studio, &role);
            }
            // The live row answers only while Hollywood's own prompt
            // stands: the same `approve` a role's prompt sends, on
            // Hollywood's ledger, and option 1's run-once allow. The
            // needs-you list's HOLLYWOOD row shares this target.
            (Some(Target::Hollywood | Target::HollywoodLive), KeyCode::Char('y'))
                if self.hollywood_asking() =>
            {
                self.run_action(studio_cmds::hollywood_approve(true));
            }
            (Some(Target::Hollywood | Target::HollywoodLive), KeyCode::Char('d'))
                if self.hollywood_asking() =>
            {
                self.run_action(studio_cmds::hollywood_approve(false));
            }
            (Some(Target::Hollywood | Target::HollywoodLive), KeyCode::Char('p')) => {
                self.hollywood_peek_live()
            }
            // A role's needs-you or asks row peeks the role itself; a
            // no-requirement studio peeks its producer.
            (Some(Target::Role(studio, role)), KeyCode::Char('p')) => {
                self.hollywood_peek(&studio, &role);
            }
            (Some(Target::Tell(key)), KeyCode::Char('p')) => {
                self.hollywood_peek(&key, "producer");
            }
            // A studio frame's `p`: peek the role on set — the live
            // worker the frame already names, else the producer. The
            // footer's `p peek` is no longer dead weight off prompt
            // rows. The READY row is `f`/`↵` only (S1 §4) — peek stays
            // on its frame.
            (Some(Target::Studio(key)), KeyCode::Char('p')) => {
                self.hollywood_peek_on_set(&key);
            }
            (Some(Target::Stage(machine, false)), KeyCode::Char('s')) => {
                self.hollywood_stage_start(machine);
            }
            // `s` on a studio frame whose machine's stage is down
            // starts it — the same offer the STAGE STOPPED row and
            // the frame's own state line make. The guard is the row's
            // own rule: an unreachable machine stays a check, not a
            // start.
            (Some(Target::Studio(key)), KeyCode::Char('s'))
                if crate::hollywood::conn_of(&self.view, key.machine.as_deref())
                    != crate::hollywood::Conn::Offline
                    && !self.machine_stage_running(key.machine.as_deref()) =>
            {
                self.hollywood_stage_start(key.machine.clone());
            }
            (Some(Target::Card(id)), KeyCode::Char('a')) => self.hollywood_card_apply(&id),
            (Some(Target::Card(id)), KeyCode::Char('x')) => self.hollywood_card_cancel(&id),
            (Some(Target::OutwardCard(key, id)), KeyCode::Char('a' | 'y')) => {
                self.hollywood_card_apply_on(key.machine, &id);
            }
            (Some(Target::OutwardCard(key, id)), KeyCode::Char('x' | 'd')) => {
                self.hollywood_card_cancel_on(key.machine, &id);
            }
            (Some(Target::OutwardCard(key, id)), KeyCode::Char('s')) => {
                self.hollywood_card_show_on(key.machine, &id);
            }
            // `s` is stage-start only (R3-M11) — a card's read is
            // `⏎`, so no card arm names `s` anymore.
            (Some(Target::Ready(key)), KeyCode::Char('f')) => {
                self.hollywood.confirm = Some(Confirm::Finish(key));
            }
            // A studio frame's `f`: arm the same finish the READY row
            // arms, but only on the studio that is `ready_to_finish`.
            // A frame and its READY row are one studio, so the chips
            // and the frame answer alike; a finished chip, an
            // open-task studio and the new-studio frame stay no-ops —
            // the guard keeps them reading false, the way the footer
            // never names a press it would leave inert.
            (Some(Target::Studio(key)), KeyCode::Char('f'))
                if self.hollywood_studio_ready(&key) =>
            {
                self.hollywood.confirm = Some(Confirm::Finish(key));
            }
            _ => return false,
        }
        true
    }

    /// The armed `y/n` confirm's answer — `y` runs the action, `n`
    /// and `Esc` stand down. True when one was armed.
    pub(super) fn hollywood_confirm_key(&mut self, code: crossterm::event::KeyCode) -> bool {
        use crossterm::event::KeyCode;
        let Some(confirm) = self.hollywood.confirm.clone() else {
            return false;
        };
        match code {
            KeyCode::Char('y') => {
                self.hollywood.confirm = None;
                match confirm {
                    Confirm::Finish(key) => self.hollywood_finish(&key),
                }
            }
            KeyCode::Char('n') | KeyCode::Esc => {
                self.hollywood.confirm = None;
            }
            _ => {}
        }
        true
    }

    /// Enter on the composing `›` line: a nonempty line goes out as
    /// `backlot tell hollywood "<line>"` (D20) and the line stays in
    /// `Compose` for the next message (R3-2's Call Sheet rule — the
    /// dock's Enter rests instead); an empty Enter sends nothing and
    /// lets the line rest.
    pub(super) fn hollywood_input_submit(&mut self) {
        let line = self.hollywood.input.as_str().trim().to_string();
        if line.is_empty() {
            self.hollywood.editing = false;
            self.hollywood.history.reset_nav();
            return;
        }
        self.sync_hollywood_history();
        self.hollywood.history.record_send(&line);
        self.hollywood.input.clear();
        self.run_action(studio_cmds::tell_hollywood(&line));
    }
}
