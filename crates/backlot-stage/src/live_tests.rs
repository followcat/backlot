//! The live-facts sidecar: the stage's summary becomes the file a role
//! terminal's `backlot status` reads, so a role's current work state
//! reaches Hollywood without a core connection.

use backlot_model::summary::{RoleSummary, StudioSummary, Summary, Work};

use crate::live::{Live, LiveRole, from_summary, path, read, work_map, write};
use crate::test_support::TestDir;

fn role_summary(role: &str, work: Option<Work>, doing: Option<&str>) -> RoleSummary {
    RoleSummary {
        role: role.into(),
        name: role.into(),
        terminal: None,
        runner: None,
        model: None,
        effort: None,
        state: "live".into(),
        doing: doing.map(String::from),
        work,
        failed: None,
        workdir: None,
        task: None,
        priority: None,
        resumable: false,
        since_ms: 0,
        attention: None,
        attention_class: None,
        asks: None,
        attention_user: false,
        attention_seq: None,
        attention_detail: None,
        stage_answer_ms: None,
    }
}

fn summary(studio: &str, roles: Vec<RoleSummary>) -> Summary {
    Summary {
        schema: "backlot/1".into(),
        updated_ms: 0,
        studios: vec![StudioSummary {
            id: studio.into(),
            title: studio.into(),
            state: "live".into(),
            requirement_version: 0,
            req_file: None,
            mode: Default::default(),
            open_reviews: 0,
            waiting: 0,
            error: None,
            next: None,
            repo: None,
            ready_to_finish: false,
            tasks_done: 0,
            tasks_total: 0,
            usage: None,
            tasks: Vec::new(),
            roles,
            exchange: Vec::new(),
            pending_tell: false,
        }],
        hollywood: None,
        backlot_version: None,
    }
}

/// A summary with one live role round-trips through the file: the same
/// work and `doing` come back.
#[test]
fn a_summary_round_trips_through_the_file() {
    let tmp = TestDir::new("backlot-live");
    let doc = summary(
        "acme-shop",
        vec![role_summary(
            "producer",
            Some(Work::Working),
            Some("running checks"),
        )],
    );
    let live = from_summary(&doc, 7_000);
    write(tmp.path(), &live).unwrap();

    let back = read(tmp.path());
    assert_eq!(back, live);
    assert_eq!(back.updated_ms, 7_000);
    let producer = back
        .roles
        .iter()
        .find(|r| r.studio == "acme-shop" && r.role == "producer")
        .expect("producer row");
    assert_eq!(producer.work, Some(Work::Working));
    assert_eq!(producer.doing.as_deref(), Some("running checks"));
    assert_eq!(
        work_map(&back).get(&("acme-shop".into(), "producer".into())),
        Some(&Work::Working)
    );
}

/// A missing file reads empty, not an error — an older state dir and a
/// stage that has never sampled both look the same.
#[test]
fn a_missing_file_reads_empty() {
    let tmp = TestDir::new("backlot-live-empty");
    assert_eq!(read(tmp.path()), Live::default());
    assert!(work_map(&read(tmp.path())).is_empty());
    assert!(!path(tmp.path()).exists());
}

/// A role the stage has not classified is left out; only rows with a
/// work state or a `doing` line become facts.
#[test]
fn only_classified_rows_become_facts() {
    let doc = summary(
        "acme",
        vec![
            role_summary("director", None, None),
            role_summary("actor-t1-c1", None, Some("writing tests")),
        ],
    );
    let live = from_summary(&doc, 1);
    assert_eq!(
        live.roles,
        vec![LiveRole {
            studio: "acme".into(),
            role: "actor-t1-c1".into(),
            work: None,
            doing: Some("writing tests".into()),
        }]
    );
}
