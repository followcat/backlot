//! Tests for `backlot msg show <seq> [--json]`: tells, asks with answers,
//! design notes, supplements, and JSON shape.

use std::collections::BTreeMap;

use backlot_model::EventKind;
use backlot_model::NewEvent;
use backlot_model::event::EventBy;

use crate::cli::{Env, Fail, run, run_stdin};
use crate::cli_testkit::{CAST_TOML, Scratch, fixture, scratch};
use crate::store;

pub(crate) fn run_args(env: &Env, args: &[&str]) -> Result<String, Fail> {
    let owned: Vec<String> = args.iter().map(|s| (*s).to_string()).collect();
    run(&owned, env)
}

pub(crate) fn ok_args(env: &Env, args: &[&str]) -> String {
    run_args(env, args).unwrap_or_else(|e| panic!("expected Ok for {args:?}, got {e:?}"))
}

pub(crate) fn refused_args(env: &Env, args: &[&str]) -> String {
    match run_args(env, args) {
        Err(Fail::Refused(m)) => m,
        other => panic!("expected Refused for {args:?}, got {other:?}"),
    }
}

pub(crate) fn usage_args(env: &Env, args: &[&str]) -> String {
    match run_args(env, args) {
        Err(Fail::Usage(m)) => m,
        other => panic!("expected Usage for {args:?}, got {other:?}"),
    }
}

pub(crate) fn setup_studio(gate: &str) -> (Scratch, Env) {
    let (scratch, env) = scratch();
    let cast = fixture(
        &scratch,
        "cast.toml",
        &format!("design_gate = \"{gate}\"\n{CAST_TOML}"),
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
    let req = fixture(&scratch, "req.md", "# Req\nBuild checkout.\n");
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
            "checkout",
        ],
    );
    (scratch, env)
}

pub(crate) fn last_seq(scratch: &Scratch, studio: &str) -> u64 {
    let dir = store::studio_dir(&scratch.dir, studio);
    store::load_events(&dir).unwrap().0.last().unwrap().seq
}

pub(crate) fn role_env(scratch: &Scratch, studio: &str, role: &str, task: Option<&str>) -> Env {
    let mut vars = BTreeMap::new();
    vars.insert(
        "BACKLOT_STATE_DIR".into(),
        scratch.dir.to_string_lossy().into_owned(),
    );
    vars.insert("BACKLOT_STUDIO".into(), studio.into());
    vars.insert("BACKLOT_ROLE".into(), role.into());
    if let Some(t) = task {
        vars.insert("BACKLOT_TASK".into(), t.into());
    }
    Env { vars }
}

#[test]
fn msg_show_reads_tell_with_seq_or_hash_prefix_and_any_role() {
    let (scratch, env) = setup_studio("off");
    let dir = store::studio_dir(&scratch.dir, "acme-shop");
    let msg_file = fixture(
        &scratch,
        "tell.md",
        "Line one of the tell.\nLine two with details.\n",
    );
    ok_args(
        &env,
        &["tell", "director", "--as", "producer", "--file", &msg_file],
    );
    let tell_seq = last_seq(&scratch, "acme-shop");
    let before_count = store::load_events(&dir).unwrap().0.len();

    let plain = ok_args(&env, &["msg", "show", &tell_seq.to_string()]);
    assert!(plain.contains(&format!("#{tell_seq}")), "{plain}");
    assert!(plain.contains("tell"), "{plain}");
    assert!(plain.contains("producer"), "{plain}");
    assert!(plain.contains("director"), "{plain}");
    assert!(plain.contains("UTC"), "{plain}");
    assert!(plain.contains("Line one of the tell."), "{plain}");
    assert!(plain.contains("Line two with details."), "{plain}");

    let hashed = ok_args(&env, &["msg", "show", &format!("#{tell_seq}")]);
    assert_eq!(plain, hashed);

    for role in ["producer", "director", "writer", "actor-t1-c1", "user"] {
        let renv = role_env(
            &scratch,
            "acme-shop",
            role,
            role.starts_with("actor-").then_some("t1"),
        );
        let out = ok_args(&renv, &["msg", "show", &tell_seq.to_string()]);
        assert!(out.contains("Line one of the tell."), "{role}: {out}");
    }
    assert_eq!(store::load_events(&dir).unwrap().0.len(), before_count);
}

