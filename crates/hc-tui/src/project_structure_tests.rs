use super::*;
use crate::sidebar::fixture::{term, view};
use serde_json::json;
use std::sync::Arc;

fn doc(view: &mut View, name: &str, machine: Option<&str>, value: serde_json::Value) {
    view.documents_mut().insert(
        hc_proto::watch::DocKey {
            machine: machine.map(str::to_owned),
            name: name.into(),
        },
        Arc::new(hc_proto::watch::Document {
            value,
            version: 1,
            ..Default::default()
        }),
    );
}

#[test]
fn app_document_wins_labels_and_remote_ids_are_qualified_once() {
    let mut a = term("t1");
    a.labels.insert("herdr.space".into(), "wrong".into());
    let mut b = term("host-a/t1");
    b.labels.insert("herdr.space".into(), "wrong".into());
    let mut v = view(vec![a, b], vec![]);
    let structure = json!({"future_version":123,"spaces":[{"id":"s","name":"work","folder":"/work","tabs":[{"id":"tab","name":"task","root":{"type":"block","content":{"terminal":"t1"}}}]}]});
    doc(&mut v, "herdr-app", None, structure.clone());
    doc(&mut v, "herdr-app", Some("host-a"), structure);
    for selected in ["t1", "host-a/t1"] {
        let projected = project(&v, Some(selected));
        assert_eq!(projected.snapshot.workspaces.len(), 1);
        assert_eq!(projected.snapshot.workspaces[0].label, "work");
        assert_eq!(projected.snapshot.tabs[0].label, "task");
        assert_eq!(projected.snapshot.panes[0].pane_id, selected);
    }
}

#[test]
fn own_layout_preserves_new_members_and_prunes_cross_tab_or_stale_leaves() {
    let mut terms = vec![term("t1"), term("t2"), term("t3")];
    for t in &mut terms {
        t.labels.insert("herdr.space".into(), "s".into());
        t.labels.insert(
            "herdr.tab".into(),
            if t.address == "t3" { "other" } else { "tab" }.into(),
        );
    }
    let mut v = view(terms, vec![]);
    let base = project(&v, Some("t1"));
    let tab = base.snapshot.focused_tab_id.unwrap();
    let root = crate::structure::Tree::split(
        crate::structure::Tree::pane("t3"),
        crate::structure::Tree::pane("t1"),
        true,
        0.25,
    );
    let mut own = crate::structure::Document::default();
    own.tabs.insert(
        layout_key(None, &tab),
        crate::structure::TabLayout {
            root: Some(root),
            focused: Some("t1".into()),
            zoomed: Some("gone".into()),
            name: Some("renamed".into()),
            ..Default::default()
        },
    );
    doc(&mut v, "hc-tui", None, serde_json::to_value(own).unwrap());
    let projected = project(&v, Some("t1"));
    let layout = &projected.layouts[&tab];
    assert_eq!(layout.root.as_ref().unwrap().terminals(), ["t1", "t2"]);
    assert!(layout.zoomed.is_none());
    assert_eq!(
        projected
            .snapshot
            .tabs
            .iter()
            .find(|t| t.tab_id == tab)
            .unwrap()
            .label,
        "renamed"
    );
}

#[test]
fn adding_many_members_to_a_saved_layout_keeps_projection_bounded() {
    let terms = (1..=512)
        .map(|n| term(&format!("t{n}")))
        .collect::<Vec<_>>();
    let mut v = view(terms, vec![]);
    let tab = project(&v, Some("t1")).snapshot.focused_tab_id.unwrap();
    let key = layout_key(None, &tab);
    doc(
        &mut v,
        "hc-tui",
        None,
        json!({"schema":1,"tabs":{key:{"root":{"type":"pane","terminal":"t1"}}}}),
    );
    let projected = project(&v, Some("t1"));
    let root = projected.layouts[&tab].root.as_ref().unwrap();
    assert!(root.valid());
    assert_eq!(root.terminals().len(), 512);
}

fn foreground(name: &str, cwd: &str) -> Option<hc_proto::terminal::Foreground> {
    Some(hc_proto::terminal::Foreground {
        pid: 200,
        name: name.into(),
        argv: vec![name.into()],
        cwd: cwd.into(),
    })
}

fn app_space(id: &str, name: &str, folder: &str, tabs: &[(&str, &str, &str)]) -> serde_json::Value {
    let tabs = tabs
        .iter()
        .map(|(tab, tab_name, terminal)| {
            json!({"id":tab,"name":tab_name,"root":{"type":"block","content":{"terminal":terminal}}})
        })
        .collect::<Vec<_>>();
    json!({"id":id,"name":name,"folder":folder,"tabs":tabs})
}

