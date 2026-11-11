//! R9 clipboard-image bridge tests: triggers, target policy, leases,
//! and the `terminal.put` upload path against `testcore::FakeCore`.
use super::*;
use crate::testcore;
use hc_vt::{Action, Key, Mods};
use serde_json::json;

const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\x00\x00\x00\x0dIHDR";

fn png_image() -> crate::clipboard::ClipboardRead {
    crate::clipboard::ClipboardRead::Image(crate::clipboard::ClipboardImage {
        bytes: PNG.to_vec(),
        extension: "png",
    })
}

fn big_image() -> crate::clipboard::ClipboardRead {
    crate::clipboard::ClipboardRead::Image(crate::clipboard::ClipboardImage {
        bytes: vec![0u8; crate::clipboard::MAX_CLIPBOARD_IMAGE_BYTES + 1],
        extension: "png",
    })
}

fn no_image() -> crate::clipboard::ClipboardRead {
    crate::clipboard::ClipboardRead::None
}

/// An owned scratch dir for one test: the per-process counter keeps
/// parallel tests apart, and `create_dir` claims the name atomically —
/// a leftover directory is retried, never adopted.
fn scratch_dir(tag: &str) -> std::path::PathBuf {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    loop {
        let dir = std::env::temp_dir().join(format!(
            "hc-tui-{tag}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        match std::fs::create_dir(&dir) {
            Ok(()) => return dir,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => panic!("create scratch dir {}: {e}", dir.display()),
        }
    }
}

fn ctrl_v(action: Action) -> input::TerminalKey {
    term_key(Key::Char('v'), Mods::CTRL, action)
}

fn ctrl_shift_v(action: Action) -> input::TerminalKey {
    term_key(Key::Char('v'), Mods::CTRL.union(Mods::SHIFT), action)
}

fn pending(app: &mut App) {
    app.open_attach = Arc::new(|_| Box::pin(std::future::pending()));
    app.pane = pane::Pane::new(app.pane_tx.clone(), app.open_attach.clone());
}

fn operations(app: &mut App) -> mpsc::Receiver<testcore::FakeCore> {
    let (accepted, rx) = mpsc::channel(4);
    let (rpc, _sub) = testcore::openers(accepted);
    app.set_operations(rpc);
    rx
}

async fn settle(app: &mut App) {
    let result = tokio::time::timeout(
        testcore::DEADLINE,
        App::operation_result(&mut app.operations),
    )
    .await
    .unwrap();
    app.on_operation(result);
}

fn remote_app(addresses: &[&str]) -> App {
    let mut app = app_with(addresses);
    pending(&mut app);
    app.set_image_paste_context(true, false);
    app.image_paste_reader = Box::new(png_image);
    app.reconcile_published_view();
    app
}

fn get_reply(address: &str, created_ms: u64) -> serde_json::Value {
    let mut term = testcore::minimal_terminal();
    term["address"] = json!(address);
    term["created_ms"] = json!(created_ms);
    term
}

/// The core id the operations store guards on must match `view.core`.
fn core_id(app: &mut App, id: &str) {
    app.view.core.as_mut().unwrap().id = id.into();
}

fn press_key(app: &mut App, key: input::TerminalKey) {
    app.on_input(input::UiEvent::Key(key), 120);
}

// ---- triggers -------------------------------------------------------

#[tokio::test]
async fn ctrl_v_press_starts_an_upload_and_is_consumed() {
    let mut app = remote_app(&["t1"]);
    let mut rx = operations(&mut app);
    core_id(&mut app, "core-fake-1");
    let server = tokio::spawn(async move {
        let mut core = rx.recv().await.unwrap();
        core.read_hello().await;
        core.answer_hello(true, &["terminal.get", "terminal.put"], &[])
            .await;
        let get = core.read_request().await;
        assert_eq!(get["method"], "terminal.get");
        core.reply(&get["id"], get_reply("t1", 0)).await;
        let put = core.read_request().await;
        assert_eq!(put["method"], "terminal.put");
        assert_eq!(put["params"]["terminal"], "t1");
        assert!(
            put["params"]["name"]
                .as_str()
                .unwrap()
                .starts_with("clipboard-")
        );
        assert!(put["params"]["name"].as_str().unwrap().ends_with(".png"));
        assert_eq!(
            put["params"]["data"],
            json!(crate::selection::base64_encode(PNG))
        );
        core.reply(&put["id"], json!({"path": "/home/user/clipboard-1.png"}))
            .await;
    });
    press_key(&mut app, ctrl_v(Action::Press));
    // The press was consumed: nothing reached the pane.
    assert_eq!(app.pane.input_backlog().0, 0);
    settle(&mut app).await;
    server.await.unwrap();
    // The returned path lands as one normal pane paste — nothing else.
    assert_eq!(app.pane.input_backlog().0, 1);
}

#[tokio::test]
async fn press_and_release_in_one_read_suppresses_the_release() {
    let mut app = remote_app(&["t1"]);
    app.host_reports_key_releases = true;
    let mut rx = operations(&mut app);
    core_id(&mut app, "core-fake-1");
    let server = tokio::spawn(async move {
        let mut core = rx.recv().await.unwrap();
        core.read_hello().await;
        core.answer_hello(true, &["terminal.get", "terminal.put"], &[])
            .await;
        let get = core.read_request().await;
        core.reply(&get["id"], get_reply("t1", 0)).await;
        let put = core.read_request().await;
        core.reply(&put["id"], json!({"path": "/p.png"})).await;
    });
    press_key(&mut app, ctrl_v(Action::Press));
    press_key(&mut app, ctrl_v(Action::Release));
    assert_eq!(app.pane.input_backlog().0, 0, "release must not leak");
    settle(&mut app).await;
    server.await.unwrap();
    assert_eq!(app.pane.input_backlog().0, 1, "the pasted path only");
}

#[tokio::test]
async fn consumed_press_repeat_is_swallowed() {
    let mut app = remote_app(&["t1"]);
    // Repeats only arrive from event-type-reporting hosts, the same
    // flag that grows the suppress set.
    app.host_reports_key_releases = true;
    let _rx = operations(&mut app);
    core_id(&mut app, "core-fake-1");
    press_key(&mut app, ctrl_v(Action::Press));
    press_key(&mut app, ctrl_v(Action::Repeat));
    assert_eq!(app.pane.input_backlog().0, 0);
}

#[tokio::test]
async fn an_orphan_repeat_reaches_the_program_on_a_local_endpoint() {
    let mut app = app_with(&["t1"]);
    pending(&mut app);
    app.image_paste_reader = Box::new(png_image);
    app.reconcile_published_view();
    // No press of this key was ever consumed — a bare repeat matching
    // the trigger must route like ordinary input.
    press_key(&mut app, ctrl_v(Action::Repeat));
    assert_eq!(app.pane.input_backlog().0, 1, "orphan repeat forwards");
}

#[tokio::test]
async fn an_orphan_repeat_reaches_the_program_on_a_remote_endpoint() {
    let mut app = remote_app(&["t1"]);
    app.image_paste_reader = Box::new(no_image);
    press_key(&mut app, ctrl_v(Action::Repeat));
    assert_eq!(app.pane.input_backlog().0, 1, "orphan repeat forwards");
}

#[tokio::test]
async fn an_unconsumed_press_clears_stale_repeat_suppression() {
    let mut app = remote_app(&["t1"]);
    app.image_paste_reader = Box::new(no_image);
    // A suppress entry orphaned by a lost release must not survive a
    // press of the same key that was forwarded unconsumed.
    app.image_paste_suppress
        .insert(ctrl_v(Action::Press).identity());
    press_key(&mut app, ctrl_v(Action::Press));
    assert_eq!(app.pane.input_backlog().0, 1, "unconsumed press forwards");
    assert!(
        app.image_paste_suppress.is_empty(),
        "stale entry removed with the unconsumed press"
    );
}

#[tokio::test]
async fn a_different_key_forwards_normally() {
    let mut app = remote_app(&["t1"]);
    press_key(
        &mut app,
        term_key(Key::Char('x'), Mods::CTRL, Action::Press),
    );
    assert_eq!(app.pane.input_backlog().0, 1);
}

#[tokio::test]
async fn ctrl_shift_v_is_not_ctrl_v() {
    let mut app = remote_app(&["t1"]);
    press_key(&mut app, ctrl_shift_v(Action::Press));
    assert_eq!(
        app.pane.input_backlog().0,
        1,
        "Ctrl+Shift+V must forward to the program"
    );
}

#[tokio::test]
async fn empty_remote_image_paste_disables_the_key_trigger() {
    let mut app = remote_app(&["t1"]);
    let mut config = crate::config::Config::default();
    config.keys.remote_image_paste = String::new();
    app.shell = crate::chrome::ClientShellConfig::from_config(
        &config,
        None,
        None,
        std::path::PathBuf::new(),
    );
    press_key(&mut app, ctrl_v(Action::Press));
    assert_eq!(app.pane.input_backlog().0, 1, "empty config forwards");
}

// ---- endpoint eligibility -------------------------------------------

#[tokio::test]
async fn local_endpoint_bridges_nothing() {
    let mut app = app_with(&["t1"]);
    pending(&mut app);
    app.image_paste_reader = Box::new(png_image);
    app.reconcile_published_view();
    press_key(&mut app, ctrl_v(Action::Press));
    app.on_input(input::UiEvent::Paste(Vec::new()), 120);
    app.on_input(input::UiEvent::Paste(b"/home/user/a.png".to_vec()), 120);
    assert_eq!(
        app.pane.input_backlog().0,
        3,
        "key and both pastes forward as plain input"
    );
}

#[tokio::test]
async fn machine_terminal_bridges_on_a_local_endpoint() {
    let mut app = app_with(&["m1/t9"]);
    pending(&mut app);
    app.image_paste_reader = Box::new(png_image);
    app.reconcile_published_view();
    let _rx = operations(&mut app);
    app.focused = Some("m1/t9".into());
    press_key(&mut app, ctrl_v(Action::Press));
    assert_eq!(app.pane.input_backlog().0, 0);
    assert_eq!(app.pending_actions, 1, "upload action submitted");
}

// ---- no image on the clipboard ---------------------------------------

#[tokio::test]
async fn no_image_forwards_the_key_unchanged_and_in_order() {
    let mut app = remote_app(&["t1"]);
    app.image_paste_reader = Box::new(no_image);
    press_key(&mut app, ctrl_v(Action::Press));
    press_key(
        &mut app,
        term_key(Key::Char('x'), Mods::NONE, Action::Press),
    );
    assert_eq!(
        app.pane.input_backlog().0,
        2,
        "the trigger key and the following key both reach the program, in order"
    );
    assert_eq!(app.pending_actions, 0);
    assert!(app.notice.is_some(), "a miss explains itself");
}

#[tokio::test]
async fn an_unreadable_clipboard_explains_and_forwards_the_key() {
    let mut app = remote_app(&["t1"]);
    app.image_paste_reader =
        Box::new(|| crate::clipboard::ClipboardRead::Unavailable("no clipboard tool"));
    press_key(&mut app, ctrl_v(Action::Press));
    assert_eq!(
        app.pane.input_backlog().0,
        1,
        "the key still reaches the program"
    );
    assert_eq!(app.pending_actions, 0);
    assert!(app.notice.is_some(), "the miss explains itself");
}

#[tokio::test]
async fn no_image_forwards_the_release_like_any_key() {
    let mut app = remote_app(&["t1"]);
    app.image_paste_reader = Box::new(no_image);
    press_key(&mut app, ctrl_v(Action::Press));
    press_key(&mut app, ctrl_v(Action::Release));
    assert_eq!(app.pane.input_backlog().0, 2, "press and release forward");
}

// ---- empty and path pastes -------------------------------------------

#[tokio::test]
async fn empty_bracketed_paste_uploads_the_clipboard_image() {
    let mut app = remote_app(&["t1"]);
    let mut rx = operations(&mut app);
    core_id(&mut app, "core-fake-1");
    let server = tokio::spawn(async move {
        let mut core = rx.recv().await.unwrap();
        core.read_hello().await;
        core.answer_hello(true, &["terminal.get", "terminal.put"], &[])
            .await;
        let get = core.read_request().await;
        core.reply(&get["id"], get_reply("t1", 0)).await;
        let put = core.read_request().await;
        assert_eq!(put["method"], "terminal.put");
        core.reply(&put["id"], json!({"path": "/p.png"})).await;
    });
    app.on_input(input::UiEvent::Paste(Vec::new()), 120);
    assert_eq!(app.pane.input_backlog().0, 0, "paste consumed");
    settle(&mut app).await;
    server.await.unwrap();
    assert_eq!(app.pane.input_backlog().0, 1);
}

#[tokio::test]
async fn empty_paste_with_no_image_stays_an_empty_paste() {
    let mut app = remote_app(&["t1"]);
    app.image_paste_reader = Box::new(no_image);
    app.on_input(input::UiEvent::Paste(Vec::new()), 120);
    assert_eq!(app.pane.input_backlog().0, 1, "empty paste forwards");
}

#[tokio::test]
async fn dropped_image_path_uploads_the_file_not_the_text() {
    let dir = scratch_dir("drop");
    let file = dir.join("shot one.png");
    std::fs::write(&file, PNG).unwrap();
    let mut app = remote_app(&["t1"]);
    let mut rx = operations(&mut app);
    core_id(&mut app, "core-fake-1");
    let server = tokio::spawn(async move {
        let mut core = rx.recv().await.unwrap();
        core.read_hello().await;
        core.answer_hello(true, &["terminal.get", "terminal.put"], &[])
            .await;
        let get = core.read_request().await;
        core.reply(&get["id"], get_reply("t1", 0)).await;
        let put = core.read_request().await;
        assert_eq!(
            put["params"]["data"],
            json!(crate::selection::base64_encode(PNG))
        );
        core.reply(&put["id"], json!({"path": "/remote/shot.png"}))
            .await;
    });
    let escaped = file.display().to_string().replace(' ', "\\ ");
    app.on_input(input::UiEvent::Paste(escaped.into_bytes()), 120);
    assert_eq!(app.pane.input_backlog().0, 0, "path paste consumed");
    settle(&mut app).await;
    server.await.unwrap();
    assert_eq!(app.pane.input_backlog().0, 1);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn an_unreadable_dropped_image_path_explains_and_forwards() {
    let mut app = remote_app(&["t1"]);
    let gone = "/tmp/hc-tui-test-definitely-missing.png";
    app.on_input(input::UiEvent::Paste(gone.as_bytes().to_vec()), 120);
    assert_eq!(
        app.pane.input_backlog().0,
        1,
        "the path text still forwards"
    );
    assert!(app.notice.is_some(), "the miss explains itself");
}

#[tokio::test]
async fn non_image_paste_text_forwards_verbatim() {
    let mut app = remote_app(&["t1"]);
    app.on_input(input::UiEvent::Paste(b"just some text".to_vec()), 120);
    app.on_input(input::UiEvent::Paste(b"relative/p.png".to_vec()), 120);
    app.on_input(input::UiEvent::Paste(b"/home/user/a.txt".to_vec()), 120);
    assert_eq!(app.pane.input_backlog().0, 3, "plain text pastes");
}

// ---- target policy -----------------------------------------------------

#[tokio::test]
async fn peek_has_no_target() {
    let mut app = remote_app(&["t1", "t2"]);
    app.open_peek("t2".into());
    press_key(&mut app, ctrl_v(Action::Press));
    assert_eq!(app.pending_actions, 0, "no upload while peeking");
}

#[tokio::test]
async fn an_armed_prefix_has_no_target() {
    let mut app = remote_app(&["t1"]);
    press_key(
        &mut app,
        term_key(Key::Char('b'), Mods::CTRL, Action::Press),
    );
    assert!(app.prefix.armed());
    press_key(&mut app, ctrl_v(Action::Press));
    assert_eq!(app.pending_actions, 0, "no upload under an armed prefix");
}

#[tokio::test]
async fn popup_terminal_is_the_target() {
    let mut app = remote_app(&["t1"]);
    app.host_reports_key_releases = true;
    let mut popup_pane = pane::Pane::new(app.pane_tx.clone(), app.open_attach.clone());
    popup_pane.want(Some(pane::Spec {
        terminal: "t9".into(),
        role: pane::Role::Active,
    }));
    app.popup = Some(popup::PopupState {
        pane: popup_pane,
        terminal: "t9".into(),
        width: None,
        height: None,
        owner_tab: None,
        seen: true,
    });
    let mut t9 = crate::sidebar::fixture::term("t9");
    t9.created_ms = 9;
    app.view
        .inventory
        .replace(2, vec![crate::sidebar::fixture::term("t1"), t9]);
    let mut rx = operations(&mut app);
    core_id(&mut app, "core-fake-1");
    let server = tokio::spawn(async move {
        let mut core = rx.recv().await.unwrap();
        core.read_hello().await;
        core.answer_hello(true, &["terminal.get", "terminal.put"], &[])
            .await;
        let get = core.read_request().await;
        core.reply(&get["id"], get_reply("t9", 9)).await;
        let put = core.read_request().await;
        assert_eq!(put["params"]["terminal"], "t9");
        core.reply(&put["id"], json!({"path": "/p.png"})).await;
    });
    press_key(&mut app, ctrl_v(Action::Press));
    assert_eq!(app.pane.input_backlog().0, 0);
    settle(&mut app).await;
    server.await.unwrap();
    assert_eq!(app.popup.as_ref().unwrap().pane.input_backlog().0, 1);
    // The consumed press's release must not reach the popup either.
    press_key(&mut app, ctrl_v(Action::Release));
    assert_eq!(app.popup.as_ref().unwrap().pane.input_backlog().0, 1);
}

// ---- upload outcomes -----------------------------------------------------

#[tokio::test]
async fn focus_moving_during_upload_shows_the_path_instead_of_typing() {
    let mut app = remote_app(&["t1", "t2"]);
    let mut rx = operations(&mut app);
    core_id(&mut app, "core-fake-1");
    let server = tokio::spawn(async move {
        let mut core = rx.recv().await.unwrap();
        core.read_hello().await;
        core.answer_hello(true, &["terminal.get", "terminal.put"], &[])
            .await;
        let get = core.read_request().await;
        core.reply(&get["id"], get_reply("t1", 0)).await;
        let put = core.read_request().await;
        core.reply(&put["id"], json!({"path": "/home/user/i.png"}))
            .await;
    });
    press_key(&mut app, ctrl_v(Action::Press));
    // Focus moves to t2 before the put resolves.
    app.focused = Some("t2".into());
    settle(&mut app).await;
    server.await.unwrap();
    assert_eq!(app.pane.input_backlog().0, 0, "nothing typed into t2");
    assert!(app.notice.is_some(), "the path surfaces as a notice");
    let backend = draw_to(&mut app, 100, 30);
    let text = screen_lines(&backend).join("\n");
    assert!(text.contains("/home/user/i.png"), "{text}");
}

#[tokio::test]
async fn a_put_failure_surfaces_as_a_notice() {
    let mut app = remote_app(&["t1"]);
    let mut rx = operations(&mut app);
    core_id(&mut app, "core-fake-1");
    let server = tokio::spawn(async move {
        let mut core = rx.recv().await.unwrap();
        core.read_hello().await;
        core.answer_hello(true, &["terminal.get", "terminal.put"], &[])
            .await;
        let get = core.read_request().await;
        core.reply(&get["id"], get_reply("t1", 0)).await;
        let put = core.read_request().await;
        let payload = serde_json::to_vec(&json!({
            "jsonrpc": "2.0", "id": put["id"],
            "error": {"code": -32000, "message": "disk full"}
        }))
        .unwrap();
        core.send(&hc_proto::frame::Frame::data(0, payload).encode())
            .await;
    });
    press_key(&mut app, ctrl_v(Action::Press));
    settle(&mut app).await;
    server.await.unwrap();
    assert_eq!(app.pane.input_backlog().0, 0);
    assert!(app.notice.is_some(), "put failure shows a notice");
}

#[tokio::test]
async fn a_core_without_put_surfaces_as_a_notice() {
    let mut app = remote_app(&["t1"]);
    let mut rx = operations(&mut app);
    core_id(&mut app, "core-fake-1");
    let server = tokio::spawn(async move {
        let mut core = rx.recv().await.unwrap();
        core.read_hello().await;
        core.answer_hello(true, &["terminal.get"], &[]).await;
        assert!(
            core.try_read_frame().await.is_none(),
            "no call follows the capability check"
        );
    });
    press_key(&mut app, ctrl_v(Action::Press));
    settle(&mut app).await;
    server.await.unwrap();
    assert_eq!(app.pane.input_backlog().0, 0);
    assert!(app.notice.is_some());
}

#[tokio::test]
async fn oversized_image_consumes_with_a_notice_and_no_rpc() {
    let mut app = remote_app(&["t1"]);
    let mut rx = operations(&mut app);
    app.image_paste_reader = Box::new(big_image);
    press_key(&mut app, ctrl_v(Action::Press));
    assert_eq!(app.pane.input_backlog().0, 0, "the key is consumed");
    assert_eq!(app.pending_actions, 0, "no action submitted");
    assert!(app.notice.is_some(), "too-large notice");
    assert!(
        rx.try_recv().is_err(),
        "no connection is even opened for an oversized image"
    );
}

/// The real reader path: the stub tool serves more than the upload
/// bound of valid-looking PNG, so `linux::read` answers `TooLarge` —
/// the key is consumed with a notice instead of falling through as
/// ordinary Ctrl+V input.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn oversized_clipboard_image_consumes_with_a_notice_and_no_rpc() {
    let dir = stub_tools::StubDir::new();
    let mut bytes = PNG.to_vec();
    bytes.resize(crate::clipboard::MAX_CLIPBOARD_IMAGE_BYTES + 64, 0);
    let payload = dir.write("big.png", &bytes);
    dir.stub("wl-paste", &format!("cat \"{}\"", payload.display()));
    let env = dir.env();
    let mut app = remote_app(&["t1"]);
    let mut rx = operations(&mut app);
    app.image_paste_reader =
        Box::new(move || crate::clipboard::linux::read(&env, std::time::Duration::from_secs(15)));
    press_key(&mut app, ctrl_v(Action::Press));
    assert_eq!(app.pane.input_backlog().0, 0, "the key is consumed");
    assert_eq!(app.pending_actions, 0, "no action submitted");
    assert!(app.notice.is_some(), "too-large notice");
    assert!(
        rx.try_recv().is_err(),
        "no connection is even opened for an oversized image"
    );
}

#[tokio::test]
async fn dropped_oversized_image_consumes_with_a_notice_and_no_rpc() {
    let dir = scratch_dir("drop-big");
    let file = dir.join("big.png");
    let mut bytes = PNG.to_vec();
    bytes.resize(crate::clipboard::MAX_CLIPBOARD_IMAGE_BYTES + 1, 0);
    std::fs::write(&file, &bytes).unwrap();
    let mut app = remote_app(&["t1"]);
    let mut rx = operations(&mut app);
    app.on_input(
        input::UiEvent::Paste(file.display().to_string().into_bytes()),
        120,
    );
    assert_eq!(app.pane.input_backlog().0, 0, "the paste is consumed");
    assert_eq!(app.pending_actions, 0, "no action submitted");
    assert!(app.notice.is_some(), "too-large notice");
    assert!(
        rx.try_recv().is_err(),
        "no connection is even opened for an oversized file"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// A scratch dir of PATH stubs for tests that run the real reader —
/// the same ETXTBSY probe discipline as `clipboard::tests`.
#[cfg(target_os = "linux")]
mod stub_tools {
    use std::path::{Path, PathBuf};

    pub(super) struct StubDir(PathBuf);

    impl StubDir {
        pub(super) fn new() -> Self {
            Self(super::scratch_dir("image-paste-test"))
        }

        pub(super) fn write(&self, name: &str, bytes: &[u8]) -> PathBuf {
            let path = self.0.join(name);
            std::fs::write(&path, bytes).unwrap();
            path
        }

        pub(super) fn stub(&self, name: &str, body: &str) {
            let path = self.0.join(name);
            std::fs::write(
                &path,
                format!("#!/bin/sh\n[ -n \"$STUB_PROBE\" ] && exit 0\n{body}\n"),
            )
            .unwrap();
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
            wait_executable(&path);
        }

        pub(super) fn env(&self) -> crate::clipboard::ClipboardEnv {
            crate::clipboard::ClipboardEnv {
                wayland: true,
                path: Some(format!("{}:/usr/bin:/bin", self.0.display()).into()),
                ..Default::default()
            }
        }
    }

    impl Drop for StubDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn wait_executable(path: &Path) {
        for _ in 0..200 {
            match std::process::Command::new(path)
                .env("STUB_PROBE", "1")
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
            {
                Ok(_) => return,
                Err(e) if e.kind() == std::io::ErrorKind::ExecutableFileBusy => {
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
                Err(e) => panic!("cannot exec stub {}: {e}", path.display()),
            }
        }
        panic!("stub {} never became executable", path.display());
    }
}
