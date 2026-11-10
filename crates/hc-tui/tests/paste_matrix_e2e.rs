//! The paste matrix, part 1 — a local `hc-tui` on one scratch core. Text
//! and image payloads, four copy sources (external paste, mouse
//! selection, copy mode `y`, popup selection), and every paste target
//! reachable without a second machine: same pane, split pane, other tab,
//! other workspace, the Alt+F popup, and right after an Alt+arrow switch.
//!
//! Ignored by default: needs `HC_TUI_TEST_CORE` — a pinned herdr-core
//! binary, never the installed `hc`. Run with
//!
//! ```sh
//! HC_TUI_TEST_CORE=/path/to/pinned/herdr-core \
//!     cargo test -p hc-tui --test paste_matrix_e2e -- --ignored --test-threads=1
//! ```
//!
//! Every cell records pass/fail instead of aborting, so the report table
//! prints even when cells fail; the test still fails at the end if any
//! did.

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

use fixture::{alt_arrow, ctrl_v};
use matrix::{Ctx, copy_by_copy_mode, copy_by_mouse, file_gains, switch_via_sidebar, wrapped};
use pty::until;

/// Text payloads. The large one stays under the input record cap.
const TEXT_SIMPLE: &str = "hello-matrix-42";
const TEXT_MULTI: &str = "line-one\nline-two\nline-three";
const TEXT_CJK: &str = "粘贴测试·こんにちは·한국어";
fn text_large() -> String {
    "x".repeat(64 * 1024)
}

/// Where each fake lives: `(space, tab, tag, spec)`. The split pair
/// shares space+tab so `tab-one` renders two panes.
const PANES: &[(&str, &str, &str, &str)] = &[
    ("ws-alpha", "tab-one", "claude", "2004,kitty1"),
    ("ws-alpha", "tab-one", "shell", ""),
    ("ws-alpha", "tab-two", "codex", "2004"),
    ("ws-beta", "tab-main", "devin", "2004"),
];

