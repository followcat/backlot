//! T6 e2e: the per-tab persistent float on real scratch cores.
//!
//! A real `hc-tui` runs in a PTY, driven by keystrokes, against pinned
//! `HC_TUI_TEST_CORE` serves. The Alt+F binding is the documented
//! example verbatim (`exec "${SHELL:-sh}"`), so the shell heuristic —
//! not an opt-in — is what makes the float persistent.
//!
//! `persistent_float_lifecycle` proves, on one scratch core: the
//! popup/owner/command labels land on a real record; hide and a tab
//! round-trip park and restore the same terminal (same pid, program
//! still answering input); `exit` reaps it and the next Alt+F spawns a
//! fresh record; an out-of-band kill clears it without any kill traffic
//! of the TUI's own; and closing the owning tab sends exactly one
//! `terminal.get`-verified `terminal.kill` for the parked float.
//!
//! `remote_owner_close` repeats the owner-gone proof on a `machine.add`'d
//! second core: the float runs on the remote machine, its owner label
//! carries the machine prefix, and the remote tab's death draws exactly
//! one kill.
//!
//! Kill calls are counted by a Unix-socket tap (`common/float_tap.rs`)
//! between hc-tui and the core; fixtures live in
//! `common/float_fixture.rs`.
//!
//! Ignored by default: needs `HC_TUI_TEST_CORE` — a pinned herdr-core
//! binary, never the installed `hc`. Run with
//!
//! ```sh
//! HC_TUI_TEST_CORE=/path/to/pinned/herdr-core \
//!     cargo test -p hc-tui --test float_e2e -- --ignored --test-threads=1
//! ```

#![cfg(unix)]

#[path = "common/fixture.rs"]
mod fixture;
#[path = "common/float_fixture.rs"]
mod float_fixture;
#[path = "common/float_tap.rs"]
mod float_tap;
#[path = "common/matrix.rs"]
mod matrix;
#[path = "common/pty_tui.rs"]
mod pty;
#[path = "../../hc-proto/tests/common/scratch.rs"]
mod scratch;

use std::time::Duration;

use hc_proto::client::Client;

use fixture::alt_arrow;
use float_fixture::{
    assert_float_labels, float_setup, kill_allow_gone, local_socket, run_rm_fake, spawn_through,
    type_until, until_float, until_no_float,
};
use float_tap::Tap;
use matrix::switch_via_sidebar;
use pty::{screen, until};

