//! U1 — a real PTY run of the studio layout on a scratch core: two
//! studios on the fake runner, one blocked actor, and the `backlot`
//! TUI captured at widths 200/120/80/40. Saves every screen as text
//! under `U1_SCREENS_DIR` (the scratch root's `screens/` otherwise).
//! Ignored by default; run with `HC_TUI_TEST_CORE` set.
//!
//! ```sh
//! HC_TUI_TEST_CORE=/path/to/herdr-core U1_SCREENS_DIR=/path/screens \
//!     cargo test -p hc-tui --test real_u1_layout -- --ignored --test-threads=1
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

/// `[limits] fake = 5` leaves room for producer+director+writer+actor.
const LIMITS: &str = "[limits]\nfake = 5\n";

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

/// Alt+arrow as a terminal delivers it (`ESC [ 1 ; 3 <arrow>`).
fn alt_arrow(tui: &Tui, arrow: char) {
    tui.pty.type_keys(format!("\x1b[1;3{arrow}").as_bytes());
}

/// Alt+PageUp/PageDown (`ESC [ 5 ; 3 ~` / `ESC [ 6 ; 3 ~`) — the
/// default studio step.
fn alt_page(tui: &Tui, down: bool) {
    let code = if down { '6' } else { '5' };
    tui.pty.type_keys(format!("\x1b[{code};3~").as_bytes());
}

/// The row `▌` (the focused cast entry) sits on, if any.
fn focus_bar_row(text: &str) -> Option<usize> {
    text.lines()
        .position(|line| line.trim_start().starts_with('▌'))
}

/// `save` the screen under the run's directory.
fn save(dir: &Path, name: &str, text: &str) {
    fs::write(dir.join(name), format!("{text}\n")).expect("write screen");
}

