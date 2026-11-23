//! The read-only and non-studio item lists behind `StudioMenu::items`
//! — a card row, a stage row, a machine row and the lot's own
//! surfaces (split from the studio/role/task lists to keep the model
//! files small).

use super::*;

impl StudioMenu {
    pub(crate) fn card_items(
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

    pub(crate) fn stage_items(
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
    pub(crate) fn machine_items(
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
            // The stage serves the whole machine — every studio on it —
            // so its stop belongs here, not on any one studio's menu.
            // `s` is free in this branch: `Start the stage` holds it
            // only while the stage is down.
            out.push(item(
                "Stop the stage…".to_owned(),
                Some('s'),
                StudioAct::StageStop,
            ));
            // S1: the stage output stays reachable without its tab
            // stealing the focus — the log opens read-only.
            out.push(item("Stage log".to_owned(), Some('l'), StudioAct::StageLog));
        }
        if self.finished_count > 0 {
            out.push(item(
                format!(
                    "{} finished studios ({})",
                    if self.finished_open { "Fold" } else { "Unfold" },
                    self.finished_count
                ),
                Some('f'),
                StudioAct::ToggleFinishedGroup,
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
    pub(crate) fn lot_items(
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

    pub(crate) fn hollywood_items(
        &self,
        seq: Option<u64>,
        item: &dyn Fn(String, Option<char>, StudioAct) -> StudioItem,
    ) -> Vec<StudioItem> {
        let read_label = match seq {
            Some(seq) => format!("Read entry #{seq}"),
            None => "Read headline".to_owned(),
        };
        vec![
            item(read_label, Some('\r'), StudioAct::HollywoodShow),
            item(
                "Focus Hollywood terminal".to_owned(),
                Some('t'),
                StudioAct::FocusHollywood,
            ),
        ]
    }

    pub(crate) fn studio_talk_items(
        &self,
        seq: u64,
        item: &dyn Fn(String, Option<char>, StudioAct) -> StudioItem,
    ) -> Vec<StudioItem> {
        let mut read = item(
            format!("Read entry #{seq}"),
            Some('\r'),
            StudioAct::ReportShow,
        );
        if self.talk_file.is_none() {
            read.dim = Some("no body".to_owned());
        }
        vec![
            read,
            item("Tell producer…".to_owned(), Some('t'), StudioAct::Tell),
            item(
                "Inspect studio sheet (v)".to_owned(),
                Some('v'),
                StudioAct::Sheet,
            ),
            item(
                "Focus producer".to_owned(),
                Some('p'),
                StudioAct::FocusTerminal,
            ),
        ]
    }
}
