//! Fail-first tests for PR-5 Rule A (`looping`) and Rule B (`no progress`).

use backlot_model::attention::AttentionClass;
use backlot_model::event::*;
use backlot_model::state::replay;
use backlot_model::testkit::*;

use crate::liveness_progress::{evaluate_looping, evaluate_no_progress};

fn refused_ev(
    seq: u64,
    ts_ms: u64,
    role: &str,
    rule: Option<&str>,
    command: &str,
    argv: &[&str],
) -> Event {
    Event {
        v: LEDGER_VERSION,
        seq,
        ts_ms,
        by: EventBy::role("stage"),
        kind: EventKind::RoleRefused {
            role: role.into(),
            task: None,
            command: command.into(),
            cwd: "/home/user/acme-shop".into(),
            argv: argv.iter().map(|s| s.to_string()).collect(),
            rule: rule.map(str::to_string),
            disposition: RefusalDisposition::Refused,
        },
    }
}

fn task_cast_ev(seq: u64, ts_ms: u64, task: &str, kind: &str, readonly: bool) -> Vec<Event> {
    let mut act = actor("a");
    if readonly {
        act.permission_profile = "readonly".into();
    }
    let role = format!("actor-{task}-c1");
    vec![
        Event {
            v: LEDGER_VERSION,
            seq,
            ts_ms,
            by: EventBy::role("director"),
            kind: task_created(task, kind, 1),
        },
        Event {
            v: LEDGER_VERSION,
            seq: seq + 1,
            ts_ms: ts_ms + 1,
            by: EventBy::role("director"),
            kind: EventKind::TaskCast {
                task: task.into(),
                candidate: "actor-a".into(),
                actor: act.clone(),
                reason: "picked".into(),
                priority: None,
            },
        },
        Event {
            v: LEDGER_VERSION,
            seq: seq + 2,
            ts_ms: ts_ms + 2,
            by: EventBy::role("stage"),
            kind: EventKind::RoleStarted {
                role,
                terminal: Some(format!("bl-acme-shop-{task}")),
                actor: Some(act),
                session_id: Some("s1".into()),
                reason: None,
                prompt_hash: None,
            },
        },
    ]
}

#[test]
fn rule_a_five_identical_non_probe_refusals_trigger_looping_once_and_isolate_roles() {
    let mut events = base_events();
    events.extend(task_cast_ev(10, 10_000, "t1", "bugfix", false));
    events.extend(task_cast_ev(20, 10_000, "t2", "bugfix", false));

    // 4 refusals on actor-t1-c1 do not trigger yet; 4 #209 probes do not count.
    for i in 0..4 {
        events.push(refused_ev(
            30 + i,
            100_000 + i * 5_000,
            "actor-t1-c1",
            Some("no-rm-rf"),
            "rm -rf /tmp/foo",
            &["rm", "-rf", "/tmp/foo"],
        ));
        events.push(refused_ev(
            40 + i,
            100_500 + i * 5_000,
            "actor-t1-c1",
            Some("probe-rule"),
            "git --version",
            &["git", "--version"],
        ));
    }
    let state_before = replay(&events);
    assert!(evaluate_looping(&state_before, "actor-t1-c1", 125_000).is_none());

    // 5th identical refusal raises looping for actor-t1-c1 routed to director, leaving actor-t2-c1 untouched.
    events.push(refused_ev(
        50,
        130_000,
        "actor-t1-c1",
        Some("no-rm-rf"),
        "rm -rf /tmp/foo",
        &["rm", "-rf", "/tmp/foo"],
    ));
    let state_5 = replay(&events);
    let alert = evaluate_looping(&state_5, "actor-t1-c1", 130_000)
        .expect("5 identical non-probe refusals in 10m must trigger looping");
    assert_eq!(alert.role, "actor-t1-c1");
    assert_eq!(alert.reviewer.as_deref(), Some("director"));
    assert!(evaluate_looping(&state_5, "actor-t2-c1", 130_000).is_none());

    // Class is never NeedsYou.
    assert_ne!(AttentionClass::of(LOOPING, false), AttentionClass::NeedsYou);

    // Repeating inside the same 10m window suppresses a second alert (even after a 6th refusal).
    events.push(Event {
        v: LEDGER_VERSION,
        seq: 51,
        ts_ms: 130_100,
        by: EventBy::role("stage"),
        kind: EventKind::AttentionRaised {
            role: "actor-t1-c1".into(),
            reason: LOOPING.into(),
            detail: "looping".into(),
            needs_user: false,
            account: None,
            reset_at_ms: None,
        },
    });
    events.push(refused_ev(
        52,
        140_000,
        "actor-t1-c1",
        Some("no-rm-rf"),
        "rm -rf /tmp/foo",
        &["rm", "-rf", "/tmp/foo"],
    ));
    let state_after = replay(&events);
    assert!(evaluate_looping(&state_after, "actor-t1-c1", 140_000).is_none());
}

