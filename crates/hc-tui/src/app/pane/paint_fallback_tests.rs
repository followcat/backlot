//! A core that omits `?9001h/l` must not hold the paint gate forever:
//! the bounded fallback releases queued input and the retained screen.
use super::*;

#[tokio::test]
async fn missing_paint_marker_unblocks_after_deadline_and_flushes_input() {
    let (mut pane, mut rx, mut accept) = rig();
    pane.want(Some(spec(Role::Active)));
    let mut core = serve(&mut accept, true).await;
    step_until_live(&mut pane, &mut rx).await;
    pane.key(&key('x'));
    // One small DATA chunk without `?9001`: the gate stays shut and
    // the fallback clock arms — an idle shell sends nothing more.
    raw_paint(&mut pane, &mut rx, &mut core, b"prompt$ ").await;
    assert!(!pane.is_painted());
    assert!(!pane.has_fresh_paint());
    assert_eq!(pane.notice().map(|n| n.title), Some("Terminal loading"));
    no_kind(&mut core, CH, &[Kind::Data], 40).await;
    let deadline = pane.next_wake().expect("paint fallback deadline");
    assert!(!pane.fire_retry(deadline - Duration::from_millis(1)));
    assert!(pane.fire_retry(deadline));
    assert!(pane.is_painted());
    assert!(pane.has_fresh_paint());
    assert!(pane.notice().is_none());
    assert_eq!(pane.paint_fallback(), Some("paint marker omitted"));
    assert!(
        pane.status_label(deadline).contains("paint marker omitted"),
        "status: {}",
        pane.status_label(deadline)
    );
    assert!(pane.next_wake().is_none());
    assert_eq!(read_kind(&mut core, CH, Kind::Data).await.payload, b"x");
}

#[tokio::test]
async fn missing_paint_marker_unblocks_on_byte_cap() {
    let (mut pane, mut rx, mut accept) = rig();
    pane.want(Some(spec(Role::Active)));
    let mut core = serve(&mut accept, true).await;
    step_until_live(&mut pane, &mut rx).await;
    pane.key(&key('k'));
    let chunk = vec![b'y'; 64 * 1024];
    for held in 0..3 {
        raw_paint(&mut pane, &mut rx, &mut core, &chunk).await;
        assert!(!pane.is_painted(), "gate released early at chunk {held}");
    }
    // The fourth chunk crosses 256 KiB without `?9001`: the gate
    // releases at once — no timer wait needed.
    raw_paint(&mut pane, &mut rx, &mut core, &chunk).await;
    assert!(pane.is_painted());
    assert!(pane.has_fresh_paint());
    assert!(pane.notice().is_none());
    assert_eq!(pane.paint_fallback(), Some("paint marker omitted"));
    assert!(pane.next_wake().is_none());
    assert_eq!(read_kind(&mut core, CH, Kind::Data).await.payload, b"k");
}

#[tokio::test]
async fn observer_resize_repaint_without_marker_recovers_on_fallback() {
    let (mut pane, mut rx, mut accept) = rig();
    pane.want(Some(spec(Role::Observe)));
    let mut core = serve(&mut accept, true).await;
    step_until_live(&mut pane, &mut rx).await;
    paint(&mut pane, &mut rx, &mut core, b"init").await;
    assert!(pane.has_fresh_paint());
    core.send_stream(CH, Kind::Resize, &[60, 0, 20, 0]).await;
    while pane.vt_grid != (60, 20) {
        step(&mut pane, &mut rx).await;
    }
    // Five bytes stay held in the repaint tail waiting for `?9001`.
    raw_paint(&mut pane, &mut rx, &mut core, b"hello").await;
    assert!(!pane.has_fresh_paint());
    let deadline = pane.next_wake().expect("repaint fallback deadline");
    assert!(pane.fire_retry(deadline));
    assert!(!pane.fire_retry(deadline));
    assert!(pane.has_fresh_paint());
    assert_eq!(pane.paint_fallback(), Some("paint marker omitted"));
    assert!(vt_text(&mut pane).contains("hello"));
}
