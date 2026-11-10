//! The paste matrix, part 2 — a smoke run against the real agent CLIs
//! (Claude Code, Codex, Devin, OpenCode) instead of the fakes. Each
//! agent runs in its own tab on an owned scratch core with the real
//! HOME (real logins) and the company proxy in its environment; the
//! harness never submits anything: pastes land, the screen is dumped,
//! the input is cleared with Ctrl+C, and the agent is quit with
//! Ctrl+C twice, falling back to `terminal.kill` only for the record.
//!
//! Screens dump under `/tmp/hctui-agent-smoke-<pid>/shots/` — the
//! report reads them; the directory and the per-agent project dirs are
//! left in place for review.
//!
//! Ignored by default: needs `HC_TUI_TEST_CORE` and `HC_TUI_REAL_AGENTS=1`.
//!
//! ```sh
//! HC_TUI_TEST_CORE=/path/to/pinned/herdr-core HC_TUI_REAL_AGENTS=1 \
//!     cargo test -p hc-tui --test real_agent_smoke_e2e -- --ignored --test-threads=1 --nocapture
//! ```

#![cfg(unix)]

#[path = "common/fixture.rs"]
mod fixture;
#[path = "common/pty_tui.rs"]
mod pty;
#[path = "../../hc-proto/tests/common/scratch.rs"]
mod scratch;

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::time::Duration;

use hc_proto::client::Client;
use hc_proto::terminal::RunParams;
use scratch::ScratchCore;

use fixture::{ctrl_v, write_clipboard_tools, write_ssh_stub};
use pty::{Tui, clipboard_set, screen};

/// `(tab label, binary)` — one tab per agent under `ws-agents`.
const AGENTS: &[(&str, &str)] = &[
    ("claude", "claude"),
    ("codex", "codex"),
    ("devin", "devin"),
    ("opencode", "opencode"),
];

/// Parses `~/.config/secrets/http-proxy.env` without ever printing a
/// value; asserts the proxy variables are set and URL-shaped (masked
/// check only).
fn proxy_env() -> BTreeMap<String, String> {
    let home = std::env::var("HOME").expect("HOME");
    let path = Path::new(&home).join(".config/secrets/http-proxy.env");
    let text = fs::read_to_string(&path).expect("read proxy env");
    let mut env = BTreeMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line);
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        env.insert(
            key.trim().to_string(),
            value.trim().trim_matches(['"', '\'']).to_string(),
        );
    }
    for key in ["http_proxy", "https_proxy", "HTTP_PROXY", "HTTPS_PROXY"] {
        let value = env
            .get(key)
            .unwrap_or_else(|| panic!("{key} not set in proxy env"));
        assert!(
            value.starts_with("http://") && value.len() > "http://".len(),
            "{key} is not a http proxy URL"
        );
    }
    env
}

/// The agent's environment: real login (HOME, XDG, PATH) plus the
/// company proxy. The stub dir leads PATH so an agent's own clipboard
/// tool lookups hit the owned stub, never the real clipboard.
fn agent_env(stub: &Path) -> BTreeMap<String, String> {
    let home = std::env::var("HOME").expect("HOME");
    let real_path = std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".into());
    let mut env = proxy_env();
    env.insert("HOME".into(), home.clone());
    env.insert("PATH".into(), format!("{}:{real_path}", stub.display()));
    env.insert("XDG_CONFIG_HOME".into(), format!("{home}/.config"));
    env.insert("XDG_STATE_HOME".into(), format!("{home}/.local/state"));
    env.insert("LANG".into(), "C.UTF-8".into());
    env
}

/// Runs a real agent CLI in `space`/`tab-{tag}` with `cwd` and the
/// agent env, named `agent-{tag}`.
async fn run_agent(
    client: &Client,
    tag: &str,
    bin: &str,
    env: BTreeMap<String, String>,
    cwd: &Path,
) {
    client
        .terminal_run(RunParams {
            argv: vec![bin.into()],
            cwd: Some(cwd.to_string_lossy().into_owned()),
            env,
            name: Some(format!("agent-{tag}")),
            labels: BTreeMap::from([
                ("herdr.space".into(), "ws-agents".into()),
                ("herdr.tab".into(), format!("tab-{tag}")),
            ]),
            ..RunParams::default()
        })
        .await
        .unwrap_or_else(|e| panic!("terminal.run {bin}: {e}"));
}

/// Pumps and sleeps, returning the settled screen text.
async fn settle(tui: &mut Tui, ms: u64) -> String {
    tokio::time::sleep(Duration::from_millis(ms)).await;
    tui.pump();
    screen(&tui.vt)
}

/// Dumps the current screen to `shots/<name>.txt`.
fn dump(tui: &mut Tui, shots: &Path, name: &str) -> String {
    tui.pump();
    let text = screen(&tui.vt);
    fs::write(shots.join(format!("{name}.txt")), &text).unwrap();
    text
}

