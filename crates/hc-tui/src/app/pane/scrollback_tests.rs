use super::*;

/// Every role that builds a VT takes its budget from
/// `[advanced] scrollback_limit_bytes`: the focused viewer and a cached
/// observer keep the *same* history, so a tab switch back finds what it
/// left instead of a role-sized slice of it.
#[test]
fn every_vt_takes_the_configured_scrollback_budget() {
    let (mut pane, _rx, _accept) = rig();
    assert_eq!(
        pane.budgets(),
        (
            crate::config::DEFAULT_ATTACH_HISTORY_LINES,
            crate::config::DEFAULT_SCROLLBACK_LIMIT_BYTES
        )
    );
    pane.set_scrollback_bytes(2_000_000);
    pane.set_attach_history(700);
    assert_eq!(pane.budgets(), (700, 2_000_000));
}

/// Feeds `lines` numbered rows through the wire and reports the
/// retained buffer size plus the first retained line index.
async fn feed_numbered_lines(
    pane: &mut Pane,
    rx: &mut mpsc::Receiver<PaneEv>,
    core: &mut FakeCore,
    lines: usize,
) -> (usize, usize) {
    let mut sent = 0usize;
    while sent < lines {
        let end = (sent + 400).min(lines);
        let mut payload = Vec::new();
        for i in sent..end {
            payload.extend_from_slice(
                format!("line {i:05} padding padding padding padding\r\n").as_bytes(),
            );
        }
        sent = end;
        raw_paint(pane, rx, core, &payload).await;
    }
    let vt = pane.vt().expect("a painted pane keeps a VT");
    let first_row = vt
        .screen_text_rows_range(0, 1)
        .unwrap()
        .into_iter()
        .next()
        .map(|row| {
            row.cells
                .iter()
                .filter_map(|cell| cell.graphemes.first().and_then(|g| char::from_u32(*g)))
                .collect::<String>()
        })
        .unwrap_or_default();
    let first = first_row
        .strip_prefix("line ")
        .and_then(|rest| rest.split(' ').next())
        .and_then(|index| index.parse::<usize>().ok())
        .expect("the retained prefix is a numbered line");
    (vt.total_rows().unwrap(), first)
}

#[tokio::test]
async fn observer_vt_reports_its_cap_and_evicts_under_load() {
    // A small budget keeps the feed short; the point is that the pane's
    // configured cap reaches libghostty and prunes.
    const BUDGET: usize = 256 * 1024;
    let (mut pane, mut rx, mut accept) = rig();
    pane.set_scrollback_bytes(BUDGET);
    pane.want(Some(spec(Role::Observe)));
    let mut core = serve(&mut accept, true).await;
    step(&mut pane, &mut rx).await;
    paint(&mut pane, &mut rx, &mut core, b"paint").await;
    // The constructed VT carries the configured cap.
    assert_eq!(pane.vt().unwrap().scrollback_max_bytes(), Some(BUDGET));
    // ~124 KiB of numbered lines overflows the byte cap: the earliest
    // rows are evicted and the buffer stays bounded rather than
    // growing with the feed.
    let (obs_total, obs_first) = feed_numbered_lines(&mut pane, &mut rx, &mut core, 2000).await;
    assert!(
        obs_first > 0,
        "observer scrollback must evict its earliest rows, first={obs_first}"
    );
    // The active viewer gets the same configured budget — a cached tab
    // must not keep less history than the focused one.
    let (mut active, mut rx2, mut accept2) = rig();
    active.set_scrollback_bytes(BUDGET);
    active.want(Some(spec(Role::Active)));
    let mut core2 = serve(&mut accept2, true).await;
    step(&mut active, &mut rx2).await;
    paint(&mut active, &mut rx2, &mut core2, b"paint").await;
    assert_eq!(active.vt().unwrap().scrollback_max_bytes(), Some(BUDGET));
    let (act_total, act_first) = feed_numbered_lines(&mut active, &mut rx2, &mut core2, 2000).await;
    assert_eq!(obs_total, act_total);
    assert_eq!(obs_first, act_first);
}

#[tokio::test]
async fn same_terminal_role_change_keeps_the_screen_and_rebuilds_the_vt() {
    let (mut pane, mut rx, mut accept) = rig();
    pane.want(Some(spec(Role::Observe)));
    let mut core = serve(&mut accept, true).await;
    step(&mut pane, &mut rx).await;
    paint(&mut pane, &mut rx, &mut core, b"kept screen").await;
    assert_eq!(
        pane.vt().unwrap().scrollback_max_bytes(),
        Some(crate::config::DEFAULT_SCROLLBACK_LIMIT_BYTES)
    );
    // An Observe→Active change on the same terminal retains the last
    // paint; the fresh attach's first DATA builds a new VT under the
    // configured cap.
    pane.want(Some(spec(Role::Active)));
    assert!(
        pane.vt().is_some(),
        "the retained screen must survive a same-terminal role change"
    );
    let (mut core2, params2) = timeout(T, accept.recv()).await.unwrap().unwrap();
    assert!(!params2.observe);
    core2
        .serve_attach(testcore::attach_result(
            CH,
            testcore::minimal_terminal(),
            true,
        ))
        .await;
    step_until_live(&mut pane, &mut rx).await;
    paint(&mut pane, &mut rx, &mut core2, b"fresh paint").await;
    assert_eq!(
        pane.vt().unwrap().scrollback_max_bytes(),
        Some(crate::config::DEFAULT_SCROLLBACK_LIMIT_BYTES)
    );
    assert!(vt_text(&mut pane).contains("fresh paint"));
}

/// A key while a pane's output still fits its screen must not pin the
/// viewport to the top: output that then outgrows the screen is
/// followed. Regression: the reset scrolled to `total - len`, which is
/// row 0 — the VT's top pin — until scrollback exists.
#[tokio::test]
async fn a_key_before_the_screen_fills_keeps_following_the_tail() {
    let (mut pane, mut rx, mut accept) = rig();
    pane.want(Some(spec(Role::Active)));
    let mut core = serve(&mut accept, true).await;
    step(&mut pane, &mut rx).await;
    paint(&mut pane, &mut rx, &mut core, b"prompt> ").await;
    pane.reset_scroll();
    let mut payload = Vec::new();
    for i in 0..200 {
        payload.extend_from_slice(format!("line {i}\r\n").as_bytes());
    }
    raw_paint(&mut pane, &mut rx, &mut core, &payload).await;
    let bar = pane.vt().unwrap().scrollbar().unwrap();
    assert!(bar.total > bar.len, "the output must outgrow the screen");
    assert_eq!(
        bar.offset,
        bar.total - bar.len,
        "the viewport must follow the tail, not stay on the head"
    );
}
