// Derived from herdr src/app/api_helpers.rs (pane_agent_status,
// tab_attention_priority) and src/protocol/wire.rs snapshot shape
// (Apache-2.0).
//
//! Projects a `watch::View` into the neutral `ClientShellSnapshot` the
//! ported chrome consumes. Structure comes from `herdr.space`/`herdr.tab`
//! labels with a machine+cwd fallback (the `herdr-app` document reader is
//! M3); status follows the reference `pane_agent_status` mapping — `idle`
//! plus an unseen (`herdr.seen=0`) agent presents as `Done`, matching the
//! reference's detection-plus-seen rule, and core's own `done`/`ready`/
//! `finished` words map to `Done` as documented in docs/parity.md.

use std::collections::BTreeMap;

use hc_proto::terminal::{Status, Terminal};
use hc_proto::watch::{Link, View};

use crate::chrome::snapshot::{
    ClientEndpointId, ClientEndpointStatus, ClientShellAgent, ClientShellEndpoint, ClientShellPane,
    ClientShellSnapshot, ClientShellTab, ClientShellWorkspace,
};
use crate::chrome::status::{AgentStatus, agent_status_from_word, status_priority};
use crate::sidebar::clean;
use crate::sidebar::{Life, classify};

#[cfg(test)]
#[path = "project_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "project_title_tests.rs"]
mod title_tests;

/// Everything the shell needs from one publish: all endpoint rows plus the
/// active endpoint's snapshot the sidebar/tab bar/panes draw.
pub(crate) struct Projection {
    pub endpoints: Vec<ClientShellEndpoint>,
    pub active_endpoint_id: ClientEndpointId,
    /// The active endpoint's snapshot (always present — an empty view still
    /// projects an empty snapshot so the shell renders an idle layout).
    pub snapshot: ClientShellSnapshot,
    pub layouts: BTreeMap<String, crate::structure::TabLayout>,
}

/// The machine part of an address: `host-b` of `host-b/t7`, `None` for a
/// bare local `t7`.
fn machine_of(address: &str) -> Option<&str> {
    address.split_once('/').map(|(prefix, _)| prefix)
}

/// Terminal number for ordering: `t7` → 7; anything else sorts last.
pub(crate) fn term_num(address: &str) -> u64 {
    let local = address.rsplit('/').next().unwrap_or(address);
    local
        .strip_prefix('t')
        .and_then(|n| n.parse().ok())
        .unwrap_or(u64::MAX)
}

/// The winning `Status` for one terminal: `sidebar::status::effective`
/// picks the most-urgent record over the root + named maps (ties prefer
/// root, then record name). The winning record's `app`/`title` fields feed
/// the agent display — a named blocked/working report still presents when
/// the root record is absent or idle.
pub(crate) fn effective_status(term: &Terminal) -> Option<&Status> {
    let effective = crate::sidebar::effective(term)?;
    match &effective.record {
        Some(name) => term.records.get(name.as_str()),
        None => term.status.as_ref(),
    }
}

/// The reference's `pane_agent_status(state, seen)` over the effective
/// record's 7501 word: `idle` + unseen is `Done`; core's `done`/`ready`/
/// `finished` words map to `Done` directly (see docs/parity.md).
fn pane_agent_status(term: &Terminal) -> AgentStatus {
    let Some(effective) = crate::sidebar::effective(term) else {
        return AgentStatus::Unknown;
    };
    match &effective.word {
        crate::sidebar::Word::Idle => AgentStatus::Idle,
        crate::sidebar::Word::Blocked { .. } => AgentStatus::Blocked,
        crate::sidebar::Word::Working { .. } => AgentStatus::Working,
        crate::sidebar::Word::Done => AgentStatus::Done,
        // `error` and unfamiliar words map through the reference word
        // table — `ready`/`finished` still present `Done`.
        crate::sidebar::Word::Error => agent_status_from_word("error"),
        crate::sidebar::Word::Other(word) => agent_status_from_word(word),
    }
}

/// The reference's `tab_attention_priority(state, seen)` — the workspace/
/// tab aggregate picks the highest-priority pane status. Mapped through
/// `pane_agent_status` so a core `done` report aggregates as `Done`, not
/// `Unknown`.
fn attention_priority(term: &Terminal) -> u8 {
    status_priority(pane_agent_status(term))
}

