//! R3: image reads run off the event loop. While a clipboard or
//! dropped-file read is in flight the app keeps drawing and serving
//! every other channel; only terminal-directed input waits behind it,
//! in the order it arrived. A reader that never answers resolves the
//! gesture as a miss after the hold bound — and its late result lands
//! dead.
mod wire;

use super::image_paste::*;
use super::*;
use crate::testcore;
use hc_vt::Action;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use wire::*;

/// A blocked clipboard read must not stall the loop: draws keep
/// landing, and once the miss lands `^V` still reaches the terminal
/// ahead of the key typed while the read ran.
#[tokio::test]
async fn blocked_clipboard_read_keeps_drawing_and_orders_input_on_a_miss() {
    let (mut app, sub_rx, attach_rx) = run_app();
    let (reader, started, release) = gated_reader(no_image);
    app.image_paste_reader = Arc::new(reader);
    let probe = Arc::clone(&app.probe);
    tokio::spawn(serve_t1(sub_rx));
    let (attached_tx, attached_rx) = std::sync::mpsc::channel();
    let (input_tx, input_rx) = std::sync::mpsc::channel();
    tokio::spawn(serve_input_wire(attach_rx, attached_tx, input_tx));
    let (ui_tx, mut ui_rx) = mpsc::channel::<UiEvent>(64);
    let (_rep_tx, mut rep_rx) = mpsc::channel::<crate::host::HostReply>(4);
    let driver = std::thread::spawn(move || {
        attached_rx
            .recv_timeout(testcore::DEADLINE)
            .expect("the t1 attach");
        let (before, during) = trigger_key_resizes(
            &ui_tx,
            &probe,
            &started,
            UiEvent::Key(ctrl_v(Action::Press)),
        );
        release.wait();
        // The 'a' typed mid-read lands once the miss replays ^V first.
        (before, during, collect_until(&input_rx, b"a"))
    });
    let mut term = Terminal::new(ratatui::backend::TestBackend::new(80, 24)).unwrap();
    app.run(&mut term, &mut ui_rx, &mut rep_rx).await.unwrap();
    let (before, during, frames) = driver.join().unwrap();
    assert!(
        during > before,
        "draws kept landing while the read was blocked ({before} -> {during})"
    );
    let wire = wire(&frames);
    let v = find(&wire, b"\x16", 0).expect("the held ^V reaches the terminal");
    find(&wire, b"a", v + 1).expect("the held key follows the replayed ^V");
    assert!(app.notice.is_some(), "a miss explains itself");
    assert_eq!(app.pending_actions, 0);
}

/// The same block with an image: the upload starts, `^V` never reaches
/// the terminal, and the mid-read key lands behind the read's result.
#[tokio::test]
async fn blocked_clipboard_read_consumes_the_gesture_on_an_image() {
    let (mut app, sub_rx, attach_rx) = run_app();
    let (reader, started, release) = gated_reader(png_image);
    app.image_paste_reader = Arc::new(reader);
    let ops_rx = operations(&mut app);
    let probe = Arc::clone(&app.probe);
    tokio::spawn(serve_t1(sub_rx));
    let put_seen = Arc::new(AtomicU64::new(0));
    tokio::spawn(serve_ops(ops_rx, Arc::clone(&put_seen)));
    let (attached_tx, attached_rx) = std::sync::mpsc::channel();
    let (input_tx, input_rx) = std::sync::mpsc::channel();
    tokio::spawn(serve_input_wire(attach_rx, attached_tx, input_tx));
    let (ui_tx, mut ui_rx) = mpsc::channel::<UiEvent>(64);
    let (_rep_tx, mut rep_rx) = mpsc::channel::<crate::host::HostReply>(4);
    let driver = std::thread::spawn(move || {
        attached_rx
            .recv_timeout(testcore::DEADLINE)
            .expect("the t1 attach");
        let (before, during) = trigger_key_resizes(
            &ui_tx,
            &probe,
            &started,
            UiEvent::Key(ctrl_v(Action::Press)),
        );
        release.wait();
        wait_atomic(&put_seen, 1, "the terminal.put");
        (
            before,
            during,
            collect_until(&input_rx, b"/home/user/i.png"),
        )
    });
    let mut term = Terminal::new(ratatui::backend::TestBackend::new(80, 24)).unwrap();
    app.run(&mut term, &mut ui_rx, &mut rep_rx).await.unwrap();
    let (before, during, frames) = driver.join().unwrap();
    assert!(
        during > before,
        "draws kept landing while the read was blocked ({before} -> {during})"
    );
    let wire = wire(&frames);
    let a = find(&wire, b"a", 0).expect("the held key forwards");
    let path = find(&wire, b"/home/user/i.png", 0).expect("the upload path pastes");
    assert!(a < path, "the held key lands ahead of the upload paste");
    assert_eq!(find(&wire, b"\x16", 0), None, "^V is consumed");
    assert_eq!(app.pending_actions, 0);
}

