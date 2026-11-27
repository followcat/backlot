use super::endpoint_nav::*;
use super::*;
use crate::input::KeybindAction as A;
use crate::sidebar::fixture::machine;

#[tokio::test]
async fn next_tab_stays_in_the_workspace() {
    // Two same-space terminals land in distinct tabs of workspace `one`.
    let mut t2 = spaced_agent("t2", "one");
    t2.labels.insert("herdr.tab".into(), "other".into());
    let mut app = app_multi(
        vec![
            spaced_agent("t1", "one"),
            t2,
            spaced_agent("host-a/t3", "three"),
        ],
        vec![machine("host-a", "connected")],
    );
    pending_attach(&mut app);
    app.focused = Some("t1".into());
    app.refresh_projection();
    let ws_one = app.projection.snapshot.focused_workspace_id.clone();

    app.on_action(A::NextTab, 80);
    assert_eq!(app.focused.as_deref(), Some("t2"));
    assert_eq!(app.projection.snapshot.focused_workspace_id, ws_one);
    assert_eq!(app.projection.active_endpoint_id, ClientEndpointId::Local);

    app.on_action(A::NextTab, 80);
    assert_eq!(
        app.focused.as_deref(),
        Some("t1"),
        "tabs wrap within the workspace"
    );
}

#[tokio::test]
async fn switch_workspace_stays_on_the_active_endpoint() {
    let mut app = app_multi(
        vec![
            spaced_agent("t1", "one"),
            spaced_agent("t2", "two"),
            spaced_agent("host-a/t3", "three"),
        ],
        vec![machine("host-a", "connected")],
    );
    pending_attach(&mut app);
    app.focused = Some("t1".into());
    app.refresh_projection();

    // Index 1 names the second workspace of the ACTIVE endpoint only.
    app.on_action(A::SwitchWorkspace(1), 80);
    assert_eq!(
        app.projection.snapshot.focused_workspace_id.as_deref(),
        Some("space:3:two")
    );
    assert_eq!(app.focused.as_deref(), Some("t2"));
    assert_eq!(app.projection.active_endpoint_id, ClientEndpointId::Local);

    // Out of the active endpoint's range: nothing moves.
    app.focused = Some("t1".into());
    app.refresh_projection();
    app.on_action(A::SwitchWorkspace(5), 80);
    assert_eq!(app.focused.as_deref(), Some("t1"));
    assert_eq!(app.projection.active_endpoint_id, ClientEndpointId::Local);
}

#[tokio::test]
async fn agent_row_reveal_counts_space_headers_across_machines() {
    // Headers open each machine's space: [one] t1, [two] host-a/t2,
    // [three] host-b/t3 — host-b/t3 is the sixth rendered entry.
    let mut app = app_multi(
        vec![
            spaced_agent("t1", "one"),
            spaced_agent("host-a/t2", "two"),
            spaced_agent("host-b/t3", "three"),
        ],
        vec![
            machine("host-a", "unreachable"),
            machine("host-b", "connected"),
        ],
    );
    app.shell.agent_panel_sort = crate::config::AgentPanelSortConfig::Spaces;
    app.shell.agent_panel_space_headers = true;
    pending_attach(&mut app);
    app.focused = Some("t1".into());
    app.refresh_projection();
    app.hit_map.agent_max_scroll = 50;
    app.hit_map.agents.clear();
    app.hit_map.endpoint_agents.clear();

    app.on_action(A::NextAgent, 80);
    assert_eq!(app.focused.as_deref(), Some("host-b/t3"));
    assert_eq!(app.shell_state.agent_scroll, 5);
}

fn ctrl_jk_user_config() -> crate::config::Config {
    toml::from_str(
        "[keys]\n\
         previous_workspace = \"ctrl+k\"\n\
         next_workspace = \"ctrl+j\"\n\
         previous_tab = \"alt+left\"\n\
         next_tab = \"alt+right\"\n\
         previous_agent = \"alt+up\"\n\
         next_agent = \"alt+down\"\n",
    )
    .expect("user [keys] config parses")
}

fn backlot_stage_terminal(address: &str) -> hc_proto::terminal::Terminal {
    let mut t = crate::sidebar::fixture::term(address);
    t.name = Some("backlot-stage".into());
    t.labels.insert("backlot.role".into(), "stage".into());
    t.labels.insert("herdr.space".into(), "backlot".into());
    t.labels.insert("herdr.tab".into(), "stage".into());
    t
}

