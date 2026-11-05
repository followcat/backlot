use super::*;
use crate::sidebar::fixture::term;
use hc_proto::terminal::{AheadBehind, Checkout};

fn facts(core: &str, checkout: &str, linked: bool) -> RepositoryFacts {
    RepositoryFacts {
        core_id: core.into(),
        cwd: "/home/user/acme-shop/src".into(),
        state: "ready".into(),
        auto_label: Some(
            if linked {
                "linked-checkout"
            } else {
                "acme-shop"
            }
            .into(),
        ),
        branch: Some(if linked { "topic" } else { "main" }.into()),
        ahead_behind: Some(AheadBehind {
            ahead: 2,
            behind: 1,
        }),
        checkout: Some(Checkout {
            repository_key: "opaque-repo".into(),
            checkout_key: checkout.into(),
            repo_name: "acme-shop".into(),
            checkout_name: checkout.into(),
            repo_root: if linked {
                "/home/user/linked-checkout"
            } else {
                "/home/user/acme-shop"
            }
            .into(),
            is_linked_worktree: linked,
            is_bare: false,
        }),
    }
}

#[test]
fn origin_keys_group_subdirectories_and_linked_checkouts_without_aliases() {
    let mut a = term("t1");
    a.repository = Some(facts("origin-a", "main-checkout", false));
    let mut same = a.clone();
    same.address = "alias-b/t2".into();
    same.cwd = "/home/user/other-launch".into();
    assert_eq!(
        crate::project::space_of(&a),
        crate::project::space_of(&same)
    );
    let mut linked = term("t3");
    linked.repository = Some(facts("origin-a", "linked-checkout", true));
    assert_ne!(
        crate::project::space_of(&a).0,
        crate::project::space_of(&linked).0
    );
    assert_eq!(
        worktree(a.repository.as_ref().unwrap()).unwrap().key,
        worktree(linked.repository.as_ref().unwrap()).unwrap().key
    );
    same.repository.as_mut().unwrap().core_id = "origin-b".into();
    assert_ne!(
        crate::project::space_of(&a).0,
        crate::project::space_of(&same).0
    );
    assert_ne!(
        worktree(a.repository.as_ref().unwrap()).unwrap().key,
        worktree(same.repository.as_ref().unwrap()).unwrap().key
    );
}

#[test]
fn projection_uses_authoritative_root_label_branch_and_nullable_counts() {
    let mut a = term("t1");
    a.cwd = "/home/user/launch".into();
    a.repository = Some(facts("origin-a", "main-checkout", false));
    let s = crate::project::endpoint_snapshot(&[&a], Some("t1"), 1, 1);
    let workspace = &s.workspaces[0];
    assert_eq!(workspace.label, "acme-shop");
    assert_eq!(workspace.new_workspace_cwd, "/home/user/acme-shop");
    assert_eq!(workspace.branch.as_deref(), Some("main"));
    assert_eq!(workspace.git_ahead_behind, Some((2, 1)));
    let repo = a.repository.as_mut().unwrap();
    repo.ahead_behind = None;
    repo.branch = None;
    let s = crate::project::endpoint_snapshot(&[&a], Some("t1"), 1, 1);
    assert_eq!(s.workspaces[0].git_ahead_behind, None);
    assert_eq!(s.workspaces[0].branch, None);
    assert!(s.workspaces[0].worktree.is_some());
    a.labels.insert("herdr.space".into(), "custom".into());
    assert_eq!(
        crate::project::space_of(&a),
        ("space:6:custom".into(), "custom".into())
    );
}

#[test]
fn pending_and_plain_directory_never_reuse_launch_git_or_known_zero() {
    let mut a = term("t1");
    let mut repo = facts("origin-a", "main-checkout", false);
    repo.checkout = None;
    repo.branch = None;
    repo.ahead_behind = None;
    repo.auto_label = None;
    repo.cwd = "/home/user/plain".into();
    repo.state = "pending".into();
    a.repository = Some(repo);
    assert_eq!(folder(&a), "/home/user/plain");
    assert_eq!(crate::project::space_of(&a).1, "plain");
    assert_eq!(counts(a.repository.as_ref().unwrap()), None);
    let repo = a.repository.as_mut().unwrap();
    repo.state = "ready".into();
    repo.ahead_behind = Some(AheadBehind {
        ahead: 0,
        behind: 0,
    });
    assert_eq!(counts(repo), Some((0, 0)));
    repo.ahead_behind = Some(AheadBehind {
        ahead: u64::MAX,
        behind: 0,
    });
    assert_eq!(counts(repo), usize::try_from(u64::MAX).ok().map(|n| (n, 0)));
}

#[test]
fn identity_length_prefixes_keep_opaque_separator_bearing_keys_distinct() {
    assert_ne!(key(&["origin", "a:b", "c"]), key(&["origin", "a", "b:c"]));
}

#[test]
fn cached_identity_survives_staleness_but_nonready_git_metadata_is_hidden() {
    let mut a = term("t1");
    a.repository = Some(facts("origin-a", "main-checkout", false));
    let identity = crate::project::space_of(&a).0;
    for state in ["stale", "pending", "unavailable", "future-state"] {
        a.repository.as_mut().unwrap().state = state.into();
        let s = crate::project::endpoint_snapshot(&[&a], Some("t1"), 1, 1);
        assert_eq!(crate::project::space_of(&a).0, identity);
        assert_eq!(s.workspaces[0].branch, None);
        assert_eq!(s.workspaces[0].git_ahead_behind, None);
        assert_eq!(s.workspaces[0].worktree, None);
    }
}
