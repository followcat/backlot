//! The paste matrix, part 2 — remote and machine targets. Three runs:
//! `hc-tui --remote host-a` through the stub `ssh` (remote endpoint —
//! the image bridge is active), a local `hc-tui` pasting into a pane on
//! `host-b` reached through the sidebar machine mirror (`machine.add`
//! via-command to a second scratch core), and `hc-tui --remote host-b`
//! direct to the second core.
//!
//! Ignored by default: needs `HC_TUI_TEST_CORE`. Run with
//!
//! ```sh
//! HC_TUI_TEST_CORE=/path/to/pinned/herdr-core \
//!     cargo test -p hc-tui --test paste_matrix_remote_e2e -- --ignored --test-threads=1
//! ```

#![cfg(unix)]

#[path = "common/fixture.rs"]
mod fixture;
#[path = "common/matrix.rs"]
mod matrix;
#[path = "common/pty_tui.rs"]
mod pty;
#[path = "../../hc-proto/tests/common/scratch.rs"]
mod scratch;

use std::fs;
use std::time::Duration;

use hc_proto::client::Client;
use scratch::ScratchCore;

use fixture::alt_arrow;
use matrix::{Ctx, copy_by_mouse, switch_via_sidebar};
use pty::until;

const PANES: &[(&str, &str, &str, &str)] = &[
    ("ws-alpha", "tab-one", "claude", "2004,kitty1"),
    ("ws-alpha", "tab-one", "shell", ""),
    ("ws-alpha", "tab-two", "codex", "2004"),
    ("ws-beta", "tab-main", "devin", "2004"),
    ("ws-remote", "tab-b", "opencode", "2004"),
];

/// Opens the Alt+F popup and waits for its marker.
async fn open_popup(tui: &mut pty::Tui) -> Result<(), String> {
    tui.type_keys(b"\x1bf");
    until(
        tui,
        |v, _| pty::screen(v).contains("MARK popup"),
        "popup open",
    )
    .await
    .then_some(())
    .ok_or_else(|| "Alt+F popup did not open".to_string())
}

/// The shared setup: two scratch cores, stub tools, fake agents on both
/// cores, and `machine.add` wiring host-b through a via-command.
async fn setup() -> (
    ScratchCore,
    ScratchCore,
    Client,
    Client,
    std::path::PathBuf,
    std::path::PathBuf,
    std::path::PathBuf,
    std::path::PathBuf,
) {
    let a = ScratchCore::spawn_identity("scratch-core-a", "host-a").await;
    let b = ScratchCore::spawn_identity("scratch-core-b", "host-b").await;
    let ca = Client::connect(&a.endpoint, None::<&std::ffi::OsStr>)
        .await
        .expect("connect A");
    let cb = Client::connect(&b.endpoint, None::<&std::ffi::OsStr>)
        .await
        .expect("connect B");

    let stub = a.root.join("stub");
    let clip = a.root.join("clip");
    let agents = a.root.join("agents");
    let out = a.root.join("out");
    for dir in [&stub, &clip, &agents, &out] {
        fs::create_dir_all(dir).unwrap();
    }
    std::os::unix::fs::symlink(scratch::core_binary(), stub.join("hc")).unwrap();
    std::os::unix::fs::symlink(scratch::core_binary(), stub.join("herdr-core")).unwrap();
    fixture::write_clipboard_tools(&stub, &clip);
    fixture::write_ssh_stub(&stub, &[("host-a", &a, &stub), ("host-b", &b, &stub)]);

    let fake = fixture::write_fake_agent(&agents);
    let popup_fake = fixture::write_popup_agent(&agents);
    for (scratch, cfg_root) in [(&a, a.root.join("config")), (&b, b.root.join("config"))] {
        let _ = scratch;
        fixture::write_tui_config(
            &cfg_root,
            &popup_fake,
            "2004",
            &out.join("popup.bin"),
            &out.join("popup.flag"),
        );
    }

    // Local fakes on A, then the remote fake on B. B's own `hc` must be
    // on the stub PATH via the via-command env.
    for &(space, tab, tag, spec) in PANES.iter().take(4) {
        fixture::run_fake(
            &ca,
            &fake,
            spec,
            &out.join(format!("{tag}.bin")),
            space,
            tab,
            tag,
        )
        .await;
    }
    fixture::run_fake(
        &cb,
        &fake,
        "2004",
        &out.join("opencode.bin"),
        "ws-remote",
        "tab-b",
        "opencode",
    )
    .await;

    // host-b joins A's machine list through the via-command — no ssh,
    // the remote line runs locally with B's environment.
    let via = fixture::write_via_stub(&stub, &fixture::dest_env(&b, &stub));
    fixture::machine_add(&ca, &via, "host-b", &b.session).await;

    (a, b, ca, cb, stub, clip, agents, out)
}

