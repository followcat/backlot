//! Store tests: torn tails, ordering under the lock, concurrency.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use backlot_model::event::EventKind;
use backlot_model::{EventBy, NewEvent};

use crate::store::*;

static NEXT: AtomicU64 = AtomicU64::new(0);

fn tmp() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "backlot-store-test-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn msg(i: impl std::fmt::Display) -> NewEvent {
    NewEvent::new(
        EventBy::role("writer"),
        EventKind::MessageQueued {
            to: "director".into(),
            file: Some(format!("messages/{i}.md")),
            purpose: None,
            source_seq: None,
        },
    )
}

/// Write a studio header so message events validate.
fn seed_studio(dir: &Path) {
    use backlot_model::event::*;
    let kind = EventKind::StudioCreated {
        title: "acme-shop".into(),
        repo: "/home/user/acme-shop".into(),
        mode: StudioMode::Standard,
        roles: Box::new(RoleConfigs {
            producer: actor(),
            director: actor(),
            writer: actor(),
        }),
        candidates: vec![Candidate {
            name: "actor-a".into(),
            runner: "runner-a".into(),
            runner_version: None,
            model: "model-a".into(),
            effort: "high".into(),
            permission_profile: "standard".into(),
        }],
        rework_cap: 2,
        stall_limit_s: 600,
        nudge_after_s: 180,
        escalate_after_s: 600,
        check_env: Vec::new(),
        checks: Vec::new(),
        task_budget_s: None,
        limits: Default::default(),
    };
    append::<(), _>(dir, |_, _| {
        Ok(Pending {
            event: NewEvent::new(EventBy::role("user"), kind),
            files: Vec::new(),
        })
    })
    .unwrap();
}

fn actor() -> backlot_model::ActorIdentity {
    backlot_model::ActorIdentity {
        runner: "runner".into(),
        runner_version: None,
        model: "model".into(),
        effort: "high".into(),
        permission_profile: "standard".into(),
    }
}

#[test]
fn torn_last_line_is_ignored_and_truncated() {
    let dir = tmp();
    seed_studio(&dir);
    let path = dir.join("events.jsonl");
    // A killed process leaves a partial line with no newline.
    let mut bytes = fs::read(&path).unwrap();
    bytes.extend_from_slice(b"{\"seq\":2,\"ts_ms\":5,\"by\":{\"role\":\"writer\"},\"kind\":\"message.queued\",\"to\":\"dir");
    fs::write(&path, &bytes).unwrap();

    // Read: the tail is ignored.
    let (events, torn) = load_events(&dir).unwrap();
    assert!(torn);
    assert_eq!(events.len(), 1);

    // Append: the tail is truncated first, then the new line goes on.
    let e = append::<(), _>(&dir, |_, seq| {
        assert_eq!(seq, 2);
        Ok(Pending {
            event: msg(2),
            files: Vec::new(),
        })
    })
    .unwrap();
    assert_eq!(e.seq, 2);
    let (events, torn) = load_events(&dir).unwrap();
    assert!(!torn);
    assert_eq!(events.len(), 2);
    assert_eq!(events[1].seq, 2);
}

#[test]
fn torn_first_line_truncates_to_empty() {
    let dir = tmp();
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("events.jsonl"), b"{\"seq\":1,partial").unwrap();
    let (events, torn) = load_events(&dir).unwrap();
    assert!(torn);
    assert!(events.is_empty());
}

#[test]
fn corrupt_middle_line_is_an_error() {
    let dir = tmp();
    seed_studio(&dir);
    let path = dir.join("events.jsonl");
    let mut bytes = fs::read(&path).unwrap();
    // A newline-terminated bad line is real corruption, not a torn write.
    bytes.extend_from_slice(b"not json\n");
    fs::write(&path, &bytes).unwrap();
    match load_events(&dir) {
        Err(StoreError::Corrupt(c)) => assert_eq!(c.line, 2),
        other => panic!("expected corrupt, got {other:?}"),
    }
}

/// Eight threads racing appends through the flock keep unique,
/// increasing seq values and lose nothing.
#[test]
fn concurrent_appends_keep_one_total_order() {
    let dir = tmp();
    seed_studio(&dir);
    let threads: usize = 8;
    let per: usize = 25;
    let mut handles = Vec::new();
    for _ in 0..threads {
        let d = dir.clone();
        handles.push(std::thread::spawn(move || {
            for i in 0..per {
                append::<String, _>(&d, |_, _| {
                    Ok(Pending {
                        event: msg(i),
                        files: Vec::new(),
                    })
                })
                .unwrap();
            }
        }));
    }
    for h in handles {
        h.join().unwrap();
    }
    let (events, torn) = load_events(&dir).unwrap();
    assert!(!torn);
    assert_eq!(events.len(), 1 + threads * per);
    let mut seqs: Vec<u64> = events.iter().map(|e| e.seq).collect();
    seqs.sort_unstable();
    seqs.dedup();
    assert_eq!(seqs, (1..=events.len() as u64).collect::<Vec<_>>());
}

#[test]
fn append_assigns_seq_and_fsyncs() {
    let dir = tmp();
    seed_studio(&dir);
    for want in 2..=5u64 {
        let e = append::<(), _>(&dir, |_, seq| {
            assert_eq!(seq, want);
            Ok(Pending {
                event: msg(seq),
                files: Vec::new(),
            })
        })
        .unwrap();
        assert_eq!(e.seq, want);
        assert!(e.ts_ms > 0);
    }
}

#[test]
fn state_dir_resolution_order() {
    let mut env = std::collections::BTreeMap::new();
    env.insert("HOME".into(), "/home/user".into());
    assert_eq!(
        base_dir(&env, "core-9"),
        PathBuf::from("/home/user/.local/state/backlot/core-9")
    );
    env.insert("XDG_STATE_HOME".into(), "/xdg".into());
    assert_eq!(
        base_dir(&env, "core-9"),
        PathBuf::from("/xdg/backlot/core-9")
    );
    // The explicit override always wins, whatever the core id is.
    env.insert("BACKLOT_STATE_DIR".into(), "/scratch".into());
    assert_eq!(base_dir(&env, "core-9"), PathBuf::from("/scratch"));
    // A core id that is not a safe name component is sanitised.
    env.remove("BACKLOT_STATE_DIR");
    assert_eq!(base_dir(&env, "co/re"), PathBuf::from("/xdg/backlot/co-re"));
}

#[test]
fn sanitize_keeps_a_small_charset() {
    assert_eq!(sanitize("note v2.md"), "note-v2.md");
    assert_eq!(sanitize("../evil"), "x..-evil");
    assert_eq!(sanitize(""), "x");
}
