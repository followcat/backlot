//! U2 — a real PTY run of studio-first `backlot` on a scratch core:
//! no studio → the opening page; `n` drives the `--interactive`
//! wizard in its popup, which creates `acme-shop` and starts the
//! stage; the cast and the per-studio space appear; `new_tab` on a
//! focused actor opens a studio shell in its worktree, listed under
//! `shells`; `backlot --plain` on the same core is the plain client.
//! Screens are saved as text under `U2_SCREENS_DIR` (the scratch
//! root's `screens/` otherwise) at widths 200/120/80/40.
//! Ignored by default; run with `HC_TUI_TEST_CORE` set.
//!
//! ```sh
//! HC_TUI_TEST_CORE=/path/to/herdr-core U2_SCREENS_DIR=/path/screens \
//!     cargo test -p hc-tui --test real_u2_studio_first -- --ignored --test-threads=1
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

use hc_vt::Vt;

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Finite bound on any single real-core step.
const DEADLINE: Duration = Duration::from_secs(30);
/// Read pump cadence while waiting on a screen predicate.
const POLL: Duration = Duration::from_millis(40);

/// The environment `backlot` sees: the scratch fixture's own plus the
/// state dir — nothing inherited.
fn tui_env(scratch: &ScratchCore, state: &Path) -> Vec<(String, String)> {
    let mut env = env(scratch, state);
    env.push(("TERM".into(), "xterm-256color".into()));
    env.push(("HERDR_UI_TEST".into(), "1".into()));
    env
}

/// `backlot` hosted in a PTY at `cols`x`rows` with a decoding `Vt`;
/// `log` keeps raw bytes for a stderr line after the host exits.
struct Tui {
    pty: Pty,
    vt: Vt,
    log: Vec<u8>,
}

impl Drop for Tui {
    /// A panicked assert must not leak the client: a surviving child
    /// keeps inherited fds — a shared build lock among them — open.
    fn drop(&mut self) {
        let _ = self.pty.child.kill();
    }
}

fn spawn_tui(
    scratch: &ScratchCore,
    state: &Path,
    cwd: &Path,
    args: &[&str],
    cols: u16,
    rows: u16,
) -> Tui {
    let pty = Pty::spawn(
        Path::new(env!("CARGO_BIN_EXE_backlot")),
        args,
        &tui_env(scratch, state),
        cwd,
        cols,
        rows,
    );
    Tui {
        pty,
        vt: Vt::new(cols, rows, 4096),
        log: Vec::new(),
    }
}

/// Pump until `pred` holds on the decoded screen — bounded, always.
async fn until(tui: &mut Tui, pred: impl Fn(&str) -> bool, what: &str) -> String {
    let deadline = Instant::now() + DEADLINE;
    loop {
        tui.pty.pump_log(&mut tui.vt, &mut tui.log);
        let text = screen(&tui.vt);
        if pred(&text) {
            return text;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {what}; screen was:\n{text}\nlog: {}",
            String::from_utf8_lossy(&tui.log)
        );
        tokio::time::sleep(POLL).await;
    }
}

/// `save` the screen under the run's directory.
fn save(dir: &Path, name: &str, text: &str) {
    fs::write(dir.join(name), format!("{text}\n")).expect("write screen");
}

/// `q` on the opening page detaches; wait the child out — bounded.
async fn detach(tui: &mut Tui) {
    tui.pty.type_keys(b"q");
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        tui.pty.pump(&mut tui.vt);
        if tui.pty.child.try_wait().unwrap().is_some() {
            return;
        }
        assert!(Instant::now() < deadline, "the client did not detach");
        tokio::time::sleep(POLL).await;
    }
}

/// `ctrl+b q` detaches once the page is gone.
async fn detach_plain(tui: &mut Tui) {
    tui.pty.type_keys(&[0x02, b'q']);
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        tui.pty.pump(&mut tui.vt);
        if tui.pty.child.try_wait().unwrap().is_some() {
            return;
        }
        assert!(Instant::now() < deadline, "the client did not detach");
        tokio::time::sleep(POLL).await;
    }
}

