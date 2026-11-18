//! The fake-core wires and drivers shared by the off-loop image-read
//! tests: an attach wire that records pane input, gated readers that
//! park inside the blocking worker, and thread-side poll helpers.
use super::super::image_paste::*;
use super::super::*;
use crate::testcore;
use hc_vt::{Action, Key, Mods};
use serde_json::json;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

/// The attach channel the fake wires negotiate.
const ATTACH_CH: u32 = 7;

fn t1_terminal() -> serde_json::Value {
    let mut t1 = testcore::minimal_terminal();
    t1["address"] = json!("t1");
    t1["labels"] = json!({"herdr.space": "space-t1", "herdr.tab": "tab-t1"});
    t1
}

/// The subscribe snapshot that focuses `t1` — the run-loop counterpart
/// of `remote_app(&["t1"])`.
fn t1_snapshot() -> serde_json::Value {
    json!({"seq": 1, "terminals": [t1_terminal()]})
}

/// An app over the fake openers — the driver serves its subscription
/// and its attach so `run` reaches a focused, live pane for real.
/// Pane input lands on the attach wire, where the test reads it back.
pub(super) fn run_app() -> (
    App,
    mpsc::Receiver<testcore::FakeCore>,
    mpsc::Receiver<testcore::FakeCore>,
) {
    let (sub_accepted, sub_rx) = mpsc::channel(4);
    let (_rpc, sub) = testcore::openers(sub_accepted);
    // Refreshes get their own conn class: answer everything, keep none.
    let (rpc_accepted, mut rpc_rx) = mpsc::channel(4);
    let (rpc, _sub) = testcore::openers(rpc_accepted);
    tokio::spawn(async move {
        while let Some(mut core) = rpc_rx.recv().await {
            // serve_rpc picks up after the hello, like spawn_fake.
            core.read_hello().await;
            tokio::spawn(testcore::serve_rpc(core));
        }
    });
    let watcher = Watcher::with_openers(
        rpc,
        sub,
        hc_proto::watch::WatchParams {
            traversal: Some(crate::project::agent_traversal),
            ..hc_proto::watch::WatchParams::default()
        },
    );
    let (attach_accepted, attach_rx) = mpsc::channel(4);
    let open: OpenAttach = Arc::new(move |params| {
        let attach_accepted = attach_accepted.clone();
        Box::pin(async move {
            let (core, read, write) = testcore::pair(64 * 1024);
            attach_accepted.send(core).await.unwrap();
            hc_proto::client::Attach::open(read, write, None, &params).await
        })
    });
    let config = crate::config::Config::default();
    let live = config.live_keybinds();
    let shell = crate::chrome::ClientShellConfig::from_config(
        &config,
        None,
        None,
        std::path::PathBuf::new(),
    );
    let mut app = App::new(watcher, open, live, shell, Arc::new(Probe::default()));
    app.set_image_paste_context(true, false);
    (app, sub_rx, attach_rx)
}

/// Serves the one attach the app opens: the handshake, the `?9001`
/// paint marker so human input ungates, then every input DATA frame
/// the writer puts on the wire forwards to `input`.
pub(super) async fn serve_input_wire(
    mut attach_rx: mpsc::Receiver<testcore::FakeCore>,
    attached: std::sync::mpsc::Sender<()>,
    input: std::sync::mpsc::Sender<Vec<u8>>,
) {
    let mut core = attach_rx.recv().await.unwrap();
    core.serve_attach(testcore::attach_result(ATTACH_CH, t1_terminal(), true))
        .await;
    core.send_stream(ATTACH_CH, hc_proto::frame::Kind::Data, b"\x1b[?9001h$ ")
        .await;
    let _ = attached.send(());
    while let Some(frame) = core.try_read_frame().await {
        if frame.channel == ATTACH_CH && frame.kind == hc_proto::frame::Kind::Data {
            let _ = input.send(frame.payload.to_vec());
        }
    }
}

/// A clipboard reader that marks its start and parks on `release` —
/// a two-party barrier — before answering `read`.
pub(super) fn gated_reader(
    read: fn() -> crate::clipboard::ClipboardRead,
) -> (
    impl Fn() -> crate::clipboard::ClipboardRead + Send + Sync + 'static,
    Arc<AtomicU64>,
    Arc<std::sync::Barrier>,
) {
    let started = Arc::new(AtomicU64::new(0));
    let release = Arc::new(std::sync::Barrier::new(2));
    let reader_started = Arc::clone(&started);
    let reader_release = Arc::clone(&release);
    (
        move || {
            reader_started.fetch_add(1, Ordering::SeqCst);
            reader_release.wait();
            read()
        },
        started,
        release,
    )
}

