//! `ProjectCache` contracts: publishes that carry no projection input
//! are recognized without rebuilding, and per-endpoint entries reuse
//! untouched structure/snapshot work while the changed endpoint rebuilds.

use super::*;
use crate::sidebar::fixture::{agent, machine, term, view};

fn labeled(address: &str, space: &str, tab: &str) -> Terminal {
    let mut terminal = term(address);
    terminal.labels.insert("herdr.space".into(), space.into());
    terminal.labels.insert("herdr.tab".into(), tab.into());
    terminal
}

fn labeled_agent(address: &str, space: &str, tab: &str, word: &str) -> Terminal {
    let mut terminal = agent(address, word);
    terminal.labels.insert("herdr.space".into(), space.into());
    terminal.labels.insert("herdr.tab".into(), tab.into());
    terminal
}

/// Local `t1` plus one terminal per remote `host-a`/`host-b`.
fn multi() -> View {
    view(
        vec![
            labeled("t1", "work", "one"),
            labeled("host-a/t1", "work", "one"),
            labeled("host-b/t1", "work", "one"),
        ],
        vec![
            machine("host-a", "connected"),
            machine("host-b", "connected"),
        ],
    )
}

fn set_doc(view: &mut View, machine: Option<&str>, name: &str, value: serde_json::Value) {
    let key = hc_proto::watch::DocKey {
        machine: machine.map(str::to_string),
        name: name.into(),
    };
    *Arc::make_mut(view.documents_mut().entry(key).or_insert_with(|| {
        Arc::new(hc_proto::watch::Document {
            value: serde_json::Value::Null,
            version: 0,
            fetching: false,
            error: None,
        })
    })) = hc_proto::watch::Document {
        value,
        version: 1,
        fetching: false,
        error: None,
    };
}

fn endpoint_key(machine: Option<&str>) -> Option<String> {
    machine.map(str::to_string)
}

fn entry<'c>(cache: &'c ProjectCache, machine: Option<&str>) -> &'c EndpointEntry {
    &cache.endpoints[&endpoint_key(machine)]
}

fn assert_same_projection(cached: &Projection, reference: &Projection) {
    assert_eq!(cached.active_endpoint_id, reference.active_endpoint_id);
    assert_eq!(cached.snapshot, reference.snapshot);
    assert_eq!(cached.layouts, reference.layouts);
    assert_eq!(cached.endpoints.len(), reference.endpoints.len());
    for (a, b) in cached.endpoints.iter().zip(&reference.endpoints) {
        assert_eq!(a.endpoint_id, b.endpoint_id);
        assert_eq!(a.label, b.label);
        assert_eq!(a.status, b.status);
        assert_eq!(a.snapshot.as_deref(), b.snapshot.as_deref());
        assert_eq!(a.snapshot_generation, b.snapshot_generation);
        assert_eq!(a.agent_recency, b.agent_recency);
    }
}

#[test]
fn an_unchanged_republish_needs_no_projection() {
    let mut cache = ProjectCache::default();
    let view = multi();
    assert!(
        !cache.is_unchanged(&view, Some("t1")),
        "the first view is always a rebuild"
    );
    cache.project(&view, Some("t1"));
    assert!(cache.is_unchanged(&view, Some("t1")));
    // A republish with a fresh inventory Arc — a `doc` wire event bumps
    // only `seq` — is still unchanged; a focus change is not.
    let mut republish = view.clone();
    republish.version += 1;
    republish
        .inventory_mut()
        .apply(&hc_proto::events::EventParams {
            seq: 2,
            kind: "noop".into(),
            terminal: None,
            machine: None,
            data: serde_json::json!({}),
        });
    assert!(cache.is_unchanged(&republish, Some("t1")));
    assert!(!cache.is_unchanged(&republish, Some("host-b/t1")));
}

#[test]
fn a_changed_remote_rebuilds_only_its_endpoint() {
    let mut cache = ProjectCache::default();
    let view = multi();
    cache.project(&view, Some("t1"));
    let local_spaces = Arc::clone(&entry(&cache, None).spaces);
    let a_spaces = Arc::clone(&entry(&cache, Some("host-a")).spaces);
    let b_spaces = Arc::clone(&entry(&cache, Some("host-b")).spaces);
    let local_builds = entry(&cache, None).snapshot_builds;
    let a_builds = entry(&cache, Some("host-a")).snapshot_builds;

    let mut renamed = view.clone();
    renamed.inventory_mut().replace(
        2,
        vec![
            labeled("t1", "work", "one"),
            labeled("host-a/t1", "work", "one"),
            {
                let mut changed = labeled("host-b/t1", "work", "one");
                changed.title = "renamed".into();
                changed
            },
        ],
    );
    let rebuilt = cache.project(&renamed, Some("t1"));

    // Local and host-a keep their exact structure + snapshot allocations;
    // host-b's title is not structural: same spaces, rebuilt snapshot.
    assert!(Arc::ptr_eq(&local_spaces, &entry(&cache, None).spaces));
    assert!(Arc::ptr_eq(
        &a_spaces,
        &entry(&cache, Some("host-a")).spaces
    ));
    assert!(Arc::ptr_eq(
        &b_spaces,
        &entry(&cache, Some("host-b")).spaces
    ));
    assert_eq!(entry(&cache, None).snapshot_builds, local_builds);
    assert_eq!(entry(&cache, Some("host-a")).snapshot_builds, a_builds);
    assert_same_projection(&rebuilt, &crate::project::project(&renamed, Some("t1")));
    assert!(
        rebuilt.endpoints.iter().any(|e| e
            .snapshot
            .as_deref()
            .is_some_and(|s| s.panes.iter().any(|p| p.pane_id == "host-b/t1"))),
        "host-b's rebuilt snapshot still carries its pane"
    );
}

