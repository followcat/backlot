//! Eligibility, probe plans, the layer-2 table and the overlay rule.
use super::*;
use crate::chrome::status::AgentStatus;
use hc_proto::terminal::{Foreground, Status};

fn agent(address: &str, word: &str, app: &str) -> hc_proto::terminal::Terminal {
    let mut term = crate::sidebar::fixture::agent(address, word);
    term.status = Some(Status {
        state: word.into(),
        app: app.into(),
        ..Status::default()
    });
    term.foreground = Some(Foreground {
        pid: 4242,
        name: app.into(),
        argv: vec![app.into()],
        cwd: "/home/user/proj".into(),
    });
    term
}

#[test]
fn the_overlay_replaces_only_a_shown_idle() {
    assert_eq!(overlay(AgentStatus::Idle, true), AgentStatus::Waiting);
    assert_eq!(overlay(AgentStatus::Idle, false), AgentStatus::Idle);
    // An unseen completion keeps `Done` — Waiting never hides it.
    assert_eq!(overlay(AgentStatus::Done, true), AgentStatus::Done);
    // Working, blocked and unknown pass through untouched.
    for status in [
        AgentStatus::Working,
        AgentStatus::Blocked,
        AgentStatus::Unknown,
    ] {
        assert_eq!(overlay(status, true), status);
    }
}

#[test]
fn only_claude_code_has_a_layer2_entry_today() {
    use super::Layer2;
    assert_eq!(
        layer2_for("claude-code", "idle"),
        Some(Layer2::ClaudeSessions)
    );
    assert_eq!(
        layer2_for("claude-code", "done"),
        Some(Layer2::ClaudeSessions)
    );
    // Its file rule is an idle/done fact; a working word probes nothing
    // until a message entry exists for it.
    assert_eq!(layer2_for("claude-code", "working"), None);
    for app in ["devin", "codex", "cursor-agent", "agy", "opencode", ""] {
        assert_eq!(layer2_for(app, "idle"), None, "{app}");
    }
}

#[test]
fn eligibility_needs_a_live_reporter_and_a_foreground_pid() {
    // A plain live agent with a foreground pid is probeable.
    assert!(Fingerprint::of(&agent("t1", "idle", "devin")).is_some());
    // No foreground — nothing to walk descendants of.
    let mut term = agent("t1", "idle", "devin");
    term.foreground = None;
    assert_eq!(Fingerprint::of(&term), None);
    // Finished and lost terminals are never eligible.
    let mut term = agent("t1", "idle", "devin");
    term.state = "finished".into();
    assert_eq!(Fingerprint::of(&term), None);
    // No agent record at all — nothing to overlay.
    let mut term = agent("t1", "idle", "devin");
    term.status = None;
    assert_eq!(Fingerprint::of(&term), None);
    // Blocked, error and unknown words are never probed.
    assert_eq!(Fingerprint::of(&agent("t1", "blocked", "devin")), None);
    assert_eq!(Fingerprint::of(&agent("t1", "error", "devin")), None);
    assert_eq!(Fingerprint::of(&agent("t1", "mystery", "devin")), None);
}

#[test]
fn the_fingerprint_pins_state_pid_incarnation_and_record() {
    let term = agent("t1", "idle", "devin");
    let fingerprint = Fingerprint::of(&term).unwrap();
    assert_eq!(fingerprint.pid, 4242);
    assert_eq!(fingerprint.created_ms, term.created_ms);
    assert_eq!(fingerprint.record, None);
    assert_eq!(fingerprint.word, "idle");

    for mutate in [
        (|t: &mut hc_proto::terminal::Terminal| {
            t.status.as_mut().unwrap().state = "done".into();
        }) as fn(&mut hc_proto::terminal::Terminal),
        |t| t.foreground.as_mut().unwrap().pid = 7,
        |t| t.created_ms += 1,
    ] {
        let mut changed = term.clone();
        mutate(&mut changed);
        assert_ne!(Fingerprint::of(&changed), Some(fingerprint.clone()));
    }
    // A different winning record is a different fingerprint.
    let mut changed = term.clone();
    changed.status = None;
    changed.records.insert(
        "sub".into(),
        Status {
            state: "idle".into(),
            app: "devin".into(),
            ..Status::default()
        },
    );
    let named = Fingerprint::of(&changed).unwrap();
    assert_eq!(named.record.as_deref(), Some("sub"));
    assert_ne!(named, fingerprint);
}

#[test]
fn a_probe_plan_routes_each_layer_to_the_terminals_machine() {
    // Local terminal, generic agent: process tree only.
    let plan = probe_plan(&agent("t7", "idle", "devin")).unwrap();
    assert_eq!(plan.machine, None);
    assert!(plan.process_tree);
    assert_eq!(plan.layer2, None);
    // A remote terminal routes to its machine.
    let plan = probe_plan(&agent("host-b/t7", "idle", "devin")).unwrap();
    assert_eq!(plan.machine.as_deref(), Some("host-b"));
    // Claude on an idle word gets the sessions file too.
    let plan = probe_plan(&agent("t7", "done", "claude-code")).unwrap();
    assert!(plan.process_tree);
    assert_eq!(plan.layer2, Some(Layer2::ClaudeSessions));
    // A working word with no message entry probes nothing.
    assert_eq!(probe_plan(&agent("t7", "working", "claude-code")), None);
    assert_eq!(probe_plan(&agent("t7", "working", "devin")), None);
    // Blocked is never probed.
    assert_eq!(probe_plan(&agent("t7", "blocked", "claude-code")), None);
}
