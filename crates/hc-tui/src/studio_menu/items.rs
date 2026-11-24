//! The studio, role and task item lists behind `StudioMenu::items` —
//! the §1 rows in the spec's order (split out to keep the model file
//! small; the card/stage/machine/lot lists live in `items_views`).

use backlot_model::state::TaskStatus;

use super::*;

impl StudioMenu {
    /// The spec §1 item lists, in order. The dim-or-hide rule: an
    /// item whose target is simply absent (no terminal, no task, no
    /// attention, no saved session) is dimmed with its reason; an
    /// item for a different state entirely (Resume on a running
    /// task, Start on a live stage, the current priority) is
    /// hidden — the menu never offers a no-op press.
    pub(crate) fn items(&self) -> Vec<StudioItem> {
        let item = |label: String, key: Option<char>, act: StudioAct| StudioItem {
            label,
            key,
            dim: None,
            teal: false,
            act,
        };
        let mut out = match self.kind {
            MenuKind::Studio => self.studio_items(&item),
            MenuKind::Role => self.role_items(&item),
            MenuKind::Prompt => self.prompt_items(&item),
            MenuKind::DirPrompt => self.dir_prompt_items(&item),
            MenuKind::Closed => self.closed_items(&item),
            MenuKind::Task => self.task_items(&item),
            MenuKind::Card => self.card_items(&item),
            MenuKind::Stage => self.stage_items(&item),
            MenuKind::Lot => self.lot_items(&item),
            MenuKind::Machine => self.machine_items(&item),
            MenuKind::Hollywood(seq) => self.hollywood_items(seq, &item),
            MenuKind::StudioTalk(seq) => self.studio_talk_items(seq, &item),
        };
        // An unreachable machine dims every row that runs a command —
        // Focus/Peek/sheet opens stay live: they read what is already
        // published. A Machine row's connect/rename/remove are local
        // endpoint ops — the repair path a dead machine needs (U6c).
        if self.offline {
            for it in &mut out {
                match it.act {
                    StudioAct::FocusTerminal
                    | StudioAct::Peek
                    | StudioAct::Sheet
                    | StudioAct::TaskSheet
                    | StudioAct::CardShow
                    | StudioAct::MachineRename
                    | StudioAct::MachineRemove
                    | StudioAct::MachineAdd
                    | StudioAct::StageLog => {}
                    _ => {
                        it.dim.get_or_insert_with(|| "unreachable".to_owned());
                    }
                }
            }
        }
        out
    }

    fn studio_items(
        &self,
        item: &dyn Fn(String, Option<char>, StudioAct) -> StudioItem,
    ) -> Vec<StudioItem> {
        if self.finished {
            return self.finished_studio_items(item);
        }
        let mut out = vec![
            item(
                "Focus producer".to_owned(),
                Some('\r'),
                StudioAct::FocusTerminal,
            ),
            item(
                "Inspect studio sheet (v)".to_owned(),
                Some('v'),
                StudioAct::Sheet,
            ),
            item(
                "Revise requirement…".to_owned(),
                Some('r'),
                StudioAct::Revise,
            ),
            item("Tell producer…".to_owned(), Some('t'), StudioAct::Tell),
        ];
        // The stage serves its whole machine, so its stop lives on the
        // machine row's menu; a studio only offers the start that
        // unblocks it.
        if self.stage_down {
            out.push(item(
                "Start stage".to_owned(),
                Some('s'),
                StudioAct::StageStart,
            ));
        }
        // A live studio keeps Finish even with tasks open: the command
        // supplies the refusal reason, so the row is never pre-dimmed.
        let mut finish = item("Finish studio…".to_owned(), Some('f'), StudioAct::Finish);
        finish.teal = self.ready;
        out.push(finish);
        out.push(item(
            "Actor records".to_owned(),
            Some('a'),
            StudioAct::Records,
        ));
        out.push(item(
            "Delete studio…".to_owned(),
            Some('d'),
            StudioAct::DeleteStudio,
        ));
        out
    }

