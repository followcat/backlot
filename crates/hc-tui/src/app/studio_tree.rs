//! W1's App wiring for the left machines tree: it collects each
//! machine's studios from the summary and the cached `studio list
//! --json`, refreshes that cache when a stage changes or every 30 s,
//! turns a tree row press into the row's action, and keeps a stage
//! start from stealing the focus.
//!
//! The row model and the merge are pure (`chrome::studio_tree`,
//! `studio_index`); this file only feeds them the view and dispatches.

use std::time::Instant;

use crate::chrome::render::{StudioRowAct, StudioRowHit};
use crate::chrome::snapshot::{ClientEndpointId, ClientEndpointStatus};
use crate::chrome::studio_tree::{self, Studio, StudioState, Tree};
use crate::panel::data::MachineSummary;
use crate::studio::StudioKey;
use crate::studio_index::{LedgerState, Machine as IndexMachine};
use crate::studio_menu::StudioMenu;

use super::App;
use super::new_studio::NsReply;

impl App {
    /// The cached ledger list answered `machine` — land it and repaint.
    pub(crate) fn on_studio_list(
        &mut self,
        machine: Option<String>,
        stage_running: bool,
        out: Result<String, String>,
    ) {
        let text = out.as_deref().map_err(String::as_str);
        self.studio_index
            .note_reply(machine, stage_running, Instant::now(), text);
        self.sched.mark_dirty(Instant::now());
    }

    /// The cached archived list answered `machine`.
    pub(crate) fn on_archived_list(
        &mut self,
        machine: Option<String>,
        out: Result<String, String>,
    ) {
        let text = out.as_deref().map_err(String::as_str);
        self.studio_index
            .note_archived_reply(machine, Instant::now(), text);
        self.sched.mark_dirty(Instant::now());
    }

    /// Ask every reachable machine for its archived studios — the
    /// `Archived (n)` row's source, on the same 30 s clock as the
    /// ledger list.
    fn sync_archived_index(&mut self) {
        let reachable: Vec<Option<String>> = self
            .projection
            .endpoints
            .iter()
            .filter(|endpoint| {
                matches!(
                    endpoint.status,
                    ClientEndpointStatus::Online | ClientEndpointStatus::Attention
                )
            })
            .map(|endpoint| match &endpoint.endpoint_id {
                ClientEndpointId::Local => None,
                ClientEndpointId::Remote(name) => Some(name.clone()),
            })
            .collect();
        let due = self.studio_index.want_archived(Instant::now(), &reachable);
        if due.is_empty() {
            return;
        }
        let run = self.ns_runner();
        let tx = self.ns_tx.clone();
        for machine in due {
            let run = run.clone();
            let tx = tx.clone();
            tokio::spawn(async move {
                let out = run(
                    machine.clone(),
                    vec![
                        "studio".into(),
                        "list".into(),
                        "--archived".into(),
                        "--json".into(),
                    ],
                    None,
                )
                .await;
                let _ = tx.send(NsReply::ArchivedList { machine, out }).await;
            });
        }
    }

    /// Ask every machine whose stage is down (or whose document is
    /// missing) for `studio list --json`, at most once per 30 s and
    /// again whenever its stage state changed.
    pub(super) fn sync_studio_index(&mut self) {
        if self.plain || !self.studio.active {
            return;
        }
        self.sync_archived_index();
        let summaries: Vec<MachineSummary> = self.panel_summaries();
        let machines: Vec<IndexMachine> = self
            .projection
            .endpoints
            .iter()
            .map(|endpoint| {
                let machine = match &endpoint.endpoint_id {
                    ClientEndpointId::Local => None,
                    ClientEndpointId::Remote(name) => Some(name.as_str()),
                };
                IndexMachine {
                    machine: machine.map(str::to_owned),
                    stage_running: self.machine_stage_running(machine),
                    has_summary: summaries
                        .iter()
                        .any(|row| row.machine.as_deref() == machine),
                }
            })
            .collect();
        let due = self.studio_index.want(Instant::now(), &machines);
        if due.is_empty() {
            return;
        }
        let stage_running = machines
            .iter()
            .map(|row| (row.machine.clone(), row.stage_running))
            .collect::<std::collections::BTreeMap<_, _>>();
        let run = self.ns_runner();
        let tx = self.ns_tx.clone();
        for machine in due {
            let run = run.clone();
            let tx = tx.clone();
            let stage = stage_running.get(&machine).copied().unwrap_or(false);
            tokio::spawn(async move {
                let out = run(
                    machine.clone(),
                    vec!["studio".into(), "list".into(), "--json".into()],
                    None,
                )
                .await;
                let _ = tx
                    .send(NsReply::StudioList {
                        machine,
                        stage_running: stage,
                        out,
                    })
                    .await;
            });
        }
    }