#[test]
fn an_app_document_change_rebuilds_only_its_endpoint() {
    let mut cache = ProjectCache::default();
    let mut view = multi();
    let app_layout = serde_json::json!({"spaces": []});
    set_doc(&mut view, Some("host-a"), "herdr-app", app_layout.clone());
    set_doc(&mut view, Some("host-b"), "herdr-app", app_layout);
    cache.project(&view, Some("t1"));
    let a_builds = entry(&cache, Some("host-a")).snapshot_builds;
    let local_builds = entry(&cache, None).snapshot_builds;

    set_doc(
        &mut view,
        Some("host-b"),
        "herdr-app",
        serde_json::json!({"spaces": [{"id": "space:4:shop", "name": "shop",
            "folder": "/home/user", "tabs": []}]}),
    );
    let rebuilt = cache.project(&view, Some("t1"));

    assert_eq!(entry(&cache, None).snapshot_builds, local_builds);
    assert_eq!(entry(&cache, Some("host-a")).snapshot_builds, a_builds);
    assert!(entry(&cache, Some("host-b")).snapshot_builds > 1);
    assert_same_projection(&rebuilt, &crate::project::project(&view, Some("t1")));
}

#[test]
fn a_remote_focus_flip_leaves_local_untouched() {
    let mut cache = ProjectCache::default();
    let view = multi();
    cache.project(&view, Some("host-a/t1"));
    let local_builds = entry(&cache, None).snapshot_builds;
    let local_spaces = Arc::clone(&entry(&cache, None).spaces);

    // Focus moves between remote panes: local's selected_here stays
    // `None`, so only the two remotes re-resolve focus.
    let rebuilt = cache.project(&view, Some("host-b/t1"));

    assert_eq!(entry(&cache, None).snapshot_builds, local_builds);
    assert!(Arc::ptr_eq(&local_spaces, &entry(&cache, None).spaces));
    assert_same_projection(&rebuilt, &crate::project::project(&view, Some("host-b/t1")));
}

/// `host-b`'s agent terminal carries `word` with `title` mutated by
/// `edit`; `replace` republishes it under a fresh inventory `seq`.
fn edited_host_b(view: &View, seq: u64, word: &str, edit: impl FnOnce(&mut Terminal)) -> View {
    let mut next = view.clone();
    next.inventory_mut().replace(
        seq,
        vec![
            labeled("t1", "work", "one"),
            labeled("host-a/t1", "work", "one"),
            {
                let mut terminal = labeled_agent("host-b/t1", "work", "one", word);
                edit(&mut terminal);
                terminal
            },
        ],
    );
    next
}

#[test]
fn a_title_change_keeps_the_spaces_and_updates_the_snapshot() {
    let mut cache = ProjectCache::default();
    let view = edited_host_b(&multi(), 1, "working", |_| {});
    cache.project(&view, Some("t1"));
    let spaces = Arc::clone(&entry(&cache, Some("host-b")).spaces);
    let builds = entry(&cache, Some("host-b")).snapshot_builds;

    let titled = edited_host_b(&view, 2, "working", |t| t.title = "renamed".into());
    let rebuilt = cache.project(&titled, Some("t1"));

    // The structure groups on labels, not titles: same `Space`s, fresh
    // per-terminal snapshot carrying the new title.
    assert!(Arc::ptr_eq(&spaces, &entry(&cache, Some("host-b")).spaces));
    assert!(entry(&cache, Some("host-b")).snapshot_builds > builds);
    assert_same_projection(&rebuilt, &crate::project::project(&titled, Some("t1")));
    let agent = entry(&cache, Some("host-b"))
        .snapshot
        .agents
        .iter()
        .find(|a| a.pane_id == "host-b/t1")
        .expect("host-b agent");
    assert_eq!(agent.terminal_title.as_deref(), Some("renamed"));
}

