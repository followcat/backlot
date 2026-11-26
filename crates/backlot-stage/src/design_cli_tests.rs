//! End-to-end CLI tests covering all 7 `backlot design` subcommands,
//! role gating, `tell producer` auto-sync, and byte-identical baseline
//! when `design_gate = "off"`.

use std::fs;

use backlot_model::EventKind;
use backlot_model::design::{DesignPhase, DesignSlot, EntrySource};

use crate::cli::{Env, Fail, run, run_stdin};
use crate::cli_testkit::{CAST_TOML, Scratch, commit_in, fixture, ok, scratch, work_area};
use crate::store;

fn run_args(env: &Env, args: &[&str]) -> Result<String, Fail> {
    let owned: Vec<String> = args.iter().map(|s| (*s).to_string()).collect();
    run(&owned, env)
}

fn ok_args(env: &Env, args: &[&str]) -> String {
    run_args(env, args).unwrap_or_else(|e| panic!("expected Ok for {args:?}, got {e:?}"))
}

fn refused_args(env: &Env, args: &[&str]) -> String {
    match run_args(env, args) {
        Err(Fail::Refused(m)) => m,
        other => panic!("expected Refused for {args:?}, got {other:?}"),
    }
}

fn setup_required(summary: &str) -> (Scratch, Env) {
    let (scratch, env) = scratch();
    let cast = fixture(
        &scratch,
        "cast.toml",
        &format!("design_gate = \"required\"\n{CAST_TOML}"),
    );
    ok_args(
        &env,
        &[
            "studio",
            "new",
            "--title",
            "Acme Shop",
            "--repo",
            "/home/user/acme-shop",
            "--cast",
            &cast,
        ],
    );
    let req = fixture(&scratch, "req.md", "# Req\nBuild feature.\n");
    ok_args(
        &env,
        &[
            "req",
            "revise",
            "--as",
            "producer",
            "--file",
            &req,
            "--summary",
            summary,
        ],
    );
    (scratch, env)
}

