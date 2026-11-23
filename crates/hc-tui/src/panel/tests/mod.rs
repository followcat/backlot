//! Panel unit tests — the row model, the document read, the config
//! file and the renderer. Neutral fixture data only: `acme-shop`,
//! `alice`, `host-a`.
mod config_tests;
mod data_tests;
mod draw_tests;
mod exchange_tests;
mod remote_tests;
mod rows_tests;
mod update_tests;

use backlot_model::summary::Summary;

/// A neutral studio: `acme-shop` live with one person per group plus
/// five actors covering every mark — alice/erin working, bob blocked,
/// carol lost, dave closed (resumable).
pub(super) fn summary_json() -> serde_json::Value {
    serde_json::json!({
        "schema": "backlot/1",
        "updated_ms": 1,
        "studios": [{
            "id": "acme-shop",
            "title": "Acme shop",
            "state": "live",
            "requirement_version": 1,
            "mode": "standard",
            "open_reviews": 0,
            "waiting": 0,
            "roles": [
                { "role": "producer", "name": "producer", "terminal": "t1",
                  "state": "live", "work": "working" },
                { "role": "director", "name": "director", "terminal": "t2",
                  "state": "live", "work": "waiting" },
                { "role": "writer", "name": "writer", "terminal": "t3",
                  "state": "live" },
                { "role": "actor-alice", "name": "alice", "terminal": "t4",
                  "runner": "agent-a", "model": "m", "state": "live",
                  "work": "working", "task": "task-1",
                  "priority": "normal", "doing": "writes code" },
                { "role": "actor-erin", "name": "erin", "terminal": "t5",
                  "state": "live", "work": "working", "task": "task-2",
                  "priority": "high" },
                { "role": "actor-bob", "name": "bob", "terminal": "t6",
                  "state": "live", "work": "blocked", "task": "task-3",
                  "priority": "low" },
                { "role": "actor-carol", "name": "carol", "terminal": "t7",
                  "state": "lost" },
                { "role": "actor-dave", "name": "dave",
                  "state": "closed", "resumable": true }
            ]
        }]
    })
}

pub(super) fn summary() -> Summary {
    serde_json::from_value(summary_json()).unwrap()
}

/// A summary wrapped as this core's machine row — the shape the
/// aggregate APIs take; `None` is no document anywhere.
pub(super) fn machines(summary: Option<&Summary>) -> Vec<crate::panel::data::MachineSummary> {
    summary
        .map(|summary| crate::panel::data::MachineSummary {
            machine: None,
            summary: summary.clone(),
        })
        .into_iter()
        .collect()
}

/// `host-b`'s row — the shape `summaries` returns for an added
/// machine.
pub(super) fn remote(summary: &Summary) -> crate::panel::data::MachineSummary {
    crate::panel::data::MachineSummary {
        machine: Some("host-b".into()),
        summary: summary.clone(),
    }
}