/// Waits for the agent UI to become stable, answering a folder-trust
/// dialog once with Enter (allowed: trust only the temp dir) and once
/// with `y` for prompts that want a letter. Never types Enter into an
/// input box — only while a dialog keyword is on screen.
async fn wait_ready(tui: &mut Tui, shots: &Path, tag: &str) -> String {
    let mut answered = 0;
    let mut last = String::new();
    let mut stable = 0;
    let deadline = std::time::Instant::now() + Duration::from_secs(90);
    loop {
        let text = settle(tui, 900).await;
        let lower = text.to_lowercase();
        if lower.contains("trust") && answered < 2 {
            tui.type_keys(if answered == 0 { b"\r" } else { b"y" });
            answered += 1;
            continue;
        }
        // Codex's "Update available · esc skip" prompt swallows the
        // first paste; Esc declines without changing anything.
        if lower.contains("update available") && answered < 3 {
            tui.type_keys(b"\x1b");
            answered += 1;
            continue;
        }
        if text == last {
            stable += 1;
            if stable >= 3 {
                return dump(tui, shots, &format!("{tag}-ready"));
            }
        } else {
            stable = 0;
            last = text;
        }
        if std::time::Instant::now() >= deadline {
            return dump(tui, shots, &format!("{tag}-ready-timeout"));
        }
    }
}

/// Focuses an agent's tab through the sidebar: click the workspace row
/// to open it if the tab row is hidden, then click the tab row. Real
/// agents print no MARK line, so success is judged by the dumped
/// screen, not a marker.
async fn focus_tab(tui: &mut Tui, tag: &str) -> Result<(), String> {
    for _ in 0..4 {
        if let Some((col, row)) = tui.find_text(&format!("tab-{tag}")) {
            tui.click(col, row);
            tokio::time::sleep(Duration::from_millis(800)).await;
            return Ok(());
        }
        if let Some((col, row)) = tui.find_text("ws-agents") {
            tui.click(col, row);
            tokio::time::sleep(Duration::from_millis(500)).await;
            tui.pump();
            continue;
        }
        return Err("no ws-agents or tab row on screen".to_string());
    }
    Err(format!("tab-{tag} never appeared on screen"))
}

/// One Ctrl+C — clears the input box in the TUIs without submitting.
async fn clear_input(tui: &mut Tui) {
    tui.type_keys(b"\x03");
    tokio::time::sleep(Duration::from_millis(600)).await;
    tui.pump();
}

/// Ctrl+C twice (the agents' own quit), then `terminal.kill` by name if
/// the terminal still reports live — recorded, not silent.
async fn quit_agent(tui: &mut Tui, client: &Client, tag: &str) -> String {
    for _ in 0..2 {
        tui.type_keys(b"\x03");
        tokio::time::sleep(Duration::from_millis(800)).await;
        tui.pump();
    }
    let name = format!("agent-{tag}");
    let Ok(value) = client
        .call(
            "terminal.get",
            Some(serde_json::json!({ "terminal": name })),
        )
        .await
    else {
        return "gone".into();
    };
    let state = value
        .get("state")
        .and_then(|s| s.as_str())
        .unwrap_or("unknown")
        .to_string();
    if state == "live" {
        let _ = client
            .call(
                "terminal.kill",
                Some(serde_json::json!({ "terminal": name })),
            )
            .await;
        return "live-needed-kill".into();
    }
    state
}

/// The text-paste cells shared by the local and remote runs: one-line,
/// multiline and CJK bracketed pastes, each dumped then cleared.
async fn text_cells(tui: &mut Tui, shots: &Path, tag: &str) {
    tui.paste_text("smoke-line-1");
    settle(tui, 1500).await;
    dump(tui, shots, &format!("{tag}-paste-oneline"));
    clear_input(tui).await;
    tui.paste_text("smoke-a\nsmoke-b\nsmoke-c");
    settle(tui, 1500).await;
    dump(tui, shots, &format!("{tag}-paste-multiline"));
    clear_input(tui).await;
    tui.paste_text("粘贴测试·こんにちは·한국어");
    settle(tui, 1500).await;
    dump(tui, shots, &format!("{tag}-paste-cjk"));
    clear_input(tui).await;
    dump(tui, shots, &format!("{tag}-cleared"));
}

/// Ctrl+V image cells: real clipboard image, then a file-manager copy.
/// On a remote endpoint the bridge uploads and pastes the path; the
/// dumped screen shows what the agent did with it.
async fn image_cells(tui: &mut Tui, shots: &Path, tag: &str, clip: &Path, work: &Path) {
    clipboard_set(clip, "image/png", pty::PNG);
    ctrl_v(tui);
    settle(tui, 2500).await;
    dump(tui, shots, &format!("{tag}-ctrlv-png"));
    clear_input(tui).await;

    let copied = work.join("copied.png");
    fs::write(&copied, pty::PNG).unwrap();
    clipboard_set(
        clip,
        "x-special/gnome-copied-files",
        format!("copy\nfile://{}\n", copied.display()).as_bytes(),
    );
    ctrl_v(tui);
    settle(tui, 2500).await;
    dump(tui, shots, &format!("{tag}-ctrlv-file"));
    clear_input(tui).await;
}

