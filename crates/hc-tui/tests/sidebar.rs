//! Sidebar model tests: grouping, status authority, counters, workers,
//! ordering, sanitization and selection — all over neutral fixtures.

mod common;

use common::{agent, machine, status, term, view};
use hc_proto::watch::{Link, View};
use hc_tui::sidebar::{Counters, RowId, Sidebar, ViewMode};

fn sidebar(view: &View, mode: ViewMode) -> Sidebar {
    let mut sb = Sidebar::new();
    sb.set_mode(mode);
    sb.update(view);
    sb
}

fn rows(sb: &Sidebar) -> Vec<String> {
    sb.rows().iter().map(|r| r.text.clone()).collect()
}

#[test]
fn spaces_group_by_label_then_cwd() {
    let mut labeled = agent("t1", "working");
    labeled.labels.insert("herdr.space".into(), "webapp".into());
    let mut other_dir = agent("t2", "idle");
    other_dir.cwd = "/home/user/api".into();
    let mut same_base = agent("t3", "idle");
    same_base.cwd = "/opt/proj".into(); // same basename as /home/user/proj
    let view = view(vec![labeled, other_dir, same_base, term("t4")], vec![]);
    let sb = sidebar(&view, ViewMode::Spaces);

    let texts = rows(&sb);
    // Label space, the api cwd space, and two distinct `proj` spaces.
    assert!(texts.contains(&"webapp (1)".to_string()));
    assert!(texts.contains(&"api (1)".to_string()));
    assert_eq!(
        texts.iter().filter(|t| t.as_str() == "proj (1)").count(),
        2,
        "{texts:?}"
    );
    assert_eq!(texts[0], "host-a");
}

#[test]
fn machines_keep_offline_state_and_inventory() {
    let mut down = machine("host-b", "unreachable");
    down.why = Some("ssh: connection refused".into());
    let view = view(
        vec![agent("host-b/t7", "blocked"), agent("t1", "done")],
        vec![down],
    );
    let sb = sidebar(&view, ViewMode::Spaces);
    let rows = sb.rows();

    let hb = rows
        .iter()
        .find(|r| r.id == RowId::Machine("host-b".into()))
        .unwrap();
    assert_eq!(hb.badge, "unreachable");
    assert_eq!(hb.note, "ssh: connection refused");
    assert!(hb.dim);
    let t = rows
        .iter()
        .find(|r| r.id == RowId::Terminal("host-b/t7".into()))
        .unwrap();
    assert!(t.dim, "offline machine dims its retained inventory");
    assert_eq!(t.badge, "blocked");
    // Offline machines still count as machines.
    assert_eq!(sb.summary().machines, 2);
    assert_eq!(sb.summary().offline, 1);
}

#[test]
fn counters_cover_agents_only_once_each() {
    let mut worker = agent("t9", "working");
    worker.parent = Some("t1".into());
    let view = view(
        vec![
            agent("t1", "blocked"),
            agent("t2", "working"),
            agent("t3", "done"),
            agent("t4", "idle"),
            worker,
            term("t5"), // a shell: not an agent, never counted
        ],
        vec![],
    );
    let sb = sidebar(&view, ViewMode::Spaces);
    assert_eq!(
        sb.counters(),
        Counters {
            your_turn: 1,
            working: 2,
            ready: 1
        }
    );
}

#[test]
fn agents_view_orders_blocked_first_then_deterministic() {
    let view = view(
        vec![
            agent("t1", "idle"),
            agent("t2", "blocked"),
            agent("t3", "done"),
            agent("host-b/t4", "blocked"),
            agent("host-a2/t1", "working"),
            term("t9"),
        ],
        vec![
            machine("host-a2", "connected"),
            machine("host-b", "connected"),
        ],
    );
    let sb = sidebar(&view, ViewMode::Agents);
    let order: Vec<&str> = sb
        .rows()
        .iter()
        .filter_map(|r| match &r.id {
            RowId::Terminal(a) => Some(a.as_str()),
            _ => None,
        })
        .collect();
    // Blocked first; ties break machine then number; shells excluded.
    assert_eq!(order, ["t2", "host-b/t4", "host-a2/t1", "t3", "t1"]);
}

