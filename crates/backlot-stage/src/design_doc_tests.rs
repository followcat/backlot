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