#[test]
fn a_status_change_keeps_the_spaces_and_updates_the_snapshot() {
    let mut cache = ProjectCache::default();
    let view = edited_host_b(&multi(), 1, "working", |_| {});
    cache.project(&view, Some("t1"));
    let spaces = Arc::clone(&entry(&cache, Some("host-b")).spaces);
    let builds = entry(&cache, Some("host-b")).snapshot_builds;

    let blocked = edited_host_b(&view, 2, "blocked", |_| {});
    let rebuilt = cache.project(&blocked, Some("t1"));

    assert!(Arc::ptr_eq(&spaces, &entry(&cache, Some("host-b")).spaces));
    assert!(entry(&cache, Some("host-b")).snapshot_builds > builds);
    assert_same_projection(&rebuilt, &crate::project::project(&blocked, Some("t1")));
    let agent = entry(&cache, Some("host-b"))
        .snapshot
        .agents
        .iter()
        .find(|a| a.pane_id == "host-b/t1")
        .expect("host-b agent");
    assert_eq!(
        agent.agent_status,
        crate::chrome::status::AgentStatus::Blocked
    );
}

#[test]
fn a_structural_change_rebuilds_the_spaces() {
    let mut cache = ProjectCache::default();
    let view = multi();
    cache.project(&view, Some("t1"));

    // A `herdr.tab` move regroups host-b's terminal under another tab.
    let mut retabbed = view.clone();
    retabbed.inventory_mut().replace(
        2,
        vec![
            labeled("t1", "work", "one"),
            labeled("host-a/t1", "work", "one"),
            labeled("host-b/t1", "work", "two"),
        ],
    );
    let spaces = Arc::clone(&entry(&cache, Some("host-b")).spaces);
    cache.project(&retabbed, Some("t1"));
    assert!(!Arc::ptr_eq(&spaces, &entry(&cache, Some("host-b")).spaces));

    // A second terminal on host-b adds a pane: structure must re-read.
    let mut grown = view.clone();
    grown.inventory_mut().replace(
        3,
        vec![
            labeled("t1", "work", "one"),
            labeled("host-a/t1", "work", "one"),
            labeled("host-b/t1", "work", "two"),
            labeled("host-b/t2", "work", "two"),
        ],
    );
    let spaces = Arc::clone(&entry(&cache, Some("host-b")).spaces);
    let rebuilt = cache.project(&grown, Some("t1"));
    assert!(!Arc::ptr_eq(&spaces, &entry(&cache, Some("host-b")).spaces));
    assert_same_projection(&rebuilt, &crate::project::project(&grown, Some("t1")));
}

#[test]
fn a_dropped_remote_is_pruned_from_the_cache() {
    let mut cache = ProjectCache::default();
    let view = multi();
    cache.project(&view, Some("t1"));
    assert!(cache.endpoints.contains_key(&Some("host-b".into())));

    let mut fewer = view.clone();
    fewer.machines = Some(vec![machine("host-a", "connected")]);
    fewer.inventory_mut().replace(
        2,
        vec![
            labeled("t1", "work", "one"),
            labeled("host-a/t1", "work", "one"),
        ],
    );
    cache.project(&fewer, Some("t1"));

    assert!(!cache.endpoints.contains_key(&Some("host-b".into())));
}

#[test]
fn a_closed_app_tab_terminal_finishing_invalidates_cached_spaces() {
    let mut cache = ProjectCache::default();
    let app_doc = serde_json::json!({
        "spaces": [{
            "id": "s1",
            "name": "work",
            "folder": "/home/user/proj",
            "tabs": [{
                "id": "tab1",
                "name": "",
                "root": {"type": "block", "content": {"terminal": "t1"}}
            }]
        }]
    });
    let t1 = agent("t1", "idle");
    let t2_live = labeled("t2", "s1", "tbffc6bc4f8e6c92a83f0b407605ffdec5");

    let mut live_view = view(vec![t1.clone(), t2_live.clone()], vec![]);
    set_doc(&mut live_view, None, "herdr-app", app_doc);
    let initial = cache.project(&live_view, Some("t1"));
    assert_eq!(initial.snapshot.tabs.len(), 2);
    let spaces = Arc::clone(&entry(&cache, None).spaces);

    let mut t2_finished = t2_live;
    t2_finished.state = "finished".into();
    t2_finished.reason = Some("killed".into());
    let mut finished_view = live_view.clone();
    finished_view
        .inventory_mut()
        .replace(2, vec![t1, t2_finished]);

    let rebuilt = cache.project(&finished_view, Some("t1"));
    assert!(!Arc::ptr_eq(&spaces, &entry(&cache, None).spaces));
    assert_eq!(rebuilt.snapshot.tabs.len(), 1);
    assert!(rebuilt.snapshot.panes.iter().all(|p| p.pane_id != "t2"));
    assert_same_projection(
        &rebuilt,
        &crate::project::project(&finished_view, Some("t1")),
    );
}
