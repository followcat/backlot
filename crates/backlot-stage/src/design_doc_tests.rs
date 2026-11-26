//! Integration tests for immutable design section files, atomic
//! assembly (`design/v<rv>.md`), and frozen sign-off snapshots
//! (`design/v<rv>-final.md`).

use std::fs;
use std::path::Path;

use backlot_model::EventKind;
use backlot_model::design::SignMode;

use crate::cli::{Env, Fail, run};
use crate::cli_testkit::{CAST_TOML, Scratch, fixture, scratch};
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

fn setup_required_studio(summary: &str) -> (Scratch, Env) {
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
    let req = fixture(&scratch, "req.md", "# Checkout v1\nAdd fast checkout.\n");
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

fn last_seq(dir: &Path) -> u64 {
    store::load_events(dir).unwrap().0.last().unwrap().seq
}

fn collect_temp_leftovers(dir: &Path, out: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let p = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with(".tmp-") || name.starts_with("design-doc-") {
            out.push(p.display().to_string());
        }
        if p.is_dir() {
            collect_temp_leftovers(&p, out);
        }
    }
}

#[test]
fn section_files_stay_immutable_across_rounds_and_doc_retains_anchors() {
    let (scratch, env) = setup_required_studio("Checkout flow v1");
    let dir = store::studio_dir(&scratch.dir, "acme-shop");

    ok_args(&env, &["design", "open", "--as", "producer"]);

    let w1 = fixture(&scratch, "w1.md", "Proposal R1 body.\nUses queue.\n");
    ok_args(
        &env,
        &[
            "design", "write", "--as", "writer", "--slot", "proposal", "--file", &w1,
        ],
    );
    let w1_seq = last_seq(&dir);
    let w1_sec_path = dir.join(format!("design/v1/r1-{w1_seq}-proposal.md"));
    let w1_initial_bytes = fs::read(&w1_sec_path).expect("r1 proposal section file must exist");
    assert_eq!(
        String::from_utf8_lossy(&w1_initial_bytes),
        "Proposal R1 body.\nUses queue.\n"
    );

    let d1 = fixture(&scratch, "d1.md", "Buildability R1 body.\nClean split.\n");
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
            "--refs",
            &w1_seq.to_string(),
        ],
    );
    let d1_seq = last_seq(&dir);

    let p1 = fixture(&scratch, "p1.md", "Producer R1 synthesis.\n");
    let q2_file = fixture(
        &scratch,
        "q2.json",
        r#"{"id":"Q2","prompt":"Cache backend?","recommendation":"sqlite"}"#,
    );
    let q2_flag = format!("@{q2_file}");
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
            "--refs",
            &format!("{w1_seq},{d1_seq}"),
            "--question",
            r#"{"id":"Q1","prompt":"Sync or async?","recommendation":"async","section_ref":"§R1.W"}"#,
            "--question",
            &q2_flag,
        ],
    );
    let p1_seq = last_seq(&dir);

    ok_args(
        &env,
        &[
            "design",
            "note",
            "--as",
            "user",
            "--from",
            "user",
            "--text",
            "Q1 async, Q2 sqlite, clarify retry policy.",
        ],
    );
    let u1_seq = last_seq(&dir);

    let sup1 = fixture(&scratch, "sup1.md", "Writer R1 retry clarification.\n");
    ok_args(
        &env,
        &[
            "design",
            "supplement",
            "--as",
            "writer",
            "--file",
            &sup1,
            "--refs",
            &p1_seq.to_string(),
        ],
    );
    let sup1_seq = last_seq(&dir);

    ok_args(&env, &["design", "next", "--as", "producer"]);
    let w2 = fixture(&scratch, "w2.md", "Proposal R2 with retry policy.\n");
    ok_args(
        &env,
        &[
            "design", "write", "--as", "writer", "--slot", "proposal", "--file", &w2,
        ],
    );
    let w2_seq = last_seq(&dir);

    let d2 = fixture(&scratch, "d2.md", "Buildability R2 verified.\n");
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
    let d2_seq = last_seq(&dir);

    let p2 = fixture(&scratch, "p2.md", "Final aligned summary.\n");
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
            &p2,
            "--refs",
            &format!("{w2_seq},{d2_seq}"),
        ],
    );
    let p2_seq = last_seq(&dir);

    // Must still be waiting for user input or allow sign directly from WaitingUser.
    ok_args(&env, &["design", "sign", "--note", "approved for dispatch"]);
    let sign_seq = last_seq(&dir);

    // Earlier section file must remain byte-identical across subsequent writes & sign.
    assert_eq!(fs::read(&w1_sec_path).unwrap(), w1_initial_bytes);
    for rel in [
        format!("design/v1/r1-{w1_seq}-proposal.md"),
        format!("design/v1/r1-{d1_seq}-buildability.md"),
        format!("design/v1/r1-{p1_seq}-summary.md"),
        format!("design/v1/r1-{u1_seq}-user_input.md"),
        format!("design/v1/r1-{sup1_seq}-supplement.md"),
        format!("design/v1/r2-{w2_seq}-proposal.md"),
        format!("design/v1/r2-{d2_seq}-buildability.md"),
        format!("design/v1/r2-{p2_seq}-summary.md"),
    ] {
        assert!(dir.join(&rel).is_file(), "missing section file {rel}");
    }

    let doc = fs::read_to_string(dir.join("design/v1.md")).expect("design/v1.md must exist");
    for needle in [
        "# Design discussion · req v1: Checkout flow v1",
        "## R1",
        &format!("### §R1.W Proposal · writer (#{w1_seq})"),
        &format!("### §R1.D Buildability · director (#{d1_seq})"),
        &format!("### §R1.P Summary · producer (#{p1_seq})"),
        "#### Questions",
        "**Q1**",
        "Sync or async?",
        "**Q2**",
        "Cache backend?",
        &format!("### §R1.U User input · note (#{u1_seq})"),
        &format!("### §R1.W+ Supplement · writer (#{sup1_seq})"),
        "## R2",
        &format!("### §R2.W Proposal · writer (#{w2_seq})"),
        &format!("### §R2.D Buildability · director (#{d2_seq})"),
        &format!("### §R2.P Summary · producer (#{p2_seq})"),
        &format!("## Sign-off (#{sign_seq})"),
        "approved for dispatch",
    ] {
        assert!(doc.contains(needle), "missing '{needle}' in v1.md:\n{doc}");
    }

    let frozen =
        fs::read_to_string(dir.join("design/v1-final.md")).expect("v1-final.md must exist");
    for needle in ["user", "approved", "UTC", "r2", "approved for dispatch"] {
        assert!(
            frozen.contains(needle),
            "missing '{needle}' in v1-final.md:\n{frozen}"
        );
    }
    assert!(frozen.contains("## R1"));
    assert!(frozen.contains("## R2"));

    let mut leftovers = Vec::new();
    collect_temp_leftovers(&dir, &mut leftovers);
    assert!(leftovers.is_empty(), "leftover temp files: {leftovers:?}");
}

