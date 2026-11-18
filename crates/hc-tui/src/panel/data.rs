//! Reading the `backlot` document out of the watcher view: every
//! machine's copy, always a soft failure — a missing, malformed or
//! wrong-schema document is "no studios", never an on-screen error.

use backlot_model::summary::{SUMMARY_SCHEMA, StudioSummary, Summary};
use hc_proto::watch::{DocKey, View};

use crate::studio::StudioKey;

/// The document name every machine's stage publishes.
const DOCUMENT: &str = "backlot";

/// This core's `backlot` key — the opening decision reads only it.
pub(crate) fn key() -> DocKey {
    DocKey {
        machine: None,
        name: DOCUMENT.into(),
    }
}

/// One machine's parsed `backlot` document: `machine` is `None` for
/// this core and `Some(name)` for an added machine. A remote
/// machine's role terminals are stored machine-qualified
/// (`host-b/t7`) so every reader compares them against the
/// inventory's addresses directly.
pub(crate) struct MachineSummary {
    pub machine: Option<String>,
    pub summary: Summary,
}

/// A studio inside the aggregate: its machine plus the document row.
#[derive(Debug, Clone, Copy)]
pub(crate) struct StudioRef<'a> {
    pub machine: Option<&'a str>,
    pub studio: &'a StudioSummary,
}

impl StudioRef<'_> {
    /// The studio's stable identity across machines — the same id on
    /// two machines is two studios.
    pub(crate) fn key(&self) -> StudioKey {
        StudioKey {
            machine: self.machine.map(str::to_owned),
            id: self.studio.id.clone(),
        }
    }
}

/// Every machine's parsed `backlot` document — this core's first,
/// then each added machine in `machine.list` order. A machine whose
/// document is missing, unreadable or foreign-schema is absent:
/// blank, never an error.
pub(crate) fn summaries(view: &View) -> Vec<MachineSummary> {
    let mut out = Vec::new();
    if let Some(summary) = view
        .documents
        .get(&key())
        .and_then(|doc| parse(&doc.value, None))
    {
        out.push(MachineSummary {
            machine: None,
            summary,
        });
    }
    let mut seen = std::collections::BTreeSet::new();
    for machine in view.machines.iter().flatten() {
        let name = &machine.name;
        if !seen.insert(name.clone()) {
            continue;
        }
        let key = DocKey {
            machine: Some(name.clone()),
            name: DOCUMENT.into(),
        };
        if let Some(summary) = view
            .documents
            .get(&key)
            .and_then(|doc| parse(&doc.value, Some(name)))
        {
            out.push(MachineSummary {
                machine: Some(name.clone()),
                summary,
            });
        }
    }
    // A document slot the machine list does not name still counts —
    // the stage's summary is as good as the list knows it.
    let mut strays: Vec<&str> = view
        .documents
        .keys()
        .filter_map(|key| match key.machine.as_deref() {
            Some(machine) if !seen.contains(machine) && key.name == DOCUMENT => Some(machine),
            _ => None,
        })
        .collect();
    strays.sort_unstable();
    for machine in strays {
        let key = DocKey {
            machine: Some(machine.into()),
            name: DOCUMENT.into(),
        };
        if let Some(summary) = view
            .documents
            .get(&key)
            .and_then(|doc| parse(&doc.value, Some(machine)))
        {
            out.push(MachineSummary {
                machine: Some(machine.into()),
                summary,
            });
        }
    }
    out
}

/// This core's parsed document alone — Hollywood's cards and the
/// headline stay this core's. Tests only: the Call Sheet reads its
/// local document through `hollywood::rows::local_summary`.
#[cfg(test)]
pub(crate) fn local_summary(view: &View) -> Option<Summary> {
    view.documents
        .get(&key())
        .and_then(|doc| parse(&doc.value, None))
}

/// Every studio in display order: machine order, then document
/// order. Studio blocks group by machine for free.
pub(crate) fn studios(machines: &[MachineSummary]) -> Vec<StudioRef<'_>> {
    machines
        .iter()
        .flat_map(|machine| {
            machine.summary.studios.iter().map(move |studio| StudioRef {
                machine: machine.machine.as_deref(),
                studio,
            })
        })
        .collect()
}

/// The studio count across machines — the panel's title and the
/// collapsed strip's total.
pub(crate) fn studio_count(machines: &[MachineSummary]) -> usize {
    machines.iter().map(|m| m.summary.studios.len()).sum()
}

