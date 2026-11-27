//! Hollywood's CLI reference: the checked-in copy matches the usage
//! strings it is generated from, the marker in its prompt is always
//! replaced, and the reference lists exactly what its launch
//! pre-allows. `BLESS=1 cargo test` refreshes the checked-in copy — it
//! is never hand-edited.

use std::fs;
use std::path::PathBuf;

use crate::hwcli::{MARKER, agents_md, generate, picks};

fn golden_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("prompts/hollywood-cli.md")
}

/// Drift: the reference is regenerated from `args_usage` /
/// `args_hwood` and compared with the checked-in copy. A usage string
/// that gained or lost a flag fails here until the copy is refreshed.
#[test]
fn the_reference_matches_its_usage_strings() {
    let generated = generate();
    if std::env::var_os("BLESS").is_some() {
        fs::write(golden_path(), &generated).unwrap();
        return;
    }
    let golden = fs::read_to_string(golden_path()).unwrap_or_default();
    assert_eq!(
        golden, generated,
        "prompts/hollywood-cli.md is out of date — run `BLESS=1 cargo test -p backlot-stage hwcli`"
    );
}

/// The prompt's marker is always replaced: the file Hollywood is handed
/// carries the reference, never a literal `{{CLI_REFERENCE}}`.
#[test]
fn the_agents_file_carries_the_reference() {
    let body = agents_md();
    assert!(!body.contains(MARKER), "the marker survived:\n{body}");
    assert!(body.contains("### Command reference"), "{body}");
    // Every command's line is present.
    for pick in picks() {
        assert!(
            body.contains(pick.trim_end()),
            "{pick} is missing from the agents file:\n{body}"
        );
    }
}

/// Every command the reference lists is one the overseer's launch
/// pre-allows — the reference never names a command that would stop at
/// a prompt.
#[test]
fn every_listed_command_is_pre_allowed() {
    let allow = crate::runners::claude::hollywood_commands();
    for pick in picks() {
        let prefix = pick.trim_end();
        let allowed = allow.iter().any(|a| {
            prefix.starts_with(a)
                || (prefix == "backlot studio new" && a.starts_with("backlot studio new"))
                || (prefix == "backlot studio cast set" && a.starts_with("backlot studio cast set"))
                || (prefix.starts_with("backlot hollywood ") && a.starts_with("backlot hollywood"))
                || (prefix == "backlot tell" && a.starts_with("backlot tell"))
        });
        assert!(allowed, "`{prefix}` is not pre-allowed: {allow:?}");
    }
}