    /// A finished studio's rows: the studio ended and every role was
    /// released, so a dead `Finish studio…` is gone (a dimmed row
    /// looks pressable but rejects "already finished"). `Reopen
    /// producer` takes Finish's place — the fixed seat reopens the
    /// same way a closed person's own row does. When the producer
    /// came back live the live menu's own `Focus producer` stands in
    /// its place: the seat is already open, so the row lands on its
    /// terminal instead of offering a reopen it does not need (A12).
    /// The rest of the set stands: the sheet, the records and the
    /// requirement read the ledger, the stage still starts and stops,
    /// and `tell` still queues mail for a person reopened later.
    fn finished_studio_items(
        &self,
        item: &dyn Fn(String, Option<char>, StudioAct) -> StudioItem,
    ) -> Vec<StudioItem> {
        let mut out = vec![if self
            .role
            .as_ref()
            .and_then(MenuRole::live_terminal)
            .is_some()
        {
            item(
                "Focus producer".to_owned(),
                Some('\r'),
                StudioAct::FocusTerminal,
            )
        } else {
            item(
                "Reopen producer (o)".to_owned(),
                Some('o'),
                StudioAct::Reopen,
            )
        }];
        out.push(item(
            "Inspect studio sheet (v)".to_owned(),
            Some('v'),
            StudioAct::Sheet,
        ));
        out.push(item(
            "Revise requirement…".to_owned(),
            Some('r'),
            StudioAct::Revise,
        ));
        out.push(item(
            "Tell producer…".to_owned(),
            Some('t'),
            StudioAct::Tell,
        ));
        if self.stage_down {
            out.push(item(
                "Start stage".to_owned(),
                Some('s'),
                StudioAct::StageStart,
            ));
        }
        out.push(item(
            "Actor records".to_owned(),
            Some('a'),
            StudioAct::Records,
        ));
        out.push(item(
            "Delete studio…".to_owned(),
            Some('d'),
            StudioAct::DeleteStudio,
        ));
        out
    }

    fn role_items(
        &self,
        item: &dyn Fn(String, Option<char>, StudioAct) -> StudioItem,
    ) -> Vec<StudioItem> {
        let role = self.role.as_ref().expect("a role row's menu");
        let mut out = Vec::new();
        let mut focus = item(
            "Focus terminal".to_owned(),
            Some('\r'),
            StudioAct::FocusTerminal,
        );
        if role.terminal.is_none() {
            focus.dim = Some("no terminal".to_owned());
        }
        out.push(focus);
        out.push(item(
            "Peek screen (p)".to_owned(),
            Some('p'),
            StudioAct::Peek,
        ));
        out.push(item(
            format!("Tell {}…", role.name),
            Some('t'),
            StudioAct::Tell,
        ));
        if let Some(task) = &role.task {
            out.push(item(
                format!("Inspect task {task} (v)"),
                Some('v'),
                StudioAct::TaskSheet,
            ));
        }
        out.extend(role_lifecycle_items(item, role, true));
        out
    }

    fn prompt_items(
        &self,
        item: &dyn Fn(String, Option<char>, StudioAct) -> StudioItem,
    ) -> Vec<StudioItem> {
        let role = self.role.as_ref().expect("a prompt row's menu");
        // A runner that cannot answer its own prompt sends nothing for
        // `y`/`d`, so its menu never offers them — peek and focus stay.
        let mut out = if !role.can_answer_prompt {
            vec![
                item("Peek screen (p)".to_owned(), Some('p'), StudioAct::Peek),
                item(
                    "Focus terminal".to_owned(),
                    Some('\r'),
                    StudioAct::FocusTerminal,
                ),
            ]
        } else {
            vec![
                item(
                    "Allow prompt (y)".to_owned(),
                    Some('y'),
                    StudioAct::Decide(true),
                ),
                item(
                    "Deny prompt (d)".to_owned(),
                    Some('d'),
                    StudioAct::Decide(false),
                ),
                item("Peek screen (p)".to_owned(), Some('p'), StudioAct::Peek),
                item(
                    "Focus terminal".to_owned(),
                    Some('\r'),
                    StudioAct::FocusTerminal,
                ),
            ]
        };
        out.extend(role_lifecycle_items(item, role, false));
        out.extend(self.attention_items(item, role));
        out
    }

    fn dir_prompt_items(
        &self,
        item: &dyn Fn(String, Option<char>, StudioAct) -> StudioItem,
    ) -> Vec<StudioItem> {
        let role = self.role.as_ref().expect("a dir-prompt row's menu");
        let mut out = vec![
            item("Peek screen (p)".to_owned(), Some('p'), StudioAct::Peek),
            item(
                "Focus terminal".to_owned(),
                Some('\r'),
                StudioAct::FocusTerminal,
            ),
        ];
        if let Some(task) = &role.task {
            out.push(item(
                format!("Inspect task {task} (v)"),
                Some('v'),
                StudioAct::TaskSheet,
            ));
        }
        out.push(item(
            "Answer prompt myself: Allow…".to_owned(),
            Some('y'),
            StudioAct::DirOverride(true),
        ));
        out.push(item(
            "Answer prompt myself: Deny…".to_owned(),
            Some('d'),
            StudioAct::DirOverride(false),
        ));
        out.extend(role_lifecycle_items(item, role, false));
        out.extend(self.attention_items(item, role));
        out
    }

