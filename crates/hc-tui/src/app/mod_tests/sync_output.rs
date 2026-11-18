//! A synchronized frame reaches the screen only when it closes. Asserted
//! through the real draw path: what `draw` renders after a holding chunk
//! is the previous complete frame, not the half-written one.

use super::pane_cache::{drive_until, next_attach, serve, visit, with_openers};
use super::*;
use crate::testcore;
use hc_proto::client::Output;
use hc_proto::frame::Kind;
use tokio::time::timeout;

/// Feeds one DATA chunk and drives the loop until the pane has it.
async fn pump_data(app: &mut App, bytes: &[u8]) {
    timeout(testcore::DEADLINE, async {
        loop {
            let event = app.pane_rx.recv().await.expect("pane event");
            let hit = matches!(
                &event,
                PaneEv::Out { out: Output::Data(data), .. } if data.as_slice() == bytes
            );
            app.pane_event(event, Instant::now());
            if hit {
                return;
            }
        }
    })
    .await
    .expect("data deadline");
}

fn drawn(app: &mut App) -> String {
    screen_lines(&draw_to(app, 120, 40)).join("\n")
}

#[tokio::test]
async fn a_synchronized_frame_reaches_the_screen_only_when_it_closes() {
    let mut app = app_with(&["t1"]);
    let mut requests = with_openers(&mut app);
    visit(&mut app, "t1");
    let (core, params) = next_attach(&mut app, &mut requests).await;
    let mut core = serve(core, &params, 1).await;
    core.send_stream(1, Kind::Data, b"\x1b[?9001lFRAME ONE")
        .await;
    drive_until(&mut app, |app| app.pane.has_fresh_paint()).await;
    assert!(drawn(&mut app).contains("FRAME ONE"));

    // The program opens a frame and erases the screen: what the user
    // sees must stay the complete previous frame.
    core.send_stream(1, Kind::Data, b"\x1b[?2026h\x1b[H\x1b[2J")
        .await;
    pump_data(&mut app, b"\x1b[?2026h\x1b[H\x1b[2J").await;
    assert!(
        drawn(&mut app).contains("FRAME ONE"),
        "an open frame must not paint its erase"
    );

    // Its first half, still inside the frame.
    core.send_stream(1, Kind::Data, b"FRAME TWO half").await;
    pump_data(&mut app, b"FRAME TWO half").await;
    let screen = drawn(&mut app);
    assert!(screen.contains("FRAME ONE"), "{screen}");
    assert!(!screen.contains("FRAME TWO"), "{screen}");

    // The close: now the whole frame paints at once.
    core.send_stream(1, Kind::Data, b" and the rest\x1b[?2026l")
        .await;
    pump_data(&mut app, b" and the rest\x1b[?2026l").await;
    let screen = drawn(&mut app);
    assert!(screen.contains("FRAME TWO half and the rest"), "{screen}");
    assert!(!screen.contains("FRAME ONE"), "{screen}");
}
