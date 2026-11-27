//! Additional `backlot msg show` tests: reports, findings, out-of-range
//! and non-message errors, path containment, and cross-studio isolation.

use backlot_model::EventKind;
use backlot_model::NewEvent;
use backlot_model::event::EventBy;

use crate::cli::Env;
use crate::cli_testkit::{CAST_TOML, commit_in, fixture, work_area};
use crate::msg_show_tests::{last_seq, ok_args, refused_args, role_env, setup_studio, usage_args};
use crate::store;

#[test]
fn msg_show_reads_report_and_finding() {
    let (scratch, env) = setup_studio("off");
    ok_args(&env, &["plan", "request", "--as", "producer"]);
    let plan = fixture(&scratch, "plan.md", "# Plan\n");
    ok_args(
        &env,
        &[
            "plan", "publish", "--as", "writer", "--kind", "plan", "--file", &plan,
        ],
    );
    let acc = fixture(&scratch, "acc.md", "# Acceptance\n");
    ok_args(
        &env,
        &[
            "plan",
            "publish",
            "--as",
            "writer",
            "--kind",
            "acceptance",
            "--file",
            &acc,
        ],
    );
    let disp = fixture(&scratch, "disp.md", "Go build it.\n");
    ok_args(&env, &["dispatch", "--as", "producer", "--file", &disp]);
    ok_args(
        &env,
        &[
            "task",
            "new",
            "--as",
            "director",
            "--title",
            "checkout",
            "--kind",
            "feature",
            "--acceptance-file",
            &acc,
        ],
    );
    ok_args(
        &env,
        &[
            "cast",
            "--as",
            "director",
            "--task",
            "t1",
            "--actor",
            "actor-a",
            "--reason",
            "strongest",
        ],
    );
    let work = work_area(&scratch, "t1");
    let a = commit_in(&work, "init");
    let b = commit_in(&work, "impl");
    let rep_deliver = fixture(&scratch, "deliv.md", "done\n");
    ok_args(
        &env,
        &[
            "deliver",
            "--as",
            "actor-t1-c1",
            "--task",
            "t1",
            "--commits",
            &format!("{a}..{b}"),
            "--report",
            &rep_deliver,
        ],
    );
    let report_deliver_seq = last_seq(&scratch, "acme-shop");
    let deliver_out = ok_args(&env, &["msg", "show", &report_deliver_seq.to_string()]);
    assert!(deliver_out.contains("delivery report"), "{deliver_out}");
    assert!(deliver_out.contains("done\n"), "{deliver_out}");

    let find_body = concat!(
        r#"[{"id":"F1","summary":"Missing timeout check","#,
        r#""severity":"high","cause":"delivery_defect"}]"#,
    );
    let find_file = fixture(&scratch, "finding.json", find_body);
    ok_args(
        &env,
        &[
            "finding",
            "--as",
            "director",
            "--task",
            "t1",
            "--delivery",
            "1",
            "--findings",
            &find_file,
        ],
    );
    let finding_seq = last_seq(&scratch, "acme-shop");

    let node_rep = fixture(&scratch, "rep.md", "Integration passes all suites.\n");
    ok_args(
        &env,
        &[
            "report",
            "--as",
            "director",
            "--node",
            "integration",
            "--file",
            &node_rep,
        ],
    );
    let report_seq = last_seq(&scratch, "acme-shop");

    let finding_out = ok_args(&env, &["msg", "show", &finding_seq.to_string()]);
    assert!(finding_out.contains("finding"), "{finding_out}");
    assert!(
        finding_out.contains("Missing timeout check"),
        "{finding_out}"
    );

    let report_out = ok_args(&env, &["msg", "show", &report_seq.to_string()]);
    assert!(report_out.contains("report"), "{report_out}");
    assert!(
        report_out.contains("Integration passes all suites."),
        "{report_out}"
    );
}

