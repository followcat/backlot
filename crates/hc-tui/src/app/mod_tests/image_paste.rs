//! R9 clipboard-image bridge tests: triggers, target policy, leases,
//! and the `terminal.put` upload path against `testcore::FakeCore`.
use super::*;
use crate::testcore;
use hc_vt::{Action, Key, Mods};
use serde_json::json;

pub(super) const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\x00\x00\x00\x0dIHDR";

pub(super) fn png_image() -> crate::clipboard::ClipboardRead {
    crate::clipboard::ClipboardRead::Image(crate::clipboard::ClipboardImage {
        bytes: PNG.to_vec(),
        extension: "png",
    })
}

pub(super) fn big_image() -> crate::clipboard::ClipboardRead {
    crate::clipboard::ClipboardRead::Image(crate::clipboard::ClipboardImage {
        bytes: vec![0u8; crate::clipboard::MAX_CLIPBOARD_IMAGE_BYTES + 1],
        extension: "png",
    })
}

pub(super) fn no_image() -> crate::clipboard::ClipboardRead {
    crate::clipboard::ClipboardRead::None
}

/// An owned scratch dir for one test: the per-process counter keeps
/// parallel tests apart, and `create_dir` claims the name atomically —
/// a leftover directory is retried, never adopted.
pub(super) fn scratch_dir(tag: &str) -> std::path::PathBuf {
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

pub(super) fn ctrl_v(action: Action) -> input::TerminalKey {
    term_key(Key::Char('v'), Mods::CTRL, action)
}

pub(super) fn ctrl_shift_v(action: Action) -> input::TerminalKey {
    term_key(Key::Char('v'), Mods::CTRL.union(Mods::SHIFT), action)
}

pub(super) fn pending(app: &mut App) {
    app.open_attach = Arc::new(|_| Box::pin(std::future::pending()));
    app.pane = pane::Pane::new(app.pane_tx.clone(), app.open_attach.clone());
}

pub(super) fn operations(app: &mut App) -> mpsc::Receiver<testcore::FakeCore> {
    let (accepted, rx) = mpsc::channel(4);
    let (rpc, _sub) = testcore::openers(accepted);
    app.set_operations(rpc);
    rx
}

pub(super) async fn settle(app: &mut App) {
    let result = tokio::time::timeout(
        testcore::DEADLINE,
        App::operation_result(&mut app.operations),
    )
    .await
    .unwrap();
    app.on_operation(result);
}

pub(super) fn remote_app(addresses: &[&str]) -> App {
    let mut app = app_with(addresses);
    pending(&mut app);
    app.set_image_paste_context(true, false);
    app.image_paste_reader = Box::new(png_image);
    app.reconcile_published_view();
    app
}

pub(super) fn get_reply(address: &str, created_ms: u64) -> serde_json::Value {
    let mut term = testcore::minimal_terminal();
    term["address"] = json!(address);
    term["created_ms"] = json!(created_ms);
    term
}

/// The core id the operations store guards on must match `view.core`.
pub(super) fn core_id(app: &mut App, id: &str) {
    app.view.core.as_mut().unwrap().id = id.into();
}

pub(super) fn press_key(app: &mut App, key: input::TerminalKey) {
    app.on_input(input::UiEvent::Key(key), 120);
}

// ---- triggers -------------------------------------------------------

#[tokio::test]
pub(super) async fn ctrl_v_press_starts_an_upload_and_is_consumed() {
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
