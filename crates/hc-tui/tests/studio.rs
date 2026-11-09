//! Binary-level tests for the studio commands and the dispatch hook in
//! `main`: the compiled `backlot` binary under a scratch state dir.

mod studio_kit;

use std::fs;

use studio_kit::*;

#[test]
fn scripted_studio_loop() {
    let s = Scratch::new();
    let cast = s.fixture("cast.toml", CAST_TOML);
    let o = ok(
        &s,
        &[
            "studio",
            "new",
            "--title",
            "Shop",
            "--repo",
            "/home/user/acme-shop",
            "--cast",
            &cast,
        ],
    );
    assert!(o.contains("studio.created"), "{o}");

    let req = s.fixture("req.md", "# requirement\n");
    ok(
        &s,
        &[
            "req",
            "revise",
            "--file",
            &req,
            "--summary",
            "first",
            "--as",
            "producer",
        ],
    );
    ok(&s, &["plan", "request", "--as", "producer"]);
    let plan = s.fixture("plan.md", "# plan\n");
    ok(
        &s,
        &[
            "plan", "publish", "--kind", "plan", "--file", &plan, "--as", "writer",
        ],
    );
    let acc = s.fixture("acc.md", "# acceptance\n");
    ok(
        &s,
        &[
            "task",
            "new",
            "--title",
            "cart",
            "--kind",
            "feature",
            "--acceptance-file",
            &acc,
            "--as",
            "director",
        ],
    );
    ok(
        &s,
        &[
            "cast", "--task", "t1", "--actor", "actor-a", "--reason", "r", "--as", "director",
        ],
    );

    let r1 = s.fixture("r1.md", "report one\n");
    let o = ok(
        &s,
        &[
            "deliver",
            "--commits",
            "a..b",
            "--report",
            &r1,
            "--check",
            "tests=pass",
            "--task",
            "t1",
            "--as",
            "actor",
        ],
    );
    assert!(o.contains("task.delivered t1 d1"), "{o}");

    let f = s.fixture(
        "f.json",
        r#"[{"id":"D1","summary":"misses tax","severity":"medium"}]"#,
    );
    ok(
        &s,
        &[
            "review",
            "--task",
            "t1",
            "--delivery",
            "1",
            "--verdict",
            "changes-requested",
            "--conclusion",
            "close",
            "--findings",
            &f,
            "--as",
            "director",
        ],
    );
    let r2 = s.fixture("r2.md", "report two\n");
    ok(
        &s,
        &[
            "deliver",
            "--commits",
            "b..c",
            "--report",
            &r2,
            "--task",
            "t1",
            "--as",
            "actor",
        ],
    );
    ok(
        &s,
        &[
            "review",
            "--task",
            "t1",
            "--delivery",
            "2",
            "--verdict",
            "accepted",
            "--conclusion",
            "done",
            "--as",
            "director",
        ],
    );

    let status = ok(&s, &["status"]);
    assert!(status.contains("acme-shop"), "{status}");
    assert!(status.contains("t1"), "{status}");
    assert!(status.contains("accepted"), "{status}");

    let status_json = ok(&s, &["status", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&status_json).unwrap();
    assert_eq!(v["tasks"]["t1"]["status"], "accepted");
    assert_eq!(v["tasks"]["t1"]["rework_rounds"], 1);

    let records = ok(&s, &["records"]);
    assert!(records.contains("not enough data"), "{records}");
    let records_json = ok(&s, &["records", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&records_json).unwrap();
    assert_eq!(v[0]["tasks"], 1);
    assert_eq!(v[0]["first_delivery"]["changes_requested"], 1);

    // Exit codes: a refused command is 1, a usage error is 2.
    let o = run(
        &s,
        &[
            "review",
            "--task",
            "t1",
            "--delivery",
            "3",
            "--verdict",
            "accepted",
            "--conclusion",
            "x",
            "--as",
            "director",
        ],
    );
    assert_eq!(o.status.code(), Some(1), "{}", stdout(&o));
    assert!(!stderr(&o).is_empty());
    let o = run(
        &s,
        &[
            "deliver",
            "--commits",
            "x..y",
            "--task",
            "t1",
            "--as",
            "actor",
        ],
    );
    assert_eq!(o.status.code(), Some(2));
    assert!(stderr(&o).contains("missing --report"), "{}", stderr(&o));

    // Events land in one ordered JSONL log.
    let log = fs::read_to_string(s.events()).unwrap();
    let seqs: Vec<u64> = log
        .lines()
        .map(|l| {
            serde_json::from_str::<serde_json::Value>(l).unwrap()["seq"]
                .as_u64()
                .unwrap()
        })
        .collect();
    assert_eq!(seqs.len(), 10, "{seqs:?}");
    assert!(seqs.windows(2).all(|w| w[1] == w[0] + 1), "{seqs:?}");
}

/// The hook leaves TUI arguments exactly as before: `--version`,
/// `--session x` (fails at the terminal, not in studio parsing), and
/// an unknown word (exit 2 from the TUI parser).
#[test]
fn hook_leaves_tui_args_alone() {
    let s = Scratch::new();
    let o = run(&s, &["--version"]);
    assert!(o.status.success());
    assert_eq!(
        stdout(&o).trim(),
        format!("backlot {}", env!("CARGO_PKG_VERSION"))
    );

    let o = run(&s, &["--session", "x"]);
    assert!(!o.status.success());
    assert!(stderr(&o).contains("terminal"), "{}", stderr(&o));

    let o = run(&s, &["frobnicate"]);
    assert_eq!(o.status.code(), Some(2));
    assert!(stderr(&o).contains("unexpected argument 'frobnicate'"));

    // A studio word routes to the stage: `backlot studio` with no
    // subcommand is a usage error from the studio parser.
    let o = run(&s, &["studio"]);
    assert_eq!(o.status.code(), Some(2));
    assert!(stderr(&o).contains("studio"), "{}", stderr(&o));
}