    /// The `Clear stale attention #<seq>…` row — a prompt always has
    /// one; on a closed role it rides `attention_seq` when open.
    fn attention_items(
        &self,
        item: &dyn Fn(String, Option<char>, StudioAct) -> StudioItem,
        role: &MenuRole,
    ) -> Vec<StudioItem> {
        match role.attention_seq {
            Some(seq) => vec![item(
                format!("Clear stale attention #{seq}…"),
                Some('x'),
                StudioAct::ClearAttention,
            )],
            None => Vec::new(),
        }
    }

    fn closed_items(
        &self,
        item: &dyn Fn(String, Option<char>, StudioAct) -> StudioItem,
    ) -> Vec<StudioItem> {
        let role = self.role.as_ref().expect("a closed row's menu");
        let mut out = Vec::new();
        // `o` reopens a resumable role; a fixed seat (the producer)
        // reopens anyway — an unresumable closed seat dims instead.
        let fixed = role.role == "producer" || !role.role.starts_with("actor");
        let mut reopen = item(
            format!("Reopen {} (o)", role.name),
            Some('o'),
            StudioAct::Reopen,
        );
        if !(role.resumable || fixed) {
            reopen.dim = Some("no saved session".to_owned());
        }
        out.push(reopen);
        if let Some(task) = &role.task {
            out.push(item(
                format!("Inspect task {task} (v)"),
                Some('v'),
                StudioAct::TaskSheet,
            ));
        }
        out.extend(self.attention_items(item, role));
        out
    }

    fn task_items(
        &self,
        item: &dyn Fn(String, Option<char>, StudioAct) -> StudioItem,
    ) -> Vec<StudioItem> {
        let task = self.task.as_ref().expect("a task row's menu");
        let mut out = vec![item(
            "Inspect deliveries & reviews (v)".to_owned(),
            Some('v'),
            StudioAct::TaskSheet,
        )];
        use backlot_model::event::Priority;
        for (label, key, level) in [
            ("High", '+', Priority::High),
            ("Normal", 'n', Priority::Normal),
            ("Low", '-', Priority::Low),
        ] {
            let mut it = item(
                format!("Priority: {label} ({key})"),
                Some(key),
                StudioAct::Priority(level),
            );
            if task.priority == level {
                it.dim = Some("current".to_owned());
            }
            out.push(it);
        }
        match task.status {
            TaskStatus::Paused => {
                out.push(item(
                    "Resume task…".to_owned(),
                    Some('r'),
                    StudioAct::TaskVerb("resume"),
                ));
            }
            TaskStatus::Accepted | TaskStatus::Abandoned | TaskStatus::TakenOver => {}
            _ => {
                out.push(item(
                    "Pause task…".to_owned(),
                    Some('p'),
                    StudioAct::TaskVerb("pause"),
                ));
            }
        }
        out.push(item(
            "Take over task…".to_owned(),
            Some('k'),
            StudioAct::TaskVerb("takeover"),
        ));
        if !matches!(
            task.status,
            TaskStatus::Abandoned | TaskStatus::Accepted | TaskStatus::TakenOver
        ) {
            out.push(item(
                "Abandon task…".to_owned(),
                Some('a'),
                StudioAct::TaskVerb("abandon"),
            ));
        }
        out
    }
}

/// A live person's lifecycle rows (L1, L2, L3): `Restart` reloads the
/// seat in place; a fixed seat — producer, director, writer — adds
/// `Recast…` and never offers the `Close` that `close` refuses on it.
fn role_lifecycle_items(
    item: &dyn Fn(String, Option<char>, StudioAct) -> StudioItem,
    role: &MenuRole,
    allow_close: bool,
) -> Vec<StudioItem> {
    let mut out = vec![item(
        format!("Restart {}", role.name),
        Some('r'),
        StudioAct::Restart,
    )];
    if role.fixed_seat() {
        out.push(item("Recast…".to_owned(), Some('e'), StudioAct::Recast));
    } else if allow_close {
        // An actor or held seat still closes; a fixed seat cannot.
        out.push(item(
            format!("Close {}…", role.name),
            Some('c'),
            StudioAct::Close,
        ));
    }
    out
}
