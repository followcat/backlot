//! Incremental projection: a publish that touched one machine rebuilds
//! only that endpoint's snapshot; a publish whose projection inputs are
//! unchanged rebuilds nothing. `App` consults `is_unchanged` before every
//! reconcile and calls `project` only when the answer is no.
use super::repository::repository_equal;
use super::snapshot_builder::structured_snapshot_from_spaces;
use super::{EndpointSnapshot, Projection, local_status, machine_of, project_with};
use crate::chrome::snapshot::ClientEndpointStatus;
use crate::structure::{self, Space};
use hc_proto::inventory::Inventory;
use hc_proto::machine::MachineView;
use hc_proto::terminal::Terminal;
use hc_proto::watch::{ActivityReceipts, DocKey, Document as WatchDocument, Link, View};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

/// Everything `project` + `presentation.project` read out of a `View`:
/// a publish matching every field here cannot change the projection.
struct LastState {
    generation: u64,
    local_status: ClientEndpointStatus,
    stream_offline: bool,
    rpc_offline: bool,
    core_id: Option<String>,
    selected: Option<String>,
    inventory: Arc<Inventory>,
    activity_receipts: ActivityReceipts,
    machines: Vec<MachineView>,
    documents: Arc<BTreeMap<DocKey, Arc<WatchDocument>>>,
}

/// One endpoint's cached pipeline: filtered terminals, the decoded
/// `structure::read` output and the finished raw snapshot + layouts.
struct EndpointEntry {
    inventory: Arc<Inventory>,
    terms: Vec<Terminal>,
    closed: BTreeMap<String, u64>,
    app: Option<Arc<WatchDocument>>,
    spaces: Arc<Vec<Space>>,
    own: Arc<structure::Document>,
    generation: u64,
    selected: Option<String>,
    snapshot: crate::chrome::snapshot::ClientShellSnapshot,
    layouts: BTreeMap<String, structure::TabLayout>,
    /// Times this endpoint's snapshot half was actually rebuilt — the
    /// tests' reuse probe.
    snapshot_builds: u64,
}

/// The decoded `hc-tui` layout document, keyed on its source `Document`.
struct OwnCache {
    source: Option<Arc<WatchDocument>>,
    document: Arc<structure::Document>,
}

/// Per-endpoint incremental cache for [`project_with`]. `Arc`s are held
/// by value — never as raw pointers — so a copy-on-write replace always
/// compares as a fresh input instead of mutating behind a stale address.
#[derive(Default)]
pub(crate) struct ProjectCache {
    last: Option<LastState>,
    own: Option<OwnCache>,
    endpoints: BTreeMap<Option<String>, EndpointEntry>,
}

fn stream_offline(view: &View) -> bool {
    !matches!(view.link, Link::Online | Link::Static { .. }) || view.inventory.is_offline()
}

fn rpc_offline(view: &View) -> bool {
    matches!(view.rpc, Link::Offline { .. } | Link::Stopped)
}

/// The core id `Presentation::project` baselines on — none until the
/// first inventory-bearing publish.
fn presentation_core_id(view: &View) -> Option<&str> {
    if view.generation == 0 && view.inventory.seq() == 0 {
        None
    } else {
        view.core.as_ref().map(|c| c.id.as_str())
    }
}

/// A `herdr-app` read normalized for comparison: absent and explicit
/// `null` carry the same structure information.
fn app_doc_of(view: &View, machine: Option<&str>) -> Option<Arc<WatchDocument>> {
    let key = DocKey {
        machine: machine.map(str::to_owned),
        name: "herdr-app".into(),
    };
    view.documents
        .get(&key)
        .filter(|d| !d.value.is_null())
        .cloned()
}

fn app_docs_match(a: &Option<Arc<WatchDocument>>, b: &Option<Arc<WatchDocument>>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => Arc::ptr_eq(a, b) || a.value == b.value,
        _ => false,
    }
}

fn terms_equal(stored: &[Terminal], current: &[&Terminal]) -> bool {
    stored.len() == current.len() && stored.iter().zip(current).all(|(a, b)| a == *b)
}

/// Exactly the fields `structure::read` consumes: identity, ordering,
/// finished state and the labels/cwd/repository facts that group terminals
/// into spaces and tabs. Title, status, turn and every other display-only
/// field are excluded — changing them reuses the `Space`s and rebuilds
/// only the per-terminal snapshot that carries them.
fn terms_structurally_equal(stored: &[Terminal], current: &[&Terminal]) -> bool {
    stored.len() == current.len()
        && stored
            .iter()
            .zip(current)
            .all(|(a, b)| structurally_equal(a, b))
}

