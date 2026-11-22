//! `backlot hollywood show <seq>` tests: report headlines with and
//! without `--body`, user `tell hollywood` lines, mirrored studio
//! message files, reports evicted past `HEADLINE_CAP`, missing seqs,
//! and path-traversal containment under `<base>/hollywood/`.

use backlot_model::hollywood::{HollywoodKind, NewHollywoodEvent};

use crate::cli_testkit::*;
use crate::hwood_tests::{argv, argv_ok, hwood_env, running};
use crate::store;

#[test]
fn hollywood_show_reads_reports_with_and_without_body() {
    let (scratch, env) = scratch();
    let body = fixture(
        &scratch,
        "report.md",
        "## Summary\n- studio-a green\n- 1 card open\n",
    );
    argv_ok(
        &env,
        &["hollywood", "report", "headline only", "--as", "hollywood"],
    );
    argv_ok(
        &env,
        &[
            "hollywood",
            "report",
            "two studios checked",
            "--body",
            &body,
            "--as",
            "hollywood",
        ],
    );

    // Without `--body`, `show` prints the headline cleanly.
    let plain = argv_ok(&env, &["hollywood", "show", "1", "--as", "user"]);
    assert_eq!(plain, "headline only");

    // With `--body`, `show` prints headline + blank line + body; `#<seq>` works too.
    let full = argv_ok(&env, &["hollywood", "show", "#2", "--as", "user"]);
    assert_eq!(
        full,
        "two studios checked\n\n## Summary\n- studio-a green\n- 1 card open"
    );
}

#[test]
fn hollywood_show_reads_older_reports_past_headline_cap_and_messages() {
    let (scratch, env) = scratch();
    running(&scratch, &env);
    let first_body = fixture(&scratch, "first.md", "early detail still on disk\n");
    argv_ok(
        &env,
        &[
            "hollywood",
            "report",
            "report #1",
            "--body",
            &first_body,
            "--as",
            "hollywood",
        ],
    );
    // Push more than HEADLINE_CAP (10) newer reports so #1 leaves reduced state.
    for i in 2..=13 {
        argv_ok(
            &env,
            &[
                "hollywood",
                "report",
                &format!("report #{i}"),
                "--as",
                "hollywood",
            ],
        );
    }
    let old = argv_ok(&env, &["hollywood", "show", "1", "--as", "user"]);
    assert_eq!(old, "report #1\n\nearly detail still on disk");

    // User `tell hollywood` inline message (#14).
    argv_ok(
        &env,
        &[
            "tell",
            "hollywood",
            "please check acme-shop's blocked actor",
            "--as",
            "user",
        ],
    );
    let inline = argv_ok(&env, &["hollywood", "show", "14", "--as", "user"]);
    assert_eq!(inline, "please check acme-shop's blocked actor");

    // Hollywood `tell producer --file` mirrors a file-backed `message.sent` (#15).
    let note = fixture(
        &scratch,
        "note.md",
        "Focus on cart tests first.\nSecond line.\n",
    );
    let lenv = hwood_env(&env);
    ok(
        &lenv,
        &format!("tell producer --file {note} --studio acme-shop --as hollywood"),
    );
    let mirrored = argv_ok(&env, &["hollywood", "show", "15", "--as", "user"]);
    assert_eq!(mirrored, "Focus on cart tests first.\nSecond line.");
}

#[test]
fn hollywood_show_refuses_missing_non_entry_and_hostile_paths() {
    let (scratch, env) = scratch();
    running(&scratch, &env);

    // Empty Hollywood ledger refuses cleanly.
    let missing = argv(&env, &["hollywood", "show", "99", "--as", "user"]).unwrap_err();
    assert!(
        missing.message().contains("no hollywood entry #99"),
        "{}",
        missing.message()
    );

    // Usage errors on missing or non-numeric `<seq>`.
    assert!(usage(&env, "hollywood show").contains("missing <seq>"));
    assert!(usage(&env, "hollywood show nope").contains("bad seq"));

    // A `card.proposed` seq (#1) is not a report or message entry.
    ok(
        &env,
        "hollywood propose pause --studio acme-shop --task t1 --why looping --as hollywood",
    );
    let non_entry = argv(&env, &["hollywood", "show", "1", "--as", "user"]).unwrap_err();
    assert!(non_entry.message().contains("no hollywood entry #1"));

    // Plant hostile `body` and `file` paths into `hollywood/events.jsonl`
    // and ensure none of them can escape `<base>/hollywood/`.
    let secret = scratch.dir.join("secret.txt");
    std::fs::write(&secret, "do not read\n").unwrap();
    let hdir = store::hollywood_dir(&scratch.dir);
    for bad in [
        "../secret.txt",
        "reports/../../secret.txt",
        "/etc/passwd",
        "drafts/secret.txt",
    ] {
        let ev = store::append_hollywood(&hdir, |_, _| {
            Ok::<_, std::convert::Infallible>(store::HPending {
                event: NewHollywoodEvent::new(
                    backlot_model::EventBy::role("hollywood"),
                    HollywoodKind::Reported {
                        headline: "bad".into(),
                        body: Some(bad.into()),
                    },
                ),
                files: vec![],
            })
        })
        .unwrap();
        let err = argv(&env, &["hollywood", "show", &ev.seq.to_string()]).unwrap_err();
        assert!(
            !err.message().contains("do not read"),
            "leaked on {bad}: {}",
            err.message()
        );
    }

    // Symlink inside `reports/` pointing outside `<base>/hollywood/` is refused.
    #[cfg(unix)]
    {
        let reports_dir = hdir.join("reports");
        std::fs::create_dir_all(&reports_dir).unwrap();
        std::os::unix::fs::symlink(&secret, reports_dir.join("link.md")).unwrap();
        let ev = store::append_hollywood(&hdir, |_, _| {
            Ok::<_, std::convert::Infallible>(store::HPending {
                event: NewHollywoodEvent::new(
                    backlot_model::EventBy::role("hollywood"),
                    HollywoodKind::Reported {
                        headline: "symlink".into(),
                        body: Some("reports/link.md".into()),
                    },
                ),
                files: vec![],
            })
        })
        .unwrap();
        let err = argv(&env, &["hollywood", "show", &ev.seq.to_string()]).unwrap_err();
        assert!(
            err.message().contains("outside hollywood ledger"),
            "{}",
            err.message()
        );
    }
}