#[tokio::test]
#[ignore = "needs HC_TUI_TEST_CORE: a real pinned herdr-core binary"]
async fn persistent_float_lifecycle() {
    let scratch = scratch::ScratchCore::spawn().await;
    let client = Client::connect(&scratch.endpoint, None::<&std::ffi::OsStr>)
        .await
        .expect("connect");
    let setup = float_setup(&scratch, &scratch.root.join("config"));

    let agent1_bin = setup.out.join("agent1.bin");
    let agent1 = run_rm_fake(
        &client,
        &setup.fake,
        &agent1_bin,
        "ws-alpha",
        "tab-one",
        "agent1",
    )
    .await;
    let float_log = setup.out.join("float.txt");

    let tap = Tap::start(scratch.root.join("tap.sock"), local_socket(&scratch));
    let mut tui = spawn_through(&scratch, &setup.stub, &tap.socket);
    assert!(
        until(&mut tui, |v, _| screen(v).contains("MARK agent1"), "render").await,
        "initial render missing MARK agent1"
    );

    // --- A: Alt+F spawns a labelled float; hide and a tab round-trip
    // park and restore the same terminal.
    tui.type_keys(b"\x1bf");
    let a = until_float(&client).await;
    assert_float_labels(&a, "ws-alpha", "tab-one");
    let addr_a = a.address.clone();
    type_until(
        &mut tui,
        &float_log,
        format!("echo A1 >> {}\r", float_log.display()),
        "A1",
        "the float did not run typed input",
    )
    .await;
    // With a marker landed the program provably exec'd — pid is set.
    let pid_a = until_float(&client).await.pid.expect("float pid");

    // Alt+F hide: the record stays parked, nothing is killed.
    tui.type_keys(b"\x1bf");
    tokio::time::sleep(Duration::from_millis(600)).await;
    let hidden = until_float(&client).await;
    assert_eq!(
        hidden.address, addr_a,
        "hiding the float dropped its record"
    );
    assert_eq!(tap.count("terminal.kill", &addr_a), 0);
    tui.type_keys(b"\x1bf"); // show again
    type_until(
        &mut tui,
        &float_log,
        format!("echo A2 >> {}\r", float_log.display()),
        "A2",
        "the re-shown float lost input",
    )
    .await;

    // A second tab appears; the visible float parks on tab-one while we
    // look at tab-two, then comes back with its program intact.
    run_rm_fake(
        &client,
        &setup.fake,
        &setup.out.join("agent2.bin"),
        "ws-alpha",
        "tab-two",
        "agent2",
    )
    .await;
    assert!(
        until(
            &mut tui,
            |v, _| screen(v).contains("tab-two"),
            "tab-two lists"
        )
        .await,
        "tab-two never reached the sidebar"
    );
    alt_arrow(&tui, 'C');
    assert!(
        until(
            &mut tui,
            |v, _| screen(v).contains("MARK agent2"),
            "tab-two"
        )
        .await,
        "Alt+Right did not reveal tab-two"
    );
    let parked = until_float(&client).await;
    assert_eq!(parked.address, addr_a, "tab switch spawned a new float");
    assert_eq!(tap.count("terminal.kill", &addr_a), 0);
    alt_arrow(&tui, 'D');
    // Its own scrollback ("A2" was the last typed command) proves the
    // same VT is back on screen.
    assert!(
        until(&mut tui, |v, _| screen(v).contains("A2"), "restore").await,
        "the float did not reappear on return to tab-one"
    );
    type_until(
        &mut tui,
        &float_log,
        format!("echo A3 >> {}\r", float_log.display()),
        "A3",
        "the restored float lost input",
    )
    .await;
    let back = until_float(&client).await;
    assert_eq!(back.address, addr_a);
    assert_eq!(back.pid, Some(pid_a), "the parked float kept its program");

    // --- B: `exit` in the float reaps it; the next Alt+F spawns fresh.
    tui.type_keys(b"exit\r");
    until_no_float(&client).await;
    type_until(
        &mut tui,
        &agent1_bin,
        "UNDER-B".to_string(),
        "UNDER-B",
        "a dead float kept the keyboard focus",
    )
    .await;
    tui.type_keys(b"\x1bf");
    let b = until_float(&client).await;
    assert_ne!(b.address, addr_a, "the respawn reused a terminal");
    assert_float_labels(&b, "ws-alpha", "tab-one");
    type_until(
        &mut tui,
        &float_log,
        format!("echo B1 >> {}\r", float_log.display()),
        "B1",
        "the respawned float did not run",
    )
    .await;

    // --- C: an out-of-band kill clears the float with no kill loop.
    let addr_b = b.address.clone();
    kill_allow_gone(&client, &addr_b).await;
    until_no_float(&client).await;
    type_until(
        &mut tui,
        &agent1_bin,
        "UNDER-C".to_string(),
        "UNDER-C",
        "an externally killed float held the focus",
    )
    .await;
    // A settle window for any reconcile that could wrongly re-kill.
    // Dismissing the dead float submits one incarnation-gated kill —
    // its `terminal.get` check meets the reaped record and `allow_gone`
    // absorbs it, so zero kills and at most one get is the contract.
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert_eq!(
        tap.count("terminal.kill", &addr_b),
        0,
        "the TUI re-killed an externally killed float; calls were {:?}",
        tap.dump()
    );
    assert!(
        tap.count("terminal.get", &addr_b) <= 1,
        "the dead float's dismissal repeated its incarnation check; calls were {:?}",
        tap.dump()
    );

    // --- D: closing the owning tab sends exactly one verified kill.
    tui.type_keys(b"\x1bf");
    let c = until_float(&client).await;
    let addr_c = c.address.clone();
    type_until(
        &mut tui,
        &float_log,
        format!("echo C1 >> {}\r", float_log.display()),
        "C1",
        "float C did not run",
    )
    .await;
    alt_arrow(&tui, 'C'); // park C on tab-one, view tab-two
    assert!(
        until(
            &mut tui,
            |v, _| screen(v).contains("MARK agent2"),
            "tab-two again"
        )
        .await,
        "Alt+Right did not reveal tab-two"
    );
    // Kill tab-one's only terminal — the tab, and with it the float's
    // owner, leaves the snapshot.
    kill_allow_gone(&client, &agent1).await;
    until_no_float(&client).await;
    assert_eq!(
        tap.count("terminal.kill", &addr_c),
        1,
        "expected exactly one kill for the dead owner's float; calls were {:?}",
        tap.dump()
    );
    assert!(
        tap.count("terminal.get", &addr_c) >= 1,
        "the owner-gone kill was not verified through terminal.get"
    );
    assert_eq!(tap.count("terminal.kill", &agent1), 0);
    // One more settle: the kill must not repeat on later reconciles.
    tokio::time::sleep(Duration::from_millis(1200)).await;
    assert_eq!(
        tap.count("terminal.kill", &addr_c),
        1,
        "the owner-gone kill repeated; calls were {:?}",
        tap.dump()
    );

    tui.detach().await;
    drop(tap);
    drop(client);
    scratch.finish().expect("scratch teardown");
}