#[test]
fn rejects_empty_entry_file_and_more_than_three_questions() {
    let (scratch, env) = setup_required_studio("Empty & question limits");
    let dir = store::studio_dir(&scratch.dir, "acme-shop");
    ok_args(&env, &["design", "open", "--as", "producer"]);

    let empty = fixture(&scratch, "empty.md", "   \n\t\n");
    let count_before = store::load_events(&dir).unwrap().0.len();
    let msg = refused_args(
        &env,
        &[
            "design", "write", "--as", "writer", "--slot", "proposal", "--file", &empty,
        ],
    );
    assert!(msg.to_lowercase().contains("empty"), "{msg}");
    assert_eq!(store::load_events(&dir).unwrap().0.len(), count_before);

    let w1 = fixture(&scratch, "w1.md", "Valid proposal.\n");
    ok_args(
        &env,
        &[
            "design", "write", "--as", "writer", "--slot", "proposal", "--file", &w1,
        ],
    );
    let d1 = fixture(&scratch, "d1.md", "Valid buildability.\n");
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

    let p1 = fixture(&scratch, "p1.md", "Summary body.\n");
    let four_q = fixture(
        &scratch,
        "four.json",
        r#"[
            {"id":"Q1","prompt":"p1","recommendation":"r1"},
            {"id":"Q2","prompt":"p2","recommendation":"r2"},
            {"id":"Q3","prompt":"p3","recommendation":"r3"},
            {"id":"Q4","prompt":"p4","recommendation":"r4"}
        ]"#,
    );
    let before_q = store::load_events(&dir).unwrap().0.len();
    let q_err = refused_args(
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
            "--questions-file",
            &four_q,
        ],
    );
    assert!(
        q_err.contains("3") || q_err.to_lowercase().contains("question"),
        "{q_err}"
    );
    assert_eq!(store::load_events(&dir).unwrap().0.len(), before_q);
}

