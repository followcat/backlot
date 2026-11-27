//! The terminal-shaped calls: a launch, a stop and a finished
//! removal, a screen sample, and a delivery send — each the sequence of
//! driver calls today's direct client calls made.

use super::*;
use crate::reconcile::Action;

#[tokio::test]
async fn a_launch_issues_the_file_writes_then_terminal_run() {
    let root = tmp("launch");
    let dir = studio_dir(&root, "acme");
    seed(&dir, false);
    let state = state_of(&dir);
    let (stage, log) = recorded_stage(&root).await;

    let start = match crate::reconcile::reconcile("acme", &state, &BTreeMap::new(), 10_000)
        .into_iter()
        .find(|a| matches!(a, Action::Start(s) if s.role == "producer"))
    {
        Some(Action::Start(start)) => start,
        other => panic!("no producer start: {other:?}"),
    };
    stage
        .act(
            "acme",
            &dir,
            &state,
            &Action::Start(start),
            &mut true,
            &nothing_blocked(),
        )
        .await
        .unwrap();

    let calls = calls(&log);
    // `terminal.run` came last, after the guard shims and hook files
    // the role needs on disk first.
    let run = calls
        .iter()
        .position(|c| c.starts_with("run "))
        .unwrap_or_else(|| panic!("no terminal.run in {calls:?}"));
    assert!(calls[run].contains("bl-acme-producer"), "{calls:?}");
    assert_eq!(
        run,
        calls.len() - 1,
        "terminal.run is the last call: {calls:?}"
    );
    assert!(
        calls[..run]
            .iter()
            .any(|c| c.starts_with("write_executable ")),
        "the guard shims are written before the launch: {calls:?}"
    );
    assert!(
        calls[..run]
            .iter()
            .any(|c| c.starts_with("write_file ") && c.contains("claude-settings.json")),
        "the hook settings file is written before the launch: {calls:?}"
    );
    // And the ledger now records the terminal the run answered.
    let after = state_of(&dir);
    assert_eq!(after.roles["producer"].terminal.as_deref(), Some("t7"));
}

/// `Stop` and `RemoveFinished` each issue the terminal calls the stage
/// made directly before: `kill` then `rm` for a stop, only `rm` for a
/// finished record whose program is gone.

#[tokio::test]
async fn kills_and_removals_issue_the_terminal_calls_in_order() {
    let root = tmp("teardown");
    let dir = studio_dir(&root, "acme");
    seed(&dir, false);
    let state = state_of(&dir);
    let (stage, log) = recorded_stage(&root).await;

    stage
        .act(
            "acme",
            &dir,
            &state,
            &Action::Stop {
                role: "producer".into(),
                terminal: "bl-acme-producer".into(),
                address: "t7".into(),
                close: false,
                reason: "test".into(),
            },
            &mut true,
            &nothing_blocked(),
        )
        .await
        .unwrap();
    assert_eq!(calls(&log), vec!["kill t7", "rm t7"]);

    log.lock().unwrap().clear();
    stage
        .act(
            "acme",
            &dir,
            &state,
            &Action::RemoveFinished {
                terminal: "bl-acme-producer".into(),
                address: "t9".into(),
            },
            &mut true,
            &nothing_blocked(),
        )
        .await
        .unwrap();
    assert_eq!(
        calls(&log),
        vec!["rm t9"],
        "a finished record is only removed"
    );
}

#[tokio::test]
async fn a_screen_sample_reads_the_role_terminal() {
    let root = tmp("sample");
    let dir = studio_dir(&root, "acme");
    seed(&dir, false);
    let state = state_of(&dir);
    let (stage, log) = recorded_stage(&root).await;
    let term = live_terminal("bl-acme-producer", "t7", &dir);
    let watched = crate::sample::Watched {
        dir: &dir,
        state: &state,
        role: "producer",
        runner: "fake",
        term: &term,
    };
    stage.sample_work("acme", &watched, 10_000).await.unwrap();
    assert_eq!(calls(&log), vec!["read t7"]);
}

/// A delivery send goes out as one `terminal.send` naming the role's
/// text to its address.
#[tokio::test]
async fn a_delivery_send_issues_one_terminal_send() {
    let root = tmp("deliver");
    let dir = studio_dir(&root, "acme");
    seed(&dir, false);
    crate::store::append(&dir, |_, _| {
        Ok::<_, std::convert::Infallible>(crate::store::Pending {
            event: NewEvent::new(
                EventBy::role("producer"),
                EventKind::MessageQueued {
                    to: "director".into(),
                    file: Some("messages/9-note.md".into()),
                    purpose: None,
                    // A source seq would have the notice pass queue a
                    // second item; without one this is the only send.
                    source_seq: None,
                },
            ),
            files: Vec::new(),
        })
    })
    .unwrap();
    // The gate wants the director live; the ledger's `role.started`
    // makes it so, its terminal the one the live list carries.
    crate::store::append(&dir, |_, _| {
        Ok::<_, std::convert::Infallible>(crate::store::Pending {
            event: NewEvent::new(
                EventBy::role("stage"),
                EventKind::RoleStarted {
                    role: "director".into(),
                    terminal: Some("t5".into()),
                    actor: Some(actor()),
                    session_id: None,
                    reason: None,
                    prompt_hash: None,
                },
            ),
            files: Vec::new(),
        })
    })
    .unwrap();
    let state = state_of(&dir);
    let (stage, log) = recorded_stage(&root).await;
    // The gate needs a stable `waiting` screen and a role up past
    // `MIN_UP_MS`; all three tracks are in-memory.
    let pair = ("acme".to_string(), "director".to_string());
    stage
        .work
        .borrow_mut()
        .insert(pair.clone(), backlot_model::summary::Work::Waiting);
    stage.screens.borrow_mut().insert(
        pair.clone(),
        crate::deliver::Screen {
            text: "stable".into(),
            same_since_ms: 0,
            at_ms: 5_000,
        },
    );
    let live = vec![live_terminal("bl-acme-director", "t5", &dir)];
    // The gate wants the role live for `MIN_UP_MS`; `since_ms` is the
    // ledger's wall clock, so the sample time is just past it.
    let now_ms = state.roles["director"].since_ms + crate::deliver::MIN_UP_MS + 1;
    stage
        .deliver("acme", &dir, &state, &live, now_ms)
        .await
        .unwrap();
    // The send is off-loop; wait for its handle to finish so the
    // recorded call lands.
    for _ in 0..200 {
        if log.lock().unwrap().iter().any(|c| c.starts_with("send ")) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    let calls = calls(&log);
    let sends: Vec<&String> = calls.iter().filter(|c| c.starts_with("send ")).collect();
    assert_eq!(sends.len(), 1, "one send went out: {calls:?}");
    assert!(sends[0].starts_with("send t5 "), "{calls:?}");
    assert!(
        sends[0].contains("messages/"),
        "the line names the queued file: {calls:?}"
    );
    assert!(
        sends[0].contains("[backlot]"),
        "the line is the stage's own: {calls:?}"
    );
}
