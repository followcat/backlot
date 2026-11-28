//! Fail-first stage & CLI tests for `backlot decide` and `decisions/current.md`.

use std::fs;
use std::path::Path;

use backlot_model::event::{EventBy, EventKind, NewEvent, Verdict};
use backlot_model::state::{Message, StudioState, TaskStatus};
use backlot_model::testkit::{cast, delivered, ev, reviewed, studio_kind, task_created};

use crate::cli::{Env, Fail, run};
use crate::cli_testkit::{CAST_TOML, Scratch, fixture, ok, scratch};
use crate::deliver::outbox;
use crate::digest::{self, OutItem};
use crate::hwood_read::{report_relpath, report_show};
use crate::stale::{drop_reason, stale_reason};
use crate::store::{self, Pending};

fn run_args(env: &Env, args: &[&str]) -> Result<String, Fail> {
    let owned: Vec<String> = args.iter().map(|s| (*s).to_string()).collect();
    run(&owned, env)
}

fn ok_args(env: &Env, args: &[&str]) -> String {
    run_args(env, args).unwrap_or_else(|e| panic!("expected Ok for {args:?}, got {e:?}"))
}

fn setup_studio(scratch: &Scratch, env: &Env) -> std::path::PathBuf {
    let cast = fixture(
        scratch,
        "cast.toml",
        &format!("design_gate = \"off\"\n{CAST_TOML}"),
    );
    ok(
        env,
        &format!("studio new --title Shop --repo /home/user/acme-shop --cast {cast}"),
    );
    let req = fixture(scratch, "req.md", "# v1\nBuild shop.\n");
    ok_args(
        env,
        &[
            "req",
            "revise",
            "--as",
            "producer",
            "--file",
            &req,
            "--summary",
            "v1",
        ],
    );
    store::studio_dir(&scratch.dir, "acme-shop")
}

fn append_ev(dir: &Path, role: &str, kind: EventKind) -> u64 {
    store::append(dir, move |_, _| {
        Ok::<_, std::convert::Infallible>(Pending {
            event: NewEvent::new(EventBy::role(role), kind),
            files: Vec::new(),
        })
    })
    .unwrap()
    .seq
}

#[test]
fn decide_without_notify_queues_producer_and_renders_current_md() {
    let (scratch, env) = scratch();
    let sdir = setup_studio(&scratch, &env);
    let body = fixture(&scratch, "ad1.md", "Store sessions in Postgres.\n");

    let out = ok_args(
        &env,
        &[
            "decide", "--as", "director", "--id", "AD-1", "--file", &body,
        ],
    );
    assert!(out.contains("AD-1"), "{out}");

    let (events, _) = store::load_events(&sdir).unwrap();
    let msg = events
        .iter()
        .find(|e| {
            matches!(
                &e.kind,
                EventKind::MessageQueued { purpose: Some(p), .. } if p == "decision"
            )
        })
        .expect("decision MessageQueued event should be appended");
    let EventKind::MessageQueued {
        to,
        file,
        source_seq,
        ..
    } = &msg.kind
    else {
        unreachable!()
    };
    assert_eq!(to, "producer");
    assert_eq!(*source_seq, None);
    let rel = file.as_deref().expect("decision message file");
    let stored = fs::read_to_string(sdir.join(rel)).unwrap();
    assert!(stored.contains("AD-1"), "{stored}");
    assert!(stored.contains("Store sessions in Postgres."), "{stored}");

    let doc = fs::read_to_string(sdir.join("decisions/current.md")).unwrap();
    assert!(doc.contains("AD-1"), "{doc}");
    assert!(doc.contains(&format!("#{}", msg.seq)), "{doc}");
    assert!(doc.contains("Store sessions in Postgres."), "{doc}");
}

fn cast_wt(task: &str, name: &str, tag: &str) -> EventKind {
    let mut id = backlot_model::testkit::actor(tag);
    id.permission_profile = "worktree".into();
    EventKind::TaskCast {
        task: task.into(),
        candidate: name.into(),
        actor: id,
        reason: "picked".into(),
        priority: None,
    }
}

fn delivered_wt(task: &str, n: u32, tag: &str, req: u32) -> EventKind {
    let mut id = backlot_model::testkit::actor(tag);
    id.permission_profile = "worktree".into();
    EventKind::TaskDelivered {
        task: task.into(),
        delivery: n,
        actor: id,
        commits: "a..b".into(),
        report: format!("reports/{task}-d{n}.md"),
        requirement_version: req,
        checks: Vec::new(),
        usage: None,
    }
}

