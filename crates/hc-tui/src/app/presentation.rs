// Derived from herdr src/client/shell/endpoint_agent_state.rs and endpoints.rs (Apache-2.0).
//! Authoritative completions and ingestion-time receipt presentation.
use super::agent_presentation::{AgentPresentation, project_aggregate_status};
use crate::chrome::status::AgentStatus;
use crate::project::Projection;
use hc_proto::watch::View;
use std::collections::{HashMap, HashSet};

#[derive(Default)]
pub(super) struct Presentation {
    core_id: Option<String>,
    agents: HashMap<String, AgentPresentation>,
}

impl Presentation {
    pub(super) fn project(
        &mut self,
        view: &View,
        projection: &mut Projection,
        waiting: &crate::waiting::Verdicts,
    ) {
        // Do not baseline an empty placeholder before the first inventory.
        if view.generation == 0 && view.inventory.seq() == 0 {
            return;
        }
        let core = view.core.as_ref().map(|c| c.id.clone());
        if self.core_id != core {
            *self = Self {
                core_id: core,
                ..Self::default()
            };
        }
        let mut present = HashSet::new();
        for endpoint in &mut projection.endpoints {
            let Some(snapshot) = endpoint.snapshot.as_mut() else {
                continue;
            };
            for agent in &mut snapshot.agents {
                let Some(term) = view.inventory.get(&agent.pane_id) else {
                    continue;
                };
                let status = self
                    .agents
                    .entry(agent.pane_id.clone())
                    .or_default()
                    .update(
                        term,
                        agent.agent_status,
                        view.activity_receipts.epoch_token(&agent.pane_id),
                        view.activity_receipts.epoch_completions(&agent.pane_id),
                    );
                // The Waiting overlay rides the projected status: only a
                // shown Idle can present it (docs/sidebar.md).
                agent.agent_status = crate::waiting::overlay(
                    status,
                    waiting.get(&agent.pane_id).is_some_and(|v| v.waiting),
                );
                present.insert(agent.pane_id.clone());
            }
            project_aggregate_status(snapshot);
        }
        self.agents.retain(|id, _| present.contains(id));
        for endpoint in &mut projection.endpoints {
            let Some(snapshot) = endpoint.snapshot.as_mut() else {
                continue;
            };
            for agent in &mut snapshot.agents {
                agent.state_change_seq = view.activity_receipts.rank(&agent.pane_id);
            }
            endpoint.agent_recency = snapshot
                .agents
                .iter()
                .map(|a| (a.pane_id.clone(), a.state_change_seq))
                .collect();
        }
        self.refresh_active(projection);
    }

    pub(super) fn acknowledge(
        &mut self,
        projection: &mut Projection,
        pane_id: &str,
        focused: Option<bool>,
        waiting: &crate::waiting::Verdicts,
    ) -> bool {
        let Some(agent) = self.agents.get_mut(pane_id) else {
            return false;
        };
        if !agent.acknowledge(focused) {
            return false;
        }
        let status = crate::waiting::overlay(
            agent.status(),
            waiting.get(pane_id).is_some_and(|v| v.waiting),
        );
        for endpoint in &mut projection.endpoints {
            if let Some(snapshot) = endpoint.snapshot.as_mut() {
                if let Some(agent) = snapshot.agents.iter_mut().find(|a| a.pane_id == pane_id) {
                    agent.agent_status = status;
                }
                project_aggregate_status(snapshot);
            }
        }
        self.refresh_active(projection);
        true
    }

    /// The pane's presentation status without the Waiting overlay —
    /// the verdict re-derives the shown value from this base.
    pub(super) fn pane_status(&self, pane_id: &str) -> Option<AgentStatus> {
        self.agents.get(pane_id).map(AgentPresentation::status)
    }

    pub(super) fn refresh_active(&self, projection: &mut Projection) {
        if let Some(snapshot) = projection
            .endpoints
            .iter()
            .find(|e| e.endpoint_id == projection.active_endpoint_id)
            .and_then(|e| e.snapshot.as_deref())
        {
            projection.snapshot = snapshot.clone();
        }
    }
}

#[cfg(test)]
#[path = "presentation_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "presentation_epoch_tests.rs"]
mod epoch_tests;

#[cfg(test)]
#[path = "presentation_reporter_tests.rs"]
mod current_reporter_tests;

#[cfg(test)]
#[path = "presentation_official_tests.rs"]
mod official_tests;
