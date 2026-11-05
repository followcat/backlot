// Derived from herdr src/client/shell/endpoint_agent_state.rs (Apache-2.0).
//! Source acknowledgement/completion policy; hc has no endpoint completion map.
//! Observed Working-to-idle transitions use the Source fallback path.
use crate::chrome::snapshot::{ClientShellAgent, ClientShellSnapshot};
use crate::chrome::status::AgentStatus;
use std::collections::{HashMap, HashSet};

#[derive(Default)]
pub(super) struct EndpointAgentPresentation {
    boot_id: Option<String>,
    acknowledged: HashMap<String, u64>,
    completed: HashMap<String, u64>,
    working: HashSet<String>,
}
impl EndpointAgentPresentation {
    pub(super) fn project_snapshot_for_generation(&mut self, snapshot: &mut ClientShellSnapshot) {
        if self.boot_id.as_deref() != Some(snapshot.boot_id.as_str()) {
            self.boot_id = Some(snapshot.boot_id.clone());
            self.acknowledged.clear();
            self.completed.clear();
            self.working.clear();
            self.acknowledged.extend(
                snapshot
                    .agents
                    .iter()
                    .map(|agent| (agent.pane_id.clone(), agent.state_change_seq)),
            );
        }
        let pane_ids: HashSet<&str> = snapshot
            .agents
            .iter()
            .map(|agent| agent.pane_id.as_str())
            .collect();
        self.acknowledged
            .retain(|pane_id, _| pane_ids.contains(pane_id.as_str()));
        self.completed
            .retain(|pane_id, _| pane_ids.contains(pane_id.as_str()));
        self.working
            .retain(|pane_id| pane_ids.contains(pane_id.as_str()));
        for agent in &mut snapshot.agents {
            match agent.agent_status {
                AgentStatus::Working => {
                    self.working.insert(agent.pane_id.clone());
                    self.completed.remove(&agent.pane_id);
                }
                AgentStatus::Blocked => {
                    self.completed.remove(&agent.pane_id);
                }
                AgentStatus::Idle | AgentStatus::Done => {
                    let observed_work = self.working.remove(&agent.pane_id);
                    let completed = observed_work
                        || self.completed.get(&agent.pane_id) == Some(&agent.state_change_seq);
                    if completed {
                        self.completed
                            .insert(agent.pane_id.clone(), agent.state_change_seq);
                    } else {
                        self.completed.remove(&agent.pane_id);
                    }
                }
                _ => {
                    self.working.remove(&agent.pane_id);
                    self.completed.remove(&agent.pane_id);
                }
            }
            agent.agent_status = self.projected_status(agent);
        }
        project_aggregate_status(snapshot);
    }

    pub(super) fn complete(&mut self, pane_id: &str, sequence: u64) {
        self.completed.insert(pane_id.to_string(), sequence);
    }

    pub(super) fn forget(&mut self, pane_id: &str) {
        self.acknowledged.remove(pane_id);
        self.completed.remove(pane_id);
        self.working.remove(pane_id);
    }

    pub(super) fn baseline(&mut self, pane_id: &str, sequence: u64) {
        self.acknowledged.insert(pane_id.to_string(), sequence);
    }

    pub(super) fn acknowledge_pane(
        &mut self,
        snapshot: &mut ClientShellSnapshot,
        pane_id: &str,
        outer_focused: Option<bool>,
    ) -> bool {
        if outer_focused == Some(false)
            || self.boot_id.as_deref() != Some(snapshot.boot_id.as_str())
        {
            return false;
        }
        let Some(agent) = snapshot
            .agents
            .iter()
            .find(|agent| agent.pane_id == pane_id)
        else {
            return false;
        };
        let acknowledged = self.acknowledged.entry(agent.pane_id.clone()).or_default();
        if *acknowledged >= agent.state_change_seq {
            return false;
        }
        *acknowledged = agent.state_change_seq;
        for agent in &mut snapshot.agents {
            agent.agent_status = self.projected_status(agent);
        }
        project_aggregate_status(snapshot);
        true
    }

    pub(super) fn seen(&self, agent: &ClientShellAgent) -> bool {
        self.completed.get(&agent.pane_id).is_none_or(|completion| {
            self.acknowledged
                .get(&agent.pane_id)
                .is_some_and(|sequence| sequence >= completion)
        })
    }

    fn projected_status(&self, agent: &ClientShellAgent) -> AgentStatus {
        match agent.agent_status {
            AgentStatus::Idle | AgentStatus::Done => {
                if self.seen(agent) {
                    AgentStatus::Idle
                } else {
                    AgentStatus::Done
                }
            }
            status => status,
        }
    }
}

fn project_aggregate_status(snapshot: &mut ClientShellSnapshot) {
    for tab in &mut snapshot.tabs {
        if let Some(status) = snapshot
            .agents
            .iter()
            .filter(|agent| agent.tab_id == tab.tab_id)
            .map(|agent| agent.agent_status)
            .max_by_key(|status| crate::chrome::status::status_priority(*status))
        {
            tab.agent_status = status;
        }
    }
    for workspace in &mut snapshot.workspaces {
        if let Some(status) = snapshot
            .agents
            .iter()
            .filter(|agent| agent.workspace_id == workspace.workspace_id)
            .map(|agent| agent.agent_status)
            .max_by_key(|status| crate::chrome::status::status_priority(*status))
        {
            workspace.agent_status = status;
        }
    }
}
