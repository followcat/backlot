// Derived from herdr src/app/api_helpers.rs (Apache-2.0).
use super::*;

/// Build one endpoint's snapshot from its terminals. `selected` is the
/// globally selected terminal address; focus ids only land on this
/// snapshot when they belong to this endpoint.
pub(super) fn endpoint_snapshot(
    terms: &[&Terminal],
    selected: Option<&str>,
    generation: u64,
    revision: u64,
) -> ClientShellSnapshot {
    // space key → (label, members), grouped then ordered by each group's
    // earliest created terminal — the reference preserves snapshot
    // creation order, not lexical order.
    let mut spaces: BTreeMap<String, (String, Vec<&Terminal>)> = BTreeMap::new();
    for term in terms {
        let (key, label) = space_of(term);
        spaces
            .entry(key)
            .or_insert_with(|| (label, Vec::new()))
            .1
            .push(term);
    }
    for (_, members) in spaces.values_mut() {
        members.sort_by_key(|t| (t.created_ms, term_num(&t.address), t.address.as_str()));
    }
    // Creation order first; a simultaneous creation falls to the terminal
    // number before any lexical label order (the reference's tie-break).
    let mut space_order: Vec<&String> = spaces.keys().collect();
    space_order.sort_by_key(|key| {
        spaces[*key]
            .1
            .first()
            .map(|term| (term.created_ms, term_num(&term.address)))
            .unwrap_or((u64::MAX, u64::MAX))
    });

    let mut workspaces = Vec::new();
    let mut tabs = Vec::new();
    let mut panes = Vec::new();
    let mut agents = Vec::new();
    let mut agent_order = Vec::new();
    for (index, space_key) in space_order.into_iter().enumerate() {
        let (space_label, members) = &spaces[space_key.as_str()];
        let number = index + 1;
        let workspace_id = space_key.clone();
        // tab key → (label, custom, members); creation-ordered like spaces.
        let mut tab_groups: BTreeMap<String, (String, bool, Vec<&Terminal>)> = BTreeMap::new();
        for term in members {
            let key = tab_key_of(term);
            tab_groups
                .entry(key)
                .or_insert_with(|| {
                    match tab_of(term) {
                        Some(tab) => (clean(tab), true, Vec::new()),
                        // The unlabelled default tab shows its number,
                        // not the workspace label.
                        None => (String::new(), false, Vec::new()),
                    }
                })
                .2
                .push(term);
        }
        let mut tab_order: Vec<&String> = tab_groups.keys().collect();
        tab_order.sort_by_key(|key| {
            tab_groups[*key]
                .2
                .first()
                .map(|term| (term.created_ms, term_num(&term.address)))
                .unwrap_or((u64::MAX, u64::MAX))
        });
        let mut tab_ids = Vec::new();
        let mut tab_number = 0usize;
        let mut workspace_status = AgentStatus::Unknown;
        let mut workspace_priority = 0u8;
        for tab_key in tab_order {
            let (tab_label, custom_label, tab_members) = &tab_groups[tab_key.as_str()];
            tab_number += 1;
            let tab_id = format!("{workspace_id}/{tab_key}");
            // The unlabelled default displays its position number.
            let tab_label = if tab_label.is_empty() {
                tab_number.to_string()
            } else {
                clean(tab_label)
            };
            tab_ids.push(tab_id.clone());
            let mut tab_status = AgentStatus::Unknown;
            let mut tab_priority = 0u8;
            for term in tab_members {
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
                custom_label: *custom_label,
                zoomed: false,
                focused: tab_members
                    .iter()
                    .any(|t| selected == Some(t.address.as_str())),
                agent_status: tab_status,
            });
        }
        let first_tab = tab_ids.first().cloned().unwrap_or_default();
        let cwd = members.first().map(|t| clean(&t.cwd)).unwrap_or_default();
        workspaces.push(ClientShellWorkspace {
            workspace_id: workspace_id.clone(),
            active_tab_id: tabs
                .iter()
                .find(|t| t.focused && t.workspace_id == *workspace_id)
                .map(|t| t.tab_id.clone())
                .unwrap_or(first_tab),
            new_workspace_cwd: cwd,
            number,
            label: clean(space_label),
            custom_label: members.iter().any(|t| t.labels.contains_key("herdr.space")),
            branch: None,
            git_ahead_behind: None,
            tokens: Vec::new(),
            worktree: None,
            focused: members.iter().any(|t| selected == Some(t.address.as_str())),
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
    }
}