/// One studio of the aggregate by key.
pub(crate) fn studio<'a>(machines: &'a [MachineSummary], key: &StudioKey) -> Option<StudioRef<'a>> {
    studios(machines).into_iter().find(|s| &s.key() == key)
}

/// Whether studios come from more than one machine — headers name
/// the machine only then; one machine draws exactly like before.
pub(crate) fn spans_machines(machines: &[MachineSummary]) -> bool {
    machines
        .iter()
        .filter(|m| !m.summary.studios.is_empty())
        .count()
        > 1
}

/// A studio's header text pair: the `id · machine` label, its bare
/// id and its bare machine — the machine narrows first but is never
/// dropped whole while one is set (A8.3).
#[derive(Debug, Clone)]
pub(crate) struct StudioLabel {
    /// `id · machine`, or `id` alone.
    pub full: String,
    /// The studio id.
    pub id: String,
    /// The machine name when one is labelled.
    pub machine: Option<String>,
}

impl StudioLabel {
    /// The label at `width` (A8.3): `id · machine` when it fits;
    /// else the machine truncated to a floor of four chars plus `…`
    /// (`beta-tools · hos…`); else the separator shortens to `@`
    /// (`beta-tools@hos…`) and the id truncates last
    /// (`beta-too…@hos…`). Without a machine it is the id truncated.
    pub(crate) fn fit(&self, width: usize) -> String {
        use crate::chrome::text::{display_width, truncate_end};
        if display_width(&self.full) <= width {
            return self.full.clone();
        }
        let Some(machine) = self.machine.as_deref() else {
            return truncate_end(&self.id, width);
        };
        // Step 2: `id · <mach…>` — the machine keeps four chars.
        let m = truncate_end(machine, 5); // 4 chars + `…`
        let with_dots = format!("{} · {}", self.id, m);
        if display_width(&with_dots) <= width {
            return with_dots;
        }
        // Step 3: `id@<mach…>` — the separator shortens.
        let at = format!("{}@{}", self.id, m);
        if display_width(&at) <= width {
            return at;
        }
        // Step 4: the id itself truncates inside `id@mach…`.
        let room = width.saturating_sub(display_width(&format!("@{m}")));
        format!("{}@{}", truncate_end(&self.id, room), m)
    }
}

/// A studio's header text: `id`, or `id · machine` when the
/// aggregate spans machines. `local` is this core's name, used for
/// local studios.
pub(crate) fn studio_label(
    id: &str,
    machine: Option<&str>,
    local: Option<&str>,
    multi: bool,
) -> StudioLabel {
    match machine.or(local).filter(|_| multi) {
        Some(machine) => StudioLabel {
            full: format!("{id} · {machine}"),
            id: id.to_string(),
            machine: Some(machine.to_string()),
        },
        None => StudioLabel {
            full: id.to_string(),
            id: id.to_string(),
            machine: None,
        },
    }
}

/// Parse one machine's document: only `schema == "backlot/1"` parses
/// — an unknown schema is a document for a different client
/// generation, not for us. An unrecognised `work` value on a live
/// person degrades to waiting (a newer stage may know states this
/// client does not) — the rest of the document still parses. A
/// remote machine's role terminals come back machine-qualified.
fn parse(value: &serde_json::Value, machine: Option<&str>) -> Option<Summary> {
    let mut value = value.clone();
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
    let mut summary: Summary = serde_json::from_value(value).ok()?;
    if summary.schema != SUMMARY_SCHEMA {
        return None;
    }
    if let Some(machine) = machine {
        for role in summary
            .studios
            .iter_mut()
            .flat_map(|studio| studio.roles.iter_mut())
        {
            if let Some(terminal) = role.terminal.take() {
                role.terminal = Some(format!("{machine}/{terminal}"));
            }
        }
    }
    Some(summary)
}

/// Whether the inventory shows a live `backlot-stage` terminal on
/// `machine` (`None` is this core) — the producer of its document.
/// When it is gone the panel says so on one line but still shows the
/// last document.
pub(crate) fn stage_running(view: &View, machine: Option<&str>) -> bool {
    view.inventory.terminals().values().any(|t| {
        t.name.as_deref() == Some("backlot-stage")
            && t.state == "live"
            && t.address.split_once('/').map(|(m, _)| m) == machine
    })
}
