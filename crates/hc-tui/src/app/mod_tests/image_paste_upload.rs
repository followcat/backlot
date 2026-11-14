use super::image_paste::*;
use super::*;
use hc_vt::{Action, Key, Mods};
use serde_json::json;

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
        persistent: false,
        key: None,
        created_ms: 9,
        seen: true,
    });
    let mut t9 = crate::sidebar::fixture::term("t9");
    t9.created_ms = 9;
    app.view
        .inventory_mut()
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
