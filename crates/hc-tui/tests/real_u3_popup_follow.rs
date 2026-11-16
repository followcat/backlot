//! U3 — a real PTY run proving a one-shot popup follows its program's
//! output: at 100x26 the popup is shorter than a 40-line program's
//! output, so the popup must show the tail (the last line and its
//! prompt), never the head. The same size drives the studio wizard to
//! its `cast [accept/edit/refuse]` prompt, which must be visible for
//! the user to answer it.
//!
//! Ignored by default; run with `HC_TUI_TEST_CORE` set.
//!
//! ```sh
//! HC_TUI_TEST_CORE=/path/to/herdr-core \
//!     cargo test -p hc-tui --test real_u3_popup_follow -- --ignored --test-threads=1
//! ```

#![cfg(unix)]

#[path = "common/real_kit.rs"]
mod kit;
#[path = "common/pty.rs"]
mod pty;
#[path = "../../hc-proto/tests/common/scratch.rs"]
mod scratch;

use kit::*;
use pty::{Pty, screen};
use scratch::ScratchCore;

use hc_proto::terminal::{ListParams, Terminal};
use hc_vt::Vt;

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

/// Finite bound on any single real-core step.
const DEADLINE: Duration = Duration::from_secs(30);
/// Read pump cadence while waiting on a screen predicate.
const POLL: Duration = Duration::from_millis(40);
/// The size the bug was observed at: a 74x25 pane surface makes the
/// default popup 37x12 cells — a 34x10 terminal — far shorter than the
/// output these tests print.
const COLS: u16 = 100;
const ROWS: u16 = 26;

/// The environment `backlot` sees: the scratch fixture's own plus the
/// state dir — nothing inherited.
fn tui_env(scratch: &ScratchCore, state: &Path) -> Vec<(String, String)> {
    let mut env = env(scratch, state);
    env.push(("TERM".into(), "xterm-256color".into()));
    env
}

/// `backlot` hosted in a PTY at `COLS`x`ROWS` with a decoding `Vt`.
struct Tui {
    pty: Pty,
    vt: Vt,
}

impl Drop for Tui {
    /// A panicked assert must not leak the client.
    fn drop(&mut self) {
        let _ = self.pty.child.kill();
    }
}

fn spawn_tui(scratch: &ScratchCore, state: &Path, cwd: &Path, args: &[&str]) -> Tui {
    Tui {
        pty: Pty::spawn(
            Path::new(tui_bin()),
            args,
            &tui_env(scratch, state),
            cwd,
            COLS,
            ROWS,
        ),
        vt: Vt::new(COLS, ROWS, 4096),
    }
}

/// Pump until `pred` holds on the decoded screen — bounded, always.
async fn until(tui: &mut Tui, pred: impl Fn(&str) -> bool, what: &str) -> String {
    let deadline = Instant::now() + DEADLINE;
    loop {
        tui.pty.pump(&mut tui.vt);
        let text = screen(&tui.vt);
        if pred(&text) {
            return text;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {what}; screen was:\n{text}"
        );
        tokio::time::sleep(POLL).await;
    }
}

/// Pump for `dur` so the app can finish attaching and repainting.
async fn settle(tui: &mut Tui, dur: Duration) {
    let deadline = Instant::now() + dur;
    while Instant::now() < deadline {
        tui.pty.pump(&mut tui.vt);
        tokio::time::sleep(POLL).await;
    }
}

/// The live terminal record carrying the popup label, if one exists.
async fn popup_record(client: &hc_proto::client::Client) -> Option<Terminal> {
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

/// The `keys.command` config the popup test needs: `alt+p` runs a
/// program that waits for one line of input, then prints forty
/// numbered lines and waits on `read` — longer than any default
/// popup is tall. The early `read` mirrors the wizard's prompts:
/// the user's keystroke lands while the program's whole output
/// still fits the popup, so a scroll reset must not pin the top.
fn write_popup_config(config_home: &Path) {
    let dir = config_home.join("herdr");
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("config.toml"),
        "[[keys.command]]\n\
         key = \"alt+p\"\n\
         type = \"popup\"\n\
         command = 'printf \"first> \"; read a; \
         i=1; while [ $i -le 40 ]; do echo \"line $i\"; i=$((i+1)); done; \
         printf \"mark> \"; read x'\n\
         description = \"forty lines\"\n",
    )
    .unwrap();
}

