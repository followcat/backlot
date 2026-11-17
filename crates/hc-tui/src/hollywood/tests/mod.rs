//! Hollywood layer unit tests — the row model, the toggle binding
//! and the renderer at every width. Neutral fixture data only:
//! `acme-shop`, `beta-tools`, `alice`.
mod draw_tests;
mod keys_tests;
mod rows_tests;

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
                      "work": "blocked", "attention": "may I run the tests?" },
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
