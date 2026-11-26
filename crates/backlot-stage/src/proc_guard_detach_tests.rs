//! Measured on this machine: a `backlot card apply` started through each
//! detach trick still shows a `BACKLOT_*` role in its own environment or in
//! its ancestors — the process gate's two checks. The tricks that keep the
//! role visible (`setsid -f`, `nohup … &`, a fresh `tmux`/`screen` daemon,
//! `at`/`batch`, whose environment is retained) stay allowed; the ones that
//! drop it (`systemd-run`, `tmux new -d` on a person's already-running
//! server) are refused by `guard_rules`.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::proc_guard::{find_role_ancestor, real_proc_entry};

use crate::test_support::TestDir;

/// The three variables a role terminal exports.
fn role_env(cmd: &mut Command) -> &mut Command {
    cmd.env("BACKLOT_ROLE", "producer")
        .env("BACKLOT_STUDIO", "acme-shop")
        .env("BACKLOT_TERMINAL", "bl-acme-shop-producer")
}

fn have(tool: &str) -> bool {
    Command::new("sh")
        .arg("-c")
        .arg(format!("command -v {tool}"))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// A process that records its own PID and then lives long enough to be
/// inspected, exactly as a `backlot card apply` would.
///
/// The PID file is the readiness signal: it names a `sleep` child that
/// is already running, and it appears only after that child exists. The
/// detach tricks return before their payload has settled (`setsid -f`
/// forks and exits at once), so a reader that took the wrapper's PID and
/// inspected it immediately could catch the payload before it was ready.
/// Writing the sleeping child's PID *after* starting it closes that
/// window: when the file is there, the process it names is alive, with
/// the role's environment inherited, and stays alive for a minute.
fn payload(dir: &TestDir, tag: &str) -> PathBuf {
    let pid_file = dir.join(format!("{tag}.pid"));
    let script = dir.join(format!("{tag}.sh"));
    fs::write(
        &script,
        format!(
            "#!/bin/sh\nsleep 60 &\nprintf %s \"$!\" > '{}'\nwait\n",
            pid_file.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    fs::remove_file(&pid_file).ok();
    script
}

fn pid_of(dir: &TestDir, tag: &str) -> Option<u32> {
    let path = dir.join(format!("{tag}.pid"));
    let deadline = Instant::now() + Duration::from_secs(8);
    while Instant::now() < deadline {
        if let Ok(text) = fs::read_to_string(&path)
            && let Ok(pid) = text.trim().parse::<u32>()
        {
            return Some(pid);
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    None
}

/// Whether the gate still sees a role: its own environment or an ancestor.
fn trips_the_gate(pid: u32) -> bool {
    find_role_ancestor(pid, &real_proc_entry).is_some()
}

fn reap(pid: u32) {
    let _ = Command::new("kill")
        .args(["-9", &pid.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

fn run_role(program: &str, args: &[&str]) {
    let mut cmd = Command::new(program);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    role_env(&mut cmd);
    let _ = cmd.status();
}

#[test]
fn setsid_f_still_trips_the_gate() {
    let dir = TestDir::new("bl-detach-setsid");
    let script = payload(&dir, "setsid");
    run_role("setsid", &["-f", script.to_str().unwrap()]);
    let pid = pid_of(&dir, "setsid").expect("setsid payload started");
    assert!(trips_the_gate(pid), "setsid -f keeps BACKLOT_ROLE visible");
    reap(pid);
}

#[test]
fn nohup_background_still_trips_the_gate() {
    let dir = TestDir::new("bl-detach-nohup");
    let script = payload(&dir, "nohup");
    run_role(
        "sh",
        &[
            "-c",
            &format!("nohup {} >/dev/null 2>&1 &", script.display()),
        ],
    );
    let pid = pid_of(&dir, "nohup").expect("nohup payload started");
    assert!(trips_the_gate(pid), "nohup & keeps BACKLOT_ROLE visible");
    reap(pid);
}

#[test]
fn systemd_run_escapes_the_gate() {
    if !have("systemd-run") {
        eprintln!("skipping: systemd-run is not installed");
        return;
    }
    let dir = TestDir::new("bl-detach-systemd");
    let script = payload(&dir, "systemd");
    let mut cmd = Command::new("systemd-run");
    cmd.args(["--user", "--quiet", script.to_str().unwrap()])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    role_env(&mut cmd);
    if !cmd.status().is_ok_and(|s| s.success()) {
        eprintln!("skipping: no user systemd manager here");
        return;
    }
    let pid = pid_of(&dir, "systemd").expect("systemd-run payload started");
    assert!(
        !trips_the_gate(pid),
        "systemd-run starts a clean service outside the role's tree"
    );
    reap(pid);
}

#[test]
fn a_fresh_tmux_server_still_trips_the_gate() {
    if !have("tmux") {
        eprintln!("skipping: tmux is not installed");
        return;
    }
    let dir = TestDir::new("bl-detach-tmux-fresh");
    let script = payload(&dir, "tmuxfresh");
    let sock = format!("backlot-probe-{}", std::process::id());
    run_role(
        "tmux",
        &["-L", &sock, "new", "-d", script.to_str().unwrap()],
    );
    let pid = pid_of(&dir, "tmuxfresh").expect("tmux payload started");
    assert!(
        trips_the_gate(pid),
        "a tmux server started from the role carries BACKLOT_ROLE"
    );
    reap(pid);
    let _ = Command::new("tmux")
        .args(["-L", &sock, "kill-server"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

#[test]
fn tmux_new_d_on_a_persons_server_escapes_the_gate() {
    if !have("tmux") {
        eprintln!("skipping: tmux is not installed");
        return;
    }
    let dir = TestDir::new("bl-detach-tmux-person");
    let sock = format!("backlot-probe-{}-p", std::process::id());
    // The person's own server: started without any role environment.
    let mut person = Command::new("tmux");
    person
        .args(["-L", &sock, "new", "-d", "sleep 300"])
        .env_remove("BACKLOT_ROLE")
        .env_remove("BACKLOT_STUDIO")
        .env_remove("BACKLOT_TERMINAL")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if !person.status().is_ok_and(|s| s.success()) {
        eprintln!("skipping: cannot start a clean tmux server");
        return;
    }
    let script = payload(&dir, "tmuxperson");
    run_role(
        "tmux",
        &["-L", &sock, "new", "-d", script.to_str().unwrap()],
    );
    let pid = pid_of(&dir, "tmuxperson").expect("tmux payload started");
    assert!(
        !trips_the_gate(pid),
        "a pane on the person's server has neither the role env nor ancestry"
    );
    reap(pid);
    let _ = Command::new("tmux")
        .args(["-L", &sock, "kill-server"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

#[test]
fn a_fresh_screen_daemon_still_trips_the_gate() {
    if !have("screen") {
        eprintln!("skipping: screen is not installed");
        return;
    }
    let dir = TestDir::new("bl-detach-screen");
    let script = payload(&dir, "screen");
    let sock = format!("backlot-probe-{}", std::process::id());
    run_role("screen", &["-dmS", &sock, script.to_str().unwrap()]);
    let Some(pid) = pid_of(&dir, "screen") else {
        eprintln!("skipping: screen did not start the payload");
        return;
    };
    assert!(
        trips_the_gate(pid),
        "screen's daemon keeps BACKLOT_ROLE visible to the window"
    );
    reap(pid);
    let _ = Command::new("screen")
        .args(["-S", &sock, "-X", "quit"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

/// `at`/`batch` are absent on the farm, so they were not measured; GNU at
/// retains the submitting environment, which keeps `BACKLOT_ROLE` and the
/// role's `PATH` shims in the job, so they stay allowed.
#[test]
fn at_and_batch_are_left_to_the_gate() {
    for name in ["at", "batch"] {
        assert_eq!(
            crate::guard_rules::refused_command(name, &["now".to_string()]),
            None,
            "{name} stays allowed"
        );
    }
    if !have("at") {
        eprintln!("at/batch not installed: nothing to measure");
    }
}