fn attn_raised(seq: u64, ts_ms: u64, role: &str, reason: &str, detail: &str) -> Event {
    Event {
        v: LEDGER_VERSION,
        seq,
        ts_ms,
        by: EventBy::role("stage"),
        kind: EventKind::AttentionRaised {
            role: role.into(),
            reason: reason.into(),
            detail: detail.into(),
            needs_user: false,
            account: None,
            reset_at_ms: None,
        },
    }
}

#[test]
fn rule_b_excludes_non_coding_and_readonly_and_subtracts_waits() {
    let mut events = base_events();
    // Non-coding kinds + readonly coding actor never trigger even after 30 minutes (1_800_000 ms).
    events.extend(task_cast_ev(10, 0, "t-train", "train", false));
    events.extend(task_cast_ev(20, 0, "t-analysis", "analysis", false));
    events.extend(task_cast_ev(30, 0, "t-verify", "verification", false));
    events.extend(task_cast_ev(40, 0, "t-ro", "bugfix", true));
    let non_coding_state = replay(&events);
    for role in [
        "actor-t-train-c1",
        "actor-t-analysis-c1",
        "actor-t-verify-c1",
        "actor-t-ro-c1",
    ] {
        assert!(
            evaluate_no_progress(&non_coding_state, role, Some(0), &[], 1_800_000).is_none(),
            "{role} must never trigger no-progress"
        );
    }

    // `bugfix` actor with a 15-minute (900_000 ms) `permission prompt` wait does not trigger
    // before 20 minutes (1_200_000 ms) of actual active Running time, then triggers once 20m elapses.
    let mut bugfix = base_events();
    bugfix.extend(task_cast_ev(10, 0, "t1", "bugfix", false));
    bugfix.push(attn_raised(
        20,
        120_000,
        "actor-t1-c1",
        PERMISSION_PROMPT,
        "prompt",
    ));
    bugfix.push(Event {
        v: LEDGER_VERSION,
        seq: 21,
        ts_ms: 1_020_000, // 15m later cleared
        by: EventBy::role("director"),
        kind: EventKind::AttentionCleared {
            target: 20,
            reason: Some("allowed".into()),
        },
    });
    let st = replay(&bugfix);
    // At 30m wall-clock (15m wait -> 15m active running): must NOT trigger yet.
    assert!(
        evaluate_no_progress(&st, "actor-t1-c1", Some(0), &[], 1_800_000).is_none(),
        "30m wall minus 15m wait = 15m active < 20m threshold"
    );
    // At 35m2s wall-clock (15m wait -> 20m+ active running): must trigger to director!
    let alert = evaluate_no_progress(&st, "actor-t1-c1", Some(0), &[], 2_101_000)
        .expect("bugfix past 20m real running time with no change must trigger no-progress");
    assert_eq!(alert.role, "actor-t1-c1");
    assert_eq!(alert.to.as_deref(), Some("director"));
    assert_ne!(
        AttentionClass::of(NO_PROGRESS, false),
        AttentionClass::NeedsYou
    );

    // Once raised in this stretch, never fires again until work area changes or commit lands.
    let mut raised = bugfix.clone();
    raised.push(attn_raised(
        22,
        2_101_000,
        "actor-t1-c1",
        NO_PROGRESS,
        "no change",
    ));
    let raised_st = replay(&raised);
    assert!(
        evaluate_no_progress(&raised_st, "actor-t1-c1", Some(0), &[], 3_600_000).is_none(),
        "must emit at most once per unchanged stretch"
    );

    // Live Work::Blocked intervals and other coding kinds work accurately too.
    assert!(
        evaluate_no_progress(
            &st,
            "actor-t1-c1",
            Some(0),
            &[(1_200_000, 1_500_000)],
            2_200_000,
        )
        .is_none(),
        "live Work::Blocked intervals must also be subtracted"
    );
    for k in "BugFix,feature,fix,refactor,impl,docs,tooling,infra,site,migration,test".split(',') {
        assert!(crate::liveness_progress::is_coding_task_kind(k));
    }
}