/// A `type = "popup"` one-shot whose output outgrows the popup shows
/// its tail — the line the program waits on — not its head, even
/// when a key was typed while the output still fit on screen.
#[tokio::test]
#[ignore = "needs HC_TUI_TEST_CORE: a real pinned herdr-core binary"]
async fn popup_shows_the_end_of_its_output() {
    let scratch = ScratchCore::spawn().await;
    let client = connect(&scratch).await;
    let state = scratch.root.join("bl-state");
    write_popup_config(&scratch.root.join("config"));

    let mut tui = spawn_tui(&scratch, &state, &scratch.work, &["--plain"]);
    until(&mut tui, |s| s.contains("agents"), "the client chrome").await;
    tui.pty.type_keys(b"\x1bp");

    let deadline = Instant::now() + DEADLINE;
    let record = loop {
        if let Some(record) = popup_record(&client).await {
            break record;
        }
        assert!(Instant::now() < deadline, "no hc-tui.popup record appeared");
        tokio::time::sleep(POLL).await;
    };

    // Answer the first prompt while the output still fits: the bug
    // pinned the viewport at the buffer top on this keystroke, and
    // the flood that followed left the head frozen on screen.
    until(&mut tui, |s| s.contains("first>"), "the first prompt").await;
    tui.pty.type_keys(b"x\r");

    // `mark>` is what the program last printed — `terminal.read`
    // proves the core side reached the prompt regardless of what
    // the popup draws. No more keys: the tail must show on its own.
    wait_read(&client, &record.address, "mark>").await;
    settle(&mut tui, Duration::from_millis(1500)).await;

    tui.pty.pump(&mut tui.vt);
    let text = screen(&tui.vt);
    assert!(
        text.contains("mark>"),
        "the popup did not show the program's last line:\n{text}"
    );
    assert!(
        text.contains("line 40"),
        "the popup did not show the output's end:\n{text}"
    );
    // `line 9` is a head-only row here (the tail holds lines
    // 32-40) and no other numbered line contains it as a prefix.
    assert!(
        !text.contains("line 9"),
        "the popup showed the output's head, not its tail:\n{text}"
    );
}

/// The studio wizard at the same small size: `n` on the opening page,
/// defaults all the way — the draft plus its `cast` prompt must be
/// visible for the user to answer it. The Enter that takes each
/// default lands while the wizard's whole output still fits the
/// popup, so it is exactly the keystroke that used to pin the head.
#[tokio::test]
#[ignore = "needs HC_TUI_TEST_CORE: a real pinned herdr-core binary"]
async fn wizard_prompt_is_visible_in_a_short_popup() {
    let scratch = ScratchCore::spawn().await;
    let client = connect(&scratch).await;
    let state = scratch.root.join("bl-state");
    let repo = scratch.work.join("acme-shop");
    git_repo(&repo);

    let mut tui = spawn_tui(&scratch, &state, &repo, &[]);
    until(&mut tui, |s| s.contains("New studio"), "opening page").await;
    tui.pty.type_keys(b"n");

    // Every prompt takes its default; a spare Enter just buffers into
    // the next prompt, so retry instead of racing the attach.
    for step in ["repository", "studio", "title"] {
        prompt_retry(&mut tui, &format!("{step} ["), step).await;
        tui.pty.type_keys(b"\r");
    }

    // The draft and its prompt flood in after the last Enter — no
    // more keys: the tail must show on its own. `terminal.read` on
    // the popup record proves the core side reached the prompt
    // regardless of what the popup draws.
    let deadline = Instant::now() + DEADLINE;
    let record = loop {
        if let Some(record) = popup_record(&client).await {
            break record;
        }
        assert!(Instant::now() < deadline, "no hc-tui.popup record appeared");
        tokio::time::sleep(POLL).await;
    };
    wait_read(&client, &record.address, "accept/edit/refuse").await;
    settle(&mut tui, Duration::from_millis(1500)).await;

    tui.pty.pump(&mut tui.vt);
    let shown = screen(&tui.vt);
    assert!(
        shown.contains("cast [accept/edit/refuse]"),
        "the cast prompt was not visible:\n{shown}"
    );
    assert!(
        !shown.contains("repository ["),
        "the popup showed the draft's head, not its tail:\n{shown}"
    );
}

/// `marker` painted — re-sending Enter every `RETRY` of silence so a
/// lost key cannot park the wizard for the whole deadline.
async fn prompt_retry(tui: &mut Tui, marker: &str, what: &str) -> String {
    const RETRY: Duration = Duration::from_secs(4);
    let deadline = Instant::now() + DEADLINE;
    let mut next_retry = Instant::now() + RETRY;
    loop {
        tui.pty.pump(&mut tui.vt);
        let text = screen(&tui.vt);
        if text.contains(marker) {
            return text;
        }
        if Instant::now() >= next_retry {
            tui.pty.type_keys(b"\r");
            next_retry = Instant::now() + RETRY;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {what}; screen was:\n{text}"
        );
        tokio::time::sleep(POLL).await;
    }
}