#[test]
fn unnamed_app_space_follows_its_first_terminals_live_folder() {
    // A space the app saved with no name and no folder, and one whose
    // folder is where its shell started while the agent works elsewhere.
    let mut a = crate::sidebar::fixture::agent("t1", "idle");
    a.cwd = "/home/user/dev-workspace".into();
    let mut b = crate::sidebar::fixture::agent("t2", "done");
    b.cwd = "/home/user/dev-workspace".into();
    b.foreground = foreground("claude", "/home/user/paidiron");
    let mut v = view(vec![a, b], vec![]);
    doc(
        &mut v,
        "herdr-app",
        None,
        json!({"spaces":[
            app_space("s1", "", "", &[("tab1", "", "t1")]),
            app_space("s2", "", "/home/user/dev-workspace", &[("tab2", "", "t2")]),
        ]}),
    );
    let labels = project(&v, Some("t1"))
        .snapshot
        .workspaces
        .iter()
        .map(|w| (w.label.clone(), w.custom_label))
        .collect::<Vec<_>>();
    assert_eq!(
        labels,
        [
            ("dev-workspace".to_owned(), false),
            ("paidiron".to_owned(), false)
        ]
    );
}

#[test]
fn a_named_app_space_keeps_its_name_over_the_live_folder() {
    let mut a = crate::sidebar::fixture::agent("t1", "idle");
    a.foreground = foreground("claude", "/home/user/elsewhere");
    let mut v = view(vec![a], vec![]);
    doc(
        &mut v,
        "herdr-app",
        None,
        json!({"spaces":[app_space("s1", "work", "/work", &[("tab1", "", "t1")])]}),
    );
    assert_eq!(project(&v, Some("t1")).snapshot.workspaces[0].label, "work");
}

#[test]
fn unnamed_app_tab_shows_its_agents_live_title() {
    let mut titled = crate::sidebar::fixture::agent("t1", "working");
    titled.title = "✳ Fix www addresses".into();
    let mut v = view(vec![titled], vec![]);
    doc(
        &mut v,
        "herdr-app",
        None,
        json!({"spaces":[app_space("s1", "", "/w", &[("tab1", "", "t1")])]}),
    );
    let tab = &project(&v, Some("t1")).snapshot.tabs[0];
    assert_eq!(tab.label, "Fix www addresses");
    assert!(!tab.custom_label);
}

#[test]
fn app_tab_shows_its_job_when_the_title_says_nothing() {
    // Each title here names only the program, a path, a user@host or the
    // terminal; the tab shows the job running in it instead.
    for title in [
        "",
        "claude",
        "~/proj",
        "/home/user/proj",
        "user@host: ~",
        "t1",
    ] {
        let mut agent = crate::sidebar::fixture::agent("t1", "idle");
        agent.title = title.into();
        agent.foreground = foreground("claude", "/w");
        let mut v = view(vec![agent], vec![]);
        doc(
            &mut v,
            "herdr-app",
            None,
            json!({"spaces":[app_space("s1", "", "/w", &[("tab1", "", "t1")])]}),
        );
        assert_eq!(
            project(&v, Some("t1")).snapshot.tabs[0].label,
            "claude",
            "{title:?}"
        );
    }
}

#[test]
fn app_tab_live_title_needs_an_agent_and_yields_to_a_name() {
    // A plain shell's title is not a tab name (its job is), and a name
    // someone gave stays over both the agent's title and its job.
    let mut shell = term("t1");
    shell.title = "Some shell title".into();
    shell.foreground = foreground("zsh", "/w");
    let mut named = crate::sidebar::fixture::agent("t2", "idle");
    named.title = "✳ Agent title".into();
    let mut v = view(vec![shell, named], vec![]);
    doc(
        &mut v,
        "herdr-app",
        None,
        json!({"spaces":[app_space("s1", "", "/w", &[("tab1", "", "t1"), ("tab2", "mine", "t2")])]}),
    );
    let labels = project(&v, Some("t1"))
        .snapshot
        .tabs
        .iter()
        .map(|t| t.label.clone())
        .collect::<Vec<_>>();
    assert_eq!(labels, ["zsh", "mine"]);
}