fn studio_and_remote_app(
    remote_terms: Vec<hc_proto::terminal::Terminal>,
    remote_studios: serde_json::Value,
    local_extra_terms: Vec<hc_proto::terminal::Terminal>,
) -> crate::app::App {
    use hc_proto::watch::{DocKey, Document};

    let config = ctrl_jk_user_config();
    let (_, diags) = config.live_keybinds_with_diagnostics().unwrap();
    assert!(diags.is_empty(), "expected no keybind conflicts: {diags:?}");

    let local_doc = serde_json::json!({
        "schema": "backlot/1",
        "updated_ms": 1,
        "backlot_version": "0.4.3",
        "studios": [{
            "id": "orchard", "title": "orchard", "state": "live",
            "requirement_version": 1, "mode": "standard",
            "open_reviews": 0, "waiting": 0,
            "roles": [
                { "role": "producer", "name": "producer", "terminal": "t1", "state": "live", "work": "working" },
                { "role": "director", "name": "director", "terminal": "t2", "state": "live", "work": "waiting" }
            ]
        }],
        "archived": [
            {"name": "old-local-20260101T120000Z", "stamp": "20260101T120000Z", "studio": "old-local", "repo": "/home/user/r"}
        ]
    });
    let remote_doc = serde_json::json!({
        "schema": "backlot/1",
        "updated_ms": 1,
        "backlot_version": "0.4.3",
        "studios": remote_studios,
        "archived": [
            {"name": "old-remote-20260101T120000Z", "stamp": "20260101T120000Z", "studio": "old-remote", "repo": "/home/user/r"}
        ]
    });

    let mut terms = vec![
        spaced_agent("t1", "orchard"),
        spaced_agent("t2", "orchard"),
        backlot_stage_terminal("t9"),
    ];
    terms.extend(local_extra_terms);
    terms.extend(remote_terms);
    terms.push(backlot_stage_terminal("t14host/t9"));

    let mut app = app_multi(terms, vec![machine("t14host", "connected")]);
    app.set_plain(false);
    app.live = config.live_keybinds();
    app.panel.mode = crate::panel::PanelMode::Expanded;
    app.area = (120, 28);
    app.host_reports_all_keys = true;
    app.host_reports_key_releases = true;
    app.set_hollywood(
        Ok(Some(backlot_stage::config::HollywoodCfg {
            runner: "fake".into(),
            model: "m".into(),
            effort: None,
            key: None,
            default_cast: None,
        })),
        std::path::Path::new("config.toml"),
    );
    for (machine_key, value) in [(None, local_doc), (Some("t14host".into()), remote_doc)] {
        app.view.documents_mut().insert(
            DocKey {
                machine: machine_key,
                name: "backlot".into(),
            },
            Document {
                value,
                ..Default::default()
            }
            .into(),
        );
    }
    pending_attach(&mut app);
    app.focused = Some("t1".into());
    app.reconcile_published_view();
    app
}