/// The `studio new` side: repository, cast file, the studio itself.
fn make_studio(scratch: &ScratchCore, state: &Path, name: &'static str, repo: &Path) {
    git_repo(repo);
    let cast = fixture(
        scratch,
        &format!("cast-{name}.toml"),
        &format!("{CAST_TOML}{LIMITS}"),
    );
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

#[tokio::test]
#[ignore = "needs HC_TUI_TEST_CORE: a real pinned herdr-core binary"]
async fn real_u1_studio_layout_screens() {
    let scratch = ScratchCore::spawn().await;
    let client = connect(&scratch).await;
    let state = scratch.root.join("bl-state");

    // Two studios with a seated actor each; acme-shop's actor then
    // raises a permission prompt — the `!` shows on both surfaces.
    make_studio(
        &scratch,
        &state,
        "acme-shop",
        &scratch.work.join("repo-shop"),
    );
    make_studio(
        &scratch,
        &state,
        "night-owl",
        &scratch.work.join("repo-owl"),
    );
    bl_unpinned(&scratch, &state, "stage start");
    for studio in ["acme-shop", "night-owl"] {
        wait_terminal(&client, &format!("bl-{studio}-producer")).await;
    }
    run_pipeline(&scratch, &state, &client, "acme-shop").await;
    run_pipeline(&scratch, &state, &client, "night-owl").await;
    for studio in ["acme-shop", "night-owl"] {
        for role in ["director", "t1"] {
            wait_terminal(&client, &format!("bl-{studio}-{role}")).await;
        }
    }
    let act_a = wait_terminal(&client, "bl-acme-shop-t1").await;
    send_text(&client, &act_a.address, "work:blocked").await;
    wait_event(&state, "acme-shop", "attention.raised", "actor-t1-c1");
    // The ledger lands before the summary republishes: wait for the
    // document itself to carry the actor's `attention` so the `!`
    // is what the captures show.
    let studio = wait_studio(&client, "acme-shop", |s| {
        s["roles"].as_array().is_some_and(|rs| {
            rs.iter()
                .any(|r| r["role"] == "actor-t1-c1" && r["attention"].is_string())
        })
    })
    .await;

    let out = std::env::var("U1_SCREENS_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| scratch.root.join("screens"));
    fs::create_dir_all(&out).unwrap();
    fs::write(
        out.join("doc-acme-shop.json"),
        serde_json::to_string_pretty(&studio).unwrap(),
    )
    .unwrap();

    /// One attach cycle: capture `first`, alt+PgDn to the other
    /// studio (`moved_pred`), alt+PgUp back, alt+down one role, the
    /// panel, detach. `ready_pred` differs because the mobile chrome
    /// (cols ≤ 64) has no sidebar for `cast ·` to land in.
    async fn attach_cycle(
        scratch: &ScratchCore,
        state: &Path,
        out: &Path,
        cols: u16,
        rows: u16,
        mobile: bool,
    ) {
        let mut tui = spawn_tui(scratch, state, cols, rows);
        let first = until(
            &mut tui,
            |s| {
                if mobile {
                    s.contains("switch")
                } else {
                    s.contains("cast ·")
                }
            },
            "attach",
        )
        .await;
        save(out, &format!("w{cols}-cast.txt"), &first);
        // alt+PgDn: next studio. Desktop shows the cast header;
        // mobile shows the focused terminal's `bl-<studio>-*` title.
        let want: Box<dyn Fn(&str) -> bool> = if mobile {
            let acme = first.contains("bl-acme-shop");
            Box::new(move |s: &str| s.contains(if acme { "bl-night-owl" } else { "bl-acme-shop" }))
        } else {
            let other = if first.contains("cast · acme-shop") {
                "night-owl"
            } else {
                "acme-shop"
            };
            let header = format!("cast · {other}");
            Box::new(move |s: &str| s.contains(&header))
        };
        alt_page(&tui, true);
        let moved = until(&mut tui, |s| want(s), "next studio").await;
        save(out, &format!("w{cols}-next-studio.txt"), &moved);
        if !mobile {
            let other = if first.contains("cast · acme-shop") {
                "night-owl"
            } else {
                "acme-shop"
            };
            let current = if other == "acme-shop" {
                "night-owl"
            } else {
                "acme-shop"
            };
            // alt+PgUp: back — then a role nav moves the ▌ marker.
            alt_page(&tui, false);
            let back = until(
                &mut tui,
                |s| s.contains(&format!("cast · {current}")),
                "prev studio",
            )
            .await;
            let mark = focus_bar_row(&back);
            alt_arrow(&tui, 'B');
            let after = until(&mut tui, |s| focus_bar_row(s) != mark, "next role").await;
            save(out, &format!("w{cols}-next-role.txt"), &after);
        }
        // The panel: bordered blocks; acme-shop's `!` shows. The
        // title `studios 2` can't collide with a message path.
        tui.pty.type_keys(&[0x02, b'y']);
        let panel = until(&mut tui, |s| s.contains("studios 2"), "panel").await;
        save(out, &format!("w{cols}-panel.txt"), &panel);
        assert!(
            panel.contains('!') || panel.contains("permission"),
            "the blocked actor's mark reaches the panel at {cols}:\n{panel}"
        );
        // Close the panel (the same toggle) before the detach chord —
        // a focused panel owns `q` itself.
        tui.pty.type_keys(&[0x02, b'y']);
        until(&mut tui, |s| !s.contains("studios 2"), "panel closed").await;
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

    for (cols, rows) in [(200u16, 40u16), (120, 34), (80, 30), (40, 30)] {
        attach_cycle(&scratch, &state, &out, cols, rows, cols <= 64).await;
    }

    // Stacking at a short height: every block but the current folds
    // to `╶ <id> …` inside the open panel.
    let mut tui = spawn_tui(&scratch, &state, 120, 10);
    until(&mut tui, |s| s.contains("cast ·"), "cast").await;
    tui.pty.type_keys(&[0x02, b'y']);
    let stacked = until(&mut tui, |s| s.contains('╶'), "a stacked block line").await;
    save(&out, "w120-short-stacked.txt", &stacked);

    drop(client);
    eprintln!("screens written to {}", out.display());
}
