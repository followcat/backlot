//! Stage-run check declarations on `studio.created` (S9b part 2):
//! the cast's `[[checks]]` and `check_env` reach the reduced config,
//! and logs written before they existed still load.

use crate::event::*;
use crate::state::replay;
use crate::testkit::*;

/// A `studio.created` written before check declarations existed
/// still loads — both fields deserialize to their empty defaults.
#[test]
fn an_old_studio_created_without_check_fields_loads() {
    let line = r#"{"seq":1,"ts_ms":0,"by":{"role":"user"},"kind":"studio.created","title":"old","repo":"/home/user/x","mode":"standard","roles":{"producer":{"runner":"r","model":"m","effort":"e","permission_profile":"p"},"director":{"runner":"r","model":"m","effort":"e","permission_profile":"p"},"writer":{"runner":"r","model":"m","effort":"e","permission_profile":"p"}},"candidates":[],"rework_cap":2,"stall_limit_s":600}"#;
    let e: Event = serde_json::from_str(line).unwrap();
    let state = replay(&[e]);
    let cfg = state.studio.expect("studio");
    assert!(cfg.check_env.is_empty());
    assert!(cfg.checks.is_empty());
}

/// The declaration lands on the reduced config: names, commands,
/// the shared environment list and per-check timeouts survive
/// `studio.created` → `StudioConfig`.
#[test]
fn the_declared_checks_reach_the_reduced_config() {
    let mut kind = studio_kind();
    let EventKind::StudioCreated {
        check_env, checks, ..
    } = &mut kind
    else {
        unreachable!()
    };
    *check_env = vec!["CARGO_HOME".into()];
    *checks = vec![
        check_decl("unit", "cargo test --lib", 600),
        check_decl("smoke", "sh smoke.sh", 30),
    ];
    let state = replay(&[ev(1, "user", kind)]);
    let cfg = state.studio.expect("studio");
    assert_eq!(cfg.check_env, ["CARGO_HOME".to_string()]);
    assert_eq!(cfg.checks.len(), 2);
    assert_eq!(cfg.checks[0].name, "unit");
    assert_eq!(cfg.checks[0].run, "cargo test --lib");
    assert_eq!(cfg.checks[0].timeout_s, 600);
    assert_eq!(cfg.checks[1].name, "smoke");
    assert_eq!(cfg.checks[1].timeout_s, 30);
}