#[test]
fn seven_design_subcommands_end_to_end_and_show_formats() {
    let (scratch, env) = setup_required("CLI two-round test");
    let dir = store::studio_dir(&scratch.dir, "acme-shop");

    let open_out = ok_args(&env, &["design", "open", "--as", "producer", "--req", "1"]);
    assert!(open_out.contains("design.opened"), "{open_out}");

    let w1 = fixture(&scratch, "w1.md", "Proposal 1 content.\n");
    let d1 = fixture(&scratch, "d1.md", "Buildability 1 content.\n");
    let p1 = fixture(&scratch, "p1.md", "Summary 1 content.\n");
    ok_args(
        &env,
        &[
            "design", "write", "--as", "writer", "--slot", "proposal", "--file", &w1,
        ],
    );
    ok_args(
        &env,
        &[
            "design",
            "write",
            "--as",
            "director",
            "--slot",
            "buildability",
            "--file",
            &d1,
        ],
    );
    ok_args(
        &env,
        &[
            "design",
            "write",
            "--as",
            "producer",
            "--slot",
            "summary",
            "--file",
            &p1,
            "--question",
            r#"{"id":"Q1","prompt":"Use SQLite?","recommendation":"yes"}"#,
        ],
    );
    let summary1_seq = store::load_events(&dir).unwrap().0.last().unwrap().seq;

    // `design note --from user --stdin` reads stdin cleanly.
    let note_args: Vec<String> = ["design", "note", "--from", "user", "--stdin"]
        .iter()
        .map(|s| (*s).to_string())
        .collect();
    run_stdin(
        &note_args,
        &env,
        Some("Use SQLite and add Round 2 details.\n"),
    )
    .unwrap();

    let sup1 = fixture(&scratch, "sup1.md", "Supplement R1.\n");
    ok_args(
        &env,
        &[
            "design",
            "supplement",
            "--as",
            "director",
            "--file",
            &sup1,
            "--refs",
            &summary1_seq.to_string(),
        ],
    );

    ok_args(&env, &["design", "next", "--as", "producer"]);
    let w2 = fixture(&scratch, "w2.md", "Proposal 2 updated.\n");
    let d2 = fixture(&scratch, "d2.md", "Buildability 2 updated.\n");
    let p2 = fixture(&scratch, "p2.md", "Summary 2 aligned.\n");
    ok_args(
        &env,
        &[
            "design", "write", "--as", "writer", "--slot", "proposal", "--file", &w2,
        ],
    );
    ok_args(
        &env,
        &[
            "design",
            "write",
            "--as",
            "director",
            "--slot",
            "buildability",
            "--file",
            &d2,
        ],
    );
    ok_args(
        &env,
        &[
            "design", "write", "--as", "producer", "--slot", "summary", "--file", &p2,
        ],
    );

    ok_args(
        &env,
        &["design", "sign", "--as", "user", "--note", "ship it"],
    );

    let show_all = ok_args(&env, &["design", "show"]);
    assert!(show_all.contains("Proposal 1 content."), "{show_all}");
    assert!(show_all.contains("Summary 2 aligned."), "{show_all}");
    assert!(show_all.contains("Sign-off"), "{show_all}");

    let show_r1 = ok_args(&env, &["design", "show", "--version", "1", "--round", "1"]);
    assert!(show_r1.contains("## R1"), "{show_r1}");
    assert!(!show_r1.contains("## R2"), "{show_r1}");

    let show_json = ok_args(&env, &["design", "show", "--json"]);
    let val: serde_json::Value = serde_json::from_str(&show_json).unwrap();
    assert_eq!(val["version"], 1);
    assert_eq!(val["phase"], "signed");
    assert_eq!(val["sign_mode"], "approved");
    assert_eq!(val["rounds"].as_array().unwrap().len(), 2);
}

#[test]
fn role_refusals_on_all_design_subcommands_and_next_in_idle() {
    let (scratch, env) = setup_required("Role gates check");
    let w1 = fixture(&scratch, "w1.md", "Proposal.\n");
    let d1 = fixture(&scratch, "d1.md", "Buildability.\n");
    let p1 = fixture(&scratch, "p1.md", "Summary.\n");

    // `design next` must fail when design is still Idle.
    assert!(!refused_args(&env, &["design", "next", "--as", "producer"]).is_empty());

    // `design open` / `next` refuse writer/director/actor.
    for role in ["writer", "director", "actor-t1-c1"] {
        assert!(!refused_args(&env, &["design", "open", "--as", role]).is_empty());
        assert!(!refused_args(&env, &["design", "next", "--as", role]).is_empty());
    }

    ok_args(&env, &["design", "open", "--as", "producer"]);

    // Proposal refuses non-writer.
    for role in ["producer", "director", "user", "actor-t1-c1"] {
        assert!(
            !refused_args(
                &env,
                &[
                    "design", "write", "--as", role, "--slot", "proposal", "--file", &w1
                ]
            )
            .is_empty()
        );
    }
    ok_args(
        &env,
        &[
            "design", "write", "--as", "writer", "--slot", "proposal", "--file", &w1,
        ],
    );

    // Buildability refuses non-director.
    for role in ["producer", "writer", "user"] {
        assert!(
            !refused_args(
                &env,
                &[
                    "design",
                    "write",
                    "--as",
                    role,
                    "--slot",
                    "buildability",
                    "--file",
                    &d1
                ]
            )
            .is_empty()
        );
    }
    ok_args(
        &env,
        &[
            "design",
            "write",
            "--as",
            "director",
            "--slot",
            "buildability",
            "--file",
            &d1,
        ],
    );

    // Summary refuses non-producer.
    for role in ["writer", "director", "user"] {
        assert!(
            !refused_args(
                &env,
                &[
                    "design", "write", "--as", role, "--slot", "summary", "--file", &p1
                ]
            )
            .is_empty()
        );
    }
    ok_args(
        &env,
        &[
            "design", "write", "--as", "producer", "--slot", "summary", "--file", &p1,
        ],
    );

    // Supplement refuses producer/user/actor.
    for role in ["producer", "user", "actor-t1-c1"] {
        assert!(
            !refused_args(
                &env,
                &[
                    "design",
                    "supplement",
                    "--as",
                    role,
                    "--file",
                    &w1,
                    "--refs",
                    "5"
                ]
            )
            .is_empty()
        );
    }

    // Note --from user refuses writer/director/actor.
    for role in ["writer", "director", "actor-t1-c1"] {
        assert!(
            !refused_args(
                &env,
                &[
                    "design", "note", "--as", role, "--from", "user", "--text", "nope"
                ]
            )
            .is_empty()
        );
    }

    // Sign refuses every role except user via --as OR BACKLOT_ROLE.
    for role in ["producer", "writer", "director", "actor", "actor-t1-c1"] {
        assert!(!refused_args(&env, &["design", "sign", "--as", role]).is_empty());
        let mut role_env = Env {
            vars: env.vars.clone(),
        };
        role_env.vars.insert("BACKLOT_ROLE".into(), role.into());
        assert!(!refused_args(&role_env, &["design", "sign"]).is_empty());
    }
}