/// The space a terminal belongs to: `herdr.space`, else its cwd. The key
/// keeps the full cwd so basenames colliding stay distinct; the label is
/// what the workspace row displays.
pub(crate) fn space_of(term: &Terminal) -> (String, String) {
    if let Some(space) = term.labels.get("herdr.space") {
        // Length-prefixed raw key: `space:a/tab:b` + tab `c` can never
        // collide with space `a` + tab `b/tab:c`.
        return (format!("space:{}:{}", space.len(), space), clean(space));
    }
    if let Some(identity) = repository::space_identity(term) {
        return identity;
    }
    let cwd = repository::folder(term);
    let base = cwd
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .filter(|b| !b.is_empty())
        .unwrap_or(cwd);
    (format!("cwd:{}:{}", cwd.len(), cwd), clean(base))
}

#[path = "project_repository.rs"]
pub(crate) mod repository;

/// The tab a terminal belongs to inside its space: `herdr.tab`, else the
/// workspace's single default tab. The key is length-prefixed raw text —
/// injective across delimiter-bearing labels — and the unlabelled default
/// gets a disjoint marker so an explicit `default` label never merges.
pub(crate) fn tab_key_of(term: &Terminal) -> String {
    match term.labels.get("herdr.tab") {
        Some(tab) => format!("tab:{}:{}", tab.len(), tab),
        None => "tab:!default".to_string(),
    }
}

/// A terminal counts as an agent the same way the old sidebar counted:
/// core says it reported (or carries status records) and it is live.
fn is_agent(term: &Terminal) -> bool {
    classify(term) == Life::Live
        && (term.reported || effective_status(term).is_some() || !term.records.is_empty())
}

/// Local endpoint status from the subscription link; remotes keep
/// `machine.list`'s own words. A dead stream reports `Reconnecting`.
fn local_status(view: &View) -> ClientEndpointStatus {
    if view.inventory.is_offline() {
        return ClientEndpointStatus::Reconnecting;
    }
    match &view.link {
        Link::Online | Link::Static { .. } => ClientEndpointStatus::Online,
        Link::Connecting { attempt } if *attempt <= 1 => ClientEndpointStatus::Connecting,
        Link::Connecting { .. } | Link::Offline { .. } => ClientEndpointStatus::Reconnecting,
        Link::Stopped => ClientEndpointStatus::Disabled,
    }
}

/// The endpoint-owned recency map `aggregate_agent_rows` sorts on:
/// each agent's observed status-change chronology (`state_change_seq`).
fn recency_of(snapshot: &ClientShellSnapshot) -> std::collections::HashMap<String, u64> {
    snapshot
        .agents
        .iter()
        .map(|a| (a.pane_id.clone(), a.state_change_seq))
        .collect()
}

fn remote_status(state: &str) -> ClientEndpointStatus {
    match state {
        "connected" => ClientEndpointStatus::Online,
        "connecting" => ClientEndpointStatus::Connecting,
        _ => ClientEndpointStatus::Attention,
    }
}

