//! The agents list follows the focused pane: any focus change — a tab
//! switch, a workspace switch, a pane click, or a focus that arrives from
//! the core — scrolls the focused agent's row into view, and nothing else
//! ever touches `agent_scroll`.

use super::*;
use crate::input::KeybindAction as A;
use crate::sidebar::fixture::{agent, machine};

/// `count` agents named `agent-NN`, all in workspace `one` with their own
/// tab `tab-NN`, so `SwitchTab` is the tab switch the user performs.
fn one_workspace_app(count: usize) -> App {
    let terms = (1..=count)
        .map(|n| {
            let address = format!("t{n}");
            let mut term = agent(&address, "working");
            term.pid = Some(1);
            term.created_ms = n as u64;
            term.name = Some(format!("agent-{n:02}"));
            term.labels.insert("herdr.space".into(), "one".into());
            term.labels
                .insert("herdr.tab".into(), format!("tab-{n:02}"));
            term
        })
        .collect();
    let mut app = app_with_terms(terms);
    app.focused = Some("t1".into());
    app.refresh_projection();
    app
}

/// Attach machinery that never resolves — focus changes still apply.
fn pending(app: &mut App) {
    app.open_attach = Arc::new(|_| Box::pin(std::future::pending()));
    app.pane = pane::Pane::new(app.pane_tx.clone(), app.open_attach.clone());
}

/// The agent-body row that draws `needle`, when it also carries the
/// focused-row background.
fn highlighted_row(
    app: &App,
    backend: &ratatui::backend::TestBackend,
    needle: &str,
) -> Option<u16> {
    let body = app.hit_map.agent_body;
    let buf = backend.buffer();
    (body.y..body.bottom()).find(|&y| {
        let text = (body.x..body.right())
            .map(|x| buf[(x, y)].symbol())
            .collect::<String>();
        text.contains(needle)
            && (body.x..body.right()).any(|x| buf[(x, y)].bg == app.shell.palette.active_row_bg)
    })
}

