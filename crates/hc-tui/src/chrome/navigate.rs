// Derived from herdr src/client/shell/workspace_navigation.rs (Apache-2.0).

use std::collections::{HashMap, HashSet};

use super::sidebar_render::workspace_entries;
use super::snapshot::{ClientEndpointId, ClientEndpointStatus, ClientShellEndpoint};

/// A client-only preview. Snapshot identity prevents Enter from using a reused
/// workspace ID.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WorkspaceNavigationTarget {
    pub endpoint_id: ClientEndpointId,
    pub workspace_id: String,
    pub(crate) boot_id: String,
    pub(crate) generation: Option<u64>,
}

impl WorkspaceNavigationTarget {
    pub(crate) fn matches(&self, endpoint_id: &ClientEndpointId, workspace_id: &str) -> bool {
        &self.endpoint_id == endpoint_id && self.workspace_id == workspace_id
    }
}

pub(crate) fn navigation_target(
    endpoints: &[ClientShellEndpoint],
    endpoint_id: &ClientEndpointId,
    workspace_id: &str,
) -> Option<WorkspaceNavigationTarget> {
    let endpoint = endpoints
        .iter()
        .find(|entry| &entry.endpoint_id == endpoint_id)?;
    let snapshot = endpoint.snapshot.as_deref()?;
    Some(WorkspaceNavigationTarget {
        endpoint_id: endpoint_id.clone(),
        workspace_id: workspace_id.to_owned(),
        boot_id: snapshot.boot_id.clone(),
        generation: endpoint.snapshot_generation,
    })
}

pub(crate) fn focused_navigation_target(
    endpoints: &[ClientShellEndpoint],
    active_endpoint_id: &ClientEndpointId,
) -> Option<WorkspaceNavigationTarget> {
    let endpoint = endpoints
        .iter()
        .find(|entry| &entry.endpoint_id == active_endpoint_id)?;
    let workspace_id = endpoint
        .snapshot
        .as_deref()?
        .focused_workspace_id
        .as_deref()?;
    navigation_target(endpoints, active_endpoint_id, workspace_id)
}

pub(crate) fn navigation_target_valid(
    endpoints: &[ClientShellEndpoint],
    target: &WorkspaceNavigationTarget,
) -> bool {
    endpoints.iter().any(|endpoint| {
        endpoint.endpoint_id == target.endpoint_id
            && endpoint.status == ClientEndpointStatus::Online
            && endpoint.snapshot_generation == target.generation
            && endpoint.snapshot.as_deref().is_some_and(|snapshot| {
                snapshot.boot_id == target.boot_id
                    && snapshot
                        .workspaces
                        .iter()
                        .any(|workspace| workspace.workspace_id == target.workspace_id)
            })
    })
}

/// Ordered navigation targets, matching `move_navigate_workspace`'s entry
/// enumeration: online endpoints only; the collapsed sidebar uses flat
/// workspace order, the expanded sidebar honors grouped/collapsed worktrees,
/// and mobile ignores collapsed groups.
pub(crate) fn workspace_navigation_targets(
    endpoints: &[ClientShellEndpoint],
    sidebar_collapsed: bool,
    mobile: bool,
    surface_available: bool,
    collapsed_groups: &HashSet<String>,
    remote_collapsed_groups: &HashMap<ClientEndpointId, HashSet<String>>,
) -> Vec<WorkspaceNavigationTarget> {
    let empty_collapsed_groups = HashSet::new();
    let mut targets = Vec::new();
    for endpoint in endpoints {
        if endpoint.status != ClientEndpointStatus::Online {
            continue;
        }
        let Some(snapshot) = endpoint.snapshot.as_deref() else {
            continue;
        };
        let entries = if sidebar_collapsed && !mobile && surface_available {
            snapshot
                .workspaces
                .iter()
                .enumerate()
                .map(|(index, _)| super::workspace::WorkspaceEntry {
                    index,
                    indented: false,
                    last_child: false,
                })
                .collect()
        } else {
            let groups = if mobile && surface_available {
                &empty_collapsed_groups
            } else if endpoint.endpoint_id.is_local() {
                collapsed_groups
            } else {
                remote_collapsed_groups
                    .get(&endpoint.endpoint_id)
                    .unwrap_or(&empty_collapsed_groups)
            };
            workspace_entries(snapshot, groups)
        };
        for entry in entries {
            targets.push(WorkspaceNavigationTarget {
                endpoint_id: endpoint.endpoint_id.clone(),
                workspace_id: snapshot.workspaces[entry.index].workspace_id.clone(),
                boot_id: snapshot.boot_id.clone(),
                generation: endpoint.snapshot_generation,
            });
        }
    }
    targets
}

/// `move_navigate_workspace` selection step: wrap on desktop, clamp on mobile.
pub(crate) fn move_index(
    targets: &[WorkspaceNavigationTarget],
    current: Option<&WorkspaceNavigationTarget>,
    delta: isize,
    mobile: bool,
) -> Option<usize> {
    if targets.is_empty() {
        return None;
    }
    let current = current.and_then(|selected| targets.iter().position(|t| t == selected));
    Some(match current {
        Some(current) if mobile => {
            (current as isize + delta).clamp(0, targets.len() as isize - 1) as usize
        }
        Some(current) => (current as isize + delta).rem_euclid(targets.len() as isize) as usize,
        None if delta < 0 => targets.len() - 1,
        None => 0,
    })
}