/// A read that never returns must not hold input forever: after the
/// bound the gesture resolves as a miss, and the worker's late result
/// is dropped — no upload, no reordered input.
#[tokio::test]
async fn a_never_returning_read_resolves_as_a_miss_at_the_bound() {
    let (mut app, sub_rx, attach_rx) = run_app();
    app.image_read_hold = Duration::from_millis(400);
    let (reader, started, release) = gated_reader(png_image);
    app.image_paste_reader = Arc::new(reader);
    let ops_rx = operations(&mut app);
    tokio::spawn(serve_t1(sub_rx));
    let put_seen = Arc::new(AtomicU64::new(0));
    tokio::spawn(serve_ops(ops_rx, Arc::clone(&put_seen)));
    let (attached_tx, attached_rx) = std::sync::mpsc::channel();
    let (input_tx, input_rx) = std::sync::mpsc::channel();
    tokio::spawn(serve_input_wire(attach_rx, attached_tx, input_tx));
    let (ui_tx, mut ui_rx) = mpsc::channel::<UiEvent>(64);
    let (_rep_tx, mut rep_rx) = mpsc::channel::<crate::host::HostReply>(4);
    let driver = std::thread::spawn(move || {
        attached_rx
            .recv_timeout(testcore::DEADLINE)
            .expect("the t1 attach");
        ui_tx
            .blocking_send(UiEvent::Key(ctrl_v(Action::Press)))
            .unwrap();
        wait_atomic(&started, 1, "the image read to start");
        ui_tx.blocking_send(UiEvent::Key(a_press())).unwrap();
        // The bound resolves the miss on its own — no release needed.
        let mut frames = collect_until(&input_rx, b"a");
        // The worker now answers — its result must land dead.
        release.wait();
        std::thread::sleep(Duration::from_millis(400));
        while let Ok(frame) = input_rx.try_recv() {
            frames.push(frame);
        }
        frames
    });
    let mut term = Terminal::new(ratatui::backend::TestBackend::new(80, 24)).unwrap();
    app.run(&mut term, &mut ui_rx, &mut rep_rx).await.unwrap();
    let frames = driver.join().unwrap();
    let wire = wire(&frames);
    let v = find(&wire, b"\x16", 0).expect("the held ^V reaches the terminal");
    find(&wire, b"a", v + 1).expect("the held key follows the expired read");
    assert_eq!(
        find(&wire, b"/home/user/i.png", 0),
        None,
        "the late result never pastes"
    );
    assert_eq!(
        put_seen.load(Ordering::SeqCst),
        0,
        "the late result never reached terminal.put"
    );
    assert!(app.notice.is_some(), "the expired read explains itself");
    assert_eq!(app.pending_actions, 0);
}

/// A dropped image path takes the same off-loop read: draws keep
/// landing while the file read is blocked, and the held input follows
/// the read's result in order.
#[tokio::test]
async fn a_dropped_image_path_reads_off_the_loop_too() {
    let (mut app, sub_rx, attach_rx) = run_app();
    let (reader, started, release) = gated_drop_reader(|_, _| png_image());
    app.image_drop_reader = Arc::new(reader);
    let ops_rx = operations(&mut app);
    let probe = Arc::clone(&app.probe);
    tokio::spawn(serve_t1(sub_rx));
    let put_seen = Arc::new(AtomicU64::new(0));
    tokio::spawn(serve_ops(ops_rx, Arc::clone(&put_seen)));
    let (attached_tx, attached_rx) = std::sync::mpsc::channel();
    let (input_tx, input_rx) = std::sync::mpsc::channel();
    tokio::spawn(serve_input_wire(attach_rx, attached_tx, input_tx));
    let (ui_tx, mut ui_rx) = mpsc::channel::<UiEvent>(64);
    let (_rep_tx, mut rep_rx) = mpsc::channel::<crate::host::HostReply>(4);
    let driver = std::thread::spawn(move || {
        attached_rx
            .recv_timeout(testcore::DEADLINE)
            .expect("the t1 attach");
        let (before, during) = trigger_key_resizes(
            &ui_tx,
            &probe,
            &started,
            UiEvent::Paste(b"/home/user/dropped.png".to_vec()),
        );
        release.wait();
        wait_atomic(&put_seen, 1, "the terminal.put");
        (
            before,
            during,
            collect_until(&input_rx, b"/home/user/i.png"),
        )
    });
    let mut term = Terminal::new(ratatui::backend::TestBackend::new(80, 24)).unwrap();
    app.run(&mut term, &mut ui_rx, &mut rep_rx).await.unwrap();
    let (before, during, frames) = driver.join().unwrap();
    assert!(
        during > before,
        "draws kept landing while the dropped-file read was blocked ({before} -> {during})"
    );
    let wire = wire(&frames);
    let a = find(&wire, b"a", 0).expect("the held key forwards");
    let path = find(&wire, b"/home/user/i.png", 0).expect("the upload path pastes");
    assert!(a < path, "the held key lands ahead of the upload paste");
    assert_eq!(app.pending_actions, 0);
}
