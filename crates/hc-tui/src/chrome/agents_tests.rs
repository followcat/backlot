//! Space headers in the grouped agents panel.

use crate::project::project;
use crate::sidebar::fixture::{agent, view};

fn panel(
    snapshot: &crate::chrome::snapshot::ClientShellSnapshot,
    sort: crate::config::AgentPanelSortConfig,
    space_headers: bool,
) -> Vec<String> {
    let mut shell = crate::chrome::ClientShellConfig::from_config(
        &crate::config::Config::default(),
        None,
        None,
        std::path::PathBuf::new(),
    );
    shell.agent_panel_sort = sort;
    shell.agent_panel_space_headers = space_headers;
    super::panel_rows(snapshot, &shell)
        .into_iter()
        .map(|row| match row {
            super::PanelRow::Space { label, gap } => {
                format!("{}[{label}]", if gap { "~" } else { "" })
            }
            super::PanelRow::Agent(agent) => agent.pane_id,
        })
        .collect()
}

fn two_spaces() -> crate::chrome::snapshot::ClientShellSnapshot {
    let spaced = |address: &str, word: &str, space: &str| {
        let mut agent = agent(address, word);
        agent.labels.insert("herdr.space".into(), space.into());
        agent
    };
    let terms = vec![
        spaced("t1", "idle", "bambooworks"),
        spaced("t2", "working", "bambooworks"),
        spaced("t3", "blocked", "dev-workspace"),
    ];
    project(&view(terms, vec![]), Some("t1")).snapshot
}

#[test]
fn space_headers_head_each_space_of_the_grouped_agents_panel() {
    assert_eq!(
        panel(
            &two_spaces(),
            crate::config::AgentPanelSortConfig::Spaces,
            true
        ),
        ["[bambooworks]", "t1", "t2", "~[dev-workspace]", "t3"]
    );
}

#[test]
fn grouped_agents_panel_has_no_headers_unless_asked() {
    assert!(!crate::config::UiConfig::default().agent_panel_space_headers);
    assert_eq!(
        panel(
            &two_spaces(),
            crate::config::AgentPanelSortConfig::Spaces,
            false
        ),
        ["t1", "t2", "t3"]
    );
}

#[test]
fn priority_agents_panel_stays_flat_with_space_headers_on() {
    assert_eq!(
        panel(
            &two_spaces(),
            crate::config::AgentPanelSortConfig::Priority,
            true
        ),
        ["t3", "t2", "t1"]
    );
}
