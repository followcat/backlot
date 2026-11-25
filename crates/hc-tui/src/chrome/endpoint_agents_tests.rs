//! Space headers in the grouped agents panel across machines.

use crate::chrome::snapshot::{ClientEndpointId, ClientShellEndpoint};
use crate::config::AgentPanelSortConfig;
use crate::project::project;
use crate::sidebar::fixture::{agent, machine, view};

fn shell(sort: AgentPanelSortConfig, space_headers: bool) -> crate::chrome::ClientShellConfig {
    let mut shell = crate::chrome::ClientShellConfig::from_config(
        &crate::config::Config::default(),
        None,
        None,
        std::path::PathBuf::new(),
    );
    shell.agent_panel_sort = sort;
    shell.agent_panel_space_headers = space_headers;
    shell
}

fn spaced(address: &str, word: &str, space: &str) -> hc_proto::terminal::Terminal {
    let mut agent = agent(address, word);
    agent.labels.insert("herdr.space".into(), space.into());
    agent
}

/// Local `dotfiles` (t1, t2), host-a `dotfiles` (t3) and `proj` (t4),
/// host-b `proj` (t5): each machine boundary falls between two spaces
/// of the same name, so only the machine tells the groups apart.
fn three_machines() -> Vec<ClientShellEndpoint> {
    let terms = vec![
        spaced("t1", "idle", "dotfiles"),
        spaced("t2", "working", "dotfiles"),
        spaced("host-a/t3", "blocked", "dotfiles"),
        spaced("host-a/t4", "idle", "proj"),
        spaced("host-b/t5", "idle", "proj"),
    ];
    let machines = vec![
        machine("host-a", "connected"),
        machine("host-b", "connected"),
    ];
    project(&view(terms, machines), Some("t1")).endpoints
}

fn panel(
    endpoints: &[ClientShellEndpoint],
    agent_view_label: Option<&str>,
    shell: &crate::chrome::ClientShellConfig,
) -> Vec<String> {
    super::panel_entries(endpoints, &ClientEndpointId::Local, agent_view_label, shell)
        .into_iter()
        .map(|entry| match entry {
            super::PanelEntry::Space { label, gap } => {
                format!("{}[{label}]", if gap { "~" } else { "" })
            }
            super::PanelEntry::Agent(row) => row.agent.pane_id,
        })
        .collect()
}

#[test]
fn space_headers_head_each_machines_space_without_naming_the_machine() {
    assert_eq!(
        panel(
            &three_machines(),
            None,
            &shell(AgentPanelSortConfig::Spaces, true)
        ),
        [
            "[dotfiles]",
            "t1",
            "t2",
            "~[dotfiles]",
            "host-a/t3",
            "~[proj]",
            "host-a/t4",
            "~[proj]",
            "host-b/t5",
        ]
    );
}

#[test]
fn multi_machine_panel_has_no_headers_unless_asked() {
    assert_eq!(
        panel(
            &three_machines(),
            None,
            &shell(AgentPanelSortConfig::Spaces, false)
        ),
        ["t1", "t2", "host-a/t3", "host-a/t4", "host-b/t5"]
    );
}

#[test]
fn multi_machine_priority_panel_stays_flat_with_space_headers_on() {
    let rows = panel(
        &three_machines(),
        None,
        &shell(AgentPanelSortConfig::Priority, true),
    );
    assert_eq!(rows.len(), 5);
    assert!(rows.iter().all(|row| !row.contains('[')), "{rows:?}");
}

#[test]
fn multi_machine_agent_view_stays_flat_with_space_headers_on() {
    assert_eq!(
        panel(
            &three_machines(),
            Some("blocked"),
            &shell(AgentPanelSortConfig::Spaces, true)
        ),
        ["t1", "t2", "host-a/t3", "host-a/t4", "host-b/t5"]
    );
}

#[test]
fn agent_entry_index_counts_the_space_headers_above_it() {
    let endpoints = three_machines();
    let index = |space_headers: bool| {
        super::agent_entry_index(
            &endpoints,
            &ClientEndpointId::Local,
            None,
            &shell(AgentPanelSortConfig::Spaces, space_headers),
            &ClientEndpointId::Remote("host-b".into()),
            "host-b/t5",
        )
    };
    assert_eq!(index(true), Some(8));
    assert_eq!(index(false), Some(4));
    assert_eq!(
        super::agent_entry_index(
            &endpoints,
            &ClientEndpointId::Local,
            None,
            &shell(AgentPanelSortConfig::Spaces, true),
            &ClientEndpointId::Remote("host-a".into()),
            "host-b/t5",
        ),
        None,
        "an agent is matched on its machine as well as its pane"
    );
}
