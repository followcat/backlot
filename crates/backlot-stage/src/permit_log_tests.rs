//! `permit log` reads only `PermitAutoAllow` rows, newest first, and
//! never lets a role argument escape the roles directory.
use crate::cli::Caller;
use crate::permit_cmd::permit_log_base;
use crate::test_support::TestDir;

fn fixture() -> (TestDir, Caller, std::path::PathBuf) {
    let dir = TestDir::new("backlot-permit-log-test");
    let base = dir.join("state");
    let studio = base.join("studios/acme");
    let signals = studio.join("roles/director/signals.jsonl");
    std::fs::create_dir_all(signals.parent().unwrap()).unwrap();
    (dir, Caller::doc_user("acme"), signals)
}

#[test]
fn log_reads_auto_allows_newest_first_and_caps_at_one_hundred() {
    let (_dir, caller, signals) = fixture();
    let mut text = String::new();
    text.push_str("{\"event\":\"PreToolUse\",\"command\":\"ignored\"}\n");
    for n in 0..105 {
        text.push_str(&format!(
            "{{\"ts_ms\":{n},\"event\":\"PermitAutoAllow\",\"command\":\"cmd{n}\",\"decision\":\"allow\"}}\n"
        ));
    }
    std::fs::write(&signals, text).unwrap();
    let rows = permit_log_base(&_dir.join("state"), &caller, "director").unwrap();
    let rows = rows.as_array().unwrap();
    assert_eq!(rows.len(), 100);
    assert_eq!(rows.first().unwrap()["command"], "cmd104");
    assert_eq!(rows.last().unwrap()["command"], "cmd5");
}

#[test]
fn log_rejects_a_role_path_escape() {
    let (dir, caller, _) = fixture();
    for role in ["../director", "a/b", "", ".", ".."] {
        assert!(
            permit_log_base(&dir.join("state"), &caller, role).is_err(),
            "{role:?}"
        );
    }
}
