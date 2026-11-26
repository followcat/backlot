//! Store, CLI, and migration tests for forward-compatible unknown ledger event kinds.

use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::path::Path;

use backlot_model::event::{EventKind, LEDGER_VERSION};
use backlot_model::{ActorIdentity, EventBy, NewEvent};
use serde_json::Value;

use crate::cli::{Fail, run};
use crate::cli_testkit::{CAST_TOML, fixture, ok, scratch};
use crate::store::{self, Pending, StoreError};
use crate::test_support::TestDir;

fn actor() -> ActorIdentity {
    ActorIdentity {
        runner: "fake".into(),
        runner_version: None,
        model: "plain".into(),
        effort: Some("high".into()),
        permission_profile: "standard".into(),
    }
}

fn seed_studio(dir: &Path, title: &str) {
    use backlot_model::event::*;
    let kind = EventKind::StudioCreated {
        title: title.into(),
        repo: format!("/home/user/{title}"),
        members: Vec::new(),
        no_git: false,
        mode: StudioMode::Standard,
        roles: Box::new(RoleConfigs {
            producer: actor(),
            director: actor(),
            writer: actor(),
        }),
        candidates: vec![Candidate {
            name: "actor-a".into(),
            runner: "fake".into(),
            runner_version: None,
            model: "plain".into(),
            effort: Some("high".into()),
            permission_profile: "standard".into(),
        }],
        rework_cap: 2,
        stall_limit_s: 600,
        nudge_after_s: 180,
        escalate_after_s: 600,
        check_env: Vec::new(),
        checks: Vec::new(),
        task_budget_s: None,
        limits: Default::default(),
        max_actors: default_max_actors(),
        push: Default::default(),
    };
    store::append::<(), _>(dir, |_, _| {
        Ok(Pending {
            event: NewEvent::new(EventBy::role("user"), kind),
            files: Vec::new(),
        })
    })
    .unwrap();
}