#[tokio::test]
#[ignore = "needs HC_TUI_TEST_CORE: a real pinned herdr-core binary"]
async fn text_and_image_paste_matrix_local() {
    let scratch = ScratchCore::spawn().await;
    let client = Client::connect(&scratch.endpoint, None::<&std::ffi::OsStr>)
        .await
        .expect("connect");

    // Owned stubs: clipboard tools backed by `clip/mime`, the fake-agent
    // script, and `hc` for any `ssh` runs.
    let stub = scratch.root.join("stub");
    let clip = scratch.root.join("clip");
    let agents = scratch.root.join("agents");
    let out = scratch.root.join("out");
    for dir in [&stub, &clip, &agents, &out] {
        fs::create_dir_all(dir).unwrap();
    }
    fixture::write_clipboard_tools(&stub, &clip);
    let fake = fixture::write_fake_agent(&agents);
    let popup_fake = fixture::write_popup_agent(&agents);
    std::os::unix::fs::symlink(scratch::core_binary(), stub.join("hc")).unwrap();
    let popup_flag = out.join("popup.flag");
    fixture::write_tui_config(
        &scratch.root.join("config"),
        &popup_fake,
        "2004",
        &out.join("popup.bin"),
        &popup_flag,
    );

    for &(space, tab, tag, spec) in PANES {
        fixture::run_fake(
            &client,
            &fake,
            spec,
            &out.join(format!("{tag}.bin")),
            space,
            tab,
            tag,
        )
        .await;
    }

    let mut tui = fixture::spawn(&scratch, &stub, None);
    assert!(
        until(
            &mut tui,
            |v, _| pty::screen(v).contains("MARK claude"),
            "render"
        )
        .await,
        "initial render missing MARK claude"
    );
    assert!(
        until(
            &mut tui,
            |v, _| pty::screen(v).contains("MARK shell"),
            "split"
        )
        .await,
        "split render missing MARK shell"
    );

    let mut ctx = Ctx {
        tui,
        client,
        panes: PANES,
        out: out.clone(),
        clip: clip.clone(),
        results: Vec::new(),
        home_tag: "claude",
        popup_flag,
    };

    // --- source: external system paste -----------------------------------
    for (name, text) in [
        ("simple", TEXT_SIMPLE),
        ("multiline", TEXT_MULTI),
        ("cjk", TEXT_CJK),
    ] {
        for &(_, _, tag, spec) in PANES {
            cell!(ctx, format!("sys-paste {name} -> {tag}"), async {
                ctx.focus(tag).await?;
                ctx.assert_paste(tag, text, wrapped(spec)).await
            });
        }
    }
    cell!(ctx, "sys-paste large(64KiB) -> claude", async {
        ctx.focus("claude").await?;
        ctx.assert_paste("claude", &text_large(), true).await
    });

    // --- the reported bug: select, switch tab, paste ----------------------
    cell!(
        ctx,
        "mouse-select -> switch tab -> paste (reported bug)",
        async {
            ctx.focus("shell").await?;
            let copied = copy_by_mouse(&mut ctx.tui, "shell").await?;
            if !String::from_utf8_lossy(&copied).contains("MARK shell abcdefgh") {
                return Err(format!("selection copied {copied:?}, not the marker"));
            }
            switch_via_sidebar(&mut ctx.tui, "tab-two", "codex").await?;
            let text = String::from_utf8_lossy(&copied).trim_end().to_string();
            ctx.assert_paste("codex", &text, true).await
        }
    );

    // --- other copy sources -----------------------------------------------
    cell!(ctx, "copy-mode y -> switch tab -> paste", async {
        ctx.focus("shell").await?;
        let copied = copy_by_copy_mode(&mut ctx.tui, "shell").await?;
        if !String::from_utf8_lossy(&copied).contains("MARK shell abcdefgh") {
            return Err(format!("copy-mode y copied {copied:?}"));
        }
        switch_via_sidebar(&mut ctx.tui, "tab-two", "codex").await?;
        let text = String::from_utf8_lossy(&copied).trim_end().to_string();
        ctx.assert_paste("codex", &text, true).await
    });

    cell!(ctx, "popup selection -> paste other pane", async {
        ctx.tui.type_keys(b"\x1bf"); // Alt+F
        if !until(
            &mut ctx.tui,
            |v, _| pty::screen(v).contains("MARK popup"),
            "popup open",
        )
        .await
        {
            return Err("Alt+F popup did not open".into());
        }
        let copied = copy_by_mouse(&mut ctx.tui, "popup").await?;
        if !String::from_utf8_lossy(&copied).contains("MARK popup abcdefgh") {
            return Err(format!("popup selection copied {copied:?}"));
        }
        let _ = fs::write(&ctx.popup_flag, b""); // end the popup program
        tokio::time::sleep(Duration::from_millis(400)).await;
        let _ = fs::remove_file(&ctx.popup_flag);
        ctx.focus("codex").await?;
        let text = String::from_utf8_lossy(&copied).trim_end().to_string();
        ctx.assert_paste("codex", &text, true).await
    });

    // --- paste targets -----------------------------------------------------
    cell!(ctx, "paste -> other pane in same tab", async {
        ctx.focus("shell").await?;
        ctx.focus("claude").await?;
        ctx.assert_paste("claude", "same-tab-split", true).await
    });

    cell!(ctx, "paste -> other workspace", async {
        ctx.focus("claude").await?;
        switch_via_sidebar(&mut ctx.tui, "ws-beta", "devin").await?;
        ctx.assert_paste("devin", "other-workspace", true).await
    });

    cell!(ctx, "paste right after Alt+Right tab switch", async {
        ctx.focus("claude").await?;
        alt_arrow(&ctx.tui, 'C');
        tokio::time::sleep(Duration::from_millis(120)).await;
        ctx.assert_paste("codex", "after-alt-right", true).await
    });

    cell!(ctx, "paste into Alt+F popup", async {
        ctx.tui.type_keys(b"\x1bf");
        if !until(
            &mut ctx.tui,
            |v, _| pty::screen(v).contains("MARK popup"),
            "popup open",
        )
        .await
        {
            return Err("Alt+F popup did not open".into());
        }
        ctx.assert_paste("popup", "into-popup", true).await
    });

    // --- images on a local endpoint ---------------------------------------
    // Ctrl+V is not the image bridge here — the program gets the key.
    for (label, mime, bytes) in [
        ("png", "image/png", pty::PNG.to_vec()),
        ("jpeg", "image/jpeg", pty::JPEG.to_vec()),
    ] {
        cell!(
            ctx,
            format!("ctrl+v {label} -> same pane (local: to program)"),
            async {
                pty::clipboard_set(&ctx.clip, mime, &bytes);
                ctx.focus("claude").await?;
                let out = ctx.out("claude");
                let mark = fs::read(&out).map(|b| b.len()).unwrap_or(0);
                ctrl_v(&ctx.tui);
                // kitty1 fake: the press forwards as CSI-u `118;5u`.
                if file_gains(
                    &mut ctx.tui,
                    &out,
                    b"\x1b[118;5u",
                    Some(mark),
                    Duration::from_secs(5),
                )
                .await
                {
                    return Ok::<(), String>(());
                }
                Err("Ctrl+V did not reach the kitty-enabled program as CSI-u".into())
            }
        );
    }

    let failures = matrix::report(&ctx.results, "paste matrix (local)");
    ctx.tui.detach().await;
    drop(ctx.client);
    drop(ctx.tui);
    scratch.finish().expect("scratch teardown");
    assert_eq!(failures, 0, "{failures} matrix cells failed");
}
