use super::*;

/// The VT scrollback budget is role-scoped: cached observers — which
/// outnumber visible panes by the off-tab LRU budget — get the tight
/// cap; the focused viewer keeps the larger budget for copy mode.
#[test]
fn scrollback_limit_is_role_scoped() {
    assert_eq!(scrollback_limit(Role::Observe), OBSERVER_SCROLLBACK);
    assert_eq!(scrollback_limit(Role::Observe), 64 * 1024);
    assert_eq!(scrollback_limit(Role::Active), ACTIVE_SCROLLBACK);
    assert_eq!(scrollback_limit(Role::Active), 256 * 1024);
    // Size holders never build a VT; the arm exists for totality.
    assert_eq!(scrollback_limit(Role::SizeLease), ACTIVE_SCROLLBACK);
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
    let (mut pane, mut rx, mut accept) = rig();
    pane.want(Some(spec(Role::Observe)));
    let mut core = serve(&mut accept, true).await;
    step(&mut pane, &mut rx).await;
    paint(&mut pane, &mut rx, &mut core, b"paint").await;
    // The constructed VT carries the role's configured cap.
    assert_eq!(
        pane.vt().unwrap().scrollback_max_bytes(),
        Some(OBSERVER_SCROLLBACK)
    );
    // ~124 KiB of numbered lines overflows the byte cap: the earliest
    // rows are evicted and the buffer stays bounded rather than
    // growing with the feed.
    let (obs_total, obs_first) = feed_numbered_lines(&mut pane, &mut rx, &mut core, 2000).await;
    assert!(
        obs_first > 0,
        "observer scrollback must evict its earliest rows, first={obs_first}"
    );
    // The active viewer gets the larger configured budget; the
    // observer never retains more than it. libghostty enforces byte
    // caps at page granularity (a ~2 standard-page floor), so today
    // the two land on the same effective bound.
    let (mut active, mut rx2, mut accept2) = rig();
    active.want(Some(spec(Role::Active)));
    let mut core2 = serve(&mut accept2, true).await;
    step(&mut active, &mut rx2).await;
    paint(&mut active, &mut rx2, &mut core2, b"paint").await;
    assert_eq!(
        active.vt().unwrap().scrollback_max_bytes(),
        Some(ACTIVE_SCROLLBACK)
    );
    let (act_total, act_first) = feed_numbered_lines(&mut active, &mut rx2, &mut core2, 2000).await;
    assert!(obs_total <= act_total);
    assert!(obs_first >= act_first);
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
        Some(OBSERVER_SCROLLBACK)
    );
    // An Observe→Active change on the same terminal retains the last
    // paint; the fresh attach's first DATA builds the VT under the
    // new role's cap.
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
        Some(ACTIVE_SCROLLBACK)
    );
    assert!(vt_text(&mut pane).contains("fresh paint"));
}