/// The production side once the stage runs: requirement, plan,
/// dispatch, task, cast — the same steps `real_two_studios` proves.
async fn run_pipeline(
    scratch: &ScratchCore,
    state: &Path,
    client: &hc_proto::client::Client,
    name: &'static str,
) {
    let req = fixture(scratch, &format!("req-{name}.md"), "# requirement\n");
    let acc = fixture(scratch, &format!("acc-{name}.md"), "# acceptance\n");
    let plan = fixture(scratch, &format!("plan-{name}.md"), "# plan\n");
    let note = fixture(scratch, &format!("note-{name}.md"), "director: go\n");
    bl(
        scratch,
        state,
        name,
        &format!(
            "req revise --file {} --summary v1 --as producer",
            req.display()
        ),
    );
    bl(scratch, state, name, "plan request --as producer");
    wait_terminal(client, &format!("bl-{name}-writer")).await;
    bl(
        scratch,
        state,
        name,
        &format!(
            "plan publish --kind plan --file {} --as writer",
            plan.display()
        ),
    );
    bl(
        scratch,
        state,
        name,
        &format!(
            "plan publish --kind acceptance --file {} --as writer",
            acc.display()
        ),
    );
    bl(
        scratch,
        state,
        name,
        &format!("dispatch --file {} --as producer", note.display()),
    );
    bl(
        scratch,
        state,
        name,
        &format!(
            "task new --title cart --kind feature --acceptance-file {} --as director",
            acc.display()
        ),
    );
    bl(
        scratch,
        state,
        name,
        "cast --task t1 --actor actor-a --reason first --as director",
    );
}

/// Every wizard prompt takes its default on Enter; a dropped key
/// is the only stall the wizard cannot absorb, and a spare Enter
/// just buffers into the next prompt's default answer.
const RETRY: Duration = Duration::from_secs(4);

/// `marker` painted — re-sending Enter every `RETRY` of silence so a
/// lost key cannot park the wizard for the whole deadline.
async fn prompt(tui: &mut Tui, marker: &str, what: &str) -> String {
    let deadline = Instant::now() + DEADLINE;
    let mut next_retry = Instant::now() + RETRY;
    loop {
        tui.pty.pump_log(&mut tui.vt, &mut tui.log);
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
            "timed out waiting for {what}; screen was:\n{text}\nlog: {}",
            String::from_utf8_lossy(&tui.log)
        );
        tokio::time::sleep(POLL).await;
    }
}

/// The wizard's whole scripted run inside the popup: Enter takes
/// every default — the repo (the popup's cwd), the id, the title,
/// the drafted cast (`accept`), then `y` on the stage offer.
async fn answer_wizard(tui: &mut Tui, out: &Path) {
    for step in ["repository", "studio", "title"] {
        prompt(tui, &format!("{step} ["), step).await;
        tui.pty.type_keys(b"\r");
    }
    // The draft is shown before `cast [accept/edit/refuse]` — this
    // is the screenshot that proves the draft step.
    let shown = prompt(tui, "accept/edit/refuse", "the drafted cast").await;
    save(out, "w120-wizard.txt", &shown);
    tui.pty.type_keys(b"\r"); // accept
    prompt(tui, "start the stage now", "the stage offer").await;
    tui.pty.type_keys(b"\r"); // yes
}

