//! Synchronized output at the pane: a program's frame reaches the VT only
//! once it closes, so every screen the draw layer reads is a whole frame.
//! The bounded release and the paths that must not be held are here too.

use super::*;

const HOLD: Duration = sync_output::SYNC_OUTPUT_TIMEOUT;

/// The screen without the padding, so a frame compares as text.
fn screen_text(pane: &mut Pane) -> String {
    vt_text(pane).trim_end().to_string()
}

/// A painted active pane with one known frame on screen.
async fn painted_active() -> (Pane, mpsc::Receiver<PaneEv>, FakeCore) {
    let (mut pane, mut rx, mut accept) = rig();
    pane.want(Some(spec(Role::Active)));
    let mut core = serve(&mut accept, true).await;
    step(&mut pane, &mut rx).await;
    paint(&mut pane, &mut rx, &mut core, b"complete frame one").await;
    (pane, rx, core)
}

#[tokio::test]
async fn a_half_written_frame_never_reaches_the_vt() {
    let (mut pane, mut rx, mut core) = painted_active().await;
    let before = screen_text(&mut pane);
    let before_cursor = pane.vt().unwrap().frame(0).unwrap().cursor.position;

    // The frame: erase, then the two halves of its row, three chunks.
    raw_paint(&mut pane, &mut rx, &mut core, b"\x1b[?2026h\x1b[H\x1b[2J").await;
    assert_eq!(before, "complete frame one");
    assert_eq!(screen_text(&mut pane), before, "the erase must not paint");
    raw_paint(&mut pane, &mut rx, &mut core, b"half written").await;
    assert_eq!(
        screen_text(&mut pane),
        before,
        "the first half must not paint"
    );
    assert_eq!(
        pane.vt().unwrap().frame(0).unwrap().cursor.position,
        before_cursor,
        "the cursor holds with the frame"
    );
    raw_paint(&mut pane, &mut rx, &mut core, b" frame\x1b[?2026l").await;
    assert_eq!(screen_text(&mut pane), "half written frame");
}

#[tokio::test]
async fn two_frames_in_one_chunk_paint_the_first_one_whole() {
    let (mut pane, mut rx, mut core) = painted_active().await;
    // A paced observer's flush can merge frames: the reset inside the
    // chunk must still paint the complete first frame.
    raw_paint(
        &mut pane,
        &mut rx,
        &mut core,
        b"\x1b[?2026h\x1b[H\x1b[2Jframe A\x1b[?2026l\x1b[?2026hframe B",
    )
    .await;
    assert_eq!(screen_text(&mut pane), "frame A");
    raw_paint(&mut pane, &mut rx, &mut core, b" done\x1b[?2026l").await;
    assert_eq!(screen_text(&mut pane), "frame Aframe B done");
}

#[tokio::test]
async fn the_first_paint_with_its_leading_reset_is_not_held() {
    let (mut pane, mut rx, mut accept) = rig();
    pane.want(Some(spec(Role::Active)));
    let mut core = serve(&mut accept, true).await;
    step(&mut pane, &mut rx).await;
    // The core's first paint ends any frame with `?2026l` before the
    // screen; a reset with no frame open is plain bytes.
    paint(
        &mut pane,
        &mut rx,
        &mut core,
        b"\x18\x1b[!p\x1b[?2026lfirst paint",
    )
    .await;
    assert!(screen_text(&mut pane).contains("first paint"));
}

#[tokio::test]
async fn a_frame_that_never_closes_paints_after_the_bound() {
    let (mut pane, mut rx, mut core) = painted_active().await;
    core.send_stream(CH, Kind::Data, b"\x1b[?2026hstuck frame")
        .await;
    step(&mut pane, &mut rx).await;
    assert!(!screen_text(&mut pane).contains("stuck frame"));
    let due = pane.next_wake().expect("a held frame arms the loop");
    assert!(due > Instant::now(), "the wake is one bound ahead");
    // Short of the bound nothing paints.
    assert!(!pane.fire_retry(Instant::now()));
    assert!(!screen_text(&mut pane).contains("stuck frame"));
    assert!(pane.fire_retry(Instant::now() + HOLD));
    assert!(screen_text(&mut pane).contains("stuck frame"));
    assert!(pane.next_wake().is_none(), "the bound disarmed the wake");
}

#[tokio::test]
async fn a_program_without_sync_output_paints_every_chunk() {
    let (mut pane, mut rx, mut core) = painted_active().await;
    raw_paint(&mut pane, &mut rx, &mut core, b"\x1b[H\x1b[2Jplain one").await;
    assert_eq!(screen_text(&mut pane), "plain one");
    raw_paint(&mut pane, &mut rx, &mut core, b" plain two").await;
    assert_eq!(screen_text(&mut pane), "plain one plain two");
}

#[tokio::test]
async fn a_new_wire_drops_a_held_frame_instead_of_painting_it() {
    let (mut pane, mut rx, mut accept) = rig();
    pane.want(Some(spec(Role::Active)));
    let mut core = serve(&mut accept, true).await;
    step(&mut pane, &mut rx).await;
    paint(&mut pane, &mut rx, &mut core, b"complete frame one").await;
    core.send_stream(CH, Kind::Data, b"\x1b[?2026horphan").await;
    step(&mut pane, &mut rx).await;
    assert!(pane.next_wake().is_some(), "the hold armed the loop");

    // The wire is replaced mid-frame: the half frame belongs to the old
    // program, so the new wire paints only its own bytes.
    pane.want(None);
    pane.want(Some(spec(Role::Active)));
    let (mut core2, _params) = timeout(T, accept.recv()).await.unwrap().unwrap();
    core2
        .serve_attach(testcore::attach_result(
            CH,
            testcore::minimal_terminal(),
            true,
        ))
        .await;
    step_until_live(&mut pane, &mut rx).await;
    paint(&mut pane, &mut rx, &mut core2, b"fresh wire").await;
    assert_eq!(screen_text(&mut pane), "fresh wire");
}
