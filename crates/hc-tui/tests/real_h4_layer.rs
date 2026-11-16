//! H4 — the floating Hollywood layer on a real PTY: a scratch core,
//! the fake runner and two studios. The collapsed strip shows the
//! mark, the role state, the studio and card counts and the newest
//! headline; `alt+y` expands the layer over the U1 layout with the
//! studio rows, the headline and the open card; `a` on the card runs
//! `backlot card apply` to `card.done`; the input line's text reaches
//! Hollywood as `tell hollywood`; `x` hides it all. Screens are saved
//! as text under `H4_SCREENS_DIR` (the scratch root's `screens/`
//! otherwise) at 200/120/80/40 columns. Ignored by default; run with
//! `HC_TUI_TEST_CORE` set.
//!
//! ```sh
//! HC_TUI_TEST_CORE=/path/to/herdr-core H4_SCREENS_DIR=/path/screens \
//!     cargo test -p hc-tui --test real_h4_layer -- --ignored --test-threads=1
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

use serde_json::Value;

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Finite bound on any single real-core step.
const DEADLINE: Duration = Duration::from_secs(30);
/// Read pump cadence while waiting on a screen predicate.
const POLL: Duration = Duration::from_millis(40);

/// The `[hollywood]` table — the fake runner only, like H3's.
const HOLLYWOOD_CFG: &str =
    "[hollywood]\nrunner = \"fake\"\nmodel = \"model-h\"\neffort = \"high\"\n";

fn write_config(scratch: &ScratchCore, text: &str) {
    let dir = scratch.root.join("config/backlot");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("config.toml"), text).unwrap();
}

/// Hollywood's own ledger lines, parsed — `<state>/hollywood/`.
fn hledger(state: &Path) -> Vec<Value> {
    fs::read_to_string(state.join("hollywood").join("events.jsonl"))
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| serde_json::from_str(l).expect("hollywood ledger line"))
        .collect()
}

/// Hollywood's ledger reaches `n` events of `kind`.
fn wait_hkind(state: &Path, kind: &str, n: usize) -> Vec<Value> {
    let deadline = Instant::now() + DEADLINE;
    loop {
        let found: Vec<Value> = hledger(state)
            .into_iter()
            .filter(|e| e["kind"] == kind)
            .collect();
        if found.len() >= n {
            return found;
        }
        assert!(
            Instant::now() < deadline,
            "wanted {n} {kind} events in the hollywood ledger: {:?}",
            hledger(state)
        );
        std::thread::sleep(POLL);
    }
}

/// The newest `card.proposed`'s seq — the `cN` id the layer shows.
fn last_card(state: &Path) -> u64 {
    hledger(state)
        .iter()
        .filter(|e| e["kind"] == "card.proposed")
        .filter_map(|e| e["seq"].as_u64())
        .max()
        .expect("no card.proposed in the hollywood ledger")
}

