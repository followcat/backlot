//! G2's `--file`/`--text`/`--stdin` sources for `tell` and
//! `req revise`, and G1's `stage stop` parse. `run_stdin` is the
//! stdin seam; the staged temp file must never survive a run.

use crate::cli::{Env, run_stdin};
use crate::cli_testkit::{CAST_TOML, STAGE_LOCK, fixture, ok, scratch, usage};
use crate::store;

fn args(s: &str) -> Vec<String> {
    s.split(' ').map(String::from).collect()
}

/// `run` with `input` as the stdin a `--stdin` command would read.
fn run_in(env: &Env, s: &str, input: Option<&str>) -> Result<String, crate::cli::Fail> {
    run_stdin(&args(s), env, input)
}

fn studio(env: &Env, scratch: &crate::cli_testkit::Scratch) {
    let cast = fixture(scratch, "cast.toml", CAST_TOML);
    ok(
        env,
        &format!("studio new --title Shop --repo /home/user/acme-shop --cast {cast}"),
    );
}

/// This thread's staged bodies that still exist: nothing a test's own `run` staged may outlive it.
/// Other tests stage `backlot-body-*` files in the same temp dir concurrently, so only this thread's count.
fn staged_left() -> Vec<std::path::PathBuf> {
    crate::cli_run::STAGED_BODIES
        .with(|s| s.borrow().iter().filter(|p| p.exists()).cloned().collect())
}

fn staged_any() -> bool {
    crate::cli_run::STAGED_BODIES.with(|s| !s.borrow().is_empty())
}

/// The only file under `messages/`, read.
fn message_body(scratch: &crate::cli_testkit::Scratch) -> String {
    let sdir = store::studio_dir(&scratch.dir, "acme-shop");
    let mut files: Vec<_> = std::fs::read_dir(sdir.join("messages"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    files.sort();
    std::fs::read_to_string(files.pop().unwrap()).unwrap()
}

#[test]
fn tell_text_queues_the_inline_message() {
    let _g = STAGE_LOCK.lock().unwrap();
    let (scratch, env) = scratch();
    studio(&env, &scratch);
    let out = run_in(&env, "tell actor-a --text hello-there --as user", None).unwrap();
    assert!(out.contains("message.queued"), "{out}");
    assert_eq!(message_body(&scratch), "hello-there");
    assert!(staged_any(), "the inline body is staged on this thread");
    assert!(staged_left().is_empty(), "{:?}", staged_left());
}

#[test]
fn tell_stdin_queues_the_piped_message() {
    let _g = STAGE_LOCK.lock().unwrap();
    let (scratch, env) = scratch();
    studio(&env, &scratch);
    let out = run_in(
        &env,
        "tell actor-a --stdin --as user",
        Some("line one\nline two\n"),
    )
    .unwrap();
    assert!(out.contains("message.queued"), "{out}");
    assert_eq!(message_body(&scratch), "line one\nline two\n");
    assert!(staged_left().is_empty(), "{:?}", staged_left());
}

#[test]
fn tell_file_is_unchanged() {
    let (scratch, env) = scratch();
    studio(&env, &scratch);
    let msg = fixture(&scratch, "note.txt", "from a file\n");
    let out = ok(&env, &format!("tell actor-a --file {msg} --as user"));
    assert!(out.contains("message.queued"), "{out}");
    assert_eq!(message_body(&scratch), "from a file\n");
}

#[test]
fn tell_refuses_two_sources_and_empty_text() {
    let _g = STAGE_LOCK.lock().unwrap();
    let (scratch, env) = scratch();
    studio(&env, &scratch);
    assert!(usage(&env, "tell actor-a --file f --text hi --as user").contains("pick exactly one"));
    assert!(usage(&env, "tell actor-a --text hi --stdin --as user").contains("pick exactly one"));
    // An empty `--text` value — the split-args helper cannot spell
    // it, so argv is built directly.
    let argv: Vec<String> = ["tell", "actor-a", "--text", "", "--as", "user"]
        .into_iter()
        .map(String::from)
        .collect();
    let err = run_stdin(&argv, &env, None).unwrap_err();
    assert!(
        err.message().contains("must not be empty"),
        "{}",
        err.message()
    );
    let err = run_in(&env, "tell actor-a --stdin --as user", Some("   \n")).unwrap_err();
    assert_eq!(err.message(), "the message body is empty");
    assert!(staged_left().is_empty(), "{:?}", staged_left());
}

#[test]
fn tell_hollywood_takes_positional_or_text() {
    let (_scratch, env) = scratch();
    let out = ok(&env, "tell hollywood hello-overseer");
    assert!(out.contains("message.sent"), "{out}");
    let out = ok(&env, "tell hollywood --text again");
    assert!(out.contains("message.sent"), "{out}");
    assert!(usage(&env, "tell hollywood hi --text again").contains("give the text once"));
    assert!(usage(&env, "tell hollywood --file f").contains("do not apply to tell hollywood"));
    assert!(usage(&env, "tell hollywood --stdin").contains("do not apply to tell hollywood"));
}

#[test]
fn req_revise_stdin_writes_the_next_version() {
    let _g = STAGE_LOCK.lock().unwrap();
    let (scratch, env) = scratch();
    studio(&env, &scratch);
    let req = fixture(&scratch, "req.md", "# requirement v1\n");
    ok(
        &env,
        &format!("req revise --file {req} --summary first --as producer"),
    );
    let out = run_in(
        &env,
        "req revise --stdin --summary second --as producer",
        Some("# requirement v2\nnew body\n"),
    )
    .unwrap();
    assert!(out.contains("req.version"), "{out}");
    let sdir = store::studio_dir(&scratch.dir, "acme-shop");
    assert_eq!(
        std::fs::read_to_string(sdir.join("req/v2.md")).unwrap(),
        "# requirement v2\nnew body\n"
    );
    assert!(staged_left().is_empty(), "{:?}", staged_left());
}

#[test]
fn req_revise_refuses_text_two_sources_and_missing_summary() {
    let _g = STAGE_LOCK.lock().unwrap();
    let (scratch, env) = scratch();
    studio(&env, &scratch);
    assert!(usage(&env, "req revise --text body --summary s").contains("--text does not apply"));
    assert!(usage(&env, "req revise --file f --stdin --summary s").contains("pick exactly one"));
    assert!(usage(&env, "req revise --stdin").contains("missing --summary"));
    let err = run_in(
        &env,
        "req revise --stdin --summary s --as producer",
        Some(""),
    )
    .unwrap_err();
    assert_eq!(err.message(), "the message body is empty");
}

#[test]
fn stage_stop_parses_with_the_targeting_flags() {
    for argv in ["stage stop", "stage stop --session s1 --remote host-b"] {
        let (common, cmd) = crate::args::parse(&args(argv)).unwrap();
        assert!(matches!(cmd, crate::args::Cmd::StageStop), "{argv}");
        if argv.contains("--remote") {
            assert_eq!(common.remote.as_deref(), Some("host-b"));
            assert_eq!(common.session.as_deref(), Some("s1"));
        }
    }
}

#[test]
fn a_body_another_test_is_staging_is_not_this_tests_leak() {
    // Another test's `--text` body sits in the shared temp dir while this one checks for leaks.
    let other = std::env::temp_dir().join(format!(
        "backlot-body-{}-other-thread.txt",
        std::process::id()
    ));
    let path = other.clone();
    std::thread::spawn(move || std::fs::write(&path, "in flight").unwrap())
        .join()
        .unwrap();
    let left = staged_left();
    let _ = std::fs::remove_file(&other);
    assert!(left.is_empty(), "{left:?}");
}