#[test]
fn remote_worker_with_local_parent_is_not_reprefixed() {
    let mut worker = agent("host-b/t9", "working");
    // Core already localized the parent: this remote worker was
    // started by local t2 — it must NOT become host-b/t2.
    worker.parent = Some("t2".into());
    let mut launcher = agent("t2", "idle");
    launcher.name = Some("boss".into());
    let view = view(vec![worker, launcher], vec![machine("host-b", "connected")]);
    let sb = sidebar(&view, ViewMode::Spaces);
    let row = sb
        .rows()
        .iter()
        .find(|r| r.id == RowId::Terminal("host-b/t9".into()))
        .unwrap();
    assert_eq!(row.note, "worker of t2 boss");
}

#[test]
fn unknown_parent_stays_visible() {
    let mut worker = agent("t5", "working");
    worker.parent = Some("host-c/t1".into());
    let view = view(vec![worker], vec![]);
    let sb = sidebar(&view, ViewMode::Spaces);
    let row = &sb.rows()[2];
    assert_eq!(row.id, RowId::Terminal("t5".into()));
    assert_eq!(row.note, "worker of host-c/t1");
}

#[test]
fn unknown_words_and_sources_survive() {
    let mut odd = agent("t1", "negotiating");
    odd.status
        .as_mut()
        .unwrap()
        .extra
        .insert("source".into(), serde_json::Value::String("screen".into()));
    let mut named = agent("t2", "working");
    named.records.insert("sub:build".into(), status("blocked"));
    let view = view(vec![odd, named], vec![]);
    let sb = sidebar(&view, ViewMode::Agents);
    let badges: Vec<&str> = sb.rows().iter().map(|r| r.badge.as_str()).collect();
    // The reported word leads; the named record trails as `word·name`.
    assert!(badges.contains(&"blocked·sub:build"));
    assert!(badges.contains(&"negotiating·screen"));
}

#[test]
fn dead_agents_stay_out_of_live_counters() {
    let mut finished = agent("t1", "blocked");
    finished.state = "finished".into();
    let mut lost = agent("t2", "working");
    lost.pid = None;
    lost.reason = Some("holder_lost".into());
    let mut pending = agent("t3", "done");
    pending.pid = None;
    let v = view(vec![finished, lost, pending, agent("t4", "idle")], vec![]);
    let sb = sidebar(&v, ViewMode::Agents);
    assert_eq!(
        sb.counters(),
        Counters {
            your_turn: 0,
            working: 0,
            ready: 0
        },
        "last reports of finished/lost/pending programs are not live"
    );
}

#[test]
fn unknown_lifecycle_words_are_not_finished() {
    let mut odd = agent("t1", "working");
    odd.state = "suspended".into();
    let v = view(vec![odd], vec![]);
    let sb = sidebar(&v, ViewMode::Agents);
    let row = &sb.rows()[0];
    assert_eq!(row.badge, "suspended");
}

#[test]
fn remote_rows_go_stale_on_subscription_loss() {
    let mut v = view(
        vec![agent("host-b/t1", "working")],
        vec![machine("host-b", "connected")],
    );
    v.link = Link::Offline {
        attempt: 3,
        error: "gone".into(),
    };
    v.inventory.set_offline(true);
    let sb = sidebar(&v, ViewMode::Spaces);
    let m = sb
        .rows()
        .iter()
        .find(|r| r.id == RowId::Machine("host-b".into()))
        .unwrap();
    assert_eq!(m.badge, "connected·stale");
    assert!(m.dim);
    assert_eq!(
        sb.summary().offline,
        2,
        "local link is dead too — both machines count offline"
    );
    let t = sb
        .rows()
        .iter()
        .find(|r| r.id == RowId::Terminal("host-b/t1".into()))
        .unwrap();
    assert!(t.dim, "cached remote terminal dims on stream loss");
}

#[test]
fn rpc_loss_marks_machine_state_stale_but_keeps_terminals_live() {
    // `machine.list` refreshes over rpc; the terminal stream does not.
    // With rpc dead but the subscription healthy, the machine's state
    // word is the stale part — the terminals are still live rows.
    let mut v = view(
        vec![agent("host-b/t1", "working")],
        vec![machine("host-b", "connected")],
    );
    v.rpc = Link::Offline {
        attempt: 1,
        error: "rpc gone".into(),
    };
    let sb = sidebar(&v, ViewMode::Spaces);
    let m = sb
        .rows()
        .iter()
        .find(|r| r.id == RowId::Machine("host-b".into()))
        .unwrap();
    assert_eq!(m.badge, "connected·stale");
    assert!(!m.dim, "rpc loss alone must not dim the machine");
    let t = sb
        .rows()
        .iter()
        .find(|r| r.id == RowId::Terminal("host-b/t1".into()))
        .unwrap();
    assert!(!t.dim, "its terminals still stream over the subscription");
    assert_eq!(sb.summary().offline, 0, "not counted offline either");
    // …and the agent's badge is untouched by the refresh-path failure.
    assert_eq!(t.badge, "working");
}