/// Project the whole view: local endpoint first (always — it owns the
/// active snapshot), then `machine.list` entries and stray prefixes,
/// remote rows sorted by name like the previous sidebar's grouping.
pub(crate) fn project(view: &View, selected: Option<&str>) -> Projection {
    // Source client/shell/endpoints.rs local_endpoint has a fixed label.
    let local_name = "Local".to_string();

    let stream_offline =
        !matches!(view.link, Link::Online | Link::Static { .. }) || view.inventory.is_offline();
    // `machine.list` refreshes over rpc; the terminal stream does not.
    // A dead rpc lane marks remote rows stale (Attention — the shared
    // `stale() == !Online` rule then dims their agents) while their
    // snapshots and terminals stay live.
    let rpc_offline = matches!(view.rpc, Link::Offline { .. } | Link::Stopped);

    let own = layout_document(view).unwrap_or_default();
    let mut by_machine: BTreeMap<String, Vec<&Terminal>> = BTreeMap::new();
    for term in view.inventory.terminals().values() {
        if own.closed.get(&term.address) == Some(&term.created_ms) {
            continue;
        }
        let key = machine_of(&term.address).unwrap_or("").to_string();
        by_machine.entry(key).or_default().push(term);
    }

    let machine_states: BTreeMap<&str, &hc_proto::machine::MachineView> = view
        .machines
        .as_deref()
        .unwrap_or_default()
        .iter()
        .map(|m| (m.name.as_str(), m))
        .collect();

    // The active endpoint is whichever machine owns the selected terminal;
    // default local so the local snapshot always drives the shell.
    let selected_machine = selected.and_then(machine_of).map(str::to_string);
    let active_endpoint_id = selected_machine
        .clone()
        .map(ClientEndpointId::Remote)
        .unwrap_or(ClientEndpointId::Local);

    let mut endpoints = Vec::new();
    let local_terms = by_machine.remove("").unwrap_or_default();
    // A remote selection can never match a local bare `tN`, so focus ids
    // land only where the selected terminal actually lives.
    let (local_snapshot, mut layouts) = structured_snapshot(
        &local_terms,
        selected,
        view.generation,
        view.version,
        &app_document(view, None),
        &own,
        None,
    );
    let active_snapshot = local_snapshot.clone();
    let local_recency = recency_of(&local_snapshot);
    endpoints.push(ClientShellEndpoint {
        endpoint_id: ClientEndpointId::Local,
        label: clean(&local_name),
        status: local_status(view),
        snapshot: Some(Box::new(local_snapshot)),
        snapshot_generation: Some(view.generation),
        agent_recency: local_recency,
    });

    let mut remote_names: Vec<String> = view
        .machines
        .as_deref()
        .unwrap_or_default()
        .iter()
        .map(|m| m.name.clone())
        .chain(
            by_machine
                .keys()
                .filter(|key| !machine_states.contains_key(key.as_str()))
                .cloned(),
        )
        .collect();
    remote_names.sort();
    remote_names.dedup();
    for name in remote_names {
        let terms = by_machine.remove(&name).unwrap_or_default();
        let state = machine_states
            .get(name.as_str())
            .map(|m| m.state.as_str())
            .unwrap_or("unlisted");
        let status = if stream_offline {
            ClientEndpointStatus::Reconnecting
        } else if rpc_offline {
            ClientEndpointStatus::Attention
        } else {
            remote_status(state)
        };
        let selected_here = selected.filter(|address| terms.iter().any(|t| t.address == *address));
        let (snapshot, remote_layouts) = structured_snapshot(
            &terms,
            selected_here,
            view.generation,
            view.version,
            &app_document(view, Some(&name)),
            &own,
            Some(&name),
        );
        if selected_machine.as_deref() == Some(&name) {
            layouts = remote_layouts;
        }
        let recency = recency_of(&snapshot);
        endpoints.push(ClientShellEndpoint {
            endpoint_id: ClientEndpointId::Remote(name.clone()),
            label: clean(&name),
            status,
            snapshot: Some(Box::new(snapshot)),
            snapshot_generation: Some(view.generation),
            agent_recency: recency,
        });
    }

    // The active snapshot: the selected machine's when remote, else local.
    let snapshot = if matches!(active_endpoint_id, ClientEndpointId::Remote(_)) {
        endpoints
            .iter()
            .find(|e| e.endpoint_id == active_endpoint_id)
            .and_then(|e| e.snapshot.as_deref().cloned())
            .unwrap_or(active_snapshot)
    } else {
        active_snapshot
    };

    Projection {
        endpoints,
        active_endpoint_id,
        snapshot,
        layouts,
    }
}

#[path = "project_snapshot.rs"]
mod snapshot_builder;
#[cfg(test)]
use snapshot_builder::endpoint_snapshot;
use snapshot_builder::structured_snapshot;

pub(crate) fn layout_key(machine: Option<&str>, id: &str) -> String {
    let machine = machine.unwrap_or_default();
    format!("{}:{machine}/{id}", machine.len())
}

pub(crate) fn layout_document(view: &View) -> Result<crate::structure::Document, String> {
    let key = hc_proto::watch::DocKey {
        machine: None,
        name: "hc-tui".into(),
    };
    crate::structure::Document::decode(
        view.documents
            .get(&key)
            .map(|d| d.value.clone())
            .unwrap_or_default(),
    )
}

fn app_document(view: &View, machine: Option<&str>) -> serde_json::Value {
    let key = hc_proto::watch::DocKey {
        machine: machine.map(str::to_owned),
        name: "herdr-app".into(),
    };
    view.documents
        .get(&key)
        .map(|d| d.value.clone())
        .unwrap_or_default()
}

#[cfg(test)]
#[path = "project_structure_tests.rs"]
mod structure_tests;