#[tokio::test]
async fn ctrl_jk_cycles_local_studio_and_remote_workspaces_excluding_stage() {
    use crate::app::input::UiEvent;
    use crate::studio::{StudioFocus, StudioKey};
    use hc_vt::{Key, Mods};

    let parse = |seq| crate::raw_events::parse_terminal_key_sequence(seq).unwrap();
    let (ctrl_j, ctrl_k) = (parse("\x1b[106;5u"), parse("\x1b[107;5u"));
    let (legacy_j, legacy_k) = (parse("\n"), parse("\x0b"));
    let remote = ClientEndpointId::Remote("t14host".into());
    assert_eq!(
        (ctrl_j.input.key, ctrl_j.input.mods),
        (Key::Char('j'), Mods::CTRL)
    );

    // 1. Remote plain workspace "work" + hidden stage on both machines + StudioFocus::Studio start.
    let mut app_work = studio_and_remote_app(
        vec![spaced_term("t14host/t1", "work")],
        serde_json::json!([]),
        vec![],
    );
    let targets: Vec<_> = app_work
        .navigate_targets()
        .into_iter()
        .map(|t| (t.endpoint_id, t.workspace_id))
        .collect();
    assert_eq!(
        targets,
        vec![
            (ClientEndpointId::Local, "space:7:orchard".to_string()),
            (remote.clone(), "space:4:work".to_string()),
        ]
    );
    app_work.studio.focus = Some(StudioFocus::Studio(StudioKey::local("orchard")));
    app_work.on_input(UiEvent::Key(ctrl_j.clone()), 120);
    assert_eq!(
        (
            app_work.focused.as_deref(),
            &app_work.projection.active_endpoint_id
        ),
        (Some("t14host/t1"), &remote)
    );
    app_work.on_input(UiEvent::Key(ctrl_k.clone()), 120);
    assert_eq!(
        (
            app_work.focused.as_deref(),
            &app_work.projection.active_endpoint_id
        ),
        (Some("t1"), &ClientEndpointId::Local)
    );
    app_work.on_input(UiEvent::Key(legacy_j), 120);
    assert_eq!(app_work.focused.as_deref(), Some("t14host/t1"));
    app_work.on_input(UiEvent::Key(legacy_k), 120);
    assert_eq!(app_work.focused.as_deref(), Some("t1"));

    // 2. Remote git checkout named "backlot" alongside t14host stage stays visible and reachable.
    let mut checkout_backlot = crate::sidebar::fixture::term("t14host/t1");
    checkout_backlot.cwd = "/home/user/src/backlot".into();
    checkout_backlot.repository = Some(hc_proto::terminal::RepositoryFacts {
        core_id: "c1".into(),
        cwd: "/home/user/src/backlot".into(),
        state: "ready".into(),
        auto_label: Some("backlot".into()),
        checkout: Some(hc_proto::terminal::Checkout {
            repository_key: "repo-backlot".into(),
            checkout_key: "main".into(),
            repo_name: "backlot".into(),
            checkout_name: "backlot".into(),
            repo_root: "/home/user/src/backlot".into(),
            is_linked_worktree: false,
            is_bare: false,
        }),
        branch: Some("main".into()),
        ahead_behind: None,
    });
    let mut app_checkout =
        studio_and_remote_app(vec![checkout_backlot], serde_json::json!([]), vec![]);
    let screen_checkout = screen_lines(&draw_to(&mut app_checkout, 120, 40)).join("\n");
    assert!(
        screen_checkout.contains("backlot") && screen_checkout.contains("main"),
        "expected remote backlot checkout row in sidebar:\n{screen_checkout}"
    );
    app_checkout.on_input(UiEvent::Key(ctrl_j.clone()), 120);
    assert_eq!(
        (
            app_checkout.focused.as_deref(),
            &app_checkout.projection.active_endpoint_id
        ),
        (Some("t14host/t1"), &remote)
    );
    let revealed = screen_lines(&draw_to(&mut app_checkout, 120, 28)).join("\n");
    assert!(
        revealed.contains("· backlot"),
        "expected reveal:\n{revealed}"
    );
    app_checkout.on_input(UiEvent::Key(ctrl_k.clone()), 120);
    assert_eq!(app_checkout.focused.as_deref(), Some("t1"));

    // 3. Remote plain directory /home/user/backlot alongside t14host stage stays visible and reachable.
    let mut cwd_backlot = crate::sidebar::fixture::term("t14host/t1");
    cwd_backlot.cwd = "/home/user/backlot".into();
    let mut app_cwd = studio_and_remote_app(vec![cwd_backlot], serde_json::json!([]), vec![]);
    app_cwd.on_input(UiEvent::Key(ctrl_j.clone()), 120);
    assert_eq!(
        (
            app_cwd.focused.as_deref(),
            &app_cwd.projection.active_endpoint_id
        ),
        (Some("t14host/t1"), &remote)
    );
    app_cwd.on_input(UiEvent::Key(ctrl_k.clone()), 120);
    assert_eq!(app_cwd.focused.as_deref(), Some("t1"));

    // 4. Remote live studio + local extra plain workspace cycle in sidebar order.
    let mut app_multi_ws = studio_and_remote_app(
        vec![spaced_agent("t14host/t1", "beta")],
        serde_json::json!([{
            "id": "beta", "title": "beta", "state": "live",
            "requirement_version": 1, "mode": "standard",
            "open_reviews": 0, "waiting": 0,
            "roles": [{ "role": "producer", "name": "producer", "terminal": "t14host/t1", "state": "live", "work": "idle" }]
        }]),
        vec![spaced_term("t3", "scratch")],
    );
    app_multi_ws.on_input(UiEvent::Key(ctrl_j.clone()), 120);
    assert_eq!(app_multi_ws.focused.as_deref(), Some("t3"));
    app_multi_ws.on_input(UiEvent::Key(ctrl_j.clone()), 120);
    assert_eq!(app_multi_ws.focused.as_deref(), Some("t14host/t1"));
    app_multi_ws.on_input(UiEvent::Key(ctrl_j.clone()), 120);
    assert_eq!(app_multi_ws.focused.as_deref(), Some("t1"));

    // 5. Stage-only remote machine keeps internal stage space excluded (no workspace target on t14host).
    let mut app_stage_only = studio_and_remote_app(vec![], serde_json::json!([]), vec![]);
    assert_eq!(app_stage_only.navigate_targets().len(), 1);
    app_stage_only.on_input(UiEvent::Key(ctrl_j), 120);
    assert_eq!(app_stage_only.focused.as_deref(), Some("t1"));
    app_stage_only.on_input(UiEvent::Key(ctrl_k), 120);
    assert_eq!(app_stage_only.focused.as_deref(), Some("t1"));
}