#[test]
fn msg_show_reads_ask_with_tell_answer_and_terminal_answer() {
    let (scratch, env) = setup_studio("off");
    let dir = store::studio_dir(&scratch.dir, "acme-shop");

    ok_args(
        &env,
        &[
            "tell",
            "user",
            "--as",
            "director",
            "--text",
            "Should checkout require a promo code?",
            "--ask",
        ],
    );
    let ask_seq = last_seq(&scratch, "acme-shop");

    let unanswered: serde_json::Value = serde_json::from_str(&ok_args(
        &env,
        &["msg", "show", &ask_seq.to_string(), "--json"],
    ))
    .unwrap();
    assert_eq!(unanswered["kind"], "ask");
    assert_eq!(unanswered["from"], "director");
    assert_eq!(unanswered["to"], "user");
    assert!(unanswered["answer"].is_null(), "{unanswered}");

    ok_args(
        &env,
        &[
            "tell",
            "director",
            "--as",
            "user",
            "--text",
            "No, keep promo codes optional.",
        ],
    );
    let answer_seq = last_seq(&scratch, "acme-shop");

    let ask_with_answer = ok_args(&env, &["msg", "show", &ask_seq.to_string()]);
    assert!(ask_with_answer.contains("ask"), "{ask_with_answer}");
    assert!(
        ask_with_answer.contains("Should checkout require a promo code?"),
        "{ask_with_answer}"
    );
    assert!(
        ask_with_answer.contains(&format!("#{answer_seq}")),
        "{ask_with_answer}"
    );
    assert!(
        ask_with_answer.contains("No, keep promo codes optional."),
        "{ask_with_answer}"
    );

    let ask_json: serde_json::Value = serde_json::from_str(&ok_args(
        &env,
        &["msg", "show", &ask_seq.to_string(), "--json"],
    ))
    .unwrap();
    assert_eq!(ask_json["kind"], "ask");
    assert_eq!(ask_json["answer"]["seq"], answer_seq);
    assert_eq!(ask_json["answer"]["from"], "user");
    assert_eq!(ask_json["answer"]["kind"], "answer");
    assert_eq!(ask_json["answer"]["body"], "No, keep promo codes optional.");

    let ans_json: serde_json::Value = serde_json::from_str(&ok_args(
        &env,
        &["msg", "show", &answer_seq.to_string(), "--json"],
    ))
    .unwrap();
    assert_eq!(ans_json["kind"], "answer");
    assert_eq!(ans_json["from"], "user");
    assert_eq!(ans_json["to"], "director");
    assert_eq!(ans_json["in_reply_to"], ask_seq);
    assert_eq!(ans_json["body"], "No, keep promo codes optional.");

    ok_args(
        &env,
        &[
            "tell",
            "user",
            "--as",
            "writer",
            "--text",
            "Use Postgres 16?",
            "--ask",
        ],
    );
    let ask2_seq = last_seq(&scratch, "acme-shop");
    store::append(&dir, move |_, _| {
        Ok::<_, std::convert::Infallible>(store::Pending {
            event: NewEvent::new(
                EventBy::role("stage"),
                EventKind::AskAnswered {
                    role: "writer".into(),
                    target: ask2_seq,
                },
            ),
            files: Vec::new(),
        })
    })
    .unwrap();
    let term_ans_seq = last_seq(&scratch, "acme-shop");

    let ask2_text = ok_args(&env, &["msg", "show", &ask2_seq.to_string()]);
    assert!(
        ask2_text.contains(&format!("#{term_ans_seq}")),
        "{ask2_text}"
    );
    let term_ans: serde_json::Value = serde_json::from_str(&ok_args(
        &env,
        &["msg", "show", &term_ans_seq.to_string(), "--json"],
    ))
    .unwrap();
    assert_eq!(term_ans["kind"], "answer");
    assert_eq!(term_ans["from"], "user");
    assert_eq!(term_ans["to"], "writer");
    assert_eq!(term_ans["in_reply_to"], ask2_seq);
}

#[test]
fn msg_show_reads_design_note_supplement_proposal_summary_and_signoff() {
    let (scratch, env) = setup_studio("required");
    let dir = store::studio_dir(&scratch.dir, "acme-shop");

    ok_args(&env, &["design", "open", "--as", "producer", "--req", "1"]);
    let w1 = fixture(&scratch, "w1.md", "Proposal body.\n");
    ok_args(
        &env,
        &[
            "design", "write", "--as", "writer", "--slot", "proposal", "--file", &w1,
        ],
    );
    let proposal_seq = last_seq(&scratch, "acme-shop");

    let d1 = fixture(&scratch, "d1.md", "Buildability body.\n");
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
    ok_args(
        &env,
        &[
            "design", "write", "--as", "producer", "--slot", "summary", "--file", &p1,
        ],
    );

    run_stdin(
        &[
            "design".into(),
            "note".into(),
            "--as".into(),
            "producer".into(),
            "--from".into(),
            "user".into(),
            "--stdin".into(),
        ],
        &env,
        Some("Latency must stay under 50ms.\n"),
    )
    .unwrap();
    let note_seq = last_seq(&scratch, "acme-shop");

    let sup1 = fixture(&scratch, "sup1.md", "Indexed query meets 20ms target.\n");
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
            &note_seq.to_string(),
        ],
    );
    let supp_seq = last_seq(&scratch, "acme-shop");

    ok_args(
        &env,
        &["design", "sign", "--as", "user", "--note", "approved v1"],
    );
    let signed_seq = last_seq(&scratch, "acme-shop");
    assert!(dir.join("design/v1-final.md").is_file());

    let note_out = ok_args(&env, &["msg", "show", &note_seq.to_string()]);
    assert!(note_out.contains("design note"), "{note_out}");
    assert!(note_out.contains("user"), "{note_out}");
    assert!(
        note_out.contains("Latency must stay under 50ms."),
        "{note_out}"
    );

    let supp_json: serde_json::Value = serde_json::from_str(&ok_args(
        &env,
        &["msg", "show", &supp_seq.to_string(), "--json"],
    ))
    .unwrap();
    assert_eq!(supp_json["studio"], "acme-shop");
    assert_eq!(supp_json["seq"], supp_seq);
    assert_eq!(supp_json["kind"], "supplement");
    assert_eq!(supp_json["from"], "writer");
    assert_eq!(supp_json["to"], "design");
    assert!(supp_json["ts_ms"].as_u64().is_some());
    assert!(supp_json["when"].as_str().unwrap().contains("UTC"));
    assert_eq!(supp_json["refs"], serde_json::json!([note_seq]));
    assert_eq!(supp_json["body"], "Indexed query meets 20ms target.\n");

    let prop_json: serde_json::Value = serde_json::from_str(&ok_args(
        &env,
        &["msg", "show", &proposal_seq.to_string(), "--json"],
    ))
    .unwrap();
    assert_eq!(prop_json["kind"], "design proposal");
    assert_eq!(prop_json["body"], "Proposal body.\n");

    let signed_out = ok_args(&env, &["msg", "show", &signed_seq.to_string()]);
    assert!(signed_out.contains("design sign-off"), "{signed_out}");
    assert!(signed_out.contains("approved v1"), "{signed_out}");
}
