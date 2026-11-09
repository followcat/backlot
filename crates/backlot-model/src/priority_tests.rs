//! Task display priority (R20, D11): reduce, validation and
//! old-log loading. Order only — nothing here schedules.

use crate::event::*;
use crate::state::{TaskStatus, replay};
use crate::testkit::*;
use crate::validate::{Refusal, validate};

/// A cast may set the priority; `task.priority` changes it again.
#[test]
fn cast_and_priority_events_set_the_task_priority() {
    let mut cast_high = cast("t1", "actor-a", "a");
    if let EventKind::TaskCast { priority, .. } = &mut cast_high {
        *priority = Some(Priority::High);
    }
    let state = state_with(&base_events(), vec![("director", cast_high)]);
    assert_eq!(state.tasks["t1"].priority, Priority::High);
    // The cast record keeps what this cast set.
    assert_eq!(
        state.tasks["t1"].cast.as_ref().unwrap().priority,
        Some(Priority::High)
    );

    let state = state_with(
        &base_events(),
        vec![
            ("director", cast("t1", "actor-a", "a")),
            (
                "user",
                EventKind::TaskPriority {
                    task: "t1".into(),
                    priority: Priority::Low,
                },
            ),
        ],
    );
    assert_eq!(state.tasks["t1"].priority, Priority::Low);
}

/// A task starts `normal`; a cast without the field changes nothing.
#[test]
fn a_cast_without_priority_keeps_the_current_one() {
    let state = state_with(
        &base_events(),
        vec![
            (
                "director",
                EventKind::TaskPriority {
                    task: "t1".into(),
                    priority: Priority::High,
                },
            ),
            ("director", cast("t1", "actor-a", "a")),
            ("director", task_created("t2", "fix", 1)),
        ],
    );
    assert_eq!(state.tasks["t1"].priority, Priority::High);
    assert_eq!(state.tasks["t2"].priority, Priority::Normal);
}

/// `task.priority` wants a recorded task — nothing else, open or
/// closed.
#[test]
fn priority_needs_a_recorded_task() {
    let state = running_state();
    let set = |task: &str| {
        new_ev(
            "user",
            EventKind::TaskPriority {
                task: task.into(),
                priority: Priority::High,
            },
        )
    };
    assert_eq!(
        validate(&state, &set("t9")),
        Err(Refusal::NoTask("t9".into()))
    );
    // A terminal task still takes it — closed people sort by it too.
    let done = state_with(
        &base_events(),
        vec![
            ("director", cast("t1", "actor-a", "a")),
            ("actor", delivered("t1", 1, "a", 1)),
            ("director", reviewed("t1", 1, Verdict::Accepted, vec![])),
        ],
    );
    assert_eq!(done.tasks["t1"].status, TaskStatus::Accepted);
    assert_eq!(validate(&done, &set("t1")), Ok(()));
    assert_eq!(validate(&state, &set("t1")), Ok(()));
}

/// Logs written before the field existed still load: no `priority`
/// on `task.cast`, no `priority` on the task.
#[test]
fn old_logs_without_priority_load() {
    let line = r#"{"seq":3,"ts_ms":3000,"by":{"role":"director"},"kind":"task.cast","task":"t1","candidate":"actor-a","actor":{"runner":"r","model":"m","effort":"high","permission_profile":"standard"},"reason":"picked"}"#;
    let mut event: Event = serde_json::from_str(line).unwrap();
    match &event.kind {
        EventKind::TaskCast { priority, .. } => assert_eq!(*priority, None),
        other => panic!("wrong event: {other:?}"),
    }
    let mut log = base_events();
    event.seq = log.len() as u64 + 1;
    event.ts_ms = event.seq * 1000;
    log.push(event);
    let state = replay(&log);
    assert_eq!(state.tasks["t1"].priority, Priority::Normal);
}

/// `low|normal|high` parse and print; anything else is a usage error
/// upstream.
#[test]
fn priority_parses_the_three_levels() {
    assert_eq!("low".parse::<Priority>(), Ok(Priority::Low));
    assert_eq!("normal".parse::<Priority>(), Ok(Priority::Normal));
    assert_eq!("high".parse::<Priority>(), Ok(Priority::High));
    assert!("urgent".parse::<Priority>().is_err());
    assert_eq!(Priority::High.to_string(), "high");
}