/// The same gate for the dropped-file reader.
pub(super) fn gated_drop_reader(
    read: fn(&std::path::Path, &'static str) -> crate::clipboard::ClipboardRead,
) -> (
    impl Fn(&std::path::Path, &'static str) -> crate::clipboard::ClipboardRead + Send + Sync + 'static,
    Arc<AtomicU64>,
    Arc<std::sync::Barrier>,
) {
    let started = Arc::new(AtomicU64::new(0));
    let release = Arc::new(std::sync::Barrier::new(2));
    let reader_started = Arc::clone(&started);
    let reader_release = Arc::clone(&release);
    (
        move |path: &std::path::Path, extension: &'static str| {
            reader_started.fetch_add(1, Ordering::SeqCst);
            reader_release.wait();
            read(path, extension)
        },
        started,
        release,
    )
}

/// Polls an app-side counter from the driver thread. The drivers run
/// on real threads, not runtime tasks: the point of the tests is that
/// an injected reader may block the whole runtime — the release and
/// the observations must not depend on its scheduling.
pub(super) fn wait_atomic(counter: &AtomicU64, at_least: u64, what: &str) {
    let deadline = std::time::Instant::now() + testcore::DEADLINE;
    while counter.load(Ordering::SeqCst) < at_least {
        assert!(
            std::time::Instant::now() < deadline,
            "timed out waiting for {what}"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// Serve the subscription: t1's snapshot, then hold the conn open —
/// dropping it ends the link and every later resubscribe just spends
/// the fake-accept backoff.
pub(super) async fn serve_t1(mut sub_rx: mpsc::Receiver<testcore::FakeCore>) {
    let mut core = sub_rx.recv().await.unwrap();
    core.read_hello().await;
    testcore::serve_subscribe(&mut core, t1_snapshot()).await;
    std::future::pending::<()>().await;
}

/// Serve every operations conn generically: `terminal.put` bumps
/// `put_seen` and answers the fixed path. Each conn gets its own task —
/// the seen/waiting workers share this opener with the action store,
/// and a long-lived worker conn must not starve the action's conn.
pub(super) async fn serve_ops(
    mut ops_rx: mpsc::Receiver<testcore::FakeCore>,
    put_seen: Arc<AtomicU64>,
) {
    while let Some(mut core) = ops_rx.recv().await {
        let put_seen = Arc::clone(&put_seen);
        tokio::spawn(async move {
            core.read_hello().await;
            core.answer_hello(
                true,
                &[
                    "terminal.get",
                    "terminal.put",
                    "terminal.label",
                    "doc.get",
                    "doc.set",
                ],
                &[],
            )
            .await;
            while let Some(frame) = core.try_read_frame().await {
                if frame.channel != 0 || frame.kind != hc_proto::frame::Kind::Data {
                    continue;
                }
                let req: serde_json::Value = serde_json::from_slice(&frame.payload).unwrap();
                let result = match req["method"].as_str() {
                    Some("terminal.get") => get_reply("t1", 1700000000000),
                    Some("terminal.put") => {
                        put_seen.fetch_add(1, Ordering::SeqCst);
                        json!({"path": "/home/user/i.png"})
                    }
                    _ => json!({}),
                };
                core.reply(&req["id"], result).await;
            }
        });
    }
}

pub(super) fn a_press() -> input::TerminalKey {
    term_key(Key::Char('a'), Mods::NONE, Action::Press)
}

/// Everything the attach wire carried, concatenated, in frame order.
pub(super) fn wire(frames: &[Vec<u8>]) -> Vec<u8> {
    frames.concat()
}

/// The first `needle` at or after `from` in `hay`.
pub(super) fn find(hay: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    hay.get(from..)?
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|i| i + from)
}

/// Receives wire frames until the concatenated stream contains
/// `until`; returns every frame seen so far.
pub(super) fn collect_until(
    input_rx: &std::sync::mpsc::Receiver<Vec<u8>>,
    until: &[u8],
) -> Vec<Vec<u8>> {
    let mut frames = vec![];
    let deadline = std::time::Instant::now() + testcore::DEADLINE;
    while find(&wire(&frames), until, 0).is_none() {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        frames.push(
            input_rx.recv_timeout(left).unwrap_or_else(|_| {
                panic!("timed out waiting for {until:?} on the wire: {frames:?}")
            }),
        );
    }
    frames
}

/// The trigger plus a key typed mid-read, then resizes that must each
/// draw while the reader is still blocked. Returns (before, during)
/// draw counts; the caller asserts they advanced.
pub(super) fn trigger_key_resizes(
    ui_tx: &mpsc::Sender<UiEvent>,
    probe: &Probe,
    started: &AtomicU64,
    trigger: UiEvent,
) -> (u64, u64) {
    ui_tx.blocking_send(trigger).unwrap();
    wait_atomic(started, 1, "the image read to start");
    ui_tx.blocking_send(UiEvent::Key(a_press())).unwrap();
    let before = probe.draws.load(Ordering::SeqCst);
    for _ in 0..5 {
        ui_tx
            .blocking_send(UiEvent::Resize { cols: 80, rows: 24 })
            .unwrap();
        std::thread::sleep(Duration::from_millis(40));
    }
    std::thread::sleep(Duration::from_millis(100));
    (before, probe.draws.load(Ordering::SeqCst))
}
