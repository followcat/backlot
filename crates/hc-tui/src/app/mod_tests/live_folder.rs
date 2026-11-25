//! New tabs and workspaces open where the focused terminal works now, as
//! the reference's `focused_pane_cwd_in_workspace` and
//! `resolved_new_workspace_cwd_from_tab` do — not where its space started.
use super::*;

/// A focused shell hc-tui started in `/home/user/dotfiles`, now in
/// `live` when given.
fn moved_shell(live: Option<&str>) -> App {
    let mut shell = crate::sidebar::fixture::term("t1");
    shell.pid = Some(1);
    shell.cwd = "/home/user/dotfiles".into();
    shell.labels.insert("herdr.space".into(), "s1".into());
    shell.labels.insert("herdr.tab".into(), "tab1".into());
    shell.foreground = live.map(|cwd| hc_proto::terminal::Foreground {
        pid: 100,
        name: "zsh".into(),
        argv: vec!["-zsh".into()],
        cwd: cwd.into(),
    });
    let mut app = app_with_terms(vec![shell]);
    app.focused = Some("t1".into());
    app.refresh_projection();
    app
}

#[tokio::test]
async fn a_new_tab_opens_where_the_focused_terminal_works_now() {
    let app = moved_shell(Some("/home/user/jayrome-website"));
    let creation = app.creation(None, false).unwrap();
    assert_eq!(
        creation.params.cwd.as_deref(),
        Some("/home/user/jayrome-website")
    );
}

#[tokio::test]
async fn a_new_tab_opens_in_the_launch_folder_when_no_live_folder_is_known() {
    for live in [None, Some("")] {
        let app = moved_shell(live);
        let creation = app.creation(None, false).unwrap();
        assert_eq!(creation.params.cwd.as_deref(), Some("/home/user/dotfiles"));
    }
}

#[tokio::test]
async fn a_new_workspace_opens_and_is_named_where_the_focused_terminal_works_now() {
    let app = moved_shell(Some("/home/user/jayrome-website"));
    let (creation, suggested) = app.workspace_creation().unwrap();
    assert_eq!(
        creation.params.cwd.as_deref(),
        Some("/home/user/jayrome-website")
    );
    assert_eq!(suggested, "jayrome-website");
}

#[tokio::test]
async fn a_new_workspace_falls_back_to_the_focused_workspace_folder() {
    for live in [None, Some("")] {
        let app = moved_shell(live);
        let (creation, suggested) = app.workspace_creation().unwrap();
        assert_eq!(creation.params.cwd.as_deref(), Some("/home/user/dotfiles"));
        assert_eq!(suggested, "dotfiles");
    }
}