#[test]
fn decide_notify_dedupes_current_md_sorts_numerically_and_folds_superseded_history() {
    let (scratch, env) = scratch();
    let sdir = setup_studio(&scratch, &env);

    append_ev(&sdir, "director", task_created("t1", "feature", 1));
    append_ev(&sdir, "director", cast_wt("t1", "actor-a", "a"));
    append_ev(&sdir, "director", task_created("t2", "feature", 1));
    append_ev(&sdir, "director", cast_wt("t2", "actor-a", "a"));
    append_ev(&sdir, "actor-t2-c1", delivered_wt("t2", 1, "a", 1));
    append_ev(
        &sdir,
        "director",
        reviewed("t2", 1, Verdict::Accepted, vec![]),
    );
    append_ev(&sdir, "director", task_created("t3", "feature", 1));
    append_ev(&sdir, "director", cast_wt("t3", "actor-a", "a"));

    let f2 = fixture(&scratch, "ad2.md", "Use JSON responses for /api/*.\n");
    ok_args(
        &env,
        &[
            "decide",
            "--as",
            "producer",
            "--id",
            "AD-2",
            "--file",
            &f2,
            "--notify",
            "director,active-actors",
        ],
    );

    let (events, _) = store::load_events(&sdir).unwrap();
    let batch: Vec<_> = events
        .iter()
        .filter(|e| {
            matches!(
                &e.kind,
                EventKind::MessageQueued { purpose: Some(p), .. } if p == "decision"
            )
        })
        .collect();
    let mut recipients: Vec<_> = batch
        .iter()
        .map(|e| match &e.kind {
            EventKind::MessageQueued { to, .. } => to.as_str(),
            _ => "",
        })
        .collect();
    recipients.sort();
    assert_eq!(recipients, vec!["actor-t1-c1", "actor-t3-c1", "director"]);
    let auth_seq = batch.iter().map(|e| e.seq).min().unwrap();

    let f10 = fixture(&scratch, "ad10.md", "Rate limit uploads at gateway.\n");
    ok_args(
        &env,
        &[
            "decide", "--as", "director", "--id", "AD-10", "--file", &f10,
        ],
    );
    let doc_before = fs::read_to_string(sdir.join("decisions/current.md")).unwrap();
    let pos_2_before = doc_before.find("### AD-2").expect("AD-2 before AD-10");
    let pos_10_before = doc_before.find("### AD-10").expect("AD-10 present");
    assert!(pos_2_before < pos_10_before, "{doc_before}");

    let f11 = fixture(&scratch, "ad11.md", "Switch /api/* responses to CBOR.\n");
    ok_args(
        &env,
        &[
            "decide",
            "--as",
            "director",
            "--id",
            "AD-11",
            "--supersedes",
            "AD-2",
            "--file",
            &f11,
        ],
    );

    let doc = fs::read_to_string(sdir.join("decisions/current.md")).unwrap();
    let current_head = doc.find("## History").expect("History section must exist");
    let current_sec = &doc[..current_head];
    let history_sec = &doc[current_head..];

    // Numeric ordering: AD-10 then AD-11 in Current, AD-2 folded to History.
    let pos_10 = current_sec.find("### AD-10").expect("AD-10 in current");
    let pos_11 = current_sec.find("### AD-11").expect("AD-11 in current");
    assert!(pos_10 < pos_11, "{current_sec}");
    assert!(!current_sec.contains("### AD-2"), "{current_sec}");
    assert!(history_sec.contains("**AD-2**"), "{history_sec}");
    assert!(
        history_sec.contains(&format!("#{auth_seq}")),
        "{history_sec}"
    );
    assert_eq!(doc.matches(&format!("#{auth_seq}")).count(), 1, "{doc}");
}

#[test]
fn decisions_current_md_readable_via_report_relpath_and_report_show() {
    let (scratch, env) = scratch();
    let _sdir = setup_studio(&scratch, &env);
    let body = fixture(&scratch, "ad1.md", "Keep API stateless.\n");
    ok_args(
        &env,
        &[
            "decide", "--as", "director", "--id", "AD-1", "--file", &body,
        ],
    );

    assert!(report_relpath(Path::new("decisions/current.md")).is_ok());
    assert!(report_relpath(Path::new("decisions/other.md")).is_err());
    assert!(report_relpath(Path::new("decisions/../events.jsonl")).is_err());

    let common = crate::args::Common {
        studio: Some("acme-shop".into()),
        ..Default::default()
    };
    let mut out = Vec::new();
    report_show(
        &common,
        &env,
        &scratch.dir,
        Path::new("decisions/current.md"),
        &mut out,
    )
    .unwrap();
    let shown = String::from_utf8(out).unwrap();
    assert!(shown.contains("AD-1"), "{shown}");
}

