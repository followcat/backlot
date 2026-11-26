//! Conformance: every runner's caps agree with the code paths that
//! remain. A runner whose caps say no approval key returns `None` from
//! `permit::approval_keys`; one whose caps name an argv resume flag
//! reads back; a prompt block exists exactly where the caps say it
//! does. Run over every runner, never a sampled few.

use backlot_model::caps::{self, Effort, EffortProblem, Resume};
use backlot_model::event::ApprovalDecision;

use super::{post_launch_session, session_in_argv, supported};
use crate::permit;

/// One screen that names each runner's prompt, where the caps say one
/// exists. `None` is a runner whose prompt the stage cannot see.
fn prompt_screen(runner: &str) -> Option<&'static str> {
    Some(match runner {
        "claude" => "────\n Bash command\nls\n╌╌╌╌\nDo you want to proceed?\n❯ 1. Yes",
        "codex" => "Would you like to run it?\n› 1. Yes",
        "devin" => "● cmd\nApproval required",
        "cursor" => "────\n$ echo hi\n→ Run (once) (y)",
        "cmd" => "────\nneeds to run\n❯ 1. Yes",
        // A cline approval only shows when `--yolo` was switched off by
        // hand; the block is still mapped for the record.
        "cline" => "Cline needs permission\nApprove tool call?",
        "fake" => "fake-work: blocked",
        "agy" => {
            "Command\n────\nRequesting permission for:\n  ls\nRun this command?\n> 1. Yes, run command\n  ↑/↓ Navigate"
        }
        "opencode" => return None,
        other => panic!("no prompt screen for {other}"),
    })
}

#[test]
fn the_caps_are_the_supported_runner_list() {
    for caps in caps::ALL {
        assert!(supported(caps.name), "{} is not supported", caps.name);
    }
    assert_eq!(caps::ALL.len(), 9, "every adapter has exactly one cap row");
}

#[test]
fn approval_keys_agree_with_the_caps() {
    for caps in caps::ALL {
        assert_eq!(
            permit::approval_keys(caps.name, ApprovalDecision::Allow),
            caps.approval_allow,
            "{} allow",
            caps.name
        );
        assert_eq!(
            permit::approval_keys(caps.name, ApprovalDecision::Deny),
            caps.approval_deny,
            "{} deny",
            caps.name
        );
        assert_eq!(
            caps.can_answer(),
            caps.approval_allow.is_some() || caps.approval_deny.is_some(),
            "{}",
            caps.name
        );
    }
}

#[test]
fn a_cap_that_cannot_answer_returns_no_key() {
    for caps in caps::ALL.iter().filter(|c| !c.can_answer()) {
        assert!(permit::approval_keys(caps.name, ApprovalDecision::Allow).is_none());
        assert!(permit::approval_keys(caps.name, ApprovalDecision::Deny).is_none());
    }
}

#[test]
fn argv_resume_flags_read_back_and_the_others_do_not() {
    for caps in caps::ALL {
        let argv = match caps.resume {
            Resume::Argv(flags) => {
                vec![
                    caps.name.to_string(),
                    flags[0].to_string(),
                    "id-1".to_string(),
                ]
            }
            Resume::Flag(flag) => vec![caps.name.to_string(), flag.to_string(), "id-1".to_string()],
            Resume::Subcommand => vec![caps.name.to_string(), "resume".into(), "id-1".into()],
            Resume::None => vec![caps.name.to_string()],
        };
        let read = session_in_argv(caps.name, &argv);
        match caps.resume {
            Resume::Argv(_) => assert_eq!(read.as_deref(), Some("id-1"), "{}", caps.name),
            _ => assert_eq!(read, None, "{}", caps.name),
        }
    }
}

#[test]
fn post_launch_session_agrees_with_discovery() {
    for caps in caps::ALL {
        assert_eq!(
            post_launch_session(caps.name),
            caps.post_launch_session(),
            "{}",
            caps.name
        );
    }
}

#[test]
fn a_prompt_block_exists_exactly_where_the_caps_say() {
    for caps in caps::ALL {
        let Some(screen) = prompt_screen(caps.name) else {
            assert!(!caps.prompt_detection.block, "{}", caps.name);
            assert!(
                permit::prompt_block(caps.name, "anything").is_none(),
                "{} maps no block",
                caps.name
            );
            continue;
        };
        if caps.prompt_detection.block {
            assert!(
                permit::prompt_block(caps.name, screen).is_some(),
                "{} should map a block",
                caps.name
            );
        } else {
            assert!(
                permit::prompt_block(caps.name, screen).is_none(),
                "{} should map none",
                caps.name
            );
        }
    }
}

#[test]
fn the_effort_rule_is_the_caps_rule() {
    let required = ["claude", "codex", "cline", "agy", "fake"];
    for name in required {
        let caps = caps::get(name).unwrap();
        assert!(matches!(caps.effort, Effort::Required(_)), "{name}");
        assert_eq!(
            caps::check_effort(name, None),
            Err(EffortProblem::Missing),
            "{name}"
        );
        assert_eq!(
            caps::check_effort(name, Some("high")),
            Ok(()),
            "{name} accepts a level"
        );
    }
    for name in ["cursor", "cmd"] {
        assert!(matches!(caps::get(name).unwrap().effort, Effort::None));
        assert_eq!(
            caps::check_effort(name, Some("high")),
            Err(EffortProblem::NotAllowed),
            "{name}"
        );
        assert_eq!(caps::check_effort(name, None), Ok(()), "{name}");
    }
    for name in ["devin", "opencode"] {
        assert_eq!(
            caps::check_effort(name, Some("high")),
            Ok(()),
            "{name} accepts and ignores a level"
        );
    }
}

#[test]
fn agy_caps_and_doc_say_launch_handles_for_folder_trust() {
    assert_eq!(caps::AGY.folder_trust, caps::FolderTrust::LaunchHandles);
    let table = super::caps_doc::capabilities_table();
    let agy_row = table
        .lines()
        .find(|line| line.starts_with("| agy |"))
        .expect("agy row in generated capabilities table");
    assert!(
        agy_row.contains("| launch handles |"),
        "expected launch handles in agy row: {agy_row}"
    );
}
