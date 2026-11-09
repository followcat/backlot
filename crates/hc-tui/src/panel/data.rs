//! Reading the `backlot` document out of the watcher view: always a
//! soft failure — a missing, malformed or wrong-schema document is
//! "no studios", never an on-screen error.

use backlot_model::summary::{SUMMARY_SCHEMA, Summary};
use hc_proto::watch::{DocKey, View};

/// The document this client reads — this core's only (`machine: None`).
pub(crate) fn key() -> DocKey {
    DocKey {
        machine: None,
        name: "backlot".into(),
    }
}

/// The parsed summary, or `None` for anything unusable. Only
/// `schema == "backlot/1"` parses — an unknown schema is a document
/// for a different client generation, not for us.
///
/// An unrecognised `work` value on a live person degrades to waiting
/// (a newer stage may know states this client does not) — the rest of
/// the document still parses.
pub(crate) fn summary(view: &View) -> Option<Summary> {
    let document = view.documents.get(&key())?;
    let mut value = document.value.clone();
    if let Some(studios) = value.get_mut("studios").and_then(|s| s.as_array_mut()) {
        for role in studios.iter_mut().flat_map(|st| {
            st.get_mut("roles")
                .and_then(|r| r.as_array_mut())
                .into_iter()
                .flatten()
        }) {
            match role.get("work").and_then(|w| w.as_str()) {
                Some("working" | "waiting" | "blocked") | None => {}
                Some(_) => {
                    role["work"] = serde_json::Value::from("waiting");
                }
            }
        }
    }
    let summary: Summary = serde_json::from_value(value).ok()?;
    (summary.schema == SUMMARY_SCHEMA).then_some(summary)
}

/// Whether the inventory shows a live `backlot-stage` terminal — the
/// producer of the document. When it is gone the panel says so on one
/// line but still shows the last document.
pub(crate) fn stage_running(view: &View) -> bool {
    view.inventory
        .terminals()
        .values()
        .any(|t| t.name.as_deref() == Some("backlot-stage") && t.state == "live")
}