#[test]
fn unknown_kind_parses_counts_next_seq_refuses_writes_and_keeps_corrupt_errors() {
    let (sc, env) = scratch();
    let cast = fixture(&sc, "cast.toml", CAST_TOML);
    run(
        &[
            "studio".into(),
            "new".into(),
            "--title".into(),
            "acme-shop".into(),
            "--repo".into(),
            "/home/user/acme-shop".into(),
            "--cast".into(),
            cast,
        ],
        &env,
    )
    .unwrap();
    let req = fixture(&sc, "req-v1.md", "first requirement");
    ok(
        &env,
        &format!("req revise --file {req} --summary first-cut --as producer"),
    );

    let dir = store::studio_dir(&sc.dir, "acme-shop");
    let unknown_line = r#"{"v":1,"seq":10,"ts_ms":10000,"by":{"role":"producer"},"kind":"design.opened","topic":"auth"}"#;
    let mut f = OpenOptions::new()
        .append(true)
        .open(dir.join("events.jsonl"))
        .unwrap();
    writeln!(f, "{unknown_line}").unwrap();

    // Parse succeeds, keeps envelope and next_seq > 10.
    let (events, torn) = store::load_events(&dir).expect("unknown kind line should parse");
    assert!(!torn);
    assert_eq!(events.len(), 3);
    assert_eq!(events[2].seq, 10);
    assert_eq!(events[2].ts_ms, 10_000);
    assert_eq!(events[2].by.role, "producer");
    assert_eq!(events[2].kind.name(), "unknown");
    assert_eq!(events[2].kind.raw_name(), "design.opened");
    let state = store::load_state(&dir).unwrap();
    assert_eq!(state.last_seq, 10);
    let next_seq = events.iter().map(|e| e.seq).max().unwrap_or(0) + 1;
    assert_eq!(next_seq, 11);

    // Status read still works and warns to upgrade backlot.
    let status_out = ok(&env, "status");
    assert!(status_out.contains("acme-shop"), "{status_out}");
    assert!(status_out.contains("upgrade backlot"), "{status_out}");

    // Store append refuses every write cleanly.
    let err = store::append::<(), _>(&dir, |_, _| {
        Ok(Pending {
            event: NewEvent::new(
                EventBy::role("user"),
                EventKind::MessageQueued {
                    to: "producer".into(),
                    file: None,
                    purpose: None,
                    source_seq: None,
                },
            ),
            files: Vec::new(),
        })
    })
    .expect_err("store::append must refuse ledgers with unknown kinds");
    let err_msg = match &err {
        StoreError::Refused(r) => r.to_string(),
        other => format!("{other:?}"),
    };
    assert_eq!(
        err_msg,
        "this ledger has records from a newer backlot (design.opened); upgrade backlot"
    );

    // CLI write verb surfaces the exact same error.
    let cli_err = run(
        &[
            "tell".into(),
            "producer".into(),
            "--text".into(),
            "hello".into(),
            "--as".into(),
            "user".into(),
        ],
        &env,
    )
    .expect_err("CLI write must fail on unknown-kind ledger");
    assert!(matches!(cli_err, Fail::Refused(_)), "{cli_err:?}");
    assert_eq!(
        cli_err.message(),
        "this ledger has records from a newer backlot (design.opened); upgrade backlot"
    );

    // Malformed lines and v > LEDGER_VERSION keep today's corrupt error behaviour.
    let bad_dir = TestDir::new("backlot-bad-line");
    seed_studio(&bad_dir, "bad-shop");
    OpenOptions::new()
        .append(true)
        .open(bad_dir.join("events.jsonl"))
        .unwrap()
        .write_all(b"not-valid-json\n")
        .unwrap();
    match store::load_events(&bad_dir) {
        Err(StoreError::Corrupt(c)) => assert_eq!(c.line, 2),
        other => panic!("expected corrupt error for malformed line, got {other:?}"),
    }

    let newer_dir = TestDir::new("backlot-newer-v");
    seed_studio(&newer_dir, "newer-shop");
    let newer_v = format!(
        r#"{{"v":{},"seq":2,"ts_ms":2000,"by":{{"role":"user"}},"kind":"design.opened"}}"#,
        LEDGER_VERSION + 1
    );
    OpenOptions::new()
        .append(true)
        .open(newer_dir.join("events.jsonl"))
        .unwrap()
        .write_all(format!("{newer_v}\n").as_bytes())
        .unwrap();
    match store::load_events(&newer_dir) {
        Err(StoreError::Corrupt(c)) => {
            assert_eq!(c.line, 2);
            assert!(c.message.contains("upgrade backlot"), "{}", c.message);
        }
        other => panic!("expected corrupt error for v > LEDGER_VERSION, got {other:?}"),
    }
}

#[test]
fn migrate_preserves_unknown_line_byte_for_byte() {
    let (sc, env) = scratch();
    let dir = store::studio_dir(&sc.dir, "acme-shop");
    fs::create_dir_all(&dir).unwrap();
    seed_studio(&dir, "acme-shop");
    let mut first: Value =
        serde_json::from_str(fs::read_to_string(dir.join("events.jsonl")).unwrap().trim()).unwrap();
    first.as_object_mut().unwrap().remove("v");
    let line0 = serde_json::to_string(&first).unwrap();
    let raw_unknown = r#"{"extra":{"z":1,"a":2},"by":{"role":"producer"},"kind":"design.opened","ts_ms":2000,"seq":2,"v":1}"#;
    fs::write(
        dir.join("events.jsonl"),
        format!("{line0}\n{raw_unknown}\n"),
    )
    .unwrap();

    let report = ok(&env, "studio migrate acme-shop");
    assert!(report.contains("migrated 1/2"), "{report}");

    let after = fs::read_to_string(dir.join("events.jsonl")).unwrap();
    let lines: Vec<&str> = after.lines().collect();
    assert_eq!(lines.len(), 2);
    assert!(lines[0].contains("\"v\":1"), "{}", lines[0]);
    assert_eq!(
        lines[1], raw_unknown,
        "unknown-kind line must stay byte-for-byte identical after migration"
    );

    let second = ok(&env, "studio migrate acme-shop");
    assert!(second.contains("already at ledger v1"), "{second}");
}