    /// The extra rows under each machine in studio mode — `None`-like
    /// when `--plain`, where herdr's tree draws alone.
    pub(super) fn studio_sidebar_tree(&self) -> Tree {
        let mut tree = Tree::default();
        if self.plain {
            return tree;
        }
        let summaries = self.panel_summaries();
        for endpoint in &self.projection.endpoints {
            let machine = match &endpoint.endpoint_id {
                ClientEndpointId::Local => None,
                ClientEndpointId::Remote(name) => Some(name.as_str()),
            };
            let has_summary = summaries
                .iter()
                .any(|row| row.machine.as_deref() == machine);
            let stage_running = self.machine_stage_running(machine);
            let reachable = endpoint.status == ClientEndpointStatus::Online;
            let mut studios: Vec<Studio<'_>> = Vec::new();
            if let Some(summary) = summaries
                .iter()
                .find(|row| row.machine.as_deref() == machine)
            {
                for studio in &summary.summary.studios {
                    // `ready_to_finish` also reads `state: "finished"`;
                    // only a studio past it has really ended.
                    let finished = studio.state == "finished" && !studio.ready_to_finish;
                    studios.push(if finished {
                        Studio::finished(&studio.id)
                    } else {
                        Studio::live(&studio.id)
                    });
                }
            }
            // The ledger fills the gap only where the document cannot:
            // a stage down, or no document at all.
            if !stage_running || !has_summary {
                for studio in self.studio_index.studios(machine) {
                    if studios
                        .iter()
                        .any(|known| known.id == studio.name && known.state == state_of(studio))
                    {
                        continue;
                    }
                    studios.push(match studio.state {
                        LedgerState::Finished => Studio::finished(&studio.name),
                        _ => Studio::live(&studio.name),
                    });
                }
            }
            let labels: Vec<&str> = endpoint
                .snapshot
                .as_deref()
                .map(|snapshot| {
                    snapshot
                        .workspaces
                        .iter()
                        .filter(|workspace| {
                            !crate::chrome::snapshot::is_backlot_stage_space(workspace)
                        })
                        .map(|workspace| workspace.label.as_str())
                        .collect()
                })
                .unwrap_or_default();
            let finished_open = self
                .studio
                .finished_open
                .contains(&machine.map(str::to_owned));
            let archived_count = self.studio_index.archived(machine).len();
            let rows = studio_tree::rows(&studio_tree::Machine {
                reachable,
                stage_running,
                space_labels: &labels,
                studios: &studios,
                finished_open,
                archived_count,
            });
            if !rows.is_empty() {
                tree.rows.insert(endpoint.endpoint_id.clone(), rows);
            }
        }
        tree
    }

    /// Every finished studio id `machine` knows — the document's
    /// finished rows and the ledger's, without duplicates.
    pub(super) fn machine_finished_ids(&self, machine: Option<&str>) -> Vec<String> {
        let summaries = self.panel_summaries();
        let mut ids: Vec<String> = Vec::new();
        if let Some(summary) = summaries
            .iter()
            .find(|row| row.machine.as_deref() == machine)
        {
            for studio in &summary.summary.studios {
                if studio.state == "finished" && !studio.ready_to_finish {
                    ids.push(studio.id.clone());
                }
            }
        }
        let stage_running = self.machine_stage_running(machine);
        let has_summary = summaries
            .iter()
            .any(|row| row.machine.as_deref() == machine);
        if !stage_running || !has_summary {
            for studio in self.studio_index.studios(machine) {
                if studio.state == LedgerState::Finished && !ids.contains(&studio.name) {
                    ids.push(studio.name.clone());
                }
            }
        }
        ids
    }

    /// Fold or unfold a machine's `finished (n)` group — the tree
    /// row's own press and the machine menu's item share this.
    pub(super) fn toggle_finished_group(&mut self, machine: Option<String>) {
        if !self.studio.finished_open.remove(&machine) {
            self.studio.finished_open.insert(machine);
        }
        self.sched.mark_dirty(Instant::now());
    }

    /// A left-tree studio row was pressed: start the machine's stage,
    /// fold the finished group, or open a finished studio's menu.
    pub(super) fn studio_row_activated(&mut self, hit: &StudioRowHit, point: (u16, u16)) {
        let machine = match &hit.endpoint_id {
            ClientEndpointId::Local => None,
            ClientEndpointId::Remote(name) => Some(name.clone()),
        };
        match &hit.act {
            StudioRowAct::StartStage => {
                self.run_action(crate::studio_cmds::stage_start(machine));
            }
            StudioRowAct::ToggleFinished => self.toggle_finished_group(machine),
            StudioRowAct::OpenArchived => self.open_archived(machine),
            StudioRowAct::OpenFinished { id } => {
                let key = StudioKey {
                    machine,
                    id: id.clone(),
                };
                let menu = self.finished_studio_menu(&key);
                // The tree is the sidebar: its menu clamps to the frame
                // left of any beside dock, like every other sidebar menu.
                self.open_studio_menu(menu, point);
            }
        }
    }

    /// A finished studio's existing menu — from the document when it
    /// still lists the studio, from the ledger's word alone otherwise.
    pub(super) fn finished_studio_menu(&self, key: &StudioKey) -> StudioMenu {
        let machines = self.panel_summaries();
        let stage_down = !self.machine_stage_running(key.machine.as_deref());
        let offline = !self.machine_reachable(key.machine.as_deref());
        match crate::panel::data::studio(&machines, key) {
            Some(studio) => StudioMenu::studio(key, studio.studio, stage_down, offline),
            None => StudioMenu::finished_unknown(key, stage_down, offline),
        }
    }
}

/// A ledger row's tree state.
fn state_of(studio: &crate::studio_index::LedgerStudio) -> StudioState {
    match studio.state {
        LedgerState::Finished => StudioState::Finished,
        _ => StudioState::Live,
    }
}
