//! The Claude Code sessions-file rule (docs/sidebar.md has the semantics).
use super::claude::*;

#[test]
fn a_shell_status_for_the_probed_pid_is_waiting() {
    let file = r#"{"pid":4242,"status":"shell","cwd":"/home/user/proj","socket":"/private/sock"}"#;
    assert!(session_is_shell(file, 4242));
}

#[test]
fn anything_but_the_exact_pair_is_not_waiting() {
    // A pid that is not the probed process is another agent's file.
    assert!(!session_is_shell(r#"{"pid":1,"status":"shell"}"#, 4242));
    // Every other status word is a quiet turn — never Waiting.
    for status in ["busy", "idle", "waiting", "gone"] {
        let file = format!(r#"{{"pid":4242,"status":"{status}"}}"#);
        assert!(!session_is_shell(&file, 4242), "{status}");
    }
    // Missing fields, wrong shapes and unreadable content.
    assert!(!session_is_shell(r#"{"status":"shell"}"#, 4242));
    assert!(!session_is_shell(r#"{"pid":4242}"#, 4242));
    assert!(!session_is_shell("not json", 4242));
    assert!(!session_is_shell(
        r#"[{"pid":4242,"status":"shell"}]"#,
        4242
    ));
}

#[test]
fn a_quoted_pid_still_matches() {
    // Tolerate a numeric string; the file is the program's own format.
    assert!(session_is_shell(r#"{"pid":"4242","status":"shell"}"#, 4242));
}