fn agent_body_text(app: &App, backend: &ratatui::backend::TestBackend) -> String {
    let body = app.hit_map.agent_body;
    let buf = backend.buffer();
    (body.y..body.bottom())
        .map(|y| {
            (body.x..body.right())
                .map(|x| buf[(x, y)].symbol())
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[tokio::test]
async fn tab_switch_to_a_row_below_the_view_scrolls_it_into_view() {
    let mut app = one_workspace_app(20);
    pending(&mut app);
    let backend = draw_to(&mut app, 80, 24);
    assert_eq!(app.shell_state.agent_scroll, 0, "nothing scrolled yet");
    assert!(!agent_body_text(&app, &backend).contains("agent-20"));

    app.on_action(A::SwitchTab(19), 80);
    assert_eq!(app.focused.as_deref(), Some("t20"));
    assert!(
        app.shell_state.reveal_focused_agent,
        "the focus change arms the reveal"
    );

    let backend = draw_to(&mut app, 80, 24);
    assert!(app.shell_state.agent_scroll > 0, "the list scrolled down");
    assert!(
        highlighted_row(&app, &backend, "agent-20").is_some(),
        "focused row not painted as active:\n{}",
        agent_body_text(&app, &backend)
    );
}

#[tokio::test]
async fn tab_switch_to_a_row_above_the_view_scrolls_back_up() {
    let mut app = one_workspace_app(20);
    pending(&mut app);
    let _ = draw_to(&mut app, 80, 24);
    app.on_action(A::SwitchTab(19), 80);
    let backend = draw_to(&mut app, 80, 24);
    assert!(app.shell_state.agent_scroll > 0);
    assert!(highlighted_row(&app, &backend, "agent-20").is_some());

    app.on_action(A::SwitchTab(0), 80);
    let backend = draw_to(&mut app, 80, 24);
    assert_eq!(app.shell_state.agent_scroll, 0, "back at the top");
    assert!(
        highlighted_row(&app, &backend, "agent-01").is_some(),
        "focused row not on screen:\n{}",
        agent_body_text(&app, &backend)
    );
}

#[tokio::test]
async fn a_focus_change_from_a_core_snapshot_also_reveals() {
    let mut app = one_workspace_app(20);
    let _ = draw_to(&mut app, 80, 24);
    assert_eq!(app.focused.as_deref(), Some("t1"));

    // The shared layout document names another pane — a focus change no
    // key produced (another client, `hc focus`).
    app.view.documents_mut().insert(
        shared::key(),
        Arc::new(hc_proto::watch::Document {
            value: serde_json::json!({ "schema": 1, "focus": "t18" }),
            version: 2,
            ..Default::default()
        }),
    );
    app.reconcile_published_view();
    assert_eq!(app.focused.as_deref(), Some("t18"));
    assert!(app.shell_state.reveal_focused_agent);

    let backend = draw_to(&mut app, 80, 24);
    assert!(
        highlighted_row(&app, &backend, "agent-18").is_some(),
        "core-driven focus did not reveal:\n{}",
        agent_body_text(&app, &backend)
    );
}

#[tokio::test]
async fn an_already_visible_row_does_not_move_the_list() {
    let mut app = one_workspace_app(20);
    pending(&mut app);
    let _ = draw_to(&mut app, 80, 24);

    // `agent-03` is drawn without scrolling at the top.
    app.on_action(A::SwitchTab(2), 80);
    assert!(app.shell_state.reveal_focused_agent);
    let backend = draw_to(&mut app, 80, 24);
    assert_eq!(app.shell_state.agent_scroll, 0);
    assert!(highlighted_row(&app, &backend, "agent-03").is_some());

    // Deep in the same list, `agent-18` is already on screen: revealing it
    // arms the flag but neither the top nor the bottom moves.
    app.on_action(A::SwitchTab(19), 80);
    let backend = draw_to(&mut app, 80, 24);
    assert!(highlighted_row(&app, &backend, "agent-20").is_some());
    let scrolled = app.shell_state.agent_scroll;
    app.on_action(A::SwitchTab(17), 80);
    assert!(app.shell_state.reveal_focused_agent);
    let backend = draw_to(&mut app, 80, 24);
    assert_eq!(app.shell_state.agent_scroll, scrolled);
    assert!(
        highlighted_row(&app, &backend, "agent-18").is_some(),
        "visible row still painted as active:\n{}",
        agent_body_text(&app, &backend)
    );
}

#[tokio::test]
async fn a_manual_scroll_survives_a_redraw_without_a_focus_change() {
    let mut app = one_workspace_app(20);
    let _ = draw_to(&mut app, 80, 24);
    app.shell_state.agent_scroll = 5;

    let _ = draw_to(&mut app, 80, 24);
    let _ = draw_to(&mut app, 80, 24);
    assert_eq!(app.shell_state.agent_scroll, 5, "focus did not change");

    // A focus change does move it, and the manual scroll after that
    // holds again until the next focus change.
    app.focused = Some("t20".into());
    app.refresh_projection();
    let _ = draw_to(&mut app, 80, 24);
    assert_ne!(app.shell_state.agent_scroll, 5, "the focus reveal ran");
    app.shell_state.agent_scroll = 2;
    let _ = draw_to(&mut app, 80, 24);
    assert_eq!(app.shell_state.agent_scroll, 2);

    // Output publishes constant ordinary updates; none of them is a
    // focus change, so the manual scroll keeps holding.
    let mut bumped = app
        .view
        .inventory
        .terminals()
        .values()
        .cloned()
        .collect::<Vec<_>>();
    bumped[0].turn += 1;
    app.view.inventory_mut().replace(3, bumped);
    app.reconcile_published_view();
    assert_eq!(app.focused.as_deref(), Some("t20"), "the focus held");
    let _ = draw_to(&mut app, 80, 24);
    assert_eq!(app.shell_state.agent_scroll, 2, "a publish moved the list");
}

#[tokio::test]
async fn focusing_a_non_agent_pane_leaves_the_scroll_unchanged() {
    let mut app = one_workspace_app(20);
    let mut shell = crate::sidebar::fixture::term("s1");
    shell.created_ms = 100;
    shell.labels.insert("herdr.space".into(), "one".into());
    shell.labels.insert("herdr.tab".into(), "tab-shell".into());
    let mut terms = app
        .view
        .inventory
        .terminals()
        .values()
        .cloned()
        .collect::<Vec<_>>();
    terms.push(shell);
    app.view.inventory_mut().replace(2, terms);
    app.refresh_projection();
    let _ = draw_to(&mut app, 80, 24);
    app.shell_state.agent_scroll = 4;

    app.focused = Some("s1".into());
    app.refresh_projection();
    assert!(app.shell_state.reveal_focused_agent);
    let _ = draw_to(&mut app, 80, 24);
    assert_eq!(app.shell_state.agent_scroll, 4, "no agent row to reveal");
}

#[tokio::test]
async fn a_tab_switch_reveals_the_right_machines_row() {
    let local = (1..=6)
        .map(|n| {
            let mut term = agent(&format!("t{n}"), "working");
            term.created_ms = n as u64;
            term.name = Some(format!("agent-local-{n}"));
            term.labels
                .insert("herdr.space".into(), format!("local-{n}"));
            term.labels.insert("herdr.tab".into(), format!("lt-{n}"));
            term
        })
        .collect::<Vec<_>>();
    let remote = (1..=9)
        .map(|n| {
            let mut term = agent(&format!("host-a/t{n}"), "working");
            term.created_ms = 10 + n as u64;
            term.name = Some(format!("agent-ha-{n}"));
            term.labels.insert("herdr.space".into(), "ha".into());
            term.labels.insert("herdr.tab".into(), format!("ht-{n}"));
            term
        })
        .collect::<Vec<_>>();
    let all = local.into_iter().chain(remote).collect();
    let mut app = super::endpoint_nav::app_multi(all, vec![machine("host-a", "connected")]);
    pending(&mut app);

    // Focus a host-a pane first: that is what makes host-a the active
    // endpoint whose rows carry the focus highlight.
    app.focused = Some("host-a/t1".into());
    app.refresh_projection();
    let _ = draw_to(&mut app, 80, 24);
    app.shell_state.agent_scroll = 0;
    let _ = draw_to(&mut app, 80, 24);

    // `agent-ha-9` sits in the last host-a tab, well below the view.
    app.on_action(A::SwitchTab(8), 80);
    assert_eq!(app.focused.as_deref(), Some("host-a/t9"));
    let backend = draw_to(&mut app, 80, 24);
    assert!(
        highlighted_row(&app, &backend, "agent-ha-9").is_some(),
        "the aggregate list did not follow the switch:\n{}",
        agent_body_text(&app, &backend)
    );
}
