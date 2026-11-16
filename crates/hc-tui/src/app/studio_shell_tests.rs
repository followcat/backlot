//! Unit tests for studio shells: the label scheme, the name scheme,
//! the workarea resolution and the next-number walk — the pieces
//! that never need a core.

use super::*;

/// A bare live-terminal record with the given labels.
fn term(address: &str, labels: &[(&str, &str)]) -> hc_proto::terminal::Terminal {
    let mut t = crate::sidebar::fixture::term(address);
    t.labels = labels
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    t
}

fn view_with(terms: Vec<hc_proto::terminal::Terminal>) -> hc_proto::watch::View {
    crate::sidebar::fixture::view(terms, vec![])
}

#[test]
fn shell_name_never_collides_with_a_role() {
    // `bl-acme-shop-sh1` vs roles `bl-acme-shop-producer`,
    // `bl-acme-shop-actor-t1-c1` — the `sh` prefix keeps the streams
    // apart.
    assert_eq!(shell_name("acme-shop", 1), "bl-acme-shop-sh1");
    assert_eq!(shell_name("acme-shop", 12), "bl-acme-shop-sh12");
}

#[test]
fn next_shell_n_walks_past_the_highest() {
    let view = view_with(vec![
        term("t1", &[(STUDIO_LABEL, "acme-shop"), (SHELL_LABEL, "1")]),
        term("t2", &[(STUDIO_LABEL, "acme-shop"), (SHELL_LABEL, "3")]),
        // Another studio's shells don't count.
        term("t3", &[(STUDIO_LABEL, "other"), (SHELL_LABEL, "9")]),
        // A role carries no shell label.
        term("t4", &[(STUDIO_LABEL, "acme-shop")]),
    ]);
    assert_eq!(next_shell_n(&view, "acme-shop"), 4);
    assert_eq!(next_shell_n(&view, "other"), 10);
    assert_eq!(next_shell_n(&view, "none"), 1);
}

#[test]
fn studio_shells_sorts_by_number() {
    let view = view_with(vec![
        term("t9", &[(STUDIO_LABEL, "acme-shop"), (SHELL_LABEL, "2")]),
        term("t1", &[(STUDIO_LABEL, "acme-shop"), (SHELL_LABEL, "1")]),
    ]);
    assert_eq!(
        studio_shells(&view, "acme-shop"),
        vec![(1, "t1".to_string()), (2, "t9".to_string())]
    );
}

#[test]
fn shell_tab_id_matches_project_keys() {
    let (space, tab) = shell_tab_id("acme-shop", "shell 2");
    assert_eq!(space, "space:9:acme-shop");
    assert_eq!(tab, "space:9:acme-shop/tab:7:shell 2");
    // The same ids `project::space_of`/`tab_key_of` derive from labels.
    let t = term(
        "t1",
        &[("herdr.space", "acme-shop"), ("herdr.tab", "shell 2")],
    );
    let (key, _) = crate::project::space_of(&t);
    assert_eq!(key, space);
    assert_eq!(format!("{key}/{}", crate::project::tab_key_of(&t)), tab);
}