#[test]
fn decision_message_delivers_formats_digest_and_drops_on_closed_actor() {
    let dir = Path::new("/state/studios/acme-shop");
    let mut state = StudioState::default();
    state.messages.push(Message {
        seq: 12,
        from: "director".into(),
        to: "actor-t1-c1".into(),
        file: Some("messages/12-ad-1.md".into()),
        purpose: Some("decision".into()),
        source_seq: None,
        delivered: false,
        dropped: false,
        dropped_reason: None,
        ts_ms: 1000,
        via: None,
        delivered_ms: None,
    });

    let out = outbox(&state, dir);
    assert_eq!(out.len(), 1);
    assert_eq!(
        out[0].text,
        "[backlot] director → actor-t1-c1: read /state/studios/acme-shop/messages/12-ad-1.md"
    );
    assert_eq!(stale_reason(&state, &state.messages[0]), None);

    let body = digest::body(&[OutItem::Message(state.messages[0].clone())], dir);
    assert!(body.contains("## decision (seq 12)"), "{body}");

    let mut closed = backlot_model::replay(&[
        ev(1, "user", studio_kind()),
        ev(
            2,
            "producer",
            EventKind::ReqVersion {
                version: 1,
                file: "req/v1.md".into(),
                summary: "s".into(),
            },
        ),
        ev(3, "director", task_created("t1", "feature", 1)),
        ev(4, "director", cast("t1", "actor-a", "a")),
        ev(5, "actor-t1-c1", delivered("t1", 1, "a", 1)),
        ev(6, "director", reviewed("t1", 1, Verdict::Accepted, vec![])),
    ]);
    assert_eq!(
        closed.tasks.get("t1").map(|t| t.status),
        Some(TaskStatus::Accepted)
    );
    let m = Message {
        seq: 13,
        from: "director".into(),
        to: "actor-t1-c1".into(),
        file: Some("messages/13-ad-2.md".into()),
        purpose: Some("decision".into()),
        source_seq: None,
        delivered: false,
        dropped: false,
        dropped_reason: None,
        ts_ms: 2000,
        via: None,
        delivered_ms: None,
    };
    closed.messages.push(m);
    assert_eq!(
        drop_reason("acme-shop", &closed, &[], &closed.messages[0]),
        Some(backlot_model::event::ACTOR_CLOSED)
    );
}

#[test]
fn decide_refuses_disallowed_roles_conflicting_headers_and_closed_targets() {
    let (scratch, env) = scratch();
    let sdir = setup_studio(&scratch, &env);

    append_ev(&sdir, "director", task_created("t1", "feature", 1));
    append_ev(&sdir, "director", cast_wt("t1", "actor-a", "a"));
    append_ev(&sdir, "actor-t1-c1", delivered_wt("t1", 1, "a", 1));
    append_ev(
        &sdir,
        "director",
        reviewed("t1", 1, Verdict::Accepted, vec![]),
    );

    let f = fixture(&scratch, "ok.md", "Keep API stateless.\n");
    for role in ["actor-t1-c1", "writer", "hollywood"] {
        let err = run_args(
            &env,
            &["decide", "--as", role, "--id", "AD-1", "--file", &f],
        )
        .unwrap_err();
        assert!(
            matches!(err, Fail::Refused(_)),
            "{role} expected refused, got {err:?}"
        );
    }

    let mismatch = fixture(&scratch, "mismatch.md", "# AD-9: Something else\n\nBody.\n");
    let err = run_args(
        &env,
        &[
            "decide", "--as", "director", "--id", "AD-1", "--file", &mismatch,
        ],
    )
    .unwrap_err();
    assert!(matches!(err, Fail::Refused(_)), "{err:?}");

    // Empty active-actors refuses cleanly.
    let err = run_args(
        &env,
        &[
            "decide",
            "--as",
            "director",
            "--id",
            "AD-1",
            "--file",
            &f,
            "--notify",
            "active-actors",
        ],
    )
    .unwrap_err();
    assert!(matches!(err, Fail::Refused(_)), "{err:?}");

    // Explicit closed actor target refuses with TellToClosedActor.
    let err = run_args(
        &env,
        &[
            "decide",
            "--as",
            "director",
            "--id",
            "AD-1",
            "--file",
            &f,
            "--notify",
            "actor-t1-c1",
        ],
    )
    .unwrap_err();
    assert!(matches!(err, Fail::Refused(_)), "{err:?}");

    // Unknown --id or self-supersede returns usage error.
    let err = run_args(
        &env,
        &["decide", "--as", "director", "--id", "foo", "--file", &f],
    )
    .unwrap_err();
    assert!(matches!(err, Fail::Usage(_)), "{err:?}");
    let err = run_args(
        &env,
        &[
            "decide",
            "--as",
            "director",
            "--id",
            "AD-1",
            "--supersedes",
            "AD-1",
            "--file",
            &f,
        ],
    )
    .unwrap_err();
    assert!(matches!(err, Fail::Usage(_)), "{err:?}");
}
