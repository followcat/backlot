//! Tests for the `backlot tell user` question guard, `--ask`, and `--note`.

use std::collections::BTreeMap;
use std::path::PathBuf;

use backlot_model::event::{ActorIdentity, EventBy, EventKind, NewEvent};
use backlot_model::summary::{open_ask, summary_with_exchange};

use crate::cli::{Caller, Env, Fail, run_stdin};
use crate::cli_testkit::{CAST_TOML, Scratch, fixture, ok, scratch};
use crate::exchange_read::studio_snippets;
use crate::store::{self, Pending};
use crate::verbs::tell_text;

const REFUSAL_MSG: &str = "this reads as a question to the person: add --ask so the studio waits for their answer, or --note to send it as information";

fn studio(scratch: &Scratch, env: &Env) -> PathBuf {
    let cast = fixture(scratch, "cast.toml", CAST_TOML);
    ok(
        env,
        &format!("studio new --title Shop --repo /home/user/acme-shop --cast {cast}"),
    );
    let sdir = store::studio_dir(&scratch.dir, "acme-shop");
    // Record role.started for producer so `RoleSummary.asks` is populated.
    store::append(&sdir, |_, _| {
        Ok::<_, std::convert::Infallible>(Pending {
            event: NewEvent::new(
                EventBy::role("stage"),
                EventKind::RoleStarted {
                    role: "producer".into(),
                    terminal: Some("bl-acme-shop-producer".into()),
                    actor: Some(ActorIdentity {
                        runner: "runner-p".into(),
                        runner_version: None,
                        model: "model-p".into(),
                        effort: Some("high".into()),
                        permission_profile: "standard".into(),
                    }),
                    session_id: Some("sess-p1".into()),
                    reason: None,
                    prompt_hash: None,
                },
            ),
            files: Vec::new(),
        })
    })
    .unwrap();
    sdir
}

fn run_argv(env: &Env, argv: &[&str], stdin: Option<&str>) -> Result<String, Fail> {
    let _g = crate::cli_testkit::STAGE_LOCK.lock().unwrap();
    let owned: Vec<String> = argv.iter().map(|s| (*s).to_string()).collect();
    run_stdin(&owned, env, stdin)
}

fn assert_guard_refused(env: &Env, argv: &[&str], stdin: Option<&str>) {
    let err = run_argv(env, argv, stdin).unwrap_err();
    match err {
        Fail::Usage(msg) => assert_eq!(msg, REFUSAL_MSG, "argv={argv:?}"),
        other => panic!("expected Fail::Usage for {argv:?}, got {other:?}"),
    }
}

#[test]
fn question_without_flag_is_refused_and_writes_no_event() {
    let (scratch, env) = scratch();
    let sdir = studio(&scratch, &env);
    let (before_events, _) = store::load_events(&sdir).unwrap();
    let before_msgs = sdir.join("messages").exists();

    // Producer with `--text`, ASCII `?` and fullwidth `？`.
    assert_guard_refused(
        &env,
        &[
            "tell",
            "user",
            "--text",
            "Should we proceed?",
            "--as",
            "producer",
        ],
        None,
    );
    assert_guard_refused(
        &env,
        &["tell", "user", "--text", "是否批准？", "--as", "producer"],
        None,
    );

    // Producer with `--file` (ASCII `?` and fullwidth `？`).
    let q_ascii = fixture(&scratch, "q-ascii.md", "Please decide: ship v1?\n");
    let q_cjk = fixture(
        &scratch,
        "q-cjk.md",
        "请裁决两项执行中发现的阻塞授权：1) 是否授权？2) 是否批准？\n",
    );
    assert_guard_refused(
        &env,
        &["tell", "user", "--file", &q_ascii, "--as", "producer"],
        None,
    );
    assert_guard_refused(
        &env,
        &["tell", "user", "--file", &q_cjk, "--as", "producer"],
        None,
    );

    // Producer with `--stdin`.
    assert_guard_refused(
        &env,
        &["tell", "user", "--stdin", "--as", "producer"],
        Some("Continue?"),
    );

    // Other studio roles (`director`, `writer`, `actor-t1-c1`) are guarded too.
    for role in ["director", "writer", "actor-t1-c1"] {
        assert_guard_refused(
            &env,
            &["tell", "user", "--text", "Need input?", "--as", role],
            None,
        );
    }

    // No event and no message file was written.
    let (after_events, _) = store::load_events(&sdir).unwrap();
    assert_eq!(after_events.len(), before_events.len());
    assert_eq!(sdir.join("messages").exists(), before_msgs);
}

