// Derived from herdr src/client/shell/endpoint_agent_state.rs (Apache-2.0).
//! Client-local acknowledgement of authoritative per-record completions.
use crate::chrome::snapshot::ClientShellSnapshot;
use crate::chrome::status::AgentStatus;
use hc_proto::terminal::Terminal;
use std::collections::{BTreeMap, HashMap};

/// Whether the record backing a baseline/completion id is present: the
/// root id tracks `status`, named ids track `records` membership —
/// matching the ingestion-time `completions_at` contract.
fn baseline_source(term: &Terminal, id: &str) -> bool {
    if id.is_empty() {
        term.status.is_some()
    } else {
        term.records.contains_key(id)
    }
}

/// Without `record_activity` the core's per-terminal `turn` is the only
/// completion marker: it bumps when the root record leaves `working`, so
/// an idle/done root above the seen marker is an unseen completion —
/// the same fact herdr-app reads on these cores.
fn turn_completion(term: &Terminal) -> BTreeMap<String, u64> {
    let finished = term
        .status
        .as_ref()
        .is_some_and(|s| matches!(s.state.as_str(), "idle" | "done"));
    (finished && term.turn > 0)
        .then(|| (String::new(), term.turn))
        .into_iter()
        .collect()
}

/// The shared seen marker (`herdr.seen` label): the highest turn every
/// viewer acknowledges — the only seen authority without `record_activity`.
fn seen_marker(term: &Terminal) -> Option<u64> {
    term.labels.get("herdr.seen")?.parse().ok()
}

#[derive(Default)]
pub(super) struct AgentPresentation {
    initialized: bool,
    lifetime: Option<(String, String, u64)>,
    created_ms: u64,
    epoch_token: Option<u64>,
    acknowledged: HashMap<String, u64>,
    completions: BTreeMap<String, u64>,
    /// The `herdr.seen` value last folded into `acknowledged` — only on
    /// cores without `record_activity`, where a raise acknowledges
    /// further turns and a deliberate lower (mark-unread) re-arms.
    seen_label: Option<u64>,
    raw: Option<AgentStatus>,
}

impl AgentPresentation {
    pub(super) fn update(
        &mut self,
        term: &Terminal,
        raw: AgentStatus,
        epoch_token: Option<u64>,
        epoch_completions: Option<&BTreeMap<String, u64>>,
    ) -> AgentStatus {
        let lifetime = term
            .activity
            .as_ref()
            .map(|a| (a.core_id.clone(), a.epoch.clone(), a.scope));
        // A lifetime (re)establishes on first presentation, a domain or
        // incarnation change, or a new ingestion boundary token — the last
        // also catches Some(A) -> None -> Some(A) where the consumed
        // lifetime looks identical.
        let established = !self.initialized
            || self.lifetime != lifetime
            || self.created_ms != term.created_ms
            || self.epoch_token != epoch_token;
        self.completions = match &term.activity {
            Some(activity) => activity
                .records
                .iter()
                .filter_map(|(id, clock)| {
                    let status = if id.is_empty() {
                        term.status.as_ref()
                    } else {
                        term.records.get(id)
                    }?;
                    matches!(status.state.as_str(), "idle" | "done")
                        .then_some((id.clone(), clock.completion_ordinal?))
                })
                .collect(),
            None => turn_completion(term),
        };
        self.acknowledged.retain(|id, _| baseline_source(term, id));
        if established {
            self.acknowledged.clear();
            // Re-fold the shared seen marker into the fresh baseline.
            self.seen_label = None;
            // Seed only from the ingestion-time baseline: `None` means the
            // arrival was not baseline-eligible — never the current facts.
            if let Some(seen) = epoch_completions {
                self.acknowledged.extend(
                    seen.iter()
                        .filter(|(id, _)| baseline_source(term, id))
                        .map(|(id, ordinal)| (id.clone(), *ordinal)),
                );
            }
        }
        // `herdr.seen` is the shared seen authority without `record_activity`:
        // fold it on every move so a raise acknowledges and a deliberate
        // lower (mark-unread) re-arms the completion — never a sticky max.
        if term.activity.is_none() {
            let seen = seen_marker(term).filter(|_| term.status.is_some());
            if seen != self.seen_label {
                self.seen_label = seen;
                match seen {
                    Some(seen) => {
                        self.acknowledged.insert(String::new(), seen);
                    }
                    None => {
                        self.acknowledged.remove("");
                    }
                }
            }
        }
        self.initialized = true;
        self.lifetime = lifetime;
        self.created_ms = term.created_ms;
        self.epoch_token = epoch_token;
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
