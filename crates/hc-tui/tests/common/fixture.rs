//! Fixture builders for the paste-matrix e2e tests: scratch cores, stub
//! PATH tools (ssh, wl-paste, hc), fake agent programs, the herdr config
//! the TUI reads, and small drive helpers. Everything lives under the
//! scratch root so nothing reaches real sessions, hosts or clipboards.

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use hc_proto::client::Client;
use hc_proto::endpoint::Endpoint;
use hc_proto::terminal::RunParams;
use serde_json::json;

use crate::pty::{self, Tui};
use crate::scratch::ScratchCore;

/// The fake programs that stand in for real agent TUIs. Each enables the
/// terminal modes the real program is observed to use, prints a READY
/// marker plus an OSC 7501 `idle` status (so it lists as an agent), then
/// appends every input byte to `$OUT` — the file records the exact wire
/// bytes the program would have received.
///
/// Bases:
/// - `shell`: no modes negotiated; canonical `cat`. A plain shell or cat.
/// - `claude`: bracketed paste + kitty disambiguate (`>1u`), raw stdin —
///   Claude Code's Ink/crossterm stack negotiates both.
/// - `codex`: bracketed paste, raw stdin — Codex's ratatui TUI negotiates
///   bracketed paste only.
/// - `devin`: bracketed paste, raw stdin — the Devin TUI negotiates
///   bracketed paste only (assumed; same surface as codex).
/// - `opencode`: bracketed paste, raw stdin — OpenCode (bubbles) negotiates
///   bracketed paste.
pub const FAKE_SPECS: &[(&str, &str)] = &[
    ("shell", ""),
    ("claude", "2004,kitty1"),
    ("codex", "2004"),
    ("devin", "2004"),
    ("opencode", "2004"),
];

/// The fake-agent script: `fake-agent <spec> <outfile> <tag>`.
pub fn write_fake_agent(dir: &Path) -> PathBuf {
    let path = dir.join("fake-agent");
    pty::write_executable(
        &path,
        "spec=\"$1\"; out=\"$2\"; tag=\"$3\"\n\
         case \",$spec,\" in *,2004,*) printf '\\033[?2004h' ;; esac\n\
         case \",$spec,\" in *,kitty1,*) printf '\\033[>1u' ;; esac\n\
         printf '\\033]7501;state=idle\\033\\\\'\n\
         printf 'FAKE %s\\n' \"$tag\"\n\
         printf 'MARK %s abcdefgh\\n' \"$tag\"\n\
         stty raw -echo\n\
         exec cat >> \"$out\"\n",
    );
    path
}

/// The popup variant: like `fake-agent` but the program exits when a
/// flag file appears — `cat` cannot be ended by `^D` in raw mode, and
/// the popup only closes when its program exits. `flag` is `$4`.
/// The watcher runs in the background: a non-interactive shell gives
/// background jobs `/dev/null` on stdin, so `cat` must stay foreground.
pub fn write_popup_agent(dir: &Path) -> PathBuf {
    let path = dir.join("popup-agent");
    pty::write_executable(
        &path,
        "spec=\"$1\"; out=\"$2\"; tag=\"$3\"; flag=\"$4\"\n\
         case \",$spec,\" in *,2004,*) printf '\\033[?2004h' ;; esac\n\
         case \",$spec,\" in *,kitty1,*) printf '\\033[>1u' ;; esac\n\
         printf '\\033]7501;state=idle\\033\\\\'\n\
         printf 'FAKE %s\\n' \"$tag\"\n\
         printf 'MARK %s abcdefgh\\n' \"$tag\"\n\
         stty raw -echo\n\
         ( while [ ! -f \"$flag\" ]; do sleep 0.05; done; kill \"$$\" ) &\n\
         exec cat >> \"$out\"\n",
    );
    path
}

/// Runs one fake agent terminal on `client`, labelled into space/tab.
pub async fn run_fake(
    client: &Client,
    fake: &Path,
    spec: &str,
    out: &Path,
    space: &str,
    tab: &str,
    tag: &str,
) {
    client
        .terminal_run(RunParams {
            argv: vec![
                "sh".into(),
                fake.to_string_lossy().into_owned(),
                spec.into(),
                out.to_string_lossy().into_owned(),
                tag.into(),
            ],
            name: Some(format!("{space}-{tag}")),
            labels: BTreeMap::from([
                ("herdr.space".into(), space.into()),
                ("herdr.tab".into(), tab.into()),
            ]),
            ..RunParams::default()
        })
        .await
        .expect("terminal.run fake");
}

/// A `wl-paste`/`xclip` stub reading from `clip/mime/<type>`.
pub fn write_clipboard_tools(dir: &Path, clip: &Path) {
    let body = pty::clipboard_stub_body(clip);
    pty::write_executable(&dir.join("wl-paste"), &body);
    pty::write_executable(&dir.join("xclip"), &body);
}

/// The herdr config the TUI reads: alt-arrow switching, an Alt+F popup
/// running a fake agent, and `copy_on_select` (herdr default true — kept
/// explicit so the matrix pins it).
pub fn write_tui_config(
    config_home: &Path,
    fake: &Path,
    popup_spec: &str,
    popup_out: &Path,
    popup_flag: &Path,
) {
    let dir = config_home.join("herdr");
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("config.toml"),
        format!(
            "[keys]\n\
             previous_tab = \"alt+left\"\n\
             next_tab = \"alt+right\"\n\
             previous_agent = \"alt+up\"\n\
             next_agent = \"alt+down\"\n\
             \n\
             [[keys.command]]\n\
             key = \"alt+f\"\n\
             type = \"popup\"\n\
             command = \"exec sh {fake} {spec} {out} popup {flag}\"\n\
             description = \"scratch popup\"\n\
             width = \"80%\"\n\
             height = \"80%\"\n",
            fake = fake.display(),
            spec = popup_spec,
            out = popup_out.display(),
            flag = popup_flag.display()
        ),
    )
    .unwrap();
}

