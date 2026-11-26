//! W1b's keyboard path into the left machines tree: the flat row list
//! the cursor walks, in the same order the expanded sidebar draws
//! (machine row, its spaces, its extra studio rows). Pure data — the
//! cursor reads it and the renderer tags the row it names, so the
//! highlight and the action always name the row the person sees.

use std::collections::{HashMap, HashSet};

use super::sidebar_render::workspace_entries_for_sidebar;
use super::snapshot::{ClientEndpointId, ClientShellEndpoint};
use super::studio_tree::{SidebarRow, Tree};

/// Every selectable left-tree row, in draw order, for the sidebar a
/// studio-mode client draws: each endpoint's machine row, its non-studio spaces
/// (unless the endpoint is collapsed), then its extra studio rows.
/// Plain mode never builds it — herdr's tree has no backlot studio
/// rows and no cursor.
pub(crate) fn rows(
    endpoints: &[ClientShellEndpoint],
    collapsed_endpoints: &HashSet<ClientEndpointId>,
    collapsed_groups: &HashSet<String>,
    remote_collapsed_groups: &HashMap<ClientEndpointId, HashSet<String>>,
    hide_stage_space: bool,
    tree: Option<&Tree>,
) -> Vec<SidebarRow> {
    let mut out = Vec::new();
    for endpoint in endpoints {
        out.push(SidebarRow::Machine(endpoint.endpoint_id.clone()));
        if collapsed_endpoints.contains(&endpoint.endpoint_id) {
            continue;
        }
        if let Some(snapshot) = endpoint.snapshot.as_deref() {
            let groups = endpoint_groups(
                &endpoint.endpoint_id,
                collapsed_groups,
                remote_collapsed_groups,
            );
            let hidden = tree.and_then(|t| t.live_studios.get(&endpoint.endpoint_id));
            out.extend(
                workspace_entries_for_sidebar(snapshot, groups, hide_stage_space, hidden)
                    .into_iter()
                    .map(|entry| SidebarRow::Workspace {
                        endpoint: endpoint.endpoint_id.clone(),
                        workspace_id: snapshot.workspaces[entry.index].workspace_id.clone(),
                    }),
            );
        }
        if let Some(tree) = tree {
            out.extend(tree.get(&endpoint.endpoint_id).iter().cloned().map(|row| {
                SidebarRow::Studio {
                    endpoint: endpoint.endpoint_id.clone(),
                    row,
                }
            }));
        }
    }
    out
}

/// The collapsed-group set an endpoint's spaces read.
pub(crate) fn endpoint_groups<'a>(
    endpoint: &ClientEndpointId,
    collapsed_groups: &'a HashSet<String>,
    remote_collapsed_groups: &'a HashMap<ClientEndpointId, HashSet<String>>,
) -> &'a HashSet<String> {
    if endpoint.is_local() {
        collapsed_groups
    } else {
        remote_collapsed_groups
            .get(endpoint)
            .unwrap_or(collapsed_groups)
    }
}
