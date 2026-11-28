//! The live-facts sidecar: the stage's summary becomes the file a role
//! terminal's `backlot status` reads, so a role's current work state
//! reaches Hollywood without a core connection.

use backlot_model::summary::{RoleRunning, RoleSummary, StudioSummary, Summary, Work};

use crate::live::{
    HEARTBEAT_MS, Live, LiveRole, STALE_MS, Write, due, from_summary, path, read, read_fresh,
    work_map, write,
};
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
        progress: None,
        running: (work == Some(Work::Working)).then(|| RoleRunning {
            cmd: "cargo test".into(),
            since_ms: 5_000,
        }),
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
        can_answer_prompt: true,
        restarted_ms: None,
        idle_since_ms: None,
        queue: None,
        account: None,
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
            actors_live: 0,
            actors_cap: 0,
            usage: None,
            tasks: Vec::new(),
            roles,
            exchange: Vec::new(),
            pending_tell: false,
            outward_cards: Vec::new(),
            needs_upgrade: false,
            unknown_kinds: Vec::new(),
            design: None,
            halted: None,
        }],
        hollywood: None,
        backlot_version: None,
        backlot_build: None,
        agent_skill: None,
        accounts: Vec::new(),
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

    // Read 7 s after the write: fresh.
    let back = read(tmp.path(), 14_000);
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
        producer.running,
        Some(RoleRunning {
            cmd: "cargo test".into(),
            since_ms: 5_000,
        })
    );
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
    assert_eq!(read(tmp.path(), 1_000), Live::default());
    assert!(work_map(&read(tmp.path(), 1_000)).is_empty());
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
            running: None,
            idle_since_ms: None,
        }]
    );
}

/// A file older than `STALE_MS` reads empty and is not "fresh": after
/// the stage stops, its last `· working` is a stale snapshot, not
/// current state.
#[test]
fn an_old_file_reads_empty() {
    let tmp = TestDir::new("backlot-live-stale");
    let doc = summary(
        "acme",
        vec![role_summary(
            "producer",
            Some(Work::Working),
            Some("running checks"),
        )],
    );
    let live = from_summary(&doc, 1_000);
    write(tmp.path(), &live).unwrap();

    // Exactly at the stale boundary is still fresh…
    assert!(read_fresh(tmp.path(), 1_000 + STALE_MS).is_some());
    // …one millisecond past it is stale.
    let late = 1_000 + STALE_MS + 1;
    assert_eq!(read(tmp.path(), late), Live::default());
    assert!(read_fresh(tmp.path(), late).is_none());
    assert!(work_map(&read(tmp.path(), late)).is_empty());
}

/// The write throttle: an unchanged summary is not rewritten within the
/// heartbeat, and is rewritten once the heartbeat is due.
#[test]
fn an_unchanged_summary_is_not_rewritten_within_the_heartbeat() {
    let doc = summary(
        "acme",
        vec![role_summary("producer", Some(Work::Working), None)],
    );
    let written = from_summary(&doc, 1_000);
    // Same roles, same tick: skip.
    assert_eq!(due(&written, Some(&written), 1_000), Write::Skip);
    // A tick later, still inside the heartbeat: skip.
    assert_eq!(
        due(&written, Some(&written), 1_000 + HEARTBEAT_MS - 1),
        Write::Skip
    );
    // The heartbeat is due: write.
    assert_eq!(
        due(&written, Some(&written), 1_000 + HEARTBEAT_MS),
        Write::Now
    );
    // A change writes at once, whatever the clock.
    let changed = from_summary(
        &summary(
            "acme",
            vec![role_summary("producer", Some(Work::Waiting), None)],
        ),
        1_001,
    );
    assert_eq!(due(&changed, Some(&written), 1_001), Write::Now);
    // No file yet: write.
    assert_eq!(due(&written, None, 1_001), Write::Now);
}

#[test]
fn accounts_propagate_through_live_json_and_trigger_due() {
    use backlot_model::summary::{AccountRoleSummary, AccountSummary, QuotaWindow};

    let tmp = TestDir::new("backlot-live-accounts");
    let mut doc = summary(
        "acme",
        vec![role_summary("producer", Some(Work::Working), None)],
    );
    let acct = AccountSummary {
        id: "claude:default".into(),
        runner: "claude".into(),
        source: "default".into(),
        active: 2,
        working: 1,
        max_active: None,
        room: None,
        queued: 0,
        stalled: true,
        last_stall_ms: Some(1_000),
        reset_at_ms: Some(19_000),
        window: Some(QuotaWindow::Short),
        roles: vec![AccountRoleSummary {
            studio: "acme".into(),
            role: "producer".into(),
            model: "opus-5-5".into(),
            active: true,
            working: true,
            held: false,
            stalled: true,
        }],
    };
    doc.accounts = vec![acct.clone()];

    let live_with = from_summary(&doc, 2_000);
    assert_eq!(live_with.accounts, vec![acct.clone()]);

    let live_without = from_summary(
        &summary(
            "acme",
            vec![role_summary("producer", Some(Work::Working), None)],
        ),
        2_000,
    );
    assert_eq!(due(&live_with, Some(&live_without), 2_500), Write::Now);

    write(tmp.path(), &live_with).unwrap();
    let view = crate::live::LiveView::read(tmp.path(), 2_500);
    assert_eq!(view.accounts, vec![acct]);

    // Legacy `live.json` without `accounts` field parses cleanly.
    std::fs::write(path(tmp.path()), r#"{"updated_ms":3000,"roles":[]}"#).unwrap();
    let legacy = crate::live::LiveView::read(tmp.path(), 3_500);
    assert!(legacy.running);
    assert!(legacy.accounts.is_empty());
}