#[tokio::test]
async fn ctrl_jk_preserves_alt_arrows_tree_focus_compose_and_modals() {
    use crate::app::input::UiEvent;
    use crate::hollywood::LayerMode;
    use crate::studio::{StudioFocus, StudioKey};

    let parse = |seq| crate::raw_events::parse_terminal_key_sequence(seq).unwrap();
    let (ctrl_j, ctrl_k) = (parse("\x1b[106;5u"), parse("\x1b[107;5u"));
    let (alt_down, alt_right) = (parse("\x1b[1;3B"), parse("\x1b[1;3C"));
    let (home, ctrl_b, key_t, down) = (parse("\x1b[H"), parse("\x02"), parse("t"), parse("\x1b[B"));
    let work_app = || {
        studio_and_remote_app(
            vec![spaced_term("t14host/t1", "work")],
            serde_json::json!([]),
            vec![],
        )
    };

    // (a) Alt+arrow studio/role navigation still switches role -> studio -> role after Ctrl+J/K.
    let mut app_alt = work_app();
    app_alt.on_input(UiEvent::Key(ctrl_j.clone()), 120);
    assert_eq!(app_alt.focused.as_deref(), Some("t14host/t1"));
    app_alt.on_input(UiEvent::Key(ctrl_k.clone()), 120);
    assert_eq!(app_alt.focused.as_deref(), Some("t1"));
    assert_eq!(
        app_alt.studio.focus,
        Some(StudioFocus::Role(StudioKey::local("orchard"), "t1".into()))
    );
    app_alt.on_input(UiEvent::Key(alt_down), 120);
    assert_eq!(app_alt.focused.as_deref(), Some("t2"));
    // One studio wraps onto itself: the step lands its remembered
    // member (t2 was the last person) in the pane.
    app_alt.on_input(UiEvent::Key(alt_right), 120);
    assert_eq!(
        app_alt.studio.focus,
        Some(StudioFocus::Role(StudioKey::local("orchard"), "t2".into()))
    );
    assert_eq!(app_alt.focused.as_deref(), Some("t2"));

    // (b) Left tree keys (`Ctrl+B t`, arrows) and focused tree own Ctrl+K/Ctrl+J.
    let mut app_tree = work_app();
    app_tree.on_input(UiEvent::Key(ctrl_b), 120);
    app_tree.on_input(UiEvent::Key(key_t), 120);
    assert!(app_tree.tree_focused());
    let before = app_tree.tree_focus.clone();
    app_tree.on_input(UiEvent::Key(down), 120);
    assert_ne!(app_tree.tree_focus, before);
    app_tree.on_input(UiEvent::Key(ctrl_j.clone()), 120);
    assert!(app_tree.tree_focused());
    assert_eq!(app_tree.focused.as_deref(), Some("t1"));

    // (c) Running dock compose keeps Ctrl+K (kills to EOL) and Ctrl+J.
    let mut app_compose = work_app();
    let key = StudioKey::local("orchard");
    app_compose.panel.focused = true;
    app_compose.panel.selected = Some(crate::panel::Target::Input(key.clone()));
    app_compose.panel.editing = true;
    app_compose.panel.drafts.insert(
        key.clone(),
        crate::chrome::text_editor::TextEditor::new("hello world", false),
    );
    app_compose.on_input(UiEvent::Key(home), 120);
    app_compose.on_input(UiEvent::Key(ctrl_k.clone()), 120);
    assert_eq!(
        app_compose.panel.drafts.get(&key).map(|d| d.as_str()),
        Some("")
    );
    assert_eq!(app_compose.focused.as_deref(), Some("t1"));
    app_compose.on_input(UiEvent::Key(ctrl_j.clone()), 120);
    assert_eq!(app_compose.focused.as_deref(), Some("t1"));

    // (c) Running Hollywood compose and expanded Call Sheet modal keep Ctrl+K/Ctrl+J.
    let mut app_hw = work_app();
    app_hw.hollywood.mode = LayerMode::Expanded;
    app_hw.hollywood.editing = true;
    app_hw.hollywood.input = crate::chrome::text_editor::TextEditor::new("tell hw", false);
    app_hw.on_input(UiEvent::Key(ctrl_k.clone()), 120);
    assert_eq!(app_hw.focused.as_deref(), Some("t1"));
    app_hw.hollywood.editing = false;
    app_hw.on_input(UiEvent::Key(ctrl_j.clone()), 120);
    assert_eq!(app_hw.focused.as_deref(), Some("t1"));

    // (c) Help modal keeps Ctrl+K/Ctrl+J.
    let mut app_help = work_app();
    app_help.help = true;
    app_help.on_input(UiEvent::Key(ctrl_j), 120);
    app_help.on_input(UiEvent::Key(ctrl_k), 120);
    assert!(app_help.help);
    assert_eq!(app_help.focused.as_deref(), Some("t1"));
}
