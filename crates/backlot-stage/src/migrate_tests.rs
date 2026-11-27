//! `backlot studio migrate`: legacy logs gain their `v` under the
//! lock with a `.bak` left behind and identical replayed state,
//! current logs are a no-op, future-stamped lines refuse with the
//! upgrade message, and a working role can't run it.

use std::fs;

use backlot_model::event::{EventBy, EventKind, NewEvent};
use backlot_model::testkit::actor;

use crate::cli_testkit::{ok, refused, scratch, usage};
use crate::store::{self, Pending};

/// Append one `kind` event to `dir`'s ledger.
fn record(dir: &std::path::Path, kind: EventKind) -> backlot_model::Event {
    store::append(dir, move |_, _| {
        Ok::<_, std::convert::Infallible>(Pending {
            event: NewEvent::new(EventBy::role("stage"), kind),
            files: Vec::new(),
        })
    })
    .unwrap()
}

/// A studio dir with `n` events written at the current version —
/// `studio.created` plus `role.started`s.
fn studio(scratch_dir: &std::path::Path, name: &str, n: u64) -> std::path::PathBuf {
    let dir = store::studio_dir(scratch_dir, name);
    fs::create_dir_all(&dir).unwrap();
    record(&dir, backlot_model::testkit::studio_kind());
    for i in 1..n {
        record(
            &dir,
            EventKind::RoleStarted {
                role: format!("r{i}"),
                terminal: Some(format!("bl-{name}-r{i}")),
                actor: Some(actor("a")),
                session_id: None,
                reason: None,
                prompt_hash: None,
            },
        );
    }
    dir
}

/// Strip the `"v":1,` envelope from every line — the shape a
/// pre-versioning log has.
fn unversion(dir: &std::path::Path) {
    let path = dir.join("events.jsonl");
    let text = fs::read_to_string(&path).unwrap();
    fs::write(&path, text.replace("\"v\":1,", "")).unwrap();
}

/// A legacy log gains `v` on every line: the `.bak` keeps the
/// original bytes, the rewritten log replays to the same state, and
/// the report counts the changed lines.
#[test]
fn a_legacy_log_migrates_with_a_backup_and_equal_state() {
    let (scratch, env) = scratch();
    let dir = studio(&scratch.dir, "acme-shop", 3);
    unversion(&dir);
    let before = store::load_state(&dir).unwrap();
    let original = fs::read(dir.join("events.jsonl")).unwrap();

    let out = ok(&env, "studio migrate acme-shop");
    assert_eq!(out.trim(), "acme-shop: migrated 3/3 events to ledger v1");

    let bak = dir.join("events.jsonl.v0.bak");
    assert_eq!(fs::read(&bak).unwrap(), original);
    let text = fs::read_to_string(dir.join("events.jsonl")).unwrap();
    assert_eq!(text.matches("\"v\":1").count(), 3, "{text}");
    assert_eq!(store::load_state(&dir).unwrap(), before);

    // A second run is a no-op and keeps the first backup.
    let out = ok(&env, "studio migrate acme-shop");
    assert_eq!(out.trim(), "acme-shop: already at ledger v1 (3 events)");
    assert_eq!(fs::read(&bak).unwrap(), original);
}

/// `--dry-run` reports the same count but writes nothing — no
/// backup, no rewrite.
#[test]
fn dry_run_reports_without_writing() {
    let (scratch, env) = scratch();
    let dir = studio(&scratch.dir, "acme-shop", 2);
    unversion(&dir);
    let original = fs::read(dir.join("events.jsonl")).unwrap();

    let out = ok(&env, "studio migrate acme-shop --dry-run");
    assert_eq!(
        out.trim(),
        "acme-shop: would migrate 2/2 events to ledger v1"
    );
    assert_eq!(fs::read(dir.join("events.jsonl")).unwrap(), original);
    assert!(!dir.join("events.jsonl.v0.bak").exists());
}

/// `--all` walks every studio — and Hollywood's own ledger when it
/// has a log.
#[test]
fn all_migrates_every_ledger_including_hollywood() {
    let (scratch, mut env) = scratch();
    env.vars.remove("BACKLOT_STUDIO");
    let a = studio(&scratch.dir, "acme-shop", 1);
    let b = studio(&scratch.dir, "beta-tools", 1);
    let hdir = store::hollywood_dir(&scratch.dir);
    fs::create_dir_all(&hdir).unwrap();
    store::append_hollywood(&hdir, |_, _| {
        Ok::<_, std::convert::Infallible>(store::HPending {
            event: backlot_model::hollywood::NewHollywoodEvent::new(
                EventBy::role("user"),
                backlot_model::hollywood::HollywoodKind::MessageSent {
                    to: "hollywood".into(),
                    studio: None,
                    text: Some("hi".into()),
                    file: None,
                    source_seq: None,
                },
            ),
            files: Vec::new(),
        })
    })
    .unwrap();
    for d in [&a, &b, &hdir] {
        unversion(d);
    }

    let out = ok(&env, "studio migrate --all");
    assert!(out.contains("acme-shop: migrated 1/1"), "{out}");
    assert!(out.contains("beta-tools: migrated 1/1"), "{out}");
    assert!(out.contains("hollywood: migrated 1/1"), "{out}");
}

/// A line stamped by a newer backlot refuses with the upgrade
/// message — on read and on migrate alike.
#[test]
fn a_future_stamped_line_refuses_clearly() {
    let (scratch, env) = scratch();
    let dir = studio(&scratch.dir, "acme-shop", 1);
    let path = dir.join("events.jsonl");
    let text = fs::read_to_string(&path).unwrap();
    fs::write(&path, text.replace("\"v\":1", "\"v\":99")).unwrap();

    let msg = refused(&env, "studio migrate acme-shop");
    assert!(
        msg.contains("version 99 is newer than supported v1; upgrade backlot"),
        "{msg}"
    );
    let msg = refused(&env, "status");
    assert!(msg.contains("newer than supported v1"), "{msg}");
}

/// Actors and writers can't run migrations — it's the user's (or the
/// producer's/director's) job.
#[test]
fn a_working_role_is_refused() {
    let (scratch, mut env) = scratch();
    studio(&scratch.dir, "acme-shop", 1);
    env.vars.insert("BACKLOT_ROLE".into(), "actor-t1-c1".into());
    let msg = refused(&env, "studio migrate acme-shop");
    assert!(msg.contains("cannot do this"), "{msg}");
}

/// The scope must be exactly one of positional studio, `--all`, or a
/// pinned studio.
#[test]
fn the_scope_is_exactly_one() {
    let (_scratch, mut env) = scratch();
    env.vars.remove("BACKLOT_STUDIO");
    assert!(usage(&env, "studio migrate").contains("missing studio"));
    assert!(usage(&env, "studio migrate acme-shop --all").contains("not both"));
    assert!(usage(&env, "studio migrate --all --studio acme-shop").contains("do not mix"));
    // A pinned studio supplies the scope — no positional needed.
    let (s2, env2) = crate::cli_testkit::scratch();
    studio(&s2.dir, "acme-shop", 1);
    let out = ok(&env2, "studio migrate");
    assert!(out.contains("acme-shop:"), "{out}");
}
