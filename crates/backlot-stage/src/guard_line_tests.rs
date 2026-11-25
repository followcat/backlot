use std::path::Path;

use backlot_model::event::PushPolicy;

use crate::cli::Env;
use crate::guard_rules::{GuardVerdict, refused_command_line};
use crate::guard_shell::classify_command_line;

#[test]
fn refuses_git_push_variants_and_global_flags() {
    assert_eq!(
        refused_command_line("git push origin HEAD").as_deref(),
        Some("git push origin HEAD")
    );
    assert_eq!(
        refused_command_line("/usr/bin/git -C /tmp/repo push -u origin main").as_deref(),
        Some("git -C /tmp/repo push -u origin main")
    );
    assert_eq!(
        refused_command_line("GIT_TRACE=1 env -u HOME git --no-pager push").as_deref(),
        Some("git --no-pager push")
    );
    assert_eq!(refused_command_line("git status"), None);
    assert_eq!(refused_command_line("git commit -m 'git push'"), None);
}

#[test]
fn refuses_chained_piped_multiline_and_nested_shells() {
    assert_eq!(
        refused_command_line("cd /tmp/repo && git push origin main").as_deref(),
        Some("git push origin main")
    );
    assert_eq!(
        refused_command_line("echo hi; gh pr create --title x").as_deref(),
        Some("gh pr create --title x")
    );
    assert_eq!(
        refused_command_line("set -e\n# comment git push\ncargo check"),
        None
    );
    assert_eq!(
        refused_command_line("set -e\ncargo publish").as_deref(),
        Some("cargo publish")
    );
    assert_eq!(
        refused_command_line(r#"bash -lc 'echo ok && ssh host-a uptime'"#).as_deref(),
        Some("ssh host-a uptime")
    );
    assert_eq!(refused_command_line(r#"echo "git push origin HEAD""#), None);
    assert_eq!(refused_command_line("command -v gh"), None);

    let env = Env::default();
    let policy = PushPolicy::default();
    assert!(matches!(
        classify_command_line(
            "echo hi; gh pr create --title x",
            &policy,
            Path::new("."),
            &env
        ),
        GuardVerdict::Card { .. }
    ));
    assert!(matches!(
        classify_command_line(
            "git push --force origin feat/x",
            &policy,
            Path::new("."),
            &env
        ),
        GuardVerdict::Refuse { .. }
    ));
    assert!(matches!(
        classify_command_line(
            r#"echo "$(git push --no-verify)""#,
            &policy,
            Path::new("."),
            &env
        ),
        GuardVerdict::Refuse { .. }
    ));
}

/// The command-line layer (agy and Cline hooks): role terminals cannot
/// reach the person's queue, core socket, server or a detached process.
#[test]
fn refuses_hc_control_socket_scripts_detach_tricks_and_server_control() {
    for line in [
        "hc run --bg 'backlot card apply c1'",
        "hc exec t1 -- ls",
        "hc send t1 hello",
        "hc keys t1 ctrl-c",
        "hc connect --session backlot",
        "hc --on t1 send hi",
        "python3 -c 'import socket; socket.socket(1, 1).connect(\"plugin.backlot.req\")'",
        "node -e 'fs.writeFileSync(\"plugin.backlot.req\", \"doc.set\")'",
        "python3 -c 'import os; print(os.environ[\"HC_SOCKET\"])'",
        "socat - UNIX-CONNECT:/run/user/1000/herdr-core/backlot/backlot.sock",
        "nc -U /home/alice/.local/state/herdr-core/backlot/backlot.sock",
        "ncat -U /tmp/hc.sock",
        "systemd-run --user --quiet sh -c 'backlot card apply c1'",
        "tmux new -d sh -c 'backlot card apply c1'",
        "tmux -L s new-session -d -- sh -c 'backlot card apply c1'",
        "backlot server stop",
        "backlot --session backlot server run",
        "backlot server adopt --stop-stages",
    ] {
        assert!(refused_command_line(line).is_some(), "must refuse `{line}`");
    }

    let heredoc = "python3 - <<'EOF'\nimport socket\ns = socket.socket(socket.AF_UNIX)\ns.connect('/run/user/1000/herdr-core/backlot/backlot.sock')\nEOF";
    assert!(
        refused_command_line(heredoc).is_some(),
        "heredoc into an interpreter"
    );
    assert!(
        refused_command_line("bash -c 'python3 -c \"print(1)\"; nc -U /tmp/plugin.backlot.req'")
            .is_some()
    );

    for line in [
        "hc ls",
        "hc get backlot",
        "hc read backlot",
        "hc wait t1",
        "hc events",
        "hc templates",
        "hc schema",
        "hc file read a.txt",
        "tmux ls",
        "screen -ls",
        "screen -dm sleep 30",
        "setsid -f sleep 30",
        "nohup sleep 30 &",
        "git status",
        "cargo test",
    ] {
        assert_eq!(refused_command_line(line), None, "must allow `{line}`");
    }
}
