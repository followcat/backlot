//! Float-e2e fixture helpers: spawn hc-tui through a socket tap, write
//! the documented Alt+F config, run `rm` fake agents as tab bodies, and
//! poll real terminal records for the float labels.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use hc_proto::client::Client;
use hc_proto::endpoint::Endpoint;
use hc_proto::terminal::{ListParams, RunParams, Terminal, TerminalParams};

use crate::matrix::file_gains;
use crate::pty::{self, Tui};
use crate::scratch::ScratchCore;

/// The endpoint's local socket path — the tap's forward target.
pub fn local_socket(scratch: &ScratchCore) -> PathBuf {
    let Endpoint::Local(local) = &scratch.endpoint else {
        panic!("scratch endpoints are always local");
    };
    local.socket.clone()
}

/// `hc-tui` against `scratch`, but connecting through `socket` (the tap)
/// instead of the real one.
pub fn spawn_through(scratch: &ScratchCore, stub: &Path, socket: &Path) -> Tui {
    let mut env = pty::env_base(&scratch.root, &scratch.session, socket);
    env.push(("PATH".into(), format!("{}:/usr/bin:/bin", stub.display())));
    pty::spawn_tui(&env, &[], &scratch.work)
}

/// The documented example, unchanged: a bare interactive shell popup —
/// the persistent-float heuristic classifies it without any opt-in.
/// Alt+arrows opt in to the four navigation chords the focused float
/// intercepts.
pub fn write_float_config(config_home: &Path) {
    let dir = config_home.join("herdr");
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("config.toml"),
        "[keys]\n\
         previous_tab = \"alt+left\"\n\
         next_tab = \"alt+right\"\n\
         previous_agent = \"alt+up\"\n\
         next_agent = \"alt+down\"\n\
         \n\
         [[keys.command]]\n\
         key = \"alt+f\"\n\
         type = \"popup\"\n\
         command = 'exec \"${SHELL:-sh}\"'\n\
         description = \"scratch float\"\n\
         width = \"80%\"\n\
         height = \"80%\"\n",
    )
    .unwrap();
}

/// `terminal.run` a fake agent labelled into space/tab with `rm`, so its
/// death removes the record — closing its tab for the owner-gone proof.
/// Returns the record address.
pub async fn run_rm_fake(
    client: &Client,
    fake: &Path,
    out: &Path,
    space: &str,
    tab: &str,
    tag: &str,
) -> String {
    client
        .terminal_run(RunParams {
            argv: vec![
                "sh".into(),
                fake.to_string_lossy().into_owned(),
                String::new(),
                out.to_string_lossy().into_owned(),
                tag.into(),
            ],
            name: Some(format!("{space}-{tag}")),
            labels: BTreeMap::from([
                ("herdr.space".into(), space.into()),
                ("herdr.tab".into(), tab.into()),
            ]),
            rm: true,
            ..RunParams::default()
        })
        .await
        .expect("terminal.run fake")
        .terminal
        .address
}

/// The live record carrying the float label, if one exists.
async fn float_record(client: &Client) -> Option<Terminal> {
    client
        .terminal_list(ListParams {
            all: false,
            labels: BTreeMap::from([("hc-tui.popup".into(), "1".into())]),
        })
        .await
        .ok()?
        .terminals
        .into_iter()
        .next()
}

/// Waits for the float record to appear and returns it.
pub async fn until_float(client: &Client) -> Terminal {
    let deadline = Instant::now() + pty::DEADLINE;
    loop {
        if let Some(term) = float_record(client).await {
            return term;
        }
        assert!(Instant::now() < deadline, "no hc-tui.popup record appeared");
        tokio::time::sleep(pty::POLL).await;
    }
}

/// `terminal.kill` where "was removed" counts as success: killing an
/// `rm` terminal races its own reap — the core finishes the program,
/// removes the record, then `record()` reports not_found. The TUI's
/// `allow_gone` exists for exactly this race.
pub async fn kill_allow_gone(client: &Client, terminal: &str) {
    match client
        .terminal_kill(TerminalParams {
            terminal: terminal.into(),
            caller: None,
        })
        .await
    {
        Ok(_) => {}
        Err(e) if e.is_not_found() => {}
        Err(e) => panic!("terminal.kill {terminal}: {e}"),
    }
}

/// Waits until no live record carries the float label.
pub async fn until_no_float(client: &Client) {
    let deadline = Instant::now() + pty::DEADLINE;
    loop {
        if float_record(client).await.is_none() {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "the float record never left inventory"
        );
        tokio::time::sleep(pty::POLL).await;
    }
}

/// Types `keys` until `needle` lands in `path`: an attach can lag the
/// record by a beat, so a too-early keystroke is retried instead of
/// asserted once. `keys` and `needle` differ when the program is a
/// shell — the file gains the command's output, not the command.
pub async fn type_until(tui: &mut Tui, path: &Path, keys: String, needle: &str, what: &str) {
    let deadline = Instant::now() + pty::DEADLINE;
    loop {
        tui.type_keys(keys.as_bytes());
        if file_gains(
            tui,
            path,
            needle.as_bytes(),
            None,
            Duration::from_millis(800),
        )
        .await
        {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "{what}; screen was:\n{}",
            tui.screen()
        );
    }
}

/// Asserts the three labels a persistent float must carry. The owner
/// names the tab by its document id — a netstring composite
/// `space:<len>:<space>/tab:<len>:<tab>` — so the check matches that
/// tail rather than the bare label.
pub fn assert_float_labels(term: &Terminal, space: &str, tab: &str) {
    let labels = &term.labels;
    assert_eq!(
        labels.get("hc-tui.popup").map(String::as_str),
        Some("1"),
        "float record missed hc-tui.popup: {labels:?}"
    );
    let owner = labels
        .get("hc-tui.owner-tab")
        .expect("float record missed hc-tui.owner-tab");
    assert!(
        owner.ends_with(&format!("tab:{}:{}", tab.len(), tab))
            && owner.contains(&format!("space:{}:{}", space.len(), space)),
        "owner-tab {owner:?} does not name {space}/{tab}"
    );
    assert_eq!(
        labels.get("hc-tui.command").map(String::as_str),
        Some("alt+f"),
        "float record missed hc-tui.command: {labels:?}"
    );
}

/// The boilerplate dirs, fake agent, and core symlinks both float
/// tests need on the stub PATH (`hc` for terminal runs, `herdr-core`
/// for the machine `connect` the via-command spawns).
pub struct FloatSetup {
    pub stub: PathBuf,
    pub out: PathBuf,
    pub fake: PathBuf,
}

/// Creates the test dirs, writes the fake agent and the float config
/// into `config_home`, and symlinks the pinned core into the stub.
pub fn float_setup(scratch: &ScratchCore, config_home: &Path) -> FloatSetup {
    let stub = scratch.root.join("stub");
    let agents = scratch.root.join("agents");
    let out = scratch.root.join("out");
    for dir in [&stub, &agents, &out] {
        fs::create_dir_all(dir).unwrap();
    }
    let fake = crate::fixture::write_fake_agent(&agents);
    for name in ["hc", "herdr-core"] {
        std::os::unix::fs::symlink(crate::scratch::core_binary(), stub.join(name)).unwrap();
    }
    write_float_config(config_home);
    FloatSetup { stub, out, fake }
}