/// A minimal studio up to a running `t1` — the same fixture
/// `real_hollywood` drives: ledger writes only, no stage needed.
fn studio(scratch: &ScratchCore, state: &Path, name: &str) {
    let repo = scratch.work.join(format!("repo-{name}"));
    git_repo(&repo);
    let cast = fixture(scratch, &format!("cast-{name}.toml"), CAST_TOML);
    let req = fixture(scratch, &format!("req-{name}.md"), "# requirement\n");
    let acc = fixture(scratch, &format!("acc-{name}.md"), "# acceptance\n");
    bl(
        scratch,
        state,
        name,
        &format!(
            "studio new --title {name} --repo {} --cast {}",
            repo.display(),
            cast.display()
        ),
    );
    bl(
        scratch,
        state,
        name,
        &format!(
            "req revise --file {} --summary v1 --as producer",
            req.display()
        ),
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

/// The environment `backlot` sees: the scratch fixture's own plus the
/// state dir — nothing inherited.
fn tui_env(scratch: &ScratchCore, state: &Path) -> Vec<(String, String)> {
    let mut env = env(scratch, state);
    env.push(("TERM".into(), "xterm-256color".into()));
    env.push(("HERDR_UI_TEST".into(), "1".into()));
    env
}

/// `backlot` hosted in a PTY at `cols`x`rows` with a decoding `Vt`.
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

fn spawn_tui(scratch: &ScratchCore, state: &Path, cols: u16, rows: u16) -> Tui {
    let pty = Pty::spawn(
        Path::new(env!("CARGO_BIN_EXE_backlot")),
        &[],
        &tui_env(scratch, state),
        &scratch.work,
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

/// `alt+y` as a terminal delivers it: `ESC y`.
fn alt_y(tui: &Tui) {
    tui.pty.type_keys(b"\x1by");
}

/// `save` the screen under the run's directory.
fn save(dir: &Path, name: &str, text: &str) {
    fs::write(dir.join(name), format!("{text}\n")).expect("write screen");
}

/// The detach chord — only safe once the layer no longer owns the
/// keys (collapsed or hidden): an expanded panel eats the armed
/// prefix's tail.
async fn detach(tui: &mut Tui) {
    tui.pty.type_keys(&[0x02, b'q']);
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        tui.pty.pump(&mut tui.vt);
        if tui.pty.child.try_wait().unwrap().is_some() {
            break;
        }
        assert!(Instant::now() < deadline, "the client did not detach");
        tokio::time::sleep(POLL).await;
    }
}

#[tokio::test]
#[ignore = "needs HC_TUI_TEST_CORE: a real pinned herdr-core binary"]
async fn real_h4_hollywood_layer() {
    let scratch = ScratchCore::spawn().await;
    let client = connect(&scratch).await;
    let state = scratch.root.join("bl-state");
    write_config(&scratch, HOLLYWOOD_CFG);

    studio(&scratch, &state, "acme-shop");
    studio(&scratch, &state, "night-owl");
    bl_unpinned(&scratch, &state, "stage start");
    wait_terminal(&client, "bl-hollywood").await;
    for s in ["acme-shop", "night-owl"] {
        wait_terminal(&client, &format!("bl-{s}-producer")).await;
    }

    // A headline and an open pause card, proposed as Hollywood — the
    // strip and the expanded rows read both from the document.
    bl_unpinned(
        &scratch,
        &state,
        "hollywood report two-shops-under-watch --as hollywood",
    );
    bl_unpinned(
        &scratch,
        &state,
        "hollywood propose pause --studio acme-shop --task t1 \
         --why hold-the-cart --as hollywood",
    );
    let deadline = Instant::now() + DEADLINE;
    loop {
        let (_, doc) = wait_doc(&client).await;
        let h = &doc["hollywood"];
        if h["state"] == "live"
            && h["headline"] == "two-shops-under-watch"
            && h["cards"].as_array().is_some_and(|c| !c.is_empty())
            && doc["studios"].as_array().is_some_and(|s| s.len() == 2)
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the doc never carried hollywood live with the card: {doc}"
        );
        tokio::time::sleep(POLL).await;
    }

    let card_id = format!("c{}", last_card(&state));

    let out = std::env::var("H4_SCREENS_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| scratch.root.join("screens"));
    fs::create_dir_all(&out).unwrap();

    // One attach per width: the collapsed strip first (the mark,
    // counts and headline cut to a third of the width), then alt+y's
    // expanded panel over the layout — full-screen at 40 columns.
    for (cols, rows) in [(200u16, 40u16), (120, 34), (80, 30), (40, 30)] {
        let mut tui = spawn_tui(&scratch, &state, cols, rows);
        let strip = until(&mut tui, |s| s.contains("✦ hollywood"), "strip").await;
        save(&out, &format!("w{cols}-strip.txt"), &strip);
        alt_y(&tui);
        let expanded = until(
            &mut tui,
            |s| s.contains("✧ two-shops-under-watch"),
            "expanded layer",
        )
        .await;
        save(&out, &format!("w{cols}-expanded.txt"), &expanded);
        assert!(
            expanded.contains("acme-shop") && expanded.contains("night-owl"),
            "the studio rows are missing at {cols}:\n{expanded}"
        );
        assert!(
            expanded.contains(&card_id) && expanded.contains("apply"),
            "the open card is missing at {cols}:\n{expanded}"
        );
        if cols >= 80 {
            assert!(
                expanded.contains("hold-the-cart"),
                "the card's why is missing at {cols}:\n{expanded}"
            );
        }
        // Back to the strip, then detach — the expanded panel owns
        // the armed prefix's tail, so collapse first.
        alt_y(&tui);
        until(
            &mut tui,
            |s| !s.contains("✧ two-shops-under-watch"),
            "collapsed again",
        )
        .await;
        detach(&mut tui).await;
    }

    // A short frame caps the floating panel at half height: at 120x10
    // the title, a scrolled body and the input row are all that fit.
    let mut tui = spawn_tui(&scratch, &state, 120, 10);
    until(&mut tui, |s| s.contains("✦ hollywood"), "strip").await;
    alt_y(&tui);
    let short = until(
        &mut tui,
        |s| s.contains("✦ hollywood live") && s.contains('›'),
        "short expanded",
    )
    .await;
    save(&out, "w120-short.txt", &short);
    alt_y(&tui);
    until(&mut tui, |s| !s.contains('›'), "collapsed").await;
    detach(&mut tui).await;

    // The action run: expand, move to the card, `a` applies it —
    // `card.done` lands in Hollywood's ledger.
    let mut tui = spawn_tui(&scratch, &state, 120, 34);
    until(&mut tui, |s| s.contains("✦ hollywood"), "strip").await;
    alt_y(&tui);
    until(
        &mut tui,
        |s| s.contains("✧ two-shops-under-watch"),
        "expanded",
    )
    .await;
    for _ in 0..4 {
        tui.pty.type_keys(b"j");
    }
    tui.pty.type_keys(b"a");
    wait_hkind(&state, "card.done", 1);

    // The input line: `i`, the text, Enter — `tell hollywood` as the
    // user; delivery to Hollywood's agent is `message.delivered`.
    tui.pty.type_keys(b"i");
    tui.pty.type_keys(b"hold the line");
    tui.pty.type_keys(b"\r");
    let deadline = Instant::now() + DEADLINE;
    loop {
        let ev = hledger(&state);
        let tell = ev
            .iter()
            .find(|e| e["kind"] == "message.sent" && e["text"] == "hold the line");
        let done = tell.is_some_and(|t| {
            ev.iter()
                .any(|e| e["kind"] == "message.delivered" && e["target"] == t["seq"])
        });
        if done {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the input line's tell was never delivered: {ev:?}"
        );
        std::thread::sleep(POLL);
    }
    let acted = until(
        &mut tui,
        |s| !s.contains("hold-the-cart"),
        "the applied card left the panel",
    )
    .await;
    save(&out, "w120-acted.txt", &acted);

    // `x` hides the layer: nothing of it stays on screen.
    tui.pty.type_keys(b"x");
    let hidden = until(&mut tui, |s| !s.contains('✦'), "hidden").await;
    save(&out, "w120-hidden.txt", &hidden);
    detach(&mut tui).await;

    drop(client);
    eprintln!("screens written to {}", out.display());
}