#[test]
fn ask_records_purpose_ask_and_shows_in_role_asks() {
    let (scratch, env) = scratch();
    let sdir = studio(&scratch, &env);

    // Both `--text --ask` and `--file --ask` record `purpose = "ask"`.
    let out = run_argv(
        &env,
        &[
            "tell",
            "user",
            "--text",
            "Which port?",
            "--ask",
            "--as",
            "producer",
        ],
        None,
    )
    .unwrap();
    assert!(out.contains("message.queued"), "{out}");

    let (events, _) = store::load_events(&sdir).unwrap();
    let state = store::load_state(&sdir).unwrap();
    let last = events.last().unwrap();
    match &last.kind {
        EventKind::MessageQueued { to, purpose, .. } => {
            assert_eq!(to, "user");
            assert_eq!(purpose.as_deref(), Some("ask"));
        }
        other => panic!("expected MessageQueued, got {other:?}"),
    }
    assert!(open_ask(&state, "producer").is_some());

    let studios = vec![("acme-shop".to_string(), state)];
    let mut cache = BTreeMap::new();
    let snippets = studio_snippets(&scratch.dir, &studios, &mut cache);
    let summary = summary_with_exchange(
        &studios,
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        &snippets,
        1,
    );
    let producer = summary.studios[0]
        .roles
        .iter()
        .find(|r| r.role == "producer")
        .unwrap();
    assert_eq!(producer.asks.as_deref(), Some("Which port?"));

    // `--file --ask` works the same way.
    let qfile = fixture(&scratch, "q.md", "Authorize deploy?\n");
    run_argv(
        &env,
        &[
            "tell", "user", "--file", &qfile, "--ask", "--as", "producer",
        ],
        None,
    )
    .unwrap();
    let (events, _) = store::load_events(&sdir).unwrap();
    let state = store::load_state(&sdir).unwrap();
    match &events.last().unwrap().kind {
        EventKind::MessageQueued { to, purpose, .. } => {
            assert_eq!(to, "user");
            assert_eq!(purpose.as_deref(), Some("ask"));
        }
        other => panic!("expected MessageQueued, got {other:?}"),
    }
    assert_eq!(
        open_ask(&state, "producer").unwrap().seq,
        events.last().unwrap().seq
    );
}

#[test]
fn note_records_plain_message_and_conflicts_with_ask() {
    let (scratch, env) = scratch();
    let sdir = studio(&scratch, &env);

    // `--note` with `--text` containing `?` records a plain message (`purpose = None`).
    let out = run_argv(
        &env,
        &[
            "tell",
            "user",
            "--text",
            "FYI: why it failed? Fixed now.",
            "--note",
            "--as",
            "producer",
        ],
        None,
    )
    .unwrap();
    assert!(out.contains("message.queued"), "{out}");

    let (events, _) = store::load_events(&sdir).unwrap();
    let state = store::load_state(&sdir).unwrap();
    match &events.last().unwrap().kind {
        EventKind::MessageQueued { to, purpose, .. } => {
            assert_eq!(to, "user");
            assert_eq!(*purpose, None);
        }
        other => panic!("expected MessageQueued, got {other:?}"),
    }
    assert_eq!(open_ask(&state, "producer"), None);

    // `--note` with `--file` containing `？` also records a plain message.
    let nfile = fixture(&scratch, "note.md", "情况说明：为何重试？已自动恢复。\n");
    run_argv(
        &env,
        &[
            "tell", "user", "--file", &nfile, "--note", "--as", "director",
        ],
        None,
    )
    .unwrap();
    let (events, _) = store::load_events(&sdir).unwrap();
    match &events.last().unwrap().kind {
        EventKind::MessageQueued { to, purpose, .. } => {
            assert_eq!(to, "user");
            assert_eq!(*purpose, None);
        }
        other => panic!("expected MessageQueued, got {other:?}"),
    }

    // `--ask` and `--note` conflict.
    let err = run_argv(
        &env,
        &[
            "tell", "user", "--text", "Which?", "--ask", "--note", "--as", "producer",
        ],
        None,
    )
    .unwrap_err();
    assert!(matches!(err, Fail::Usage(_)), "{err:?}");

    // `--note` only applies to `tell user`.
    let err = run_argv(
        &env,
        &[
            "tell", "director", "--text", "FYI?", "--note", "--as", "producer",
        ],
        None,
    )
    .unwrap_err();
    assert!(matches!(err, Fail::Usage(_)), "{err:?}");
}

