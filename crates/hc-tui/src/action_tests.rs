//! The ActionTracker's state machine: the pending text, dedup,
//! toast titles and the copy line (S3 §3).

use super::*;

fn action(machine: Option<&str>) -> Action {
    Action {
        label: "pausing t1".into(),
        done: "paused task t1".into(),
        name: "pause task t1".into(),
        mark: "task:Local:acme-shop:t1".into(),
        argv: vec![
            "task".into(),
            "pause".into(),
            "--reason".into(),
            "why not".into(),
        ],
        machine: machine.map(str::to_owned),
        stdin: None,
        view: false,
    }
}

#[test]
fn the_pending_text_names_the_machine() {
    let mut t = ActionTracker::default();
    t.start(action(Some("host-b")));
    let mut local = action(None);
    local.mark = "task:Local:acme-shop:t2".into();
    t.start(local);
    let remote = &t.pending[0];
    let local = &t.pending[1];
    assert_eq!(remote.text, "pausing t1 on host-b");
    assert_eq!(local.text, "pausing t1");
}

#[test]
fn a_duplicate_press_on_the_same_mark_is_ignored() {
    let mut t = ActionTracker::default();
    assert!(t.start(action(None)).is_some());
    // Same mark — ignored even with a different label.
    let mut dup = action(None);
    dup.label = "resuming t1".into();
    assert!(t.start(dup).is_none());
    // A different row still starts.
    let mut other = action(None);
    other.mark = "task:Local:acme-shop:t2".into();
    assert!(t.start(other).is_some());
    assert_eq!(t.pending.len(), 2);
}

#[test]
fn finish_removes_and_frees_the_mark() {
    let mut t = ActionTracker::default();
    let id = t.start(action(None)).unwrap();
    assert!(t.is_pending("task:Local:acme-shop:t1"));
    assert_eq!(t.finish(id).unwrap().id, id);
    assert!(!t.is_pending("task:Local:acme-shop:t1"));
    assert!(t.start(action(None)).is_some());
}

#[test]
fn the_toast_titles_match_the_spec() {
    let mut t = ActionTracker::default();
    let local = t.start(action(None)).unwrap();
    let remote = {
        let mut a = action(Some("host-b"));
        a.mark = "task:host-b:acme-shop:t1".into();
        t.start(a).unwrap()
    };
    let p = |id| t.pending.iter().find(|p| p.id == id).unwrap();
    assert_eq!(done_title(p(local)), "paused task t1");
    assert_eq!(done_title(p(remote)), "host-b: paused task t1");
    assert_eq!(fail_title(p(local)), "failed: pause task t1");
    assert_eq!(fail_title(p(remote)), "host-b failed: pause task t1");
}

#[test]
fn the_failure_body_is_the_first_nonempty_stderr_line() {
    assert_eq!(
        fail_body("\n  \nprompt screen changed\nrest"),
        "prompt screen changed"
    );
    assert_eq!(fail_body("only\n"), "only");
    assert_eq!(fail_body(" \n\n"), "command failed");
}

#[test]
fn the_copy_line_carries_the_remote_prefix() {
    let mut t = ActionTracker::default();
    let id = t.start(action(None)).unwrap();
    let p = &t.pending[0];
    assert_eq!(copy_line(p), "backlot task pause --reason 'why not'");
    let _ = id;
    let mut remote = action(Some("host-b"));
    remote.argv = vec!["approve".into(), "--person".into(), "a".into()];
    assert_eq!(
        copy_line(&Pending {
            id: 0,
            action: remote,
            text: String::new(),
        }),
        "ssh host-b -- backlot approve --person a"
    );
}