#[tokio::test]
#[ignore = "needs HC_TUI_TEST_CORE: a real pinned herdr-core binary"]
async fn paste_matrix_remote_and_machine() {
    let (a, b, ca, cb, stub, clip, _agents, out) = setup().await;

    // --- run 1: hc-tui --remote host-a -----------------------------------
    let tui = fixture::spawn(&a, &stub, Some("host-a"));
    let mut ctx = Ctx {
        tui,
        client: ca.clone(),
        panes: PANES,
        out: out.clone(),
        clip: clip.clone(),
        results: Vec::new(),
        home_tag: "claude",
        popup_flag: out.join("popup.flag"),
    };
    assert!(
        until(
            &mut ctx.tui,
            |v, _| pty::screen(v).contains("MARK claude"),
            "render a"
        )
        .await,
        "remote TUI did not render claude"
    );
    // The machine mirror takes a moment; host-b must list before the
    // sidebar cells can click it.
    assert!(
        until(
            &mut ctx.tui,
            |v, _| pty::screen(v).contains("ws-remote"),
            "mirror"
        )
        .await,
        "host-b workspace never mirrored"
    );

    cell!(ctx, "remote: sys-paste -> claude", async {
        ctx.focus("claude").await?;
        ctx.assert_paste("claude", "remote-text", true).await
    });
    cell!(ctx, "remote: sys-paste multiline -> shell", async {
        ctx.focus("shell").await?;
        ctx.assert_paste("shell", "r-one\nr-two", false).await
    });
    cell!(ctx, "remote: mouse-select -> switch tab -> paste", async {
        ctx.focus("shell").await?;
        let copied = copy_by_mouse(&mut ctx.tui, "shell").await?;
        switch_via_sidebar(&mut ctx.tui, "tab-two", "codex").await?;
        let text = String::from_utf8_lossy(&copied).trim_end().to_string();
        ctx.assert_paste("codex", &text, true).await
    });
    cell!(ctx, "remote: ctrl+v png -> claude (image upload)", async {
        pty::clipboard_set(&ctx.clip, "image/png", pty::PNG);
        ctx.focus("claude").await?;
        ctx.assert_image_paste("claude", pty::PNG).await
    });
    cell!(
        ctx,
        "remote: ctrl+v jpeg -> devin (other workspace)",
        async {
            pty::clipboard_set(&ctx.clip, "image/jpeg", pty::JPEG);
            ctx.focus("claude").await?;
            switch_via_sidebar(&mut ctx.tui, "ws-beta", "devin").await?;
            ctx.assert_image_paste("devin", pty::JPEG).await
        }
    );
    cell!(ctx, "remote: ctrl+v copied-file -> shell", async {
        let png_path = a.work.join("copied.png");
        fs::write(&png_path, pty::PNG).unwrap();
        pty::clipboard_set(
            &ctx.clip,
            "x-special/gnome-copied-files",
            format!("copy\nfile://{}\n", png_path.display()).as_bytes(),
        );
        ctx.focus("shell").await?;
        ctx.assert_image_paste("shell", pty::PNG).await
    });
    cell!(
        ctx,
        "remote: paste into host-b pane (sidebar machine)",
        async {
            ctx.focus("claude").await?;
            switch_via_sidebar(&mut ctx.tui, "ws-remote", "opencode").await?;
            ctx.assert_paste("opencode", "into-machine-pane", true)
                .await
        }
    );
    cell!(ctx, "remote: ctrl+v png -> host-b pane", async {
        pty::clipboard_set(&ctx.clip, "image/png", pty::PNG);
        ctx.focus("claude").await?;
        switch_via_sidebar(&mut ctx.tui, "ws-remote", "opencode").await?;
        ctx.assert_image_paste("opencode", pty::PNG).await
    });
    cell!(ctx, "remote: paste right after Alt+Right", async {
        ctx.focus("claude").await?;
        alt_arrow(&ctx.tui, 'C');
        tokio::time::sleep(Duration::from_millis(120)).await;
        ctx.assert_paste("codex", "remote-alt-right", true).await
    });
    cell!(ctx, "remote: paste into popup", async {
        ctx.focus("claude").await?;
        open_popup(&mut ctx.tui).await?;
        ctx.assert_paste("popup", "remote-popup", true).await
    });
    cell!(
        ctx,
        "remote: select in machine pane -> paste local",
        async {
            ctx.focus("claude").await?;
            switch_via_sidebar(&mut ctx.tui, "ws-remote", "opencode").await?;
            let copied = copy_by_mouse(&mut ctx.tui, "opencode").await?;
            if !String::from_utf8_lossy(&copied).contains("MARK opencode abcdefgh") {
                return Err(format!("machine selection copied {copied:?}"));
            }
            switch_via_sidebar(&mut ctx.tui, "ws-alpha", "claude").await?;
            let text = String::from_utf8_lossy(&copied).trim_end().to_string();
            ctx.assert_paste("claude", &text, true).await
        }
    );

    let failures_a = matrix::report(&ctx.results, "paste matrix (--remote host-a)");
    ctx.tui.detach().await;
    drop(ctx);

    // --- run 2: hc-tui --remote host-b ------------------------------------
    let tui = fixture::spawn_with_session(&b, &stub, Some("host-b"), Some(&b.session));
    let mut ctx = Ctx {
        tui,
        client: cb.clone(),
        panes: PANES,
        out: out.clone(),
        clip: clip.clone(),
        results: Vec::new(),
        home_tag: "opencode",
        popup_flag: out.join("popup.flag"),
    };
    assert!(
        until(
            &mut ctx.tui,
            |v, _| pty::screen(v).contains("MARK opencode"),
            "render b"
        )
        .await,
        "remote-b TUI did not render opencode"
    );
    cell!(ctx, "remote-b: sys-paste -> opencode", async {
        ctx.focus("opencode").await?;
        ctx.assert_paste("opencode", "remote-b-text", true).await
    });
    cell!(ctx, "remote-b: ctrl+v png -> opencode", async {
        pty::clipboard_set(&ctx.clip, "image/png", pty::PNG);
        ctx.focus("opencode").await?;
        ctx.assert_image_paste("opencode", pty::PNG).await
    });
    let failures_b = matrix::report(&ctx.results, "paste matrix (--remote host-b)");
    ctx.tui.detach().await;
    drop(ctx);

    // --- run 3: local hc-tui, machine pane through the sidebar ------------
    let tui = fixture::spawn(&a, &stub, None);
    let mut ctx = Ctx {
        tui,
        client: ca.clone(),
        panes: PANES,
        out: out.clone(),
        clip: clip.clone(),
        results: Vec::new(),
        home_tag: "claude",
        popup_flag: out.join("popup.flag"),
    };
    assert!(
        until(
            &mut ctx.tui,
            |v, _| pty::screen(v).contains("MARK claude"),
            "render local"
        )
        .await,
        "local TUI did not render"
    );
    assert!(
        until(
            &mut ctx.tui,
            |v, _| pty::screen(v).contains("ws-remote"),
            "mirror local"
        )
        .await,
        "host-b never mirrored locally"
    );
    cell!(
        ctx,
        "local: paste into host-b pane (sidebar machine)",
        async {
            ctx.focus("claude").await?;
            switch_via_sidebar(&mut ctx.tui, "ws-remote", "opencode").await?;
            ctx.assert_paste("opencode", "local-into-machine", true)
                .await
        }
    );
    cell!(
        ctx,
        "local: ctrl+v png -> host-b pane (machine bridge)",
        async {
            pty::clipboard_set(&ctx.clip, "image/png", pty::PNG);
            ctx.focus("claude").await?;
            switch_via_sidebar(&mut ctx.tui, "ws-remote", "opencode").await?;
            ctx.assert_image_paste("opencode", pty::PNG).await
        }
    );
    let failures_c = matrix::report(&ctx.results, "paste matrix (local + machine sidebar)");
    ctx.tui.detach().await;
    drop(ctx);

    drop(ca);
    drop(cb);
    a.finish().expect("scratch A teardown");
    b.finish().expect("scratch B teardown");
    assert_eq!(
        failures_a + failures_b + failures_c,
        0,
        "matrix cells failed"
    );
}