#[test]
fn msg_show_rejects_out_of_range_non_message_and_usage_errors() {
    let (scratch, env) = setup_studio("off");
    let err1 = refused_args(&env, &["msg", "show", "1"]);
    assert!(
        err1.contains("not a message"),
        "expected non-message error, got: {err1}"
    );
    let err2 = refused_args(&env, &["msg", "show", "2"]);
    assert!(
        err2.contains("not a message"),
        "expected non-message error, got: {err2}"
    );

    let oob_zero = refused_args(&env, &["msg", "show", "0"]);
    assert!(oob_zero.contains("#0"), "{oob_zero}");
    let oob_high = refused_args(&env, &["msg", "show", "999"]);
    assert!(oob_high.contains("#999"), "{oob_high}");

    usage_args(&env, &["msg"]);
    usage_args(&env, &["msg", "unknown"]);
    usage_args(&env, &["msg", "show"]);
    usage_args(&env, &["msg", "show", "not-a-num"]);
    usage_args(&env, &["msg", "show", "1", "--unknown"]);

    let outside = fixture(&scratch, "outside.txt", "secret\n");
    let dir = store::studio_dir(&scratch.dir, "acme-shop");
    store::append(&dir, move |_, _| {
        Ok::<_, std::convert::Infallible>(store::Pending {
            event: NewEvent::new(
                EventBy::role("producer"),
                EventKind::MessageQueued {
                    to: "director".into(),
                    file: Some(outside.clone()),
                    purpose: None,
                    source_seq: None,
                },
            ),
            files: Vec::new(),
        })
    })
    .unwrap();
    let bad_seq = last_seq(&scratch, "acme-shop");
    let escape_err = refused_args(&env, &["msg", "show", &bad_seq.to_string()]);
    assert!(
        escape_err.contains("studio-relative") || escape_err.contains("outside"),
        "{escape_err}"
    );
}

#[test]
fn msg_show_refuses_other_studio_seq() {
    let (scratch, env) = setup_studio("off");
    let cast = fixture(&scratch, "cast.toml", CAST_TOML);
    let mut beta_vars = env.vars.clone();
    beta_vars.insert("BACKLOT_STUDIO".into(), "beta-app".into());
    let beta_env = Env { vars: beta_vars };
    ok_args(
        &beta_env,
        &[
            "studio",
            "new",
            "--title",
            "Beta App",
            "--repo",
            "/home/user/beta-app",
            "--cast",
            &cast,
        ],
    );
    let req_b = fixture(&scratch, "req-b.md", "# Beta\n");
    ok_args(
        &beta_env,
        &[
            "req",
            "revise",
            "--as",
            "producer",
            "--file",
            &req_b,
            "--summary",
            "beta",
        ],
    );
    let secret = fixture(&scratch, "beta-tell.md", "SECRET BETA PAYLOAD\n");
    ok_args(
        &beta_env,
        &["tell", "director", "--as", "producer", "--file", &secret],
    );
    ok_args(
        &beta_env,
        &[
            "tell",
            "user",
            "--as",
            "director",
            "--text",
            "Another beta message",
        ],
    );
    let beta_seq = last_seq(&scratch, "beta-app");
    assert!(beta_seq > last_seq(&scratch, "acme-shop"));

    let actor_acme = role_env(&scratch, "acme-shop", "actor-t1-c1", Some("t1"));
    let cross_studio = refused_args(
        &actor_acme,
        &["msg", "show", &beta_seq.to_string(), "--studio", "beta-app"],
    );
    assert!(cross_studio.contains("BACKLOT_STUDIO"), "{cross_studio}");

    let missing_in_acme = refused_args(&actor_acme, &["msg", "show", &beta_seq.to_string()]);
    assert!(missing_in_acme.contains("acme-shop"), "{missing_in_acme}");
    assert!(
        !missing_in_acme.contains("SECRET BETA"),
        "{missing_in_acme}"
    );
}
