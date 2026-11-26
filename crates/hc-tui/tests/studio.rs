//! Binary-level tests for the studio commands and the dispatch hook in
//! `main`: the compiled `backlot` binary under a scratch state dir.

mod studio_kit;
mod support;

use std::fs;

use studio_kit::*;

#[test]
fn scripted_studio_loop() {
    let s = Scratch::new();
    let cast = s.fixture("cast.toml", &format!("design_gate = \"off\"\n{CAST_TOML}"));
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

    let work = work_area(&s, "t1");
    let a = commit_in(&work, "one");
    let b = commit_in(&work, "two");
    let c = commit_in(&work, "three");
    let r1 = s.fixture("r1.md", "report one\n");
    let o = ok(
        &s,
        &[
            "deliver",
            "--commits",
            &format!("{a}..{b}"),
            "--report",
            &r1,
            "--check",
            "tests=pass",
            "--task",
            "t1",
            "--as",
            "actor-t1-c1",
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
            &format!("{b}..{c}"),
            "--report",
            &r2,
            "--task",
            "t1",
            "--as",
            "actor-t1-c1",
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
            "actor-t1-c1",
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
    assert_eq!(stdout(&o).trim(), hc_tui::cli::version_text());

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

/// `backlot guide` prints backlot's own docs from the binary: the
/// listing, one topic, a grep, and a mistyped topic's refusal — no
/// state dir, no core, no disk read.
#[test]
fn guide_prints_docs_from_the_binary() {
    let s = Scratch::new();
    let listing = ok(&s, &["guide"]);
    assert!(listing.contains("guide"), "{listing}");
    assert!(listing.contains("topic"), "{listing}");

    let topic = ok(&s, &["guide", "guide"]);
    assert!(topic.starts_with("# Running a studio"), "{topic}");

    let grepped = ok(&s, &["guide", "guide", "--grep", "permission"]);
    assert!(grepped.lines().count() > 1, "{grepped}");

    let o = run(&s, &["guide", "nope"]);
    assert!(!o.status.success());
    assert!(stderr(&o).contains("no such guide topic"), "{}", stderr(&o));
}

/// The guide verb is in the TUI's help, so a person knows it exists.
#[test]
fn help_mentions_the_guide() {
    let s = Scratch::new();
    let o = run(&s, &["--help"]);
    assert!(o.status.success());
    assert!(stdout(&o).contains("backlot guide"), "{}", stdout(&o));
}
