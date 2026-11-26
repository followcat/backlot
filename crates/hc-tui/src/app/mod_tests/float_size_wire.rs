//! Wire-level proof that adoption, host resize, show, and hide size the
//! core terminal to the drawn popup `terminal_cols x terminal_rows` surface.
use super::float_size::{expected_geometry, live_with_popup, press_alt_f};
use super::floats::{base_terms, float_term, pending, publish, two_spaces};
use super::*;
use crate::popup_size::PopupSize;
use crate::testcore::{self, FakeCore};
use hc_proto::client::Attach;
use hc_proto::frame::Kind;
use hc_proto::terminal::AttachParams;
use tokio::time::timeout;

async fn next_float_attach(
    app: &mut App,
    requests: &mut mpsc::Receiver<(FakeCore, AttachParams)>,
) -> (FakeCore, AttachParams) {
    timeout(testcore::DEADLINE, async {
        loop {
            tokio::select! {
                req = requests.recv() => return req.expect("attach request"),
                ev = app.pane_rx.recv() => {
                    app.pane_event(ev.expect("pane event"), Instant::now());
                }
            }
        }
    })
    .await
    .expect("attach request deadline")
}

async fn read_resize_driving_app(app: &mut App, core: &mut FakeCore, channel: u32) -> (u16, u16) {
    timeout(testcore::DEADLINE, async {
        loop {
            tokio::select! {
                frame = core.read_stream(channel) => {
                    if frame.kind == Kind::Resize {
                        let cols = u16::from_le_bytes([frame.payload[0], frame.payload[1]]);
                        let rows = u16::from_le_bytes([frame.payload[2], frame.payload[3]]);
                        return (cols, rows);
                    }
                }
                ev = app.pane_rx.recv() => {
                    app.pane_event(ev.expect("pane event"), Instant::now());
                }
            }
        }
    })
    .await
    .expect("resize frame deadline")
}

#[tokio::test]
async fn float_wire_sizes_core_terminal_to_drawn_surface_across_lifecycle() {
    let mut app = two_spaces();
    pending(&mut app);
    let (accepted, mut requests) = mpsc::channel(16);
    app.open_attach = Arc::new(move |params: AttachParams| {
        let accepted = accepted.clone();
        Box::pin(async move {
            let (core, read, write) = testcore::pair(64 * 1024);
            accepted.send((core, params.clone())).await.unwrap();
            Attach::open(read, write, None, &params).await
        })
    });
    app.area = (120, 40);
    app.live = live_with_popup(Some("80%"), Some("80%"));
    let owner = app.focused_owner_key().unwrap();
    let geom_120 = expected_geometry(
        &app,
        Some(PopupSize::Percent(80)),
        Some(PopupSize::Percent(80)),
    );

    // 1. Adoption: SizeLease attaches at 80% terminal_cols x terminal_rows and
    //    resizes a core terminal whose grid was smaller.
    let mut t9 = float_term("t9", &owner, 9);
    t9.cols = 40;
    t9.rows = 12;
    let mut terms = base_terms();
    terms.push(t9.clone());
    publish(&mut app, terms);

    let (mut lease_core, lease_params) = next_float_attach(&mut app, &mut requests).await;
    assert_eq!(lease_params.terminal, "t9");
    assert!(!lease_params.observe);
    assert_eq!(
        (lease_params.cols, lease_params.rows),
        (geom_120.terminal_cols, geom_120.terminal_rows)
    );
    lease_core
        .serve_attach(testcore::attach_result(
            1,
            serde_json::to_value(&t9).unwrap(),
            true,
        ))
        .await;
    assert_eq!(
        read_resize_driving_app(&mut app, &mut lease_core, 1).await,
        (geom_120.terminal_cols, geom_120.terminal_rows)
    );

    // 2. Host resize while hidden: SizeLease sends Resize to the new 80% size.
    app.on_input(
        UiEvent::Resize {
            cols: 150,
            rows: 46,
        },
        150,
    );
    let geom_150 = expected_geometry(
        &app,
        Some(PopupSize::Percent(80)),
        Some(PopupSize::Percent(80)),
    );
    assert_eq!(
        read_resize_driving_app(&mut app, &mut lease_core, 1).await,
        (geom_150.terminal_cols, geom_150.terminal_rows)
    );

    // 3. Show: Active attaches at geom_150 terminal_cols x terminal_rows,
    //    core terminal size equals the drawn popup surface.
    press_alt_f(&mut app);
    let (mut active_core, active_params) = next_float_attach(&mut app, &mut requests).await;
    assert_eq!(active_params.terminal, "t9");
    assert!(!active_params.observe);
    assert_eq!(
        (active_params.cols, active_params.rows),
        (geom_150.terminal_cols, geom_150.terminal_rows)
    );
    let mut core_term = t9.clone();
    core_term.cols = geom_150.terminal_cols;
    core_term.rows = geom_150.terminal_rows;
    active_core
        .serve_attach(testcore::attach_result(
            2,
            serde_json::to_value(&core_term).unwrap(),
            true,
        ))
        .await;
    let corner = format!(
        "\x1b[H#\x1b[{};{}H$\x1b[?9001l",
        geom_150.terminal_rows, geom_150.terminal_cols
    );
    active_core
        .send_stream(2, Kind::Data, corner.as_bytes())
        .await;
    timeout(testcore::DEADLINE, async {
        while !app
            .popup
            .as_ref()
            .is_some_and(|popup| popup.pane.is_painted())
        {
            let ev = app.pane_rx.recv().await.expect("pane event");
            app.pane_event(ev, Instant::now());
        }
    })
    .await
    .expect("popup paint deadline");
    assert_eq!(
        app.popup
            .as_ref()
            .unwrap()
            .pane
            .vt_ref()
            .unwrap()
            .size()
            .unwrap(),
        (geom_150.terminal_cols, geom_150.terminal_rows)
    );

    app.notice = None;
    let backend = draw_to(&mut app, 150, 46);
    let buf = backend.buffer();
    let surface_x = geom_150.inner.x;
    let surface_y = geom_150.inner.y;
    assert_eq!(buf[(surface_x, surface_y)].symbol(), "#");
    assert_eq!(
        buf[(
            surface_x + geom_150.terminal_cols - 1,
            surface_y + geom_150.terminal_rows - 1
        )]
            .symbol(),
        "$"
    );
    assert_ne!(
        buf[(
            surface_x + geom_150.terminal_cols,
            surface_y + geom_150.terminal_rows - 1
        )]
            .symbol(),
        "$"
    );

    // 4. Hide: new SizeLease attaches at the same 80% terminal_cols x terminal_rows.
    press_alt_f(&mut app);
    let (_rehide_core, rehide_params) = next_float_attach(&mut app, &mut requests).await;
    assert_eq!(
        (rehide_params.cols, rehide_params.rows),
        (geom_150.terminal_cols, geom_150.terminal_rows)
    );
}