#[test]
fn evidence_tracks_last_work_area_change_ms_and_resets_on_file_or_commit() {
    use std::path::PathBuf;

    use crate::liveness::{Evidence, ObserveInput};
    use crate::liveness_diff::WorkAreaSnap;
    use crate::workareas::GitOut;

    let root = PathBuf::from("/home/user/acme-shop/work/actor-t1-c1");
    let roots = [root];
    let mut git = |_: &[&str], _: &str| GitOut {
        code: 0,
        stdout: String::new(),
        stderr: String::new(),
    };
    let mk_input = |now_ms| ObserveInput {
        now_ms,
        runner: "fake",
        eligible: true,
        raw_screen: "idle",
        raw_same_since_ms: 0,
        signal: None,
        terminal_working: true,
        activity_revision: None,
        local_machine: true,
        pid: None,
        work_roots: &roots,
        ledger: None,
    };
    let mut ev = Evidence::new(
        &mk_input(1_000),
        &|_| Vec::new(),
        &|_, _, _| WorkAreaSnap {
            head_sig: "abc".into(),
            index_mtime_ns: Some(1),
            status_hash: Some(10),
        },
        &mut git,
    );
    assert_eq!(ev.work_area_change_ms(), Some(1_000));
    ev.observe(
        &mk_input(65_000),
        &|_| Vec::new(),
        &|_, _, _| WorkAreaSnap {
            head_sig: "abc".into(),
            index_mtime_ns: Some(1),
            status_hash: Some(10),
        },
        &mut git,
    );
    assert_eq!(ev.work_area_change_ms(), Some(1_000));
    ev.observe(
        &mk_input(130_000),
        &|_| Vec::new(),
        &|_, _, _| WorkAreaSnap {
            head_sig: "def".into(),
            index_mtime_ns: Some(2),
            status_hash: Some(20),
        },
        &mut git,
    );
    assert_eq!(ev.work_area_change_ms(), Some(130_000));
    let empty_ev = Evidence::new(
        &mk_input(1_000),
        &|_| Vec::new(),
        &|_, _, _| WorkAreaSnap::default(),
        &mut git,
    );
    assert_eq!(empty_ev.work_area_change_ms(), None);
}

#[test]
fn rule_a_fixed_role_routes_to_producer_and_expires_after_window() {
    let mut events = base_events();
    for i in 0..5 {
        events.push(refused_ev(
            10 + i,
            100_000 + i * 1_000,
            "writer",
            Some("no-rm"),
            "rm -rf src",
            &["rm", "-rf", "src"],
        ));
    }
    let st = replay(&events);
    let alert = evaluate_looping(&st, "writer", 110_000).expect("writer looping");
    assert_eq!(alert.reviewer.as_deref(), Some("producer"));
    // Once window passes (100_000 + 600_000 < 715_000), window expires and a new burst can trigger.
    assert!(evaluate_looping(&st, "writer", 715_000).is_none());
}

#[test]
fn looping_and_no_progress_route_notices_without_halting_or_needing_user() {
    let mut events = task_cast_ev(10, 1_000, "t1", "bugfix", false);
    events.push(Event {
        v: LEDGER_VERSION,
        seq: 13,
        ts_ms: 2_000,
        by: EventBy::role("director"),
        kind: EventKind::RoleStarted {
            role: "director".into(),
            terminal: Some("ttyd-d".into()),
            actor: None,
            session_id: None,
            reason: Some("direct".into()),
            prompt_hash: None,
        },
    });
    events.push(attn_raised(
        14,
        5_000,
        "actor-t1-c1",
        LOOPING,
        "5 blocked git push calls",
    ));
    events.push(attn_raised(
        15,
        6_000,
        "actor-t1-c1",
        NO_PROGRESS,
        "no changes for 20m",
    ));
    let st = replay(&events);
    assert!(st.halted.is_none());
    assert!(st.open_attention().all(|a| !a.needs_user));
    let owed = crate::notices::owed(&st, std::path::Path::new("/state/studios/acme-shop"));
    assert!(
        owed.iter()
            .any(|n| n.label == "looping" && n.to == "director" && n.source_seq == 14)
    );
    assert!(
        owed.iter()
            .any(|n| n.label == "no progress" && n.to == "director" && n.source_seq == 15)
    );
}
