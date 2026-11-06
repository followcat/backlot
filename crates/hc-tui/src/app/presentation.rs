// Derived from herdr src/client/shell/endpoint_agent_state.rs and endpoints.rs (Apache-2.0).
//! Authoritative record completions, lifetime baselines and constrained receipt order.
use super::activity_order::{ActivityOrder, Clock};
use super::agent_presentation::{AgentPresentation, project_aggregate_status};
use crate::project::Projection;
use hc_proto::watch::View;
use std::collections::{HashMap, HashSet};

#[derive(Default)]
pub(super) struct Presentation {
    core_id: Option<String>,
    epochs: HashSet<(String, String)>,
    agents: HashMap<String, AgentPresentation>,
    order: ActivityOrder,
}

impl Presentation {
    pub(super) fn project(&mut self, view: &View, projection: &mut Projection) {
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
        let epochs = view
            .inventory
            .terminals()
            .values()
            .filter_map(|t| t.activity.as_ref())
            .map(|a| (a.core_id.clone(), a.epoch.clone()))
            .collect::<HashSet<_>>();
        let new_epochs = epochs
            .difference(&self.epochs)
            .cloned()
            .collect::<HashSet<_>>();
        self.epochs.extend(epochs);
        let mut clocks = Vec::new();
        let mut present = HashSet::new();
        for endpoint in &mut projection.endpoints {
            let Some(snapshot) = endpoint.snapshot.as_mut() else {
                continue;
            };
            for agent in &mut snapshot.agents {
                let Some(term) = view.inventory.get(&agent.pane_id) else {
                    continue;
                };
                let baseline = term
                    .activity
                    .as_ref()
                    .is_some_and(|a| new_epochs.contains(&(a.core_id.clone(), a.epoch.clone())));
                agent.agent_status = self
                    .agents
                    .entry(agent.pane_id.clone())
                    .or_default()
                    .update(term, agent.agent_status, baseline);
                clocks.push((agent.pane_id.clone(), Clock::of(term)));
                present.insert(agent.pane_id.clone());
            }
            project_aggregate_status(snapshot);
        }
        self.agents.retain(|id, _| present.contains(id));
        let ranks = self.order.ranks(clocks);
        for endpoint in &mut projection.endpoints {
            let Some(snapshot) = endpoint.snapshot.as_mut() else {
                continue;
            };
            for agent in &mut snapshot.agents {
                agent.state_change_seq = ranks[&agent.pane_id];
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
    ) -> bool {
        let Some(agent) = self.agents.get_mut(pane_id) else {
            return false;
        };
        if !agent.acknowledge(focused) {
            return false;
        }
        let status = agent.status();
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

    fn refresh_active(&self, projection: &mut Projection) {
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