#[test]
fn design_sign_skip_records_sign_off_without_final_snapshot() {
    let (scratch, env) = setup_required_studio("Fix typo in footer");
    let dir = store::studio_dir(&scratch.dir, "acme-shop");

    ok_args(
        &env,
        &[
            "design",
            "open",
            "--as",
            "producer",
            "--suggest-skip",
            "--reason",
            "typo fix only",
        ],
    );
    ok_args(
        &env,
        &[
            "design",
            "sign",
            "--skip",
            "--reason",
            "agreed trivial",
            "--note",
            "go straight to dispatch",
        ],
    );

    assert!(!dir.join("design/v1-final.md").exists());
    let doc = fs::read_to_string(dir.join("design/v1.md")).expect("v1.md must exist");
    assert!(doc.contains("## Sign-off"), "{doc}");
    assert!(doc.contains("skipped"), "{doc}");

    let events = store::load_events(&dir).unwrap().0;
    match &events.last().unwrap().kind {
        EventKind::DesignSigned {
            version,
            round,
            mode,
            doc_file,
            note,
        } => {
            assert_eq!(*version, 1);
            assert_eq!(*round, 0);
            assert_eq!(*mode, SignMode::Skipped);
            assert_eq!(doc_file, "");
            assert_eq!(note.as_deref(), Some("go straight to dispatch"));
        }
        other => panic!("expected DesignSigned, got {other:?}"),
    }
}

#[test]
fn concurrent_entry_appends_leave_clean_sections_and_no_temp_leftovers() {
    let (scratch, env) = setup_required_studio("Concurrent WaitingUser writes");
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
    let summary_seq = last_seq(&dir);
    let sw = fixture(&scratch, "sw.md", "Writer concurrent supplement.\n");
    let sd = fixture(&scratch, "sd.md", "Director concurrent supplement.\n");
    let sum_ref = summary_seq.to_string();

    std::thread::scope(|s| {
        let e1 = &env;
        let e2 = &env;
        let e3 = &env;
        let sw_p = &sw;
        let sd_p = &sd;
        let sr = &sum_ref;
        let t1 = s.spawn(move || {
            ok_args(
                e1,
                &[
                    "design",
                    "note",
                    "--as",
                    "user",
                    "--from",
                    "user",
                    "--text",
                    "Concurrent user decision.",
                ],
            )
        });
        let t2 = s.spawn(move || {
            ok_args(
                e2,
                &[
                    "design",
                    "supplement",
                    "--as",
                    "writer",
                    "--file",
                    sw_p,
                    "--refs",
                    sr,
                ],
            )
        });
        let t3 = s.spawn(move || {
            ok_args(
                e3,
                &[
                    "design",
                    "supplement",
                    "--as",
                    "director",
                    "--file",
                    sd_p,
                    "--refs",
                    sr,
                ],
            )
        });
        t1.join().unwrap();
        t2.join().unwrap();
        t3.join().unwrap();
    });

    let doc = fs::read_to_string(dir.join("design/v1.md")).unwrap();
    assert!(doc.contains("Concurrent user decision."), "{doc}");
    assert!(doc.contains("Writer concurrent supplement."), "{doc}");
    assert!(doc.contains("Director concurrent supplement."), "{doc}");

    let mut leftovers = Vec::new();
    collect_temp_leftovers(&dir, &mut leftovers);
    assert!(leftovers.is_empty(), "leftover temp files: {leftovers:?}");
}