fn structurally_equal(a: &Terminal, b: &Terminal) -> bool {
    a.address == b.address
        && a.created_ms == b.created_ms
        && (a.state == "finished") == (b.state == "finished")
        && a.cwd == b.cwd
        && a.labels.get("herdr.space") == b.labels.get("herdr.space")
        && a.labels.get("herdr.tab") == b.labels.get("herdr.tab")
        && repository_equal(a.repository.as_ref(), b.repository.as_ref())
}

/// The projection-relevant slice of a documents map: `hc-tui` on this
/// core and `herdr-app` on every endpoint. `fetching`, `error`, `version`
/// and remote `hc-tui` copies never reach the projection.
fn doc_value<'a>(
    docs: &'a BTreeMap<DocKey, Arc<WatchDocument>>,
    machine: Option<&str>,
    name: &str,
) -> Option<&'a serde_json::Value> {
    docs.get(&DocKey {
        machine: machine.map(str::to_owned),
        name: name.to_owned(),
    })
    .map(|d| &d.value)
    .filter(|v| !v.is_null())
}

fn relevant_documents_equal(old: &BTreeMap<DocKey, Arc<WatchDocument>>, view: &View) -> bool {
    for name in ["hc-tui", "herdr-app"] {
        if doc_value(old, None, name) != doc_value(&view.documents, None, name) {
            return false;
        }
    }
    let mut remotes: BTreeSet<&str> = view
        .machines
        .as_deref()
        .unwrap_or_default()
        .iter()
        .map(|m| m.name.as_str())
        .collect();
    remotes.extend(
        view.inventory
            .terminals()
            .values()
            .filter_map(|t| machine_of(&t.address)),
    );
    remotes.into_iter().all(|m| {
        doc_value(old, Some(m), "herdr-app") == doc_value(&view.documents, Some(m), "herdr-app")
    })
}

impl ProjectCache {
    /// True when `view` + `selected` project to what `project` last
    /// returned — equal `Arc`s are kept, equal-by-value ones are swapped
    /// in so the next check hits `Arc::ptr_eq`.
    pub(crate) fn is_unchanged(&mut self, view: &View, selected: Option<&str>) -> bool {
        let Some(last) = self.last.as_mut() else {
            return false;
        };
        if view.generation != last.generation
            || local_status(view) != last.local_status
            || stream_offline(view) != last.stream_offline
            || rpc_offline(view) != last.rpc_offline
            || presentation_core_id(view) != last.core_id.as_deref()
            || selected != last.selected.as_deref()
            || last.activity_receipts != view.activity_receipts
            || last.machines.as_slice() != view.machines.as_deref().unwrap_or_default()
        {
            return false;
        }
        if !Arc::ptr_eq(&last.inventory, &view.inventory) {
            if !last.inventory.same_projection_state(&view.inventory) {
                return false;
            }
            last.inventory = Arc::clone(&view.inventory);
        }
        if !Arc::ptr_eq(&last.documents, &view.documents) {
            if !relevant_documents_equal(&last.documents, view) {
                return false;
            }
            last.documents = Arc::clone(&view.documents);
        }
        true
    }

    /// The decoded `hc-tui` document — reused while its source `Document`
    /// is the same `Arc` or an equal value.
    pub(crate) fn own_document(&mut self, view: &View) -> Arc<structure::Document> {
        let key = DocKey {
            machine: None,
            name: "hc-tui".into(),
        };
        let source = view.documents.get(&key).cloned();
        if let Some(cache) = &self.own {
            let same = match (&cache.source, &source) {
                (None, None) => true,
                (Some(a), Some(b)) => Arc::ptr_eq(a, b) || a.value == b.value,
                _ => false,
            };
            if same {
                return Arc::clone(&cache.document);
            }
        }
        let document = Arc::new(super::layout_document(view).unwrap_or_default());
        self.own = Some(OwnCache {
            source,
            document: Arc::clone(&document),
        });
        document
    }

    /// Build `Projection` with per-endpoint reuse without recording `last`
    /// — provisional focus-fallback reads warm endpoint entries without
    /// marking the publish already installed when `fallback == focused`.
    pub(crate) fn preview(&mut self, view: &View, selected: Option<&str>) -> Projection {
        let own = self.own_document(view);
        let endpoints = &mut self.endpoints;
        let projection = project_with(view, selected, &own, |machine, terms, selected_here| {
            endpoint(endpoints, view, &own, machine, terms, selected_here)
        });
        // Drop entries for machines that left `machine.list` and whose
        // terminals are gone — `None` (local) always stays.
        let remotes: BTreeSet<&str> = view
            .machines
            .as_deref()
            .unwrap_or_default()
            .iter()
            .map(|m| m.name.as_str())
            .chain(
                view.inventory
                    .terminals()
                    .values()
                    .filter_map(|t| machine_of(&t.address)),
            )
            .collect();
        self.endpoints
            .retain(|key, _| key.as_deref().is_none_or(|m| remotes.contains(m)));
        projection
    }