#[tokio::test]
#[ignore = "needs HC_TUI_TEST_CORE: a real pinned herdr-core binary"]
async fn real_u2_studio_first_screens() {
    let scratch = ScratchCore::spawn().await;
    let client = connect(&scratch).await;
    let state = scratch.root.join("bl-state");
    // The wizard's repository default is the client's launch cwd —
    // a committed git repo named `acme-shop` defaults the studio id.
    let repo = scratch.work.join("acme-shop");
    git_repo(&repo);

    let out = std::env::var("U2_SCREENS_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| scratch.root.join("screens"));
    fs::create_dir_all(&out).unwrap();

    // 1. With no studio every width opens on the opening page —
    // `q` detaches straight from it.
    for (cols, rows) in [(200u16, 40u16), (120, 34), (80, 30), (40, 30)] {
        let mut tui = spawn_tui(&scratch, &state, &repo, &[], cols, rows);
        let page = until(&mut tui, |s| s.contains("New studio"), "opening page").await;
        save(&out, &format!("w{cols}-opening.txt"), &page);
        detach(&mut tui).await;
    }

    // 2. `n` → the popup wizard: defaults all the way, cast accepted,
    //    stage started — the publish then lifts the page itself.
    let mut tui = spawn_tui(&scratch, &state, &repo, &[], 120, 34);
    until(&mut tui, |s| s.contains("New studio"), "opening page").await;
    tui.pty.type_keys(b"n");
    answer_wizard(&mut tui, &out).await;
    for name in [
        "bl-acme-shop-producer",
        "bl-acme-shop-director",
        "backlot-stage",
    ] {
        wait_terminal(&client, name).await;
    }
    // The document's roles land a pass behind the terminals — wait
    // for a published producer so the cast's people rows are on it.
    wait_studio(&client, "acme-shop", |s| {
        s["roles"].as_array().is_some_and(|rs| {
            rs.iter()
                .any(|r| r["role"] == "producer" && r["terminal"].is_string())
        })
    })
    .await;
    let cast = until(&mut tui, |s| s.contains("cast · acme-shop"), "cast").await;
    save(&out, "w120-cast.txt", &cast);

    // 3. Seat an actor through the ordinary pipeline.
    run_pipeline(&scratch, &state, &client, "acme-shop").await;
    wait_terminal(&client, "bl-acme-shop-t1").await;
    wait_role(&client, "acme-shop", "actor-t1-c1", |r| {
        r["terminal"].is_string()
    })
    .await;

    // 4. Focus the actor (role keys cycle people), then `prefix+c`
    //    opens a studio shell in its worktree — `shells` lists it.
    for _ in 0..6 {
        let text = until(&mut tui, |s| s.contains("cast · acme-shop"), "cast").await;
        // `▌` marks the focused person; the actor's row names it.
        let on_actor = text
            .lines()
            .any(|line| line.trim_start().starts_with('▌') && line.contains("actor"));
        if on_actor {
            break;
        }
        tui.pty.type_keys(b"\x1b[1;3B"); // alt+down
    }
    tui.pty.type_keys(&[0x02, b'c']); // prefix+c = new_tab
    let shells = until(
        &mut tui,
        |s| s.contains("shells") && s.contains("shell 1"),
        "the shells group",
    )
    .await;
    save(&out, "w120-shells.txt", &shells);
    assert!(
        shells.contains('t') && shells.contains("t1"),
        "the shell row shows the actor's worktree tail:\n{shells}"
    );
    detach_plain(&mut tui).await;

    // 5. The studio view at every width — cast where a sidebar fits,
    //    the studio's own space and its role tabs everywhere.
    for (cols, rows) in [(200u16, 40u16), (80, 30), (40, 30)] {
        let mut tui = spawn_tui(&scratch, &state, &repo, &[], cols, rows);
        let want = if cols <= 64 { "acme-" } else { "cast ·" };
        let text = until(&mut tui, |s| s.contains(want), "studio view").await;
        save(&out, &format!("w{cols}-cast.txt"), &text);
        detach_plain(&mut tui).await;
    }

    // 6. `backlot --plain` on the same core: the plain multiplexer —
    //    Herdr's agents list, no cast, no page.
    for (cols, rows) in [(200u16, 40u16), (120, 34), (80, 30), (40, 30)] {
        let mut tui = spawn_tui(&scratch, &state, &repo, &["--plain"], cols, rows);
        let want = if cols <= 64 { "acme-" } else { "agents" };
        let text = until(&mut tui, |s| s.contains(want), "plain view").await;
        assert!(
            !text.contains("cast ·"),
            "the plain client shows no cast:\n{text}"
        );
        assert!(
            !text.contains("New studio"),
            "the plain client shows no opening page:\n{text}"
        );
        save(&out, &format!("w{cols}-plain.txt"), &text);
        detach_plain(&mut tui).await;
    }

    drop(client);
    eprintln!("screens written to {}", out.display());
}
