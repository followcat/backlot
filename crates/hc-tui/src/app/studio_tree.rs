//! W1's App wiring for the left machines tree: it collects each
//! machine's studios from the summary and the cached `studio list
//! --json`, refreshes that cache when a stage changes or every 30 s,
//! and turns a tree row press into the row's action.
//!
//! The row model and the merge are pure (`chrome::studio_tree`,
//! `studio_index`); this file only feeds them the view and dispatches.

use std::time::Instant;

use crate::chrome::render::{StudioRowAct, StudioRowHit};
use crate::chrome::snapshot::{ClientEndpointId, ClientEndpointStatus};
use crate::chrome::studio_tree::{self, Studio, Tree};
use crate::panel::data::MachineSummary;
use crate::studio_index::{LedgerState, Machine as IndexMachine};

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

    /// Ask every machine whose stage is down (or whose document is
    /// missing) for `studio list --json`, at most once per 30 s and
    /// again whenever its stage state changed.
    pub(super) fn sync_studio_index(&mut self) {
        if self.plain || !self.studio.active {
            return;
        }
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
                    // only a studio past it has really ended, and a
                    // finished one draws no tree row.
                    if studio.state == "finished" && !studio.ready_to_finish {
                        continue;
                    }
                    studios.push(Studio::live(&studio.id));
                }
            }
            // The ledger fills the gap only where the document cannot:
            // a stage down, or no document at all.
            if !stage_running || !has_summary {
                for studio in self.studio_index.studios(machine) {
                    if studio.state == LedgerState::Finished
                        || studios.iter().any(|known| known.id == studio.name)
                    {
                        continue;
                    }
                    studios.push(Studio::live(&studio.name));
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
            let rows = studio_tree::rows(&studio_tree::Machine {
                reachable,
                stage_running,
                space_labels: &labels,
                studios: &studios,
            });
            if !rows.is_empty() {
                tree.rows.insert(endpoint.endpoint_id.clone(), rows);
            }
        }
        tree
    }

    /// A left-tree studio row was pressed: start the machine's stage.
    pub(super) fn studio_row_activated(&mut self, hit: &StudioRowHit) {
        let machine = match &hit.endpoint_id {
            ClientEndpointId::Local => None,
            ClientEndpointId::Remote(name) => Some(name.clone()),
        };
        if matches!(hit.act, StudioRowAct::StartStage) {
            self.run_action(crate::studio_cmds::stage_start(machine));
        }
    }
}
