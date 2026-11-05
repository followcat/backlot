// Derived from herdr src/client/shell/endpoint_agent_state.rs (Apache-2.0).
//! Adapt observed core state changes into Source's endpoint-local sequence.
use std::collections::HashMap;

use hc_proto::watch::View;

use super::agent_presentation::EndpointAgentPresentation;
use crate::chrome::snapshot::ClientEndpointId;
use crate::chrome::status::AgentStatus;
use crate::project::Projection;

struct Observed {
    created_ms: u64,
    state: AgentStatus,
    turn: u64,
    sequence: u64,
}

#[derive(Default)]
pub(super) struct Presentation {
    core_id: Option<String>,
    sequence: u64,
    observed: HashMap<String, Observed>,
    endpoints: HashMap<ClientEndpointId, EndpointAgentPresentation>,
}

impl Presentation {
    pub(super) fn project(&mut self, view: &View, projection: &mut Projection) {
        let core = view.core.as_ref().map(|core| core.id.clone());
        if self.core_id != core {
            *self = Self {
                core_id: core,
                ..Self::default()
            };
        }
        self.observed
            .retain(|id, _| view.inventory.get(id).is_some());
        for endpoint in &mut projection.endpoints {
            let Some(snapshot) = endpoint.snapshot.as_mut() else {
                continue;
            };
            // CoreInfo.id survives a transport reconnect. A terminal's created_ms
            // below separately invalidates a reused address's client state.
            snapshot.boot_id = format!(
                "{}:{:?}",
                self.core_id.as_deref().unwrap_or("pending"),
                endpoint.endpoint_id
            );
            let presentation = self
                .endpoints
                .entry(endpoint.endpoint_id.clone())
                .or_default();
            let mut order: Vec<usize> = (0..snapshot.agents.len()).collect();
            order.sort_by_key(|&index| {
                let agent = &snapshot.agents[index];
                let term = view.inventory.get(&agent.pane_id);
                (term.and_then(|term| term.status_ms).unwrap_or(0), index)
            });
            for index in order {
                let agent = &mut snapshot.agents[index];
                let Some(term) = view.inventory.get(&agent.pane_id) else {
                    continue;
                };
                let changed = self.observed.get(&agent.pane_id).is_none_or(|old| {
                    old.created_ms != term.created_ms
                        || old.state != agent.agent_status
                        || old.turn != term.turn
                });
                if changed {
                    let completed =
                        matches!(agent.agent_status, AgentStatus::Idle | AgentStatus::Done)
                            && self.observed.get(&agent.pane_id).is_some_and(|old| {
                                old.created_ms == term.created_ms
                                    && (term.turn > old.turn
                                        || (agent.agent_status == AgentStatus::Done
                                            && old.state != AgentStatus::Done))
                            });
                    if self
                        .observed
                        .get(&agent.pane_id)
                        .is_some_and(|old| old.created_ms != term.created_ms)
                    {
                        presentation.forget(&agent.pane_id);
                    }
                    self.sequence += 1;
                    if completed {
                        presentation.complete(&agent.pane_id, self.sequence);
                    }
                    self.observed.insert(
                        agent.pane_id.clone(),
                        Observed {
                            created_ms: term.created_ms,
                            state: agent.agent_status,
                            turn: term.turn,
                            sequence: self.sequence,
                        },
                    );
                }
                agent.state_change_seq = self.observed[&agent.pane_id].sequence;
            }
            presentation.project_snapshot_for_generation(snapshot);
            endpoint.agent_recency = snapshot
                .agents
                .iter()
                .map(|agent| (agent.pane_id.clone(), agent.state_change_seq))
                .collect();
        }
        self.refresh_active(projection);
    }

    pub(super) fn acknowledge(
        &mut self,
        projection: &mut Projection,
        pane_id: &str,
        focused: Option<bool>,
    ) -> bool {
        let mut changed = false;
        for endpoint in &mut projection.endpoints {
            if let Some(snapshot) = endpoint.snapshot.as_mut()
                && let Some(presentation) = self.endpoints.get_mut(&endpoint.endpoint_id)
            {
                changed |= presentation.acknowledge_pane(snapshot, pane_id, focused);
            }
        }
        self.refresh_active(projection);
        changed
    }

    fn refresh_active(&self, projection: &mut Projection) {
        if let Some(snapshot) = projection
            .endpoints
            .iter()
            .find(|endpoint| endpoint.endpoint_id == projection.active_endpoint_id)
            .and_then(|endpoint| endpoint.snapshot.as_deref())
        {
            projection.snapshot = snapshot.clone();
        }
    }
}

#[cfg(test)]
#[path = "presentation_tests.rs"]
mod tests;