#[test]
fn tell_producer_in_waiting_user_syncs_user_input_and_others_do_not() {
    let (scratch, env) = setup_required("Tell sync test");
    let dir = store::studio_dir(&scratch.dir, "acme-shop");
    ok_args(&env, &["design", "open", "--as", "producer"]);
    let w1 = fixture(&scratch, "w1.md", "Proposal.\n");
    let d1 = fixture(&scratch, "d1.md", "Buildability.\n");
    let p1 = fixture(&scratch, "p1.md", "Summary.\n");
    ok_args(
        &env,
        &[
            "design", "write", "--as", "writer", "--slot", "proposal", "--file", &w1,
        ],
    );
    ok_args(
        &env,
        &[
            "design",
            "write",
            "--as",
            "director",
            "--slot",
            "buildability",
            "--file",
            &d1,
        ],
    );
    ok_args(
        &env,
        &[
            "design", "write", "--as", "producer", "--slot", "summary", "--file", &p1,
        ],
    );

    // User telling writer or director must NOT write UserInput.
    ok_args(
        &env,
        &["tell", "--as", "user", "writer", "--text", "note writer"],
    );
    ok_args(
        &env,
        &["tell", "--as", "user", "director", "--text", "note dir"],
    );
    assert!(
        !store::load_events(&dir).unwrap().0.iter().any(|e| matches!(
            &e.kind,
            EventKind::DesignEntry {
                slot: DesignSlot::UserInput,
                ..
            }
        ))
    );

    // Non-user telling producer must NOT write UserInput.
    ok_args(
        &env,
        &[
            "tell",
            "--as",
            "writer",
            "producer",
            "--text",
            "from writer",
        ],
    );
    assert!(
        !store::load_events(&dir).unwrap().0.iter().any(|e| matches!(
            &e.kind,
            EventKind::DesignEntry {
                slot: DesignSlot::UserInput,
                ..
            }
        ))
    );

    // User telling producer in WaitingUser atomically records MessageQueued + DesignEntry(UserInput).
    ok_args(
        &env,
        &[
            "tell",
            "--as",
            "user",
            "producer",
            "--text",
            "Approve option A",
        ],
    );
    let events = store::load_events(&dir).unwrap().0;
    let len = events.len();
    let msg_ev = &events[len - 2];
    let entry_ev = &events[len - 1];
    assert!(matches!(msg_ev.kind, EventKind::MessageQueued { ref to, .. } if to == "producer"));
    match &entry_ev.kind {
        EventKind::DesignEntry {
            version,
            round,
            slot,
            file,
            source,
            refs,
            ..
        } => {
            assert_eq!(*version, 1);
            assert_eq!(*round, 1);
            assert_eq!(*slot, DesignSlot::UserInput);
            assert_eq!(*source, Some(EntrySource::Tell));
            assert_eq!(refs, &vec![msg_ev.seq]);
            let body = fs::read_to_string(dir.join(file)).unwrap();
            assert_eq!(body.trim(), "Approve option A");
        }
        other => panic!("expected DesignEntry, got {other:?}"),
    }
    let doc = fs::read_to_string(dir.join("design/v1.md")).unwrap();
    assert!(
        doc.contains(&format!("### §R1.U User input · tell (#{})", entry_ev.seq)),
        "{doc}"
    );
    assert!(doc.contains("Approve option A"), "{doc}");

    // Outside WaitingUser (once R2 starts), `tell producer` must NOT append UserInput.
    ok_args(&env, &["design", "next", "--as", "producer"]);
    let count_before = store::load_events(&dir).unwrap().0.len();
    ok_args(
        &env,
        &[
            "tell",
            "--as",
            "user",
            "producer",
            "--text",
            "side note during R2",
        ],
    );
    let after = store::load_events(&dir).unwrap().0;
    assert_eq!(after.len(), count_before + 1);
    assert!(matches!(
        after.last().unwrap().kind,
        EventKind::MessageQueued { .. }
    ));
}