    /// `preview`, then record the inputs the next `is_unchanged` checks.
    pub(crate) fn project(&mut self, view: &View, selected: Option<&str>) -> Projection {
        let projection = self.preview(view, selected);
        self.last = Some(LastState {
            generation: view.generation,
            local_status: local_status(view),
            stream_offline: stream_offline(view),
            rpc_offline: rpc_offline(view),
            core_id: presentation_core_id(view).map(str::to_owned),
            selected: selected.map(str::to_owned),
            inventory: Arc::clone(&view.inventory),
            activity_receipts: view.activity_receipts.clone(),
            machines: view.machines.clone().unwrap_or_default(),
            documents: Arc::clone(&view.documents),
        });
        projection
    }
}

/// `project_with`'s per-endpoint builder: reuse the cached snapshot when
/// every input it was built from still matches, else reuse `spaces` and
/// re-run only the snapshot half — never re-reading structure.
fn endpoint(
    endpoints: &mut BTreeMap<Option<String>, EndpointEntry>,
    view: &View,
    own: &Arc<structure::Document>,
    machine: Option<&str>,
    terms: &[&Terminal],
    selected: Option<&str>,
) -> EndpointSnapshot {
    let key = machine.map(str::to_owned);
    let app = app_doc_of(view, machine);
    // Local gets the global selection — narrow it to this endpoint's own
    // terminals so focusing between remote panes keeps Local's entry.
    let selected = match machine {
        None => {
            selected.filter(|a| machine_of(a).is_none() && terms.iter().any(|t| t.address == *a))
        }
        Some(_) => selected,
    };
    if let Some(entry) = endpoints.get(&key) {
        let same_terms =
            Arc::ptr_eq(&entry.inventory, &view.inventory) || terms_equal(&entry.terms, terms);
        // Display-only edits keep the structure: the same `Space`s drive
        // a fresh snapshot built from the current terminals.
        let same_spaces = same_terms || terms_structurally_equal(&entry.terms, terms);
        if same_spaces && app_docs_match(&entry.app, &app) && entry.closed == own.closed {
            let mut builds = entry.snapshot_builds;
            let (mut snapshot, layouts) = if same_terms
                && Arc::ptr_eq(&entry.own, own)
                && entry.generation == view.generation
                && entry.selected.as_deref() == selected
            {
                (entry.snapshot.clone(), entry.layouts.clone())
            } else {
                builds += 1;
                structured_snapshot_from_spaces(
                    &entry.spaces,
                    terms,
                    selected,
                    view.generation,
                    view.version,
                    own,
                    machine,
                )
            };
            snapshot.revision = view.version;
            let entry = endpoints.get_mut(&key).expect("entry checked above");
            entry.inventory = Arc::clone(&view.inventory);
            entry.terms = terms.iter().map(|t| (*t).clone()).collect();
            entry.app = app;
            entry.own = Arc::clone(own);
            entry.generation = view.generation;
            entry.selected = selected.map(str::to_owned);
            entry.snapshot = snapshot.clone();
            entry.layouts = layouts.clone();
            entry.snapshot_builds = builds;
            return (snapshot, layouts);
        }
    }
    let spaces = Arc::new(structure::read(
        terms,
        app.as_ref().map(|d| &d.value).unwrap_or(&super::NULL_DOC),
        machine,
    ));
    let (snapshot, layouts) = structured_snapshot_from_spaces(
        &spaces,
        terms,
        selected,
        view.generation,
        view.version,
        own,
        machine,
    );
    let builds = endpoints
        .get(&key)
        .map(|entry| entry.snapshot_builds)
        .unwrap_or(0);
    endpoints.insert(
        key,
        EndpointEntry {
            inventory: Arc::clone(&view.inventory),
            terms: terms.iter().map(|t| (*t).clone()).collect(),
            closed: own.closed.clone(),
            app,
            spaces,
            own: Arc::clone(own),
            generation: view.generation,
            selected: selected.map(str::to_owned),
            snapshot: snapshot.clone(),
            layouts: layouts.clone(),
            snapshot_builds: builds + 1,
        },
    );
    (snapshot, layouts)
}

#[cfg(test)]
#[path = "project_cache_tests.rs"]
mod tests;