/// An `ssh` stub that routes `hc connect` by the hostname argument:
/// `ssh host-a <line>` gets socket A's env, `ssh host-b` gets B's. The
/// last ssh argument is the remote command line; it runs locally with
/// the destination core's environment.
pub fn write_ssh_stub(dir: &Path, hosts: &[(&str, &ScratchCore, &Path)]) {
    let mut body = String::from(
        "last=\"\"; target=''\n\
         for a in \"$@\"; do last=\"$a\"; case \"$a\" in host-*) target=\"$a\";; esac; done\n\
         case \"$target\" in\n",
    );
    for (name, scratch, stub_dir) in hosts {
        let Endpoint::Local(local) = &scratch.endpoint else {
            panic!("scratch endpoint is always local");
        };
        let env = dest_env(scratch, stub_dir);
        let exports = env
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join(" ");
        body.push_str(&format!(
            "  {name}) exec env -u SSH_AUTH_SOCK HC_SOCKET={} {exports} sh -c \"$last\";;\n",
            local.socket.display()
        ));
    }
    body.push_str("  *) echo \"unknown host $target\" >&2; exit 64;;\nesac\n");
    pty::write_executable(&dir.join("ssh"), &body);
}

/// A `machine.add` via-command: runs the remote line with the second
/// core's environment so `herdr-core connect` reaches core B. The line
/// embeds `HC_SESSION=<session>` itself when `session` was passed to
/// machine.add.
pub fn write_via_stub(dir: &Path, dest_env: &[(String, String)]) -> PathBuf {
    let path = dir.join("via-machine-b");
    let exports = dest_env
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join(" ");
    pty::write_executable(&path, &format!("exec env {exports} sh -c \"$1\"\n"));
    path
}

/// Isolated env for a destination core (B) — the herdr-core `connect`
/// resolves HOME/XDG for its own state and HC_SOCKET for the socket.
/// HC_SOCKET is mandatory: the caller's env leaks through `env` and
/// would point the connect back at the origin core.
pub fn dest_env(scratch: &ScratchCore, stub: &Path) -> Vec<(String, String)> {
    let Endpoint::Local(local) = &scratch.endpoint else {
        panic!("scratch endpoint is always local");
    };
    let root = |dir: &str| scratch.root.join(dir).to_string_lossy().into_owned();
    vec![
        ("PATH".into(), format!("{}:/usr/bin:/bin", stub.display())),
        ("HOME".into(), root("home")),
        ("XDG_STATE_HOME".into(), root("state")),
        ("XDG_CONFIG_HOME".into(), root("config")),
        ("XDG_RUNTIME_DIR".into(), root("run")),
        (
            "HC_SOCKET".into(),
            local.socket.to_string_lossy().into_owned(),
        ),
    ]
}

/// `machine.add` through a via-command, pinned to `session`.
pub async fn machine_add(client: &Client, via: &Path, name: &str, session: &str) {
    client
        .call(
            "machine.add",
            Some(json!({
                "via": via.to_string_lossy(),
                "name": name,
                "session": session,
            })),
        )
        .await
        .unwrap_or_else(|e| panic!("machine.add {name}: {e}"));
}

/// Spawns `hc-tui` against a scratch core; `remote` selects `--remote
/// host-x` through the stub `ssh` (which must route to the core's socket).
pub fn spawn(scratch: &ScratchCore, stub: &Path, remote: Option<&str>) -> Tui {
    spawn_with_session(scratch, stub, remote, None)
}

/// [`spawn`] with a different session name: for `--remote host-b` the
/// remote `hc connect --session <s>` line must name B's session, which is
/// taken from `HC_SESSION` in the TUI's env.
pub fn spawn_with_session(
    scratch: &ScratchCore,
    stub: &Path,
    remote: Option<&str>,
    session: Option<&str>,
) -> Tui {
    let Endpoint::Local(local) = &scratch.endpoint else {
        panic!("scratch endpoint is always local");
    };
    let mut env = pty::env_base(
        &scratch.root,
        session.unwrap_or(&scratch.session),
        &local.socket,
    );
    env.push(("PATH".into(), format!("{}:/usr/bin:/bin", stub.display())));
    let args = remote
        .map(|host| vec!["--remote".to_string(), host.to_string()])
        .unwrap_or_default();
    pty::spawn_tui(&env, &args, &scratch.work)
}

/// Types a keystroke for a tab/agent switch: Alt+arrow as a terminal
/// delivers it (`ESC [ 1 ; 3 <arrow>`).
pub fn alt_arrow(tui: &Tui, arrow: char) {
    tui.type_keys(format!("\x1b[1;3{arrow}").as_bytes());
}

/// The kitty-stream Ctrl+V image-paste gesture: press `CSI 118;5u`,
/// release `CSI 118;5:3u` — what the host emits under pushed flags.
pub fn ctrl_v(tui: &Tui) {
    tui.type_keys(b"\x1b[118;5u\x1b[118;5:3u");
}

/// `Ctrl+B [` enters copy mode on the focused pane.
pub fn prefix(tui: &Tui, key: u8) {
    tui.type_keys(&[0x02, key]);
}
