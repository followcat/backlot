use super::{ContextMenu, ContextMenuTarget};

fn labels(menu: &ContextMenu) -> Vec<&'static str> {
    menu.items().iter().map(|item| item.label).collect()
}

fn menu(target: ContextMenuTarget) -> ContextMenu {
    ContextMenu {
        target,
        x: 4,
        y: 4,
        highlighted: 0,
    }
}

#[test]
fn workspace_items_match_reference() {
    // Non-git workspace: rename/close only.
    let plain = menu(ContextMenuTarget::Workspace {
        workspace_id: "s".into(),
        is_git: false,
        is_linked_worktree: false,
        has_worktree_children: false,
        close_group: false,
        collapsed: false,
    });
    assert_eq!(labels(&plain), ["Rename", "Close"]);
    // A git checkout without worktree children gains the worktree rows.
    let git = menu(ContextMenuTarget::Workspace {
        workspace_id: "s".into(),
        is_git: true,
        is_linked_worktree: false,
        has_worktree_children: false,
        close_group: false,
        collapsed: false,
    });
    assert_eq!(
        labels(&git),
        ["Rename", "Close", "New worktree", "Open worktree..."]
    );
    let linked = menu(ContextMenuTarget::Workspace {
        workspace_id: "s".into(),
        is_git: true,
        is_linked_worktree: true,
        has_worktree_children: false,
        close_group: false,
        collapsed: false,
    });
    assert_eq!(
        labels(&linked),
        ["Rename", "Close", "Delete worktree checkout..."]
    );
    let parent = menu(ContextMenuTarget::Workspace {
        workspace_id: "s".into(),
        is_git: true,
        is_linked_worktree: false,
        has_worktree_children: true,
        close_group: true,
        collapsed: true,
    });
    assert_eq!(
        labels(&parent),
        [
            "Rename",
            "Close group",
            "New worktree",
            "Open worktree...",
            "Expand"
        ]
    );
}

#[test]
fn tab_items_match_reference() {
    let tab = menu(ContextMenuTarget::Tab {
        tab_id: "t".into(),
        workspace_id: "s".into(),
    });
    assert_eq!(labels(&tab), ["New tab", "Rename", "Close"]);
}

#[test]
fn pane_items_match_reference() {
    let pane = menu(ContextMenuTarget::Pane {
        pane_id: "p".into(),
        workspace_id: "s".into(),
        source_pane_id: None,
        has_manual_label: false,
        right_click_passthrough: false,
    });
    assert_eq!(
        labels(&pane),
        [
            "Rename pane",
            "Split right",
            "Split down",
            "Zoom",
            "Send right-clicks to pane",
            "Close pane",
        ]
    );
    let full = menu(ContextMenuTarget::Pane {
        pane_id: "p".into(),
        workspace_id: "s".into(),
        source_pane_id: Some("q".into()),
        has_manual_label: true,
        right_click_passthrough: true,
    });
    assert_eq!(
        labels(&full),
        [
            "Rename pane",
            "Clear pane name",
            "Swap with focused pane",
            "Split right",
            "Split down",
            "Zoom",
            "Use Herdr right-click menu",
            "Close pane",
        ]
    );
}

#[test]
fn machine_items_are_the_hc_tui_extension() {
    let local = menu(ContextMenuTarget::Machine { name: None });
    assert_eq!(labels(&local), ["Add machine"]);
    let remote = menu(ContextMenuTarget::Machine {
        name: Some("host-a".into()),
    });
    assert_eq!(
        labels(&remote),
        ["Connect", "Rename machine", "Remove machine"]
    );
}