#[test]
fn design_gate_off_preserves_byte_identical_baseline() {
    let (scratch, env) = scratch();
    let dir = store::studio_dir(&scratch.dir, "acme-shop");
    let cast = fixture(&scratch, "cast.toml", CAST_TOML);
    ok_args(
        &env,
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
    let req = fixture(&scratch, "req.md", "# requirement\nbuild the shop\n");
    ok(
        &env,
        &format!("req revise --file {req} --summary first-cut --as producer"),
    );
    ok(&env, "plan request --as producer");
    let plan = fixture(&scratch, "plan.md", "# plan\n");
    ok(
        &env,
        &format!("plan publish --kind plan --file {plan} --as writer"),
    );
    let acc = fixture(&scratch, "acc.md", "# acceptance\n");
    ok(
        &env,
        &format!("plan publish --kind acceptance --file {acc} --as writer"),
    );
    let disp = fixture(&scratch, "dispatch.md", "director: go\n");
    ok(&env, &format!("dispatch --file {disp} --as producer"));
    ok(
        &env,
        &format!("task new --title cart --kind feature --acceptance-file {acc} --as director"),
    );
    ok(
        &env,
        "cast --task t1 --actor actor-a --reason strongest --as director",
    );
    let work = work_area(&scratch, "t1");
    let a = commit_in(&work, "one");
    let b = commit_in(&work, "two");
    let rep = fixture(&scratch, "r1.md", "report 1\n");
    ok(
        &env,
        &format!(
            "deliver --commits {a}..{b} --report {rep} --check tests=pass --as actor-t1-c1 --task t1"
        ),
    );
    ok(&env, "tell --as user producer --text ship");

    assert!(
        !dir.join("design").exists(),
        "design dir must stay absent when gate is off"
    );
    let (events, _) = store::load_events(&dir).unwrap();
    let state = store::load_state(&dir).unwrap();
    assert_eq!(state.design_phase(), DesignPhase::Idle);
    assert!(state.design.is_none());
    assert!(state.design_history.is_empty());
    assert!(!events.iter().any(|e| matches!(
        e.kind,
        EventKind::DesignOpened { .. }
            | EventKind::DesignEntry { .. }
            | EventKind::DesignSigned { .. }
    )));
    let shown = ok_args(&env, &["design", "show"]);
    assert!(
        shown.to_lowercase().contains("no opening design"),
        "{shown}"
    );
}