#[test]
fn post_copy_runs_only_after_ledger_commit_and_failed_write_leaves_no_doc() {
    use std::sync::atomic::{AtomicBool, Ordering};

    let (scratch, _env) = setup_required_studio("Commit before doc render");
    let dir = store::studio_dir(&scratch.dir, "acme-shop");

    let build_open = |_state: &backlot_model::state::StudioState, _seq: u64| {
        Ok::<store::Pending, std::convert::Infallible>(store::Pending {
            event: backlot_model::NewEvent::new(
                backlot_model::EventBy::role("producer"),
                EventKind::DesignOpened {
                    version: 1,
                    round: 1,
                    suggest_skip: false,
                    reason: None,
                    doc_file: "design/v1.md".into(),
                },
            ),
            files: Vec::new(),
        })
    };

    // If writing events.jsonl fails, post_copy must never run and no design doc may appear.
    let ran_post = AtomicBool::new(false);
    let err = store::append_with_test_hooks(
        &dir,
        build_open,
        |_, _| Err(std::io::Error::other("simulated write failure")),
        |_, _| {
            ran_post.store(true, Ordering::SeqCst);
        },
    )
    .unwrap_err();
    assert!(matches!(err, store::StoreError::Io(_)), "{err:?}");
    assert!(!ran_post.load(Ordering::SeqCst));
    assert!(!dir.join("design/v1.md").exists());
    assert!(
        !store::load_events(&dir)
            .unwrap()
            .0
            .iter()
            .any(|e| matches!(e.kind, EventKind::DesignOpened { .. }))
    );

    // On normal write, post_copy observes the newly stamped event already committed in events.jsonl.
    let saw_committed = AtomicBool::new(false);
    let ev = store::append_with_test_hooks(
        &dir,
        build_open,
        store::write_events_file,
        |state, stamped| {
            let events_on_disk = store::load_events(&dir).unwrap().0;
            let last = events_on_disk.last().unwrap();
            assert_eq!(last.seq, stamped[0].seq);
            assert!(matches!(
                last.kind,
                EventKind::DesignOpened {
                    version: 1,
                    round: 1,
                    ..
                }
            ));
            saw_committed.store(true, Ordering::SeqCst);
            crate::design_doc::sync_on_append(&dir, state, stamped);
        },
    )
    .unwrap();
    assert!(saw_committed.load(Ordering::SeqCst));
    assert!(dir.join("design/v1.md").is_file());
    assert_eq!(
        ev.seq,
        store::load_events(&dir).unwrap().0.last().unwrap().seq
    );
}

