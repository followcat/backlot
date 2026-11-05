// Derived from herdr src/app/creation.rs pane_metadata and
// src/server/client_shell.rs (Apache-2.0).

use super::*;
use crate::sidebar::fixture::{agent, term};

#[test]
fn optional_manual_label_is_not_synthesized_from_other_title_fields() {
    let mut reported = agent("t7", "working");
    reported.name = Some("alice".into());
    reported.title = "osc-title".into();
    reported.status.as_mut().unwrap().title = "reported-title".into();
    let shell = term("t8");
    let snapshot = endpoint_snapshot(&[&reported, &shell], Some("t7"), 1, 1);
    assert!(
        snapshot.panes.iter().all(|pane| pane.label.is_none()),
        "reference pane.label is terminal.manual_label only: {:?}",
        snapshot.panes
    );
    let agent = &snapshot.agents[0];
    assert_eq!(agent.name.as_deref(), Some("alice"));
    assert_eq!(agent.title.as_deref(), Some("reported-title"));
    assert_eq!(agent.terminal_title.as_deref(), Some("osc-title"));
}
