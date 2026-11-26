//! Hollywood layer unit tests — the row model, the toggle binding
//! and the renderer at every width. Neutral fixture data only:
//! `acme-shop`, `beta-tools`, `alice`.
mod decision_tests;
mod draw_tests;
mod keys_tests;
mod reader_design_tests;
mod rows_tests;
mod zones_tests;

use backlot_model::summary::Summary;

/// Three studios under a live Hollywood with a headline and two open
/// cards: `acme-shop` live with a working cast and an attention,
/// `beta-tools` finished, `gamma-site` with a ledger error.
pub(super) fn summary_json() -> serde_json::Value {
    serde_json::json!({
        "schema": "backlot/1",
        "updated_ms": 1,
        "studios": [
            {
                "id": "acme-shop", "title": "Acme shop", "state": "live",
                "requirement_version": 2, "mode": "standard",
                "open_reviews": 0, "waiting": 2,
                "next": "review delivery 1 — director",
                "roles": [
                    { "role": "producer", "name": "producer", "state": "live",
                      "work": "working" },
                    { "role": "actor-alice", "name": "alice", "state": "live",
                      "work": "blocked", "attention": "stalled",
                      "attention_detail": "may I run the tests?" },
                    { "role": "actor-bob", "name": "bob", "state": "lost" },
                    { "role": "actor-carol", "name": "carol", "state": "live",
                      "work": "waiting" }
                ]
            },
            {
                "id": "beta-tools", "title": "Beta tools", "state": "finished",
                "requirement_version": 1, "mode": "standard",
                "open_reviews": 0, "waiting": 0,
                "roles": [
                    { "role": "producer", "name": "producer", "state": "live",
                      "work": "waiting" }
                ]
            },
            {
                "id": "gamma-site", "title": "Gamma site", "state": "live",
                "requirement_version": 3, "mode": "standard",
                "open_reviews": 1, "waiting": 0,
                "error": "events.jsonl: unreadable",
                "roles": []
            }
        ],
        "hollywood": {
            "state": "live",
            "runner": "fake",
            "model": "model-h",
            "effort": "high",
            "headline": "acme-shop needs a look",
            "cards": [
                {
                    "id": "c3", "studio": "acme-shop",
                    "action": "pause t1", "why": "the actor is looping"
                },
                {
                    "id": "c4", "studio": "beta-tools",
                    "action": "studio-new beta-tools 'Beta tools'",
                    "why": "the user asked for a second shop"
                }
            ]
        }
    })
}

pub(super) fn summary() -> Summary {
    serde_json::from_value(summary_json()).unwrap()
}

/// The fixture with `attention_user` on `role` of `studio` — the
/// D9 user-owned prompt a sheet prompt row needs.
pub(super) fn with_user_prompt(studio: &str, role: &str) -> Summary {
    let mut value = summary_json();
    for s in value["studios"].as_array_mut().unwrap() {
        if s["id"] == studio {
            for r in s["roles"].as_array_mut().unwrap() {
                if r["role"] == role {
                    r["attention_user"] = true.into();
                    r["attention"] = "permission prompt".into();
                    r["attention_class"] = "needs_you".into();
                    r["attention_seq"] = 7.into();
                    r["attention_detail"] = "may I run the tests?".into();
                }
            }
        }
    }
    serde_json::from_value(value).unwrap()
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
