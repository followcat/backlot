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
            created_by: "director".into(),
            last_ts_ms: 0,
            closed_ts_ms: None,
            closed_seq: None,
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
        if matches!(kind, RoleKind::Writer | RoleKind::Actor) {
            let req = dir.join("req/v2.md").to_string_lossy().into_owned();
            assert!(text.contains(&req), "{role} misses the requirement file");
            assert!(text.contains("v2"), "{role} misses the version");
        }
        if matches!(kind, RoleKind::Producer | RoleKind::Director) {
            let req = dir.join("req").to_string_lossy().into_owned();
            assert!(text.contains(&req), "{role} misses the req folder");
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

/// Producer and director start before any requirement exists: the
/// rendered prompt names the req folder and shows neither an empty
/// file nor a version zero — writer and actor, who start after a
/// requirement exists, keep the concrete file and version.
#[test]
fn pre_requirement_prompt_names_the_req_folder() {
    let dir = tmp();
    let state = StudioState::default();
    let req_dir = dir.join("req").to_string_lossy().into_owned();
    for kind in [RoleKind::Producer, RoleKind::Director] {
        let st = start(kind, kind.name(), None);
        let file = prepare(&dir, "acme-shop", &st, &state).unwrap();
        let text = fs::read_to_string(&file).unwrap();
        assert!(!text.contains("``"), "{kind:?} has empty backticks: {text}");
        assert!(!text.contains("v0"), "{kind:?} shows v0: {text}");
        assert!(
            text.contains(&req_dir),
            "{kind:?} misses the req folder: {text}"
        );
    }
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

/// Once the plan and the acceptance list are both published, the
/// writer's last step is telling the producer — the dispatch is
/// the producer's to send, not the director's to take.
#[test]
fn writer_done_goes_through_the_producer() {
    let text = template(RoleKind::Writer);
    let tail = text.rsplit("published").next().unwrap();
    assert!(
        tail.contains("backlot tell producer --file <file>"),
        "writer.md does not end by telling the producer: {tail}"
    );
}

/// The restart run (S8r): a director told only "report at the
/// nodes" asked for the node's name on its own screen, where nobody
/// reads it, and a recovered producer reviewed a task the director
/// had already accepted. The director names the integration node and
/// routes questions through `tell`; the producer leaves reviews to
/// the director after a dispatch.
#[test]
fn integration_report_and_review_have_one_owner() {
    let director = template(RoleKind::Director);
    for want in [
        "`backlot report --node integration --file <file>`",
        "Nobody reads your screen",
        "`backlot tell producer --file <file>`",
    ] {
        assert!(director.contains(want), "director.md lacks {want}");
    }
    let producer = template(RoleKind::Producer);
    let after = producer
        .split("backlot dispatch --file <file>")
        .nth(1)
        .unwrap();
    assert!(
        after.contains("do not review the tasks yourself") && after.contains("`integration` node"),
        "producer.md does not leave reviews to the director after a dispatch: {after}"
    );
}

/// All four prompts carry the shared one-command rule verbatim —
/// a chained command falls outside `Bash(backlot *)` and waits on
/// a person — and nothing outside that rule instructs a chained
/// shell, an automatic permission mode or a bypass flag.
#[test]
fn prompts_share_the_one_command_rule() {
    const SHARED_RULE: &str = concat!(
        "- Run `backlot` commands one per shell call, exactly as written:\n",
        "  no `&&`, `;`, pipes, `cd` or shell variables around them.\n",
        "  These run without asking; anything chained stops at a\n",
        "  permission prompt and waits for a person. A file that is only\n",
        "  input to a `backlot` command goes in your session's scratchpad\n",
        "  directory — your system prompt names it — never the repo or\n",
        "  your work area: write it with your file-writing tool, then\n",
        "  pass its absolute path. Read files with your file-reading\n",
        "  tool, not `cat`."
    );
    for (kind, name) in [
        (RoleKind::Producer, "producer"),
        (RoleKind::Writer, "writer"),
        (RoleKind::Director, "director"),
        (RoleKind::Actor, "actor"),
    ] {
        let text = template(kind);
        assert!(
            text.contains(SHARED_RULE),
            "{name}.md misses the shared one-command rule"
        );
        let rest = text.replace(SHARED_RULE, "");
        for bad in ["&&", "auto mode", "skip-permissions"] {
            assert!(
                !rest.contains(bad),
                "{name}.md instructs `{bad}` outside the shared rule"
            );
        }
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
