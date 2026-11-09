//! Prompt rendering: every placeholder fills, every `backlot`
//! command line the four prompts name parses through the real
//! argument parser, and the first prompt a fresh start gets is the
//! one line pointing at the rendered file.

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use backlot_model::event::{ActorIdentity, Priority};
use backlot_model::state::{StudioState, Task, TaskStatus};

use crate::args;
use crate::prompts::*;
use crate::reconcile::{RoleKind, Start};

static NEXT: AtomicU64 = AtomicU64::new(0);

fn tmp() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "backlot-prompt-test-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn actor() -> ActorIdentity {
    ActorIdentity {
        runner: "fake".into(),
        runner_version: None,
        model: "plain".into(),
        effort: "high".into(),
        permission_profile: "standard".into(),
    }
}

fn start(kind: RoleKind, role: &str, task: Option<&str>) -> Start {
    Start {
        role: role.into(),
        kind,
        task: task.map(String::from),
        terminal: format!("bl-shop-{role}"),
        actor: actor(),
        repo: "/home/user/acme-shop".into(),
        resume: None,
    }
}

/// A studio at requirement v2 with one task whose acceptance list
/// lives in the ledger copy.
fn state_with_task() -> StudioState {
    let mut state = StudioState {
        requirement_version: 2,
        ..StudioState::default()
    };
    state.tasks.insert(
        "t1".into(),
        Task {
            id: "t1".into(),
            title: "front page".into(),
            task_kind: "feature".into(),
            requirement_version: 1,
            acceptance: "plan/9-t1-acceptance.md".into(),
            dependencies: Vec::new(),
            status: TaskStatus::Running,
            paused_from: None,
            rework_rounds: 0,
            priority: Priority::Normal,
            cast: None,
            casts: Vec::new(),
            deliveries: Vec::new(),
            reviews: Vec::new(),
            finding_reports: Vec::new(),
            state_changes: Vec::new(),
            created_seq: 9,
            created_ts_ms: 0,
            last_ts_ms: 0,
            closed_ts_ms: None,
        },
    );
    state
}

/// Render fills every placeholder: no `{name}` survives in any role's
/// output, and the concrete values land where they should.
#[test]
fn render_fills_every_placeholder() {
    let dir = tmp();
    let state = state_with_task();
    for (kind, role, task) in [
        (RoleKind::Producer, "producer", None),
        (RoleKind::Writer, "writer", None),
        (RoleKind::Director, "director", None),
        (RoleKind::Actor, "actor-t1-c1", Some("t1")),
    ] {
        let st = start(kind, role, task);
        let file = prepare(&dir, "acme-shop", &st, &state).unwrap();
        let text = fs::read_to_string(&file).unwrap();
        assert!(!text.contains('{'), "{role} left a placeholder: {text}");
        assert!(text.contains("`acme-shop`"), "{role} misses the studio");
        if !matches!(kind, RoleKind::Producer) {
            let req = dir.join("req/v2.md").to_string_lossy().into_owned();
            assert!(text.contains(&req), "{role} misses the requirement file");
            assert!(text.contains("v2"), "{role} misses the version");
        }
    }
    // The actor's copy carries its task, title, acceptance and branch.
    let st = start(RoleKind::Actor, "actor-t1-c1", Some("t1"));
    let file = prepare(&dir, "acme-shop", &st, &state).unwrap();
    let text = fs::read_to_string(&file).unwrap();
    assert!(text.contains("task `t1`: front page"), "{text}");
    let accept = dir
        .join("plan/9-t1-acceptance.md")
        .to_string_lossy()
        .into_owned();
    assert!(text.contains(&accept), "{text}");
    assert!(text.contains("backlot/acme-shop/t1"), "{text}");
    let work = dir.join("work/t1").to_string_lossy().into_owned();
    assert!(text.contains(&work), "{text}");
    // The director's names the integration branch instead.
    let st = start(RoleKind::Director, "director", None);
    let file = prepare(&dir, "acme-shop", &st, &state).unwrap();
    let text = fs::read_to_string(&file).unwrap();
    assert!(text.contains("backlot/acme-shop/integration"), "{text}");
}

/// Every `backlot ...` span the four prompts name goes through the
/// real argument parser — a renamed flag or command fails here, not
/// in front of an agent.
#[test]
fn every_command_in_the_prompts_parses() {
    for (kind, name) in [
        (RoleKind::Producer, "producer"),
        (RoleKind::Writer, "writer"),
        (RoleKind::Director, "director"),
        (RoleKind::Actor, "actor"),
    ] {
        let text = template(kind);
        let mut found = 0;
        for span in text.split('`').skip(1).step_by(2) {
            let Some(cmd) = span.strip_prefix("backlot ") else {
                continue;
            };
            found += 1;
            let words: Vec<String> = cmd.split(' ').map(String::from).collect();
            assert!(
                args::parse(&words).is_ok(),
                "{name}.md: `backlot {cmd}` does not parse"
            );
        }
        assert!(found > 0, "{name}.md names no backlot command");
    }
}

/// The first prompt is one line pointing at the rendered file; for
/// `devin` the line itself lands in a file the argv names.
#[test]
fn first_prompt_is_one_line() {
    let file = PathBuf::from("/state/studios/acme-shop/prompts/actor-t1-c1.md");
    let line = first_line(&file);
    assert_eq!(
        line,
        "Read /state/studios/acme-shop/prompts/actor-t1-c1.md and follow it."
    );
    let dir = tmp();
    fs::create_dir_all(dir.join("prompts")).unwrap();
    let f = first_file(&dir, "actor-t1-c1", &line).unwrap();
    assert_eq!(f, dir.join("prompts").join("actor-t1-c1.first.txt"));
    assert_eq!(fs::read_to_string(&f).unwrap(), format!("{line}\n"));
}
