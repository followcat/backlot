//! Distinct identity and metadata values expose projection collisions.

use super::*;
use crate::sidebar::fixture::term;

fn snapshot(terms: &[Terminal]) -> ClientShellSnapshot {
    let refs: Vec<_> = terms.iter().collect();
    endpoint_snapshot(&refs, Some(&terms[0].address), 1, 1)
}

fn labeled(address: &str, space: &str, tab: &str) -> Terminal {
    let mut terminal = term(address);
    terminal.labels.insert("herdr.space".into(), space.into());
    terminal.labels.insert("herdr.tab".into(), tab.into());
    terminal
}

// ---- Migrated sidebar-model semantics (from the retired tests/sidebar.rs) ----
// The old row model is gone; these assert the same behaviors through the
// projected snapshot the ported chrome renders. Custom presentations the
// reference never had (`word·name`, `·source`, counters, `·stale` badges)
// are asserted absent, per docs/parity.md.

use crate::sidebar::fixture::{agent, machine, status, view};

fn endpoint<'p>(projected: &'p Projection, name: &str) -> &'p ClientShellEndpoint {
    projected
        .endpoints
        .iter()
        .find(|e| match &e.endpoint_id {
            ClientEndpointId::Local => name == "local",
            ClientEndpointId::Remote(n) => n == name,
        })
        .unwrap_or_else(|| panic!("no endpoint {name}: {:?}", projected.endpoints))
}

#[path = "project_tests_parts/cases_1.rs"]
mod cases_1;

#[path = "project_tests_parts/cases_2.rs"]
mod cases_2;

#[path = "project_traversal_tests.rs"]
mod traversal_tests;