#[tokio::test]
#[ignore = "needs HC_TUI_TEST_CORE: a real pinned herdr-core binary"]
async fn remote_owner_close() {
    let a = scratch::ScratchCore::spawn_identity("scratch-core-a", "host-a").await;
    let b = scratch::ScratchCore::spawn_identity("scratch-core-b", "host-b").await;
    let ca = Client::connect(&a.endpoint, None::<&std::ffi::OsStr>)
        .await
        .expect("connect A");
    let cb = Client::connect(&b.endpoint, None::<&std::ffi::OsStr>)
        .await
        .expect("connect B");

    let setup = float_setup(&a, &a.root.join("config"));

    let agent_a = run_rm_fake(
        &ca,
        &setup.fake,
        &setup.out.join("agent-a.bin"),
        "ws-alpha",
        "tab-one",
        "agenta",
    )
    .await;
    let agent_b = run_rm_fake(
        &cb,
        &setup.fake,
        &setup.out.join("agent-b.bin"),
        "ws-remote",
        "tab-b",
        "agentb",
    )
    .await;
    let float_log = setup.out.join("remote-float.txt");

    // One tap on the TUI's connection to A. Remote calls never reach B
    // on a TUI socket: `submit_action` uses the single local ops store,
    // so the remote float's kill goes out qualified (`host-b/t2`) and
    // core A's own machine link forwards it — core-internal traffic no
    // tap of ours should see.
    let tap_a = Tap::start(a.root.join("tap-a.sock"), local_socket(&a));
    let env_b = fixture::dest_env(&b, &setup.stub);
    let via = fixture::write_via_stub(&setup.stub, &env_b);
    fixture::machine_add(&ca, &via, "host-b", &b.session).await;

    let mut tui = spawn_through(&a, &setup.stub, &tap_a.socket);
    assert!(
        until(&mut tui, |v, _| screen(v).contains("MARK agenta"), "render").await,
        "initial render missing MARK agenta"
    );
    assert!(
        until(&mut tui, |v, _| screen(v).contains("ws-remote"), "mirror").await,
        "host-b's workspace never mirrored"
    );

    // Focus the remote tab, then open the float: `terminal.run` lands on
    // B with the machine-qualified owner label.
    switch_via_sidebar(&mut tui, "ws-remote", "agentb")
        .await
        .expect("focus remote tab");
    tui.type_keys(b"\x1bf");
    let r = until_float(&cb).await;
    assert_float_labels(&r, "ws-remote", "tab-b");
    let owner = r.labels.get("hc-tui.owner-tab").unwrap();
    assert!(
        owner.contains("host-b"),
        "remote float owner {owner:?} missed the machine prefix"
    );
    let addr_r = r.address.clone();
    type_until(
        &mut tui,
        &float_log,
        format!("echo R1 >> {}\r", float_log.display()),
        "R1",
        "the remote float did not run typed input",
    )
    .await;

    // Park it: Alt+F hides the float (the modal layer swallows sidebar
    // clicks), then focus back on the local tab.
    tui.type_keys(b"\x1bf");
    tokio::time::sleep(Duration::from_millis(400)).await;
    switch_via_sidebar(&mut tui, "ws-alpha", "agenta")
        .await
        .expect("focus local tab");

    // Kill the remote tab's only terminal: B's snapshot loses tab-b, so
    // the parked float's owner is gone and the TUI must kill it once.
    kill_allow_gone(&cb, &agent_b).await;
    until_no_float(&cb).await;
    assert_eq!(
        tap_a.count("terminal.kill", &addr_r),
        1,
        "expected exactly one kill for the remote owner's float; calls were {:?}",
        tap_a.dump()
    );
    assert!(
        tap_a.count("terminal.get", &addr_r) >= 1,
        "the remote kill was not verified through terminal.get"
    );
    assert_eq!(tap_a.count("terminal.kill", &agent_a), 0);
    assert_eq!(tap_a.count("terminal.kill", &agent_b), 0);
    tokio::time::sleep(Duration::from_millis(1200)).await;
    assert_eq!(
        tap_a.count("terminal.kill", &addr_r),
        1,
        "the remote owner-gone kill repeated; calls were {:?}",
        tap_a.dump()
    );

    tui.detach().await;
    drop(tap_a);
    drop(ca);
    drop(cb);
    a.finish().expect("scratch A teardown");
    b.finish().expect("scratch B teardown");
}