#[test]
fn spaces_outside_the_app_document_keep_their_reference_names() {
    // Live folder naming applies to app spaces only; tabs, such as one
    // opened from hc-tui itself, name themselves like the app's everywhere.
    let mut a = crate::sidebar::fixture::agent("t1", "idle");
    a.title = "✳ Agent title".into();
    a.foreground = foreground("claude", "/home/user/elsewhere");
    let projected = project(&view(vec![a], vec![]), Some("t1"));
    assert_eq!(projected.snapshot.workspaces[0].label, "proj");
    assert_eq!(projected.snapshot.tabs[0].label, "Agent title");
}

/// The tab labels of a one-space projection whose tabs each hold `terms`.
fn tab_labels(terms: Vec<Terminal>) -> Vec<String> {
    let first = terms[0].address.clone();
    project(&view(terms, vec![]), Some(&first))
        .snapshot
        .tabs
        .iter()
        .map(|t| t.label.clone())
        .collect()
}

#[test]
fn an_unnamed_shell_tab_shows_the_job_it_started_as() {
    // macOS names a login shell `-zsh` and Claude Code by its version;
    // the tab shows the name each program was started under.
    let started = [
        (vec!["/bin/zsh", "-l"], "zsh", "zsh"),
        (vec!["-zsh"], "zsh", "zsh"),
        (vec!["claude", "--resume", "x"], "2.1.330", "claude"),
        (vec!["/usr/bin/vim", "notes.md"], "vim", "vim"),
    ];
    for (argv, process_name, label) in started {
        let mut shell = term("t1");
        shell.foreground = foreground(process_name, "/w");
        shell.foreground.as_mut().unwrap().argv = argv.iter().map(|a| a.to_string()).collect();
        assert_eq!(tab_labels(vec![shell]), [label], "{argv:?}");
    }
}

#[test]
fn a_job_with_no_start_name_falls_back_to_its_process_name() {
    for argv in [vec![], vec![String::new()], vec!["-".to_owned()]] {
        let mut shell = term("t1");
        shell.foreground = foreground("bash", "/w");
        shell.foreground.as_mut().unwrap().argv = argv.clone();
        assert_eq!(tab_labels(vec![shell]), ["bash"], "{argv:?}");
    }
}

#[test]
fn a_tab_keeps_its_number_when_no_live_terminal_reports_a_job() {
    // A shell whose foreground is unknown, and a finished terminal that
    // still reports the program it ran.
    let unknown = term("t1");
    let mut finished = tagged(term("t2"), "work", "");
    finished.state = "finished".into();
    finished.foreground = foreground("zsh", "/w");
    let mut nameless = term("t3");
    nameless.foreground = foreground("", "/w");
    nameless.foreground.as_mut().unwrap().argv.clear();
    assert_eq!(tab_labels(vec![unknown]), ["1"]);
    assert_eq!(tab_labels(vec![finished]), ["1"]);
    assert_eq!(tab_labels(vec![nameless]), ["1"]);
}

#[test]
fn a_tab_shows_the_job_of_its_first_live_terminal() {
    // The first pane has finished; the live one beside it names the tab.
    let mut done = term("t1");
    done.state = "finished".into();
    done.foreground = foreground("make", "/w");
    let mut live = term("t2");
    live.foreground = foreground("htop", "/w");
    let mut v = view(vec![done, live], vec![]);
    doc(
        &mut v,
        "herdr-app",
        None,
        json!({"spaces":[{"id":"s1","name":"","folder":"/w","tabs":[{"id":"tab1","name":"","root":{"type":"row","children":[
            {"type":"block","content":{"terminal":"t1"}},
            {"type":"block","content":{"terminal":"t2"}}]}}]}]}),
    );
    let snapshot = project(&v, Some("t2")).snapshot;
    assert_eq!(snapshot.tabs.len(), 1, "{:?}", snapshot.tabs);
    assert_eq!(snapshot.tabs[0].label, "htop");
}

fn tagged(mut terminal: Terminal, space: &str, tab: &str) -> Terminal {
    terminal.labels.insert("herdr.space".into(), space.into());
    terminal.labels.insert("herdr.tab".into(), tab.into());
    terminal
}

const CLOSED_TAB: &str = "tbffc6bc4f8e6c92a83f0b407605ffdec5";

#[test]
fn a_finished_terminal_of_a_closed_app_tab_does_not_come_back_as_a_tab() {
    let kept = crate::sidebar::fixture::agent("t1", "idle");
    let mut killed = tagged(term("t2"), "s1", CLOSED_TAB);
    killed.state = "finished".into();
    killed.reason = Some("killed".into());
    let mut v = view(vec![kept, killed], vec![]);
    doc(
        &mut v,
        "herdr-app",
        None,
        json!({"spaces":[app_space("s1", "", "/w", &[("tab1", "", "t1")])]}),
    );
    let snapshot = project(&v, Some("t1")).snapshot;
    assert_eq!(snapshot.tabs.len(), 1, "{:?}", snapshot.tabs);
    assert!(snapshot.panes.iter().all(|pane| pane.pane_id != "t2"));
}

