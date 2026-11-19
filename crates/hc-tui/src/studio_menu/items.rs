//! The per-kind item lists behind `StudioMenu::items` — the §1
//! rows in the spec's order (split out to keep the model file
//! small).

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
                    | StudioAct::MachineAdd => {}
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
        if self.stage_down {
            out.push(item(
                "Start stage".to_owned(),
                Some('s'),
                StudioAct::StageStart,
            ));
        } else {
            out.push(item(
                "Stop stage…".to_owned(),
                Some('s'),
                StudioAct::StageStop,
            ));
        }
        let mut finish = item("Finish studio…".to_owned(), Some('f'), StudioAct::Finish);
        finish.teal = self.ready;
        out.push(finish);
        out.push(item(
            "Actor records".to_owned(),
            Some('a'),
            StudioAct::Records,
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
        out.push(item(
            format!("Close {}…", role.name),
            Some('c'),
            StudioAct::Close,
        ));
        out
    }

    fn prompt_items(
        &self,
        item: &dyn Fn(String, Option<char>, StudioAct) -> StudioItem,
    ) -> Vec<StudioItem> {
        let role = self.role.as_ref().expect("a prompt row's menu");
        let mut out = vec![
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
        ];
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

    fn card_items(
        &self,
        item: &dyn Fn(String, Option<char>, StudioAct) -> StudioItem,
    ) -> Vec<StudioItem> {
        vec![
            item("Apply card (a)".to_owned(), Some('a'), StudioAct::CardApply),
            item(
                "Show card details (s)".to_owned(),
                Some('s'),
                StudioAct::CardShow,
            ),
            item(
                "Cancel card… (x)".to_owned(),
                Some('x'),
                StudioAct::CardCancel,
            ),
        ]
    }

    fn stage_items(
        &self,
        item: &dyn Fn(String, Option<char>, StudioAct) -> StudioItem,
    ) -> Vec<StudioItem> {
        if self.offline {
            return Vec::new();
        }
        vec![item(
            "Start stage".to_owned(),
            Some('s'),
            StudioAct::StageStart,
        )]
    }

    /// A machine row's menu (U6c §4): only the valid actions for
    /// the row's own state — upgrade when behind the known latest,
    /// install when the probe said `no backlot`, start vs restart
    /// the stage on its running state, `New studio here` when a
    /// `backlot` can answer, then the endpoint ops the herdr menu
    /// owned before.
    fn machine_items(
        &self,
        item: &dyn Fn(String, Option<char>, StudioAct) -> StudioItem,
    ) -> Vec<StudioItem> {
        let mut out = Vec::new();
        let installed = match &self.backlot {
            Some(MenuBacklot::Installed(version)) => Some(version.clone()),
            _ => None,
        };
        let behind = installed
            .as_deref()
            .zip(self.latest.as_deref())
            .is_some_and(|(installed, latest)| crate::machine_versions::behind(installed, latest));
        if behind {
            let latest = self.latest.clone().unwrap_or_default();
            out.push(item(
                format!("Upgrade backlot to v{latest}"),
                Some('u'),
                StudioAct::MachineInstall,
            ));
        } else if matches!(self.backlot, Some(MenuBacklot::Missing)) {
            out.push(item(
                "Install backlot".to_owned(),
                Some('i'),
                StudioAct::MachineInstall,
            ));
        }
        if installed.is_some() && self.stage_down {
            out.push(item(
                "Start the stage".to_owned(),
                Some('s'),
                StudioAct::StageStart,
            ));
        } else if installed.is_some() {
            out.push(item(
                "Restart the stage".to_owned(),
                Some('r'),
                StudioAct::StageRestart,
            ));
        }
        if installed.is_some() {
            out.push(item(
                "New studio here".to_owned(),
                Some('n'),
                StudioAct::NewStudio,
            ));
        }
        if self.machine.is_none() {
            out.push(item(
                "Add machine".to_owned(),
                Some('a'),
                StudioAct::MachineAdd,
            ));
        } else {
            out.push(item(
                "Connect machine".to_owned(),
                Some('c'),
                StudioAct::MachineConnect,
            ));
            out.push(item(
                "Rename machine".to_owned(),
                Some('e'),
                StudioAct::MachineRename,
            ));
            out.push(item(
                "Remove machine".to_owned(),
                Some('x'),
                StudioAct::MachineRemove,
            ));
        }
        out
    }

    /// The lot pill's quick menu (S5 §2.3) — lot-wide surfaces in
    /// the spec's order.
    fn lot_items(
        &self,
        item: &dyn Fn(String, Option<char>, StudioAct) -> StudioItem,
    ) -> Vec<StudioItem> {
        vec![
            item(
                "Open Call Sheet".to_owned(),
                Some('y'),
                StudioAct::CallSheet,
            ),
            item(
                "Toggle Studio Dock".to_owned(),
                Some('d'),
                StudioAct::ToggleDock,
            ),
            item("New studio…".to_owned(), Some('n'), StudioAct::NewStudio),
            item(
                "Hollywood settings…".to_owned(),
                Some('h'),
                StudioAct::HollywoodSettings,
            ),
        ]
    }
}