#[test]
fn code_spans_fences_and_urls_do_not_trigger_the_guard() {
    let (scratch, env) = scratch();
    let sdir = studio(&scratch, &env);

    // Safe messages: code spans, fenced blocks (``` and ~~~), and URLs (`?key=`).
    let safe_cases = [
        "Fixed `option?` and `what？` handling.",
        "Handled ``a ? b : c`` in parser.",
        "Report:\n```rust\nlet x = ok ? 1 : 2;\n// 中文？\n```\nDone.",
        "Report:\n~~~~\n```\nwhat?\n```\n~~~~\nDone.",
        "See https://example.com/search?q=backlot&page=2 for logs.",
        "Check [ticket](https://example.com/items?id=42) and /runs?state=ok.",
        "All tasks merged cleanly.",
    ];
    for (idx, body) in safe_cases.iter().enumerate() {
        let out = run_argv(
            &env,
            &["tell", "user", "--text", body, "--as", "producer"],
            None,
        )
        .unwrap_or_else(|e| panic!("safe case {idx} ({body:?}) was refused: {e:?}"));
        assert!(out.contains("message.queued"), "{out}");
    }
    let (events, _) = store::load_events(&sdir).unwrap();
    assert_eq!(events.len(), 2 + safe_cases.len());

    // Prose `?` outside code spans / fences / URLs still triggers.
    for body in [
        "Look at `foo?",
        "Checked `foo?`, should we ship?",
        "```sh\necho hi?\n```\nApprove release?",
        "Did you see https://example.com/search?q=backlot?",
        "What? key=value is invalid.",
        "Is key=1?",
    ] {
        assert_guard_refused(
            &env,
            &["tell", "user", "--text", body, "--as", "producer"],
            None,
        );
    }
}

#[test]
fn exempt_targets_and_callers_are_unaffected() {
    let (scratch, env) = scratch();
    let sdir = studio(&scratch, &env);

    // 1. `tell` to any role other than `user` is unaffected.
    let out = run_argv(
        &env,
        &[
            "tell",
            "director",
            "--text",
            "Can we split t2?",
            "--as",
            "producer",
        ],
        None,
    )
    .unwrap();
    assert!(out.contains("message.queued"), "{out}");

    // 2. Caller `user` is not a studio role.
    let out = run_argv(
        &env,
        &["tell", "user", "--text", "Self note?", "--as", "user"],
        None,
    )
    .unwrap();
    assert!(out.contains("message.queued"), "{out}");

    // 3. `plugin.backlot.req` / `tell_text` is exempt (both doc_user and direct tell_text).
    let doc_res = tell_text(
        &scratch.dir,
        &env,
        &Caller::doc_user("acme-shop"),
        "user".into(),
        "Any update?",
    )
    .unwrap();
    assert!(doc_res.seq > 0);
    let ui_role_res = tell_text(
        &scratch.dir,
        &env,
        &Caller {
            studio: "acme-shop".into(),
            role: "producer".into(),
            task: None,
            session: None,
            via: Some("doc".into()),
        },
        "user".into(),
        "Any update?",
    )
    .unwrap();
    assert!(ui_role_res.seq > doc_res.seq);

    // 4. Hollywood is exempt (`tell hollywood` and `tell producer --as hollywood`).
    let hw_out = run_argv(&env, &["tell", "hollywood", "any studio waiting?"], None).unwrap();
    assert!(hw_out.contains("message.sent"), "{hw_out}");
    let hw_prod = run_argv(
        &env,
        &[
            "tell",
            "producer",
            "--studio",
            "acme-shop",
            "--text",
            "Status?",
            "--as",
            "hollywood",
        ],
        None,
    )
    .unwrap();
    assert!(hw_prod.contains("message.queued"), "{hw_prod}");

    // Hollywood trying `tell user` hits the Hollywood studio-write gate (Refused), not the guard.
    let hw_err = run_argv(
        &env,
        &[
            "tell",
            "user",
            "--studio",
            "acme-shop",
            "--text",
            "Hello?",
            "--as",
            "hollywood",
        ],
        None,
    )
    .unwrap_err();
    assert!(matches!(hw_err, Fail::Refused(_)), "{hw_err:?}");

    let (events, _) = store::load_events(&sdir).unwrap();
    assert_eq!(events.len(), 7);
}