#[test]
fn finished_lost_pending_and_offline_are_distinct() {
    let mut finished = agent("t1", "done");
    finished.state = "finished".into();
    finished.exit_code = Some(0);
    let mut lost = agent("t2", "working");
    lost.pid = None;
    lost.reason = Some("holder_lost".into());
    let mut pending = agent("t3", "working");
    pending.pid = None;
    pending.reason = Some("spawn_pending".into());
    let mut v = view(
        vec![finished, lost, pending, agent("t4", "blocked")],
        vec![],
    );
    let sb = sidebar(&v, ViewMode::Agents);
    let badge = |a: &str| {
        sb.rows()
            .iter()
            .find(|r| r.id == RowId::Terminal(a.into()))
            .map(|r| r.badge.as_str())
    };
    assert_eq!(badge("t1"), Some("finished:0"));
    assert_eq!(badge("t2"), Some("lost·holder_lost"));
    assert_eq!(badge("t3"), Some("starting"));

    // Now the whole local link drops: inventory stays, rows dim.
    v.link = Link::Offline {
        attempt: 2,
        error: "socket closed".into(),
    };
    v.inventory.set_offline(true);
    let mut sb2 = Sidebar::new();
    sb2.update(&v);
    let local = &sb2.rows()[0];
    assert_eq!(local.badge, "offline");
    assert_eq!(local.note, "socket closed");
    assert!(local.dim);
    let t4 = sb2
        .rows()
        .iter()
        .find(|r| r.id == RowId::Terminal("t4".into()))
        .unwrap();
    assert!(t4.dim);
    assert_eq!(t4.badge, "blocked", "last status stays, only dimmed");
}

#[test]
fn unlisted_machine_prefix_gets_its_own_group() {
    let view = view(vec![agent("host-z/t1", "idle")], vec![]);
    let sb = sidebar(&view, ViewMode::Spaces);
    let group = sb
        .rows()
        .iter()
        .find(|r| r.id == RowId::Machine("host-z".into()))
        .unwrap();
    assert_eq!(group.badge, "unlisted");
    assert!(group.dim);
}

#[test]
fn every_string_is_sanitized() {
    let mut t = agent("t1", "working");
    t.name = Some("bad\x1b[2Jname".into());
    t.title = "evil\x1b]8;;x\x07title".into();
    t.cwd = "/home/user/\x1bdir".into();
    let mut dirty = agent("t\x1b[0m9", "working");
    dirty.name = Some("tricky".into());
    let mut v = view(vec![t, dirty], vec![]);
    v.link = Link::Offline {
        attempt: 1,
        error: "oops\x1b[31mred".into(),
    };
    let mut m = machine("host-b\x07", "unreachable");
    m.why = Some("why\x1b!".into());
    v.machines = Some(vec![m]);
    let mut sb = Sidebar::new();
    sb.update(&v);
    for row in sb.rows() {
        for s in [&row.text, &row.note, &row.badge] {
            assert!(
                !s.chars().any(|c| c.is_control()),
                "control char survived in {s:?}"
            );
        }
    }
}

#[test]
fn selection_survives_machine_and_space_reorder() {
    let one = view(vec![agent("t1", "idle"), agent("t9", "idle")], vec![]);
    let mut sb = Sidebar::new();
    sb.update(&one);
    sb.last();
    assert_eq!(sb.selected_terminal(), Some("t9"));
    // A snapshot ordering terminals differently keeps t9 selected.
    let two = view(vec![agent("t9", "idle"), agent("t1", "idle")], vec![]);
    sb.update(&two);
    assert_eq!(sb.selected_terminal(), Some("t9"));
    // Removing it selects whatever now sits at its old position.
    let three = view(vec![agent("t1", "idle")], vec![]);
    sb.update(&three);
    assert!(sb.selected().is_some());
}

#[test]
fn mode_switch_resets_to_top() {
    let view = view(vec![agent("t1", "blocked"), agent("t2", "idle")], vec![]);
    let mut sb = Sidebar::new();
    sb.update(&view);
    sb.last();
    sb.set_mode(ViewMode::Agents);
    sb.update(&view);
    assert_eq!(sb.selected_index(), Some(0));
    assert_eq!(sb.selected_terminal(), Some("t1"));
}
