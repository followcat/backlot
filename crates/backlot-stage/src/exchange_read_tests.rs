//! Tests for `exchange_read`: snippet extraction from `messages/…` and
//! `reports/…`, Markdown heading cleanup, caching, and path-traversal
//! / symlink containment under `<base>/studios/<studio>/`.

use std::collections::BTreeMap;
use std::fs;

use backlot_model::hollywood::HollywoodKind;

use crate::cli_testkit::*;
use crate::exchange_read::{read_snippet, studio_snippets};
use crate::hwood_tests::hwood_env;
use crate::store;

#[test]
fn studio_snippets_extracts_user_producer_hollywood_and_report_turns() {
    let (scratch, env) = scratch();
    let cast = fixture(&scratch, "cast.toml", CAST_TOML);
    ok(
        &env,
        &format!("studio new --title Shop --repo /home/user/acme-shop --cast {cast}"),
    );

    // 1. User -> producer `--text`.
    ok(&env, "tell producer --text check-the-cart --as user");
    // 2. Hollywood -> producer `--file`.
    let hw_note = fixture(
        &scratch,
        "hw.md",
        "Focus on checkout reliability.\nMore detail.\n",
    );
    let lenv = hwood_env(&env);
    ok(
        &lenv,
        &format!("tell producer --file {hw_note} --studio acme-shop --as hollywood"),
    );
    // 3. Producer -> user `--file` with Markdown heading and `#7` token check.
    let reply = fixture(
        &scratch,
        "reply.md",
        "\n## Cart Status\n\nAll green on #7.\n",
    );
    ok(&env, &format!("tell user --file {reply} --as producer"));
    // 4. Producer -> user `report`.
    let rep = fixture(
        &scratch,
        "rep.md",
        "#7 delivered and merged\nDetails follow.\n",
    );
    ok(
        &env,
        &format!("report --node milestone --file {rep} --as producer"),
    );
    // Internal director -> producer report is ignored.
    ok(
        &env,
        &format!("report --node integration --file {rep} --as director"),
    );

    let sdir = store::studio_dir(&scratch.dir, "acme-shop");
    let state = store::load_state(&sdir).unwrap();
    let studios = vec![("acme-shop".to_string(), state)];
    let mut cache = BTreeMap::new();
    let snippets = studio_snippets(&scratch.dir, &studios, &mut cache);
    assert_eq!(cache.len(), 4);

    let summary = backlot_model::summary::summary_with_exchange(
        &studios,
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        &snippets,
        1,
    );
    let ex = &summary.studios[0].exchange;
    assert_eq!(ex.len(), 4);
    assert_eq!(ex[0].who, "user");
    assert_eq!(ex[0].text, "check-the-cart");
    assert_eq!(ex[1].who, "hollywood");
    assert_eq!(ex[1].text, "Focus on checkout reliability.");
    assert_eq!(ex[2].who, "producer");
    assert_eq!(ex[2].text, "Cart Status");
    assert_eq!(ex[3].who, "producer");
    assert_eq!(ex[3].text, "#7 delivered and merged");

    // Hollywood ledger also mirrored its tell (#2).
    let (hevents, _) = store::load_hevents(&store::hollywood_dir(&scratch.dir)).unwrap();
    assert!(matches!(
        hevents.first().map(|e| &e.kind),
        Some(HollywoodKind::MessageSent { .. })
    ));
}

#[test]
fn read_snippet_refuses_traversal_foreign_roots_and_escaping_symlinks() {
    let (scratch, env) = scratch();
    let cast = fixture(&scratch, "cast.toml", CAST_TOML);
    ok(
        &env,
        &format!("studio new --title Shop --repo /home/user/acme-shop --cast {cast}"),
    );
    let sdir = store::studio_dir(&scratch.dir, "acme-shop");
    let secret = scratch.files.join("secret.txt");
    fs::write(&secret, "never-publish-me\n").unwrap();

    for bad in [
        "../secret.txt",
        "messages/../../secret.txt",
        "/etc/passwd",
        "events.jsonl",
        "plan/1-plan.md",
    ] {
        assert_eq!(read_snippet(&sdir, bad), None, "accepted {bad}");
    }

    #[cfg(unix)]
    {
        fs::create_dir_all(sdir.join("messages")).unwrap();
        std::os::unix::fs::symlink(&secret, sdir.join("messages/escape.md")).unwrap();
        assert_eq!(read_snippet(&sdir, "messages/escape.md"), None);
    }
}