#[test]
fn a_live_terminal_of_a_closed_app_tab_stays_without_its_raw_tab_id() {
    // Still running, so it stays reachable; the app's tab id is a key, not
    // a name, so the tab takes its number like any unnamed tab.
    let kept = crate::sidebar::fixture::agent("t1", "idle");
    let orphan = tagged(term("t2"), "s1", CLOSED_TAB);
    let mut v = view(vec![kept, orphan], vec![]);
    doc(
        &mut v,
        "herdr-app",
        None,
        json!({"spaces":[app_space("s1", "", "/w", &[("tab1", "", "t1")])]}),
    );
    let labels = project(&v, Some("t1"))
        .snapshot
        .tabs
        .iter()
        .map(|t| t.label.clone())
        .collect::<Vec<_>>();
    assert_eq!(labels, ["1", "2"]);
}

#[test]
fn a_finished_terminal_outside_any_app_space_stays_as_a_pane() {
    let mut finished = tagged(term("t1"), "work", "task");
    finished.state = "finished".into();
    finished.reason = Some("killed".into());
    let snapshot = project(&view(vec![finished], vec![]), Some("t1")).snapshot;
    assert_eq!(snapshot.tabs.len(), 1);
    assert_eq!(snapshot.tabs[0].label, "task");
    assert_eq!(snapshot.panes[0].pane_id, "t1");
}

/// A one-terminal space hc-tui created: tagged `herdr.space`/`herdr.tab`,
/// started in `/home/user/dotfiles`, with its adoption recording `name`.
fn created_space(foreground_cwd: Option<&str>, name: &str, custom_label: bool) -> View {
    let mut shell = tagged(term("t1"), "s1", "tab1");
    shell.cwd = "/home/user/dotfiles".into();
    shell.foreground = foreground_cwd.and_then(|cwd| foreground("zsh", cwd));
    let mut v = view(vec![shell], vec![]);
    let mut own = crate::structure::Document::default();
    own.adoptions.insert(
        layout_key(None, "space:2:s1"),
        crate::structure::Adoption {
            space: "s1".into(),
            name: name.into(),
            custom_label,
            ..Default::default()
        },
    );
    doc(&mut v, "hc-tui", None, serde_json::to_value(own).unwrap());
    v
}

fn workspace_label(v: &View) -> (String, bool) {
    let workspace = &project(v, Some("t1")).snapshot.workspaces[0];
    (workspace.label.clone(), workspace.custom_label)
}

#[test]
fn a_new_workspace_that_kept_its_suggested_name_follows_its_live_folder() {
    let v = created_space(Some("/home/user/jayrome-website"), "dotfiles", false);
    assert_eq!(workspace_label(&v), ("jayrome-website".to_owned(), false));
}

#[test]
fn a_chosen_workspace_name_stays_when_its_terminal_moves() {
    let v = created_space(Some("/home/user/jayrome-website"), "mine", true);
    assert_eq!(workspace_label(&v), ("mine".to_owned(), true));
}

#[test]
fn a_new_workspace_with_no_live_folder_keeps_its_recorded_name() {
    let mut v = created_space(None, "dotfiles", false);
    let mut shell = v.inventory.get("t1").unwrap().clone();
    shell.cwd = String::new();
    v.inventory_mut().replace(2, vec![shell]);
    assert_eq!(workspace_label(&v), ("dotfiles".to_owned(), false));
}

#[test]
fn an_app_space_named_in_the_app_wins_over_an_automatic_adoption() {
    let mut a = crate::sidebar::fixture::agent("t1", "idle");
    a.foreground = foreground("claude", "/home/user/elsewhere");
    let mut v = view(vec![a], vec![]);
    doc(
        &mut v,
        "herdr-app",
        None,
        json!({"spaces":[app_space("s1", "work", "/work", &[("tab1", "", "t1")])]}),
    );
    let mut own = crate::structure::Document::default();
    own.adoptions.insert(
        layout_key(None, "space:2:s1"),
        crate::structure::Adoption {
            space: "s1".into(),
            name: "old".into(),
            custom_label: false,
            ..Default::default()
        },
    );
    doc(&mut v, "hc-tui", None, serde_json::to_value(own).unwrap());
    assert_eq!(project(&v, Some("t1")).snapshot.workspaces[0].label, "work");
}
