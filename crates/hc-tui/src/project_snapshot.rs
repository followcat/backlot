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
                let selection = agent_record(term);
                let status = selection.map_or(AgentStatus::Unknown, |record| record.state);
                let priority = status_priority(status);
                if priority > tab_priority {
                    tab_priority = priority;
                    tab_status = status;
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
                    let record = selection.and_then(|selected| selected.status(term));
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
                        // Presentation supplies a rank from origin clocks;
                        // timestamps and turns are not chronology facts.
                        state_change_seq: 0,
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
        let members = space
            .tabs
            .iter()
            .flat_map(|tab| tab.root.terminals())
            .collect::<Vec<_>>();
        let facts = selected
            .filter(|id| members.iter().any(|member| member == id))
            .and_then(|id| term_by_id.get(id))
            .and_then(|term| term.repository.as_ref())
            .or_else(|| {
                members
                    .iter()
                    .filter_map(|id| term_by_id.get(id.as_str()))
                    .find_map(|term| term.repository.as_ref())
            });
        let cwd = clean(
            facts
                .and_then(|facts| facts.checkout.as_ref())
                .map_or(space.folder.as_str(), |checkout| {
                    checkout.repo_root.as_str()
                }),
        );
        // A rename stores the label under the workspace's own layout key
        // (including cwd-grouped spaces, whose identity never maps through
        // `space:N:`); a promotion key still matches by space identity.
        // Adoptions now also override foreign (herdr-app) labels — that is
        // how rename works on app-owned workspaces.
        let adopted = own
            .adoptions
            .get(&layout_key(machine, workspace_id))
            .or_else(|| {
                own.adoptions.values().find(|a| {
                    a.machine.as_deref() == machine
                        && format!("space:{}:{}", a.space.len(), a.space) == *workspace_id
                })
            });
        workspaces.push(ClientShellWorkspace {
            workspace_id: workspace_id.clone(),
            active_tab_id: tabs
                .iter()
                .find(|t| t.focused && t.workspace_id == *workspace_id)
                .map(|t| t.tab_id.clone())
                .or_else(|| {
                    own.active
                        .get(&layout_key(machine, workspace_id))
                        .and_then(|saved| {
                            tab_ids.iter().find(|id| layout_key(machine, id) == *saved)
                        })
                        .cloned()
                })
                .unwrap_or(first_tab),
            new_workspace_cwd: cwd,
            number,
            label: clean(adopted.map(|a| a.name.as_str()).unwrap_or_else(|| {
                if space.custom_name {
                    &space.name
                } else {
                    facts
                        .and_then(|facts| facts.auto_label.as_deref())
                        .unwrap_or(&space.name)
                }
            })),
            custom_label: adopted.map_or(space.custom_name, |a| a.custom_label),
            branch: facts
                .filter(|facts| facts.state == "ready")
                .and_then(|facts| facts.branch.as_deref())
                .map(clean),
            git_ahead_behind: facts.and_then(repository::counts),
            tokens: Vec::new(),
            checkout: facts.and_then(repository::checkout),
            // Herdr only groups explicit worktree_space memberships. Ordinary
            // Git linked checkouts remain independent workspaces (creation.rs).
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