#[tokio::test]
#[ignore = "launches the real agent CLIs; needs HC_TUI_TEST_CORE and HC_TUI_REAL_AGENTS=1"]
async fn real_agent_paste_smoke() {
    if std::env::var("HC_TUI_REAL_AGENTS").ok().as_deref() != Some("1") {
        return;
    }
    let scratch = ScratchCore::spawn_identity("scratch-smoke", "host-a").await;
    let client = Client::connect(&scratch.endpoint, None::<&std::ffi::OsStr>)
        .await
        .expect("connect");

    let smoke = std::env::temp_dir().join(format!("hctui-agent-smoke-{}", std::process::id()));
    let shots = smoke.join("shots");
    let stub = scratch.root.join("stub");
    let clip = scratch.root.join("clip");
    for dir in [&smoke, &shots, &stub, &clip] {
        fs::create_dir_all(dir).unwrap();
    }
    std::os::unix::fs::symlink(scratch::core_binary(), stub.join("hc")).unwrap();
    std::os::unix::fs::symlink(scratch::core_binary(), stub.join("herdr-core")).unwrap();
    write_clipboard_tools(&stub, &clip);
    write_ssh_stub(&stub, &[("host-a", &scratch, &stub)]);

    // Each agent gets a fresh temp project dir under the smoke root;
    // the dir stays for the leftovers list.
    for (tag, bin) in AGENTS {
        let proj = smoke.join(format!("proj-{bin}"));
        fs::create_dir_all(&proj).unwrap();
        run_agent(&client, tag, bin, agent_env(&stub), &proj).await;
    }

    let mut notes: Vec<String> = Vec::new();

    // --- run 1: local endpoint -------------------------------------
    let mut tui = fixture::spawn(&scratch, &stub, None);
    assert!(
        pty::until(&mut tui, |v, _| screen(v).contains("ws-agents"), "sidebar").await,
        "sidebar never rendered"
    );
    dump(&mut tui, &shots, "local-sidebar");

    for (tag, bin) in AGENTS {
        let _ = bin;
        if let Err(e) = focus_tab(&mut tui, tag).await {
            notes.push(format!("{tag}: focus failed: {e}"));
            dump(&mut tui, &shots, &format!("{tag}-focus-failed"));
            continue;
        }
        let ready = wait_ready(&mut tui, &shots, tag).await;
        let lower = ready.to_lowercase();
        if lower.contains("sign in") || lower.contains("login") || lower.contains("407") {
            notes.push(format!("{tag}: auth/proxy block on boot screen"));
            continue;
        }
        text_cells(&mut tui, &shots, tag).await;

        // Local endpoint: Ctrl+V must reach the program as a key —
        // record what the agent shows for it.
        clipboard_set(&clip, "image/png", pty::PNG);
        ctrl_v(&tui);
        settle(&mut tui, 1500).await;
        dump(&mut tui, &shots, &format!("{tag}-ctrlv-local"));
        clear_input(&mut tui).await;

        let end = quit_agent(&mut tui, &client, tag).await;
        notes.push(format!("{tag}: quit -> {end}"));
        dump(&mut tui, &shots, &format!("{tag}-after-quit"));
    }
    tui.detach().await;
    drop(tui);

    // --- run 2: hc-tui --remote host-a (image bridge active) -------
    // A fresh claude in its own tab: the first run's pane stays in
    // tab-claude after quitting and would shadow focus.
    let proj = smoke.join("proj-rclaude");
    fs::create_dir_all(&proj).unwrap();
    run_agent(&client, "rclaude", "claude", agent_env(&stub), &proj).await;
    let mut tui = fixture::spawn(&scratch, &stub, Some("host-a"));
    assert!(
        pty::until(
            &mut tui,
            |v, _| screen(v).contains("ws-agents"),
            "remote sidebar"
        )
        .await,
        "remote sidebar never rendered"
    );
    if let Err(e) = focus_tab(&mut tui, "rclaude").await {
        notes.push(format!("remote claude: focus failed: {e}"));
    } else {
        let ready = wait_ready(&mut tui, &shots, "remote-claude").await;
        let lower = ready.to_lowercase();
        if lower.contains("sign in") || lower.contains("login") {
            notes.push("remote claude: auth wall".to_string());
        } else {
            text_cells(&mut tui, &shots, "remote-claude").await;
            image_cells(&mut tui, &shots, "remote-claude", &clip, &scratch.work).await;
            let end = quit_agent(&mut tui, &client, "rclaude").await;
            notes.push(format!("remote claude: quit -> {end}"));
        }
    }
    tui.detach().await;
    drop(tui);

    // What is still running (for the report's leftover list).
    if let Ok(list) = client
        .call("terminal.list", Some(serde_json::json!({})))
        .await
    {
        notes.push(format!(
            "terminal.list at end: {}",
            abbrev(&list.to_string())
        ));
    }
    eprintln!("=== smoke notes ===");
    for line in &notes {
        eprintln!("{line}");
    }
    eprintln!("shots under {}", shots.display());
    scratch.finish().expect("scratch teardown");
}

/// Truncates a JSON blob for the note line.
fn abbrev(text: &str) -> String {
    text.chars().take(600).collect()
}
