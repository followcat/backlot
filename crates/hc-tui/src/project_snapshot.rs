// Derived from herdr src/app/api_helpers.rs (Apache-2.0).
use super::*;

/// Build one endpoint's snapshot from its terminals. `selected` is the
/// globally selected terminal address; focus ids only land on this
/// snapshot when they belong to this endpoint.
pub(super) fn structured_snapshot(
    terms: &[&Terminal],
    selected: Option<&str>,
    generation: u64,
    revision: u64,
    app: &serde_json::Value,
    own: &crate::structure::Document,
    machine: Option<&str>,
) -> (
    ClientShellSnapshot,
    BTreeMap<String, crate::structure::TabLayout>,
) {
    let spaces = crate::structure::read(terms, app, machine);
    let term_by_id = terms
        .iter()
        .copied()
        .map(|t| (t.address.as_str(), t))
        .collect::<BTreeMap<_, _>>();
    let mut workspaces = Vec::new();
    let mut tabs = Vec::new();
    let mut panes = Vec::new();
    let mut agents = Vec::new();
    let mut agent_order = Vec::new();
    let mut layouts = BTreeMap::new();
    for (index, space) in spaces.iter().enumerate() {
        let number = index + 1;
        let workspace_id = &space.id;
        let mut ordered = space.tabs.iter().collect::<Vec<_>>();
        if let Some(order) = own.order.get(&layout_key(machine, workspace_id)) {
            ordered.sort_by_key(|tab| {
                order
                    .iter()
                    .position(|id| id == &tab.id)
                    .unwrap_or(usize::MAX)
            });
        }
        let mut tab_ids = Vec::new();
        let mut tab_number = 0usize;
        let mut workspace_status = AgentStatus::Unknown;
        let mut workspace_priority = 0u8;
        for tab in ordered {
            tab_number += 1;
            let tab_id = tab.id.clone();
            let stored = own.tabs.get(&layout_key(machine, &tab_id));
            let membership = tab
                .root
                .terminals()
                .into_iter()
                .collect::<std::collections::BTreeSet<_>>();
            let mut root = stored
                .and_then(|l| l.root.as_ref())
                .and_then(|r| r.retain(&|id| membership.contains(id)))
                .unwrap_or_else(|| tab.root.clone());
            let present = root
                .terminals()
                .into_iter()
                .collect::<std::collections::BTreeSet<_>>();
            let missing = tab
                .root
                .terminals()
                .into_iter()
                .filter(|id| !present.contains(id))
                .collect::<Vec<_>>();
            if let Some(added) = crate::structure::Tree::balanced(&missing, false) {
                root = crate::structure::Tree::split(root, added, false, 0.5);
            }
            if !root.valid() {
                root = crate::structure::Tree::balanced(&root.terminals(), false)
                    .expect("nonempty membership");
            }
            let tab_members = root
                .terminals()
                .into_iter()
                .filter_map(|id| term_by_id.get(id.as_str()).copied())
                .collect::<Vec<_>>();
            let name = stored.and_then(|l| l.name.as_deref()).unwrap_or(&tab.name);
            let tab_label = if name.is_empty() {
                tab_number.to_string()
            } else {
                clean(name)
            };
            let custom_label = stored
                .and_then(|l| l.name.as_ref())
                .map_or(tab.custom_name, |name| !name.is_empty());
            let members = root.terminals();
            let focused = selected
                .filter(|id| members.iter().any(|t| t == id))
                .map(str::to_owned)
                .or_else(|| {
                    stored
                        .and_then(|l| l.focused.clone())
                        .filter(|id| members.contains(id))
                })
                .or_else(|| members.first().cloned());
            let zoomed = stored
                .and_then(|l| l.zoomed.as_ref())
                .filter(|id| members.contains(id))
                .and_then(|_| focused.clone());
            layouts.insert(
                tab_id.clone(),
                crate::structure::TabLayout {
                    root: Some(root),
                    focused,
                    zoomed: zoomed.clone(),
                    name: stored.and_then(|l| l.name.clone()),
                    ..Default::default()
                },
            );
            tab_ids.push(tab_id.clone());
            let mut tab_status = AgentStatus::Unknown;
            let mut tab_priority = 0u8;
            for term in &tab_members {
                let priority = attention_priority(term);
                if priority > tab_priority {
                    tab_priority = priority;
                    tab_status = pane_agent_status(term);
                }
                let focused = selected == Some(term.address.as_str());
                panes.push(ClientShellPane {
                    pane_id: term.address.clone(),
                    workspace_id: workspace_id.clone(),
                    tab_id: tab_id.clone(),
                    // Source pane_metadata uses terminal.manual_label;
                    // names and reported/OSC titles are separate fields.
                    label: term
                        .labels
                        .get("herdr.title")
                        .map(|label| clean(label))
                        .filter(|label| !label.is_empty()),
                    cwd: Some(clean(&term.cwd)),
                    foreground_cwd: term.foreground.as_ref().map(|fg| clean(&fg.cwd)),
                    focused,
                    right_click_passthrough: false,
                });
                if is_agent(term) {
                    let status = pane_agent_status(term);
                    let record = effective_status(term);
                    agents.push(ClientShellAgent {
                        pane_id: term.address.clone(),
                        workspace_id: workspace_id.clone(),
                        tab_id: tab_id.clone(),
                        name: term.name.as_deref().map(clean),
                        display_agent: record.map(|s| clean(&s.app)).filter(|a| !a.is_empty()),
                        agent: record.map(|s| clean(&s.app)).filter(|a| !a.is_empty()),
                        // `title` is the reported status title;
                        // `terminal_title` is the program's OSC title —
                        // distinct slots, never swapped.
                        title: record.map(|s| clean(&s.title)).filter(|t| !t.is_empty()),
                        terminal_title: if term.title.is_empty() {
                            None
                        } else {
                            Some(clean(&term.title))
                        },
                        terminal_title_stripped: crate::chrome::title_text::stripped_terminal_title(
                            &crate::sidebar::clean(&term.title),
                        ),
                        agent_status: status,
                        // Recency follows the observed status-change
                        // chronology (`status_ms`), not the per-terminal
                        // turn counter — a quiet terminal with many past
                        // turns does not outrank a fresh report.
                        state_change_seq: term.status_ms.unwrap_or(0),
                        state_labels: Vec::new(),
                        tokens: Vec::new(),
                        focused,
                    });
                    agent_order.push(term.address.clone());
                }
            }
            if tab_priority > workspace_priority {
                workspace_priority = tab_priority;
                workspace_status = tab_status;
            }
            tabs.push(ClientShellTab {
                tab_id: tab_id.clone(),
                workspace_id: workspace_id.clone(),
                number: tab_number,
                label: tab_label.clone(),
                custom_label,
                zoomed: zoomed.is_some(),
                focused: tab_members
                    .iter()
                    .any(|t| selected == Some(t.address.as_str())),
                agent_status: tab_status,
            });
        }
        let first_tab = tab_ids.first().cloned().unwrap_or_default();
        let cwd = clean(&space.folder);
        workspaces.push(ClientShellWorkspace {
            workspace_id: workspace_id.clone(),
            active_tab_id: tabs
                .iter()
                .find(|t| t.focused && t.workspace_id == *workspace_id)
                .map(|t| t.tab_id.clone())
                .unwrap_or(first_tab),
            new_workspace_cwd: cwd,
            number,
            label: clean(&space.name),
            custom_label: space.custom_name,
            branch: None,
            git_ahead_behind: None,
            tokens: Vec::new(),
            worktree: None,
            focused: panes
                .iter()
                .any(|p| p.workspace_id == *workspace_id && p.focused),
            agent_status: workspace_status,
        });
    }

    let focused_pane_id = selected
        .filter(|address| panes.iter().any(|p| p.pane_id == *address))
        .map(str::to_string);
    let focused_workspace_id = focused_pane_id.as_ref().and_then(|id| {
        panes
            .iter()
            .find(|p| &p.pane_id == id)
            .map(|p| p.workspace_id.clone())
    });
    let focused_tab_id = focused_pane_id.as_ref().and_then(|id| {
        panes
            .iter()
            .find(|p| &p.pane_id == id)
            .map(|p| p.tab_id.clone())
    });

    (
        ClientShellSnapshot {
            boot_id: generation.to_string(),
            revision,
            focused_workspace_id,
            focused_tab_id,
            focused_pane_id,
            tab_bar_right: Vec::new(),
            tab_bar_right_separator: String::new(),
            agent_order,
            agent_view_label: None,
            workspaces,
            tabs,
            panes,
            agents,
            commands: Vec::new(),
        },
        layouts,
    )
}

#[cfg(test)]
pub(super) fn endpoint_snapshot(
    terms: &[&Terminal],
    selected: Option<&str>,
    generation: u64,
    revision: u64,
) -> ClientShellSnapshot {
    structured_snapshot(
        terms,
        selected,
        generation,
        revision,
        &serde_json::Value::Null,
        &crate::structure::Document::default(),
        None,
    )
    .0
}
