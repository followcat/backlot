//! `studio new --ask` / `--ask-file` (W6): the person's first message
//! to the new producer lands as a `message.queued` — the same event
//! `tell producer --as user` records — right after `studio.created`,
//! and `--dry-run` restates it without recording. An empty ask is
//! refused and nothing is created.

use std::fs;
use std::path::PathBuf;

use crate::cli::{Fail, run_stdin};
use crate::cli_testkit::{CAST_TOML, Scratch, fixture, scratch};
use crate::store;

fn args(s: &str) -> Vec<String> {
    s.split(' ').map(String::from).collect()
}

fn studio_dir(scratch: &Scratch) -> PathBuf {
    store::studio_dir(&scratch.dir, "acme-shop")
}

/// The only file under `messages/`, read.
fn message_body(scratch: &Scratch) -> String {
    let mut files: Vec<_> = fs::read_dir(studio_dir(scratch).join("messages"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    files.sort();
    fs::read_to_string(files.pop().expect("a queued message file")).unwrap()
}

#[test]
fn ask_queues_the_first_tell_after_the_create() {
    let (scratch, env) = scratch();
    let cast = fixture(&scratch, "cast.toml", CAST_TOML);
    let out = run_stdin(
        &args(&format!(
            "studio new --title Shop --repo /home/user/acme-shop --cast {cast} --ask build-the-shop"
        )),
        &env,
        None,
    )
    .unwrap();
    assert!(out.contains("studio.created"), "{out}");
    assert!(out.contains("message.queued"), "{out}");

    let (events, _) = store::load_events(&studio_dir(&scratch)).unwrap();
    assert_eq!(events.len(), 2, "the create and the first tell");
    assert!(matches!(
        events[0].kind,
        backlot_model::EventKind::StudioCreated { .. }
    ));
    match &events[1].kind {
        backlot_model::EventKind::MessageQueued { to, purpose, .. } => {
            assert_eq!(to, "producer");
            assert_eq!(*purpose, None, "a plain tell carries no purpose");
        }
        other => panic!("expected the queued tell, got {other:?}"),
    }
    // The tell is the person's: the ledger names the user.
    assert_eq!(events[1].by.role, "user");
    assert_eq!(message_body(&scratch), "build-the-shop");
}

#[test]
fn ask_file_queues_the_files_body() {
    let (scratch, env) = scratch();
    let cast = fixture(&scratch, "cast.toml", CAST_TOML);
    let ask = fixture(&scratch, "ask.md", "build the shop\nwith a cart\n");
    let out = run_stdin(
        &args(&format!(
            "studio new --title Shop --repo /home/user/acme-shop --cast {cast} --ask-file {ask}"
        )),
        &env,
        None,
    )
    .unwrap();
    assert!(out.contains("message.queued"), "{out}");
    assert_eq!(message_body(&scratch), "build the shop\nwith a cart\n");
}

#[test]
fn dry_run_prints_the_ask_and_records_nothing() {
    let (scratch, env) = scratch();
    // The dry run checks runners too: `fake` is the only one whose
    // launch file ships with the repo.
    let cast = fixture(&scratch, "cast.toml", super::draft_tests::FAKE_CAST);
    // The dry run asks git itself: a scratch path is not enough.
    let repo = scratch.files.join("repo");
    fs::create_dir_all(&repo).unwrap();
    crate::cli_testkit::git(&repo, &["init", "-b", "main"]);
    let repo = repo.to_string_lossy().into_owned();
    let out = run_stdin(
        &args(&format!(
            "studio new --title Shop --repo {repo} --cast {cast} \
             --ask build-the-shop --dry-run"
        )),
        &env,
        None,
    )
    .unwrap();
    assert!(out.contains("dry run"), "{out}");
    assert!(
        out.contains("first message to the producer: build-the-shop"),
        "{out}"
    );
    // Nothing is recorded: the studio directory was never made.
    assert!(!studio_dir(&scratch).join("events.jsonl").exists());
}

#[test]
fn an_empty_ask_is_refused_and_creates_nothing() {
    let (scratch, env) = scratch();
    let cast = fixture(&scratch, "cast.toml", CAST_TOML);
    // The split-args helper cannot spell an empty value; argv is
    // built directly, as the `tell --text` test does.
    let argv: Vec<String> = [
        "studio",
        "new",
        "--title",
        "Shop",
        "--repo",
        "/home/user/acme-shop",
        "--cast",
        &cast,
        "--ask",
        "",
    ]
    .into_iter()
    .map(String::from)
    .collect();
    let err = run_stdin(&argv, &env, None).unwrap_err();
    assert!(matches!(err, Fail::Usage(_)), "{err:?}");
    assert!(err.message().contains("must not be empty"), "{err:?}");
    assert!(!studio_dir(&scratch).join("events.jsonl").exists());

    // An `--ask-file` holding only whitespace is refused too.
    let empty = fixture(&scratch, "empty.md", "  \n\n");
    let err = run_stdin(
        &args(&format!(
            "studio new --title Shop --repo /home/user/acme-shop --cast {cast} --ask-file {empty}"
        )),
        &env,
        None,
    )
    .unwrap_err();
    assert!(matches!(err, Fail::Refused(_)), "{err:?}");
    assert!(err.message().contains("the ask is empty"), "{err:?}");
    assert!(!studio_dir(&scratch).join("events.jsonl").exists());
}

#[test]
fn ask_and_ask_file_never_combine() {
    let (scratch, env) = scratch();
    let cast = fixture(&scratch, "cast.toml", CAST_TOML);
    let err = run_stdin(
        &args(&format!(
            "studio new --title Shop --repo /home/user/acme-shop --cast {cast} \
             --ask hi --ask-file a.md"
        )),
        &env,
        None,
    )
    .unwrap_err();
    assert!(matches!(err, Fail::Usage(_)), "{err:?}");
    assert!(err.message().contains("pick one of --ask"), "{err:?}");
}