#[test]
fn unwritable_or_file_design_still_commits_event_and_next_append_or_show_heals() {
    struct RestoreDirMode(std::path::PathBuf);
    impl Drop for RestoreDirMode {
        fn drop(&mut self) {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = fs::set_permissions(&self.0, fs::Permissions::from_mode(0o755));
            }
        }
    }

    let (scratch, env) = setup_required_studio("Recover missed design render");
    let dir = store::studio_dir(&scratch.dir, "acme-shop");

    // Verify exact stderr warning format helper.
    let warn = crate::design_doc::sync_warning(&std::io::Error::new(
        std::io::ErrorKind::PermissionDenied,
        "denied",
    ));
    assert_eq!(
        warn,
        "backlot: design doc not updated: denied; it is rebuilt on the next design write"
    );

    // 1. Place a regular file at `design`: `design open` must still commit cleanly.
    fs::write(dir.join("design"), "blocking file\n").unwrap();
    ok_args(&env, &["design", "open", "--as", "producer"]);
    assert!(dir.join("design").is_file());
    assert!(store::load_events(&dir).unwrap().0.iter().any(|e| matches!(
        e.kind,
        EventKind::DesignOpened {
            version: 1,
            round: 1,
            ..
        }
    )));

    // 2. Removing the blocker and appending next entry rebuilds `design/v1.md` from state.
    fs::remove_file(dir.join("design")).unwrap();
    let w1 = fixture(
        &scratch,
        "w1.md",
        "Proposal recovered after file blocker.\n",
    );
    ok_args(
        &env,
        &[
            "design", "write", "--as", "writer", "--slot", "proposal", "--file", &w1,
        ],
    );
    let doc_after_w1 = fs::read_to_string(dir.join("design/v1.md")).unwrap();
    assert!(doc_after_w1.contains("## R1"), "{doc_after_w1}");
    assert!(
        doc_after_w1.contains("Proposal recovered after file blocker."),
        "{doc_after_w1}"
    );

    // 3. Make `design/` unwritable (plus directory blocker if running as uid 0) while `design/v1/`
    // remains writable for the section file: `buildability` commits cleanly without failing.
    let design_dir = dir.join("design");
    let doc_file = design_dir.join("v1.md");
    fs::remove_file(&doc_file).unwrap();
    let v1_blocker = doc_file.join("blocker");
    #[cfg(unix)]
    let _mode_guard = {
        use std::os::unix::fs::PermissionsExt;
        if unsafe { libc::geteuid() } == 0 {
            fs::create_dir_all(&v1_blocker).unwrap();
        }
        fs::set_permissions(&design_dir, fs::Permissions::from_mode(0o555)).unwrap();
        RestoreDirMode(design_dir.clone())
    };
    let d1 = fixture(&scratch, "d1.md", "Buildability while v1.md blocked.\n");
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
    assert!(store::load_events(&dir).unwrap().0.iter().any(|e| matches!(
        e.kind,
        EventKind::DesignEntry {
            slot: backlot_model::design::DesignSlot::Buildability,
            ..
        }
    )));
    assert!(!doc_file.is_file());

    // 4. Restore `design/` permissions and run `backlot design show`: missed render heals on disk.
    drop(_mode_guard);
    if v1_blocker.exists() {
        fs::remove_dir_all(&doc_file).unwrap();
    }
    let shown = ok_args(&env, &["design", "show"]);
    assert!(
        shown.contains("Proposal recovered after file blocker."),
        "{shown}"
    );
    assert!(
        shown.contains("Buildability while v1.md blocked."),
        "{shown}"
    );
    let healed = fs::read_to_string(&doc_file).expect("design show must heal v1.md on disk");
    assert!(
        healed.contains("Proposal recovered after file blocker."),
        "{healed}"
    );
    assert!(
        healed.contains("Buildability while v1.md blocked."),
        "{healed}"
    );
}

#[test]
fn freeze_final_is_idempotent_and_never_overwrites_differing_content() {
    let (scratch, env) = setup_required_studio("Idempotent freeze");
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
    ok_args(&env, &["design", "sign", "--note", "freeze test"]);

    let (events, state) = (
        store::load_events(&dir).unwrap().0,
        store::load_state(&dir).unwrap(),
    );
    let sign_ev = events.last().unwrap().clone();
    let final_path = dir.join("design/v1-final.md");
    let frozen_bytes = fs::read(&final_path).unwrap();

    // Re-syncing identical frozen content is idempotent (`Ok`).
    crate::design_doc::try_sync_on_append(&dir, &state, std::slice::from_ref(&sign_ev)).unwrap();
    assert_eq!(fs::read(&final_path).unwrap(), frozen_bytes);

    // Differing existing final file must never be overwritten.
    fs::write(&final_path, "CUSTOM FROZEN CONTENT\n").unwrap();
    let err = crate::design_doc::try_sync_on_append(&dir, &state, std::slice::from_ref(&sign_ev))
        .unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::AlreadyExists);
    assert_eq!(
        fs::read_to_string(&final_path).unwrap(),
        "CUSTOM FROZEN CONTENT\n"
    );

    // Best-effort heals (`sync_on_append` and `design show`) still warn without failing or overwriting.
    crate::design_doc::sync_on_append(&dir, &state, &[sign_ev]);
    let shown = ok_args(&env, &["design", "show"]);
    assert!(shown.contains("## Sign-off"), "{shown}");
    assert_eq!(
        fs::read_to_string(&final_path).unwrap(),
        "CUSTOM FROZEN CONTENT\n"
    );
}
