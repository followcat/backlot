//! `ps -A -o pid=,ppid=,etime=,args=` parsing and the tool-shell rule.
use super::STARTUP_WINDOW_SECS;
use super::ps::*;
use std::collections::HashSet;

fn row(pid: u32, ppid: u32, etime: &str, args: &str) -> ProcessRow {
    ProcessRow {
        pid,
        ppid,
        etime: parse_etime(etime).expect("fixture etime"),
        argv: args.split_whitespace().map(str::to_owned).collect(),
    }
}

#[test]
fn etime_parses_every_portable_form() {
    assert_eq!(parse_etime("00:01"), Some(1));
    assert_eq!(parse_etime("45:07"), Some(45 * 60 + 7));
    assert_eq!(parse_etime("01:02:03"), Some(3723));
    assert_eq!(parse_etime("2-03:04:05"), Some(183_845));
    // Zero fields are real — a process can be seconds old.
    assert_eq!(parse_etime("00:00"), Some(0));
    for bad in ["", "abc", "1:2:3:4", "-", "1-", "12:", "aa:bb", "1-2"] {
        assert_eq!(parse_etime(bad), None, "{bad:?}");
    }
}

#[test]
fn parse_processes_skips_blank_and_short_lines() {
    let list = parse_processes(
        "    1       0    3-01:02:03 init\n\
         \n\
         junk\n\
         441     440       02:11 zsh -c sleep 30\n\
         999\n",
    );
    assert_eq!(list.len(), 2);
    assert_eq!(list[1].pid, 441);
    assert_eq!(list[1].ppid, 440);
    assert_eq!(list[1].argv, ["zsh", "-c", "sleep", "30"]);
}

#[test]
fn a_tool_shell_is_a_shell_with_a_command_flag() {
    for (argv, want) in [
        (vec!["zsh", "-c", "sleep 30"], true),
        (vec!["/bin/bash", "-lc", "cmd"], true),
        (vec!["sh", "-ic", "cmd"], true),
        // A login-shell argv0 still counts once a -c form is there.
        (vec!["-zsh", "-lic", "cmd"], true),
        (vec!["dash", "-xc", "cmd"], true),
        // Long-lived helpers are not shells.
        (vec!["node", "-c", "x"], false),
        (vec!["npm", "exec", "tool"], false),
        (vec!["uvx", "tool"], false),
        // An interactive shell has no -c form.
        (vec!["bash"], false),
        (vec!["-zsh"], false),
        // `-exec`-lookalikes and long options are not command strings.
        (vec!["zsh", "-exec", "x"], false),
        (vec!["sh", "--command", "x"], false),
        // The flag must come from the shell's own argv, not the payload's.
        (vec!["fish", "script.fish"], false),
    ] {
        let argv: Vec<String> = argv.iter().map(|s| s.to_string()).collect();
        assert_eq!(tool_shell(&argv), want, "{argv:?}");
    }
}

#[test]
fn only_shell_descendants_outside_the_start_window_count() {
    let rows = vec![
        row(100, 1, "08:20", "devin acp"),
        // Descendant tool shell, older than the 20s start window.
        row(200, 100, "07:00", "zsh -c sleep 30"),
        // A second tool shell, but inside the window: 15s after the
        // agent start — a startup helper, not waiting evidence.
        row(201, 100, "08:05", "sh -c true"),
        // Interactive helper — no -c form.
        row(202, 100, "07:00", "bash"),
        // Long-lived helper — not a shell.
        row(203, 100, "07:00", "node /usr/lib/server.js"),
        // A tool shell that belongs to another agent entirely.
        row(300, 1, "06:00", "bash -lc sleep 9"),
    ];
    assert!(!tool_shell_descendant_pids(&rows, 100, STARTUP_WINDOW_SECS).is_empty());

    // Without the old-enough descendant there is no evidence.
    let rows = rows
        .into_iter()
        .filter(|r| r.pid != 200)
        .collect::<Vec<_>>();
    assert!(tool_shell_descendant_pids(&rows, 100, STARTUP_WINDOW_SECS).is_empty());
}

#[test]
fn deep_descendants_through_non_shell_helpers_count() {
    let rows = vec![
        row(100, 1, "08:20", "devin"),
        row(200, 100, "08:10", "devin acp"),
        row(300, 200, "07:00", "bash -c sleep 30"),
    ];
    assert!(!tool_shell_descendant_pids(&rows, 100, STARTUP_WINDOW_SECS).is_empty());
}

#[test]
fn a_missing_agent_row_yields_no_evidence() {
    let rows = vec![row(200, 100, "07:00", "zsh -c sleep 30")];
    assert!(tool_shell_descendant_pids(&rows, 100, STARTUP_WINDOW_SECS).is_empty());
}

#[test]
fn the_candidate_set_holds_every_qualifying_shell_pid() {
    let rows = vec![
        row(100, 1, "08:20", "devin"),
        row(200, 100, "07:00", "zsh -c sleep 30"),
        row(201, 100, "06:50", "sh -c sleep 20"),
        // Inside the start window — not a candidate.
        row(202, 100, "08:05", "sh -c true"),
        // Not a shell — not a candidate.
        row(203, 100, "07:00", "node server.js"),
        // Another agent's shell — not this agent's candidate.
        row(300, 1, "06:00", "bash -lc sleep 9"),
    ];
    let pids = tool_shell_descendant_pids(&rows, 100, STARTUP_WINDOW_SECS);
    assert_eq!(pids, HashSet::from([200, 201]));
}
