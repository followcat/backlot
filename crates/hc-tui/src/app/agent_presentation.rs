// Derived from herdr src/client/shell/endpoint_agent_state.rs (Apache-2.0).
//! Client-local acknowledgement of authoritative per-record completions.
use crate::chrome::snapshot::ClientShellSnapshot;
use crate::chrome::status::AgentStatus;
use hc_proto::terminal::Terminal;
use std::collections::{BTreeMap, HashMap};

#[derive(Default)]
pub(super) struct AgentPresentation {
    initialized: bool,
    lifetime: Option<(String, String, u64)>,
    created_ms: u64,
    acknowledged: HashMap<String, u64>,
    completions: BTreeMap<String, u64>,
    raw: Option<AgentStatus>,
}

impl AgentPresentation {
    pub(super) fn update(
        &mut self,
        term: &Terminal,
        raw: AgentStatus,
        baseline: bool,
    ) -> AgentStatus {
        let lifetime = term
            .activity
            .as_ref()
            .map(|a| (a.core_id.clone(), a.epoch.clone(), a.scope));
        let reset =
            self.initialized && (self.lifetime != lifetime || self.created_ms != term.created_ms);
        self.completions = term
            .activity
            .as_ref()
            .into_iter()
            .flat_map(|a| &a.records)
            .filter_map(|(id, clock)| {
                let status = if id.is_empty() {
                    term.status.as_ref()
                } else {
                    term.records.get(id)
                }?;
                matches!(status.state.as_str(), "idle" | "done")
                    .then_some((id.clone(), clock.completion_ordinal?))
            })
            .collect();
        self.acknowledged
            .retain(|id, _| id.is_empty() || term.records.contains_key(id));
        if reset || baseline {
            self.acknowledged.clear();
            self.acknowledged.extend(self.completions.clone());
        }
        self.initialized = true;
        self.lifetime = lifetime;
        self.created_ms = term.created_ms;
        self.raw = Some(raw);
        self.status()
    }

    pub(super) fn status(&self) -> AgentStatus {
        match self.raw.unwrap_or(AgentStatus::Unknown) {
            AgentStatus::Idle | AgentStatus::Done => {
                if self.completions.iter().any(|(id, ordinal)| {
                    self.acknowledged.get(id).is_none_or(|seen| seen < ordinal)
                }) {
                    AgentStatus::Done
                } else {
                    AgentStatus::Idle
                }
            }
            status => status,
        }
    }

    // Called only by the existing composed, fresh, visible-pane render gate.
    // No ordinal can establish an output-consumption fence across connections.
    pub(super) fn acknowledge(&mut self, outer_focused: Option<bool>) -> bool {
        if outer_focused == Some(false) {
            return false;
        }
        let mut changed = false;
        for (id, ordinal) in &self.completions {
            if self.acknowledged.get(id).is_none_or(|seen| seen < ordinal) {
                self.acknowledged.insert(id.clone(), *ordinal);
                changed = true;
            }
        }
        changed
    }
}

pub(super) fn project_aggregate_status(snapshot: &mut ClientShellSnapshot) {
    for tab in &mut snapshot.tabs {
        if let Some(status) = snapshot
            .agents
            .iter()
            .filter(|a| a.tab_id == tab.tab_id)
            .map(|a| a.agent_status)
            .max_by_key(|s| crate::chrome::status::status_priority(*s))
        {
            tab.agent_status = status;
        }
    }
    for workspace in &mut snapshot.workspaces {
        if let Some(status) = snapshot
            .agents
            .iter()
            .filter(|a| a.workspace_id == workspace.workspace_id)
            .map(|a| a.agent_status)
            .max_by_key(|s| crate::chrome::status::status_priority(*s))
        {
            workspace.agent_status = status;
        }
    }
}
